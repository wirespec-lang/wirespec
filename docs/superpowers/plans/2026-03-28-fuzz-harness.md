# Fuzz Harness Generation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generate a libFuzzer harness (`_fuzz.c`) in the C backend that tests the round-trip property: parse → serialize → re-parse → re-serialize must be stable.

**Architecture:** Add `emit_fuzz_harness()` to `wirespec-backend-c/src/source.rs`. When `CBackendOptions.emit_fuzz_harness` is true, generate an additional `_fuzz.c` artifact via `ArtifactSink`. The harness targets the first frame, capsule, or packet in the module. No changes to other crates.

**Tech Stack:** Rust, `wirespec-backend-c` only

---

### Task 1: Implement fuzz harness generation

**Files:**
- Modify: `crates/wirespec-backend-c/src/source.rs` — add `emit_fuzz_source()`
- Modify: `crates/wirespec-backend-c/src/lib.rs` — emit fuzz artifact when flag is set
- Test: `crates/wirespec-backend-c/tests/codegen_tests.rs`

- [ ] **Step 1: Add `emit_fuzz_source()` to source.rs**

```rust
pub fn emit_fuzz_source(module: &CodecModule, prefix: &str) -> Option<String> {
    // Select fuzz target: first frame, then capsule, then packet
    let (type_snake, _) = if let Some(f) = module.frames.first() {
        (to_snake_case(&f.name), &f.name)
    } else if let Some(c) = module.capsules.first() {
        (to_snake_case(&c.name), &c.name)
    } else if let Some(p) = module.packets.first() {
        (to_snake_case(&p.name), &p.name)
    } else {
        return None;
    };

    let tname = format!("{prefix}_{type_snake}_t");
    let parse_fn = format!("{prefix}_{type_snake}_parse");
    let serialize_fn = format!("{prefix}_{type_snake}_serialize");

    Some(format!(r#"#include "{prefix}.h"
#include <stdint.h>
#include <stddef.h>
#include <string.h>

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {{
    {tname} obj;
    size_t consumed;
    wirespec_result_t r = {parse_fn}(data, size, &obj, &consumed);
    if (r != WIRESPEC_OK) return 0;

    uint8_t buf[65536];
    size_t written;
    r = {serialize_fn}(&obj, buf, sizeof(buf), &written);
    if (r != WIRESPEC_OK) return 0;

    {tname} obj2;
    size_t consumed2;
    r = {parse_fn}(buf, written, &obj2, &consumed2);
    if (r != WIRESPEC_OK) __builtin_trap();
    if (consumed2 != written) __builtin_trap();

    uint8_t buf2[65536];
    size_t written2;
    r = {serialize_fn}(&obj2, buf2, sizeof(buf2), &written2);
    if (r != WIRESPEC_OK) __builtin_trap();
    if (written != written2 || memcmp(buf, buf2, written) != 0) __builtin_trap();

    return 0;
}}
"#))
}
```

- [ ] **Step 2: Emit fuzz artifact in lib.rs**

In `CBackend::emit()`, after emitting header and source, if `lowered.emit_fuzz` is true and fuzz source exists:

```rust
if lowered.emit_fuzz {
    if let Some(fuzz_src) = source::emit_fuzz_source(&codec_module, &lowered.prefix) {
        // Need to store codec module reference — or generate fuzz in lower()
    }
}
```

Actually, generate fuzz content in `lower()` and store in `CLoweredModule`:

```rust
pub struct CLoweredModule {
    pub header_content: String,
    pub source_content: String,
    pub fuzz_content: Option<String>,  // NEW
    pub prefix: String,
    pub emit_fuzz: bool,
}
```

In `lower()`:
```rust
let fuzz_content = if emit_fuzz {
    source::emit_fuzz_source(module, &ctx.module_prefix)
} else {
    None
};
```

In `emit()`, if `fuzz_content` is Some, write it as a third artifact with `ArtifactKind("c-fuzz-source")`.

- [ ] **Step 3: Add tests**

```rust
#[test]
fn codegen_fuzz_harness() {
    let ast = wirespec_syntax::parse("packet P { x: u8, y: u16 }").unwrap();
    let sem = wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    let backend = CBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(CBackendOptions { emit_fuzz_harness: true }),
        checksum_bindings: Arc::new(checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };
    let mut sink = MemorySink::new();
    let output = backend.lower_and_emit(&codec, &ctx, &mut sink).unwrap();
    assert_eq!(output.artifacts.len(), 3); // .h + .c + _fuzz.c
    let fuzz = &sink.artifacts[2];
    let fuzz_src = String::from_utf8_lossy(&fuzz.1);
    assert!(fuzz_src.contains("LLVMFuzzerTestOneInput"));
    assert!(fuzz_src.contains("__builtin_trap"));
    assert!(fuzz_src.contains("memcmp"));
}

#[test]
fn codegen_no_fuzz_by_default() {
    let (_, _) = generate_c("packet P { x: u8 }");
    // Default options have emit_fuzz_harness: false
    // generate_c uses default options, so no fuzz artifact
}

#[test]
fn codegen_fuzz_targets_frame() {
    let src = r#"frame F = match tag: u8 { 0 => A {}, _ => B { data: bytes[remaining] } }
    packet P { x: u8 }"#;
    // With fuzz enabled, should target the frame (first priority), not the packet
    let ast = wirespec_syntax::parse(src).unwrap();
    let sem = wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    let backend = CBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(CBackendOptions { emit_fuzz_harness: true }),
        checksum_bindings: Arc::new(checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };
    let lowered = Backend::lower(&backend, &codec, &ctx).unwrap();
    assert!(lowered.fuzz_content.is_some());
    assert!(lowered.fuzz_content.unwrap().contains("test_f_parse")); // frame, not packet
}
```

- [ ] **Step 4: Run tests, commit**

Run: `cargo test --workspace`
