# Cross-Module Type Resolution + CLI Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable multi-module compilation where imported types are resolved during semantic analysis, and provide a CLI binary (`wirespec compile`) for end-user use.

**Architecture:** (1) Extend `analyze()` to accept `ExternalTypes` and register imported type names in the `TypeRegistry` before Pass 2. (2) Wire `ExternalTypes` through the pipeline so the driver's forward-chaining pattern actually works. (3) Add a thin CLI binary that wraps the driver API with argument parsing.

**Tech Stack:** Rust (edition 2024), existing wirespec crates

---

## File Modifications

| File | Change |
|------|--------|
| `wirespec-sema/src/analyzer.rs` | `analyze()` accepts optional `&ExternalTypes`, registers imported types in `TypeRegistry` |
| `wirespec-sema/src/resolve.rs` | `TypeRegistry` gains `register_external()` for imported types |
| `wirespec-driver/src/pipeline.rs` | `compile_module()` passes `external_types` to `analyze()` |
| `wirespec-driver/src/driver.rs` | No change needed — already passes ExternalTypes correctly |
| NEW: `wirespec-driver/src/bin/wirespec.rs` | CLI binary with `compile` and `check` subcommands |

---

## Chunk 1: Cross-Module Type Resolution

### Task 1: Extend TypeRegistry for external types

**Files:**
- Modify: `crates/wirespec-sema/src/resolve.rs`

- [ ] **Step 1: Add register_external method**

```rust
/// Register an externally-imported type (from another module).
pub fn register_external(&mut self, name: &str, kind: DeclKind) {
    // External types go into the same declarations map.
    // They won't conflict with local declarations because the
    // driver processes modules in dependency order.
    self.declarations.insert(name.to_string(), kind);
}
```

- [ ] **Step 2: Commit**

### Task 2: Extend analyze() to accept ExternalTypes

**Files:**
- Modify: `crates/wirespec-sema/src/analyzer.rs`
- Modify: `crates/wirespec-sema/src/lib.rs`

- [ ] **Step 1: Update analyze() signature**

```rust
pub fn analyze(
    ast: &AstModule,
    profile: ComplianceProfile,
    external_types: Option<&ExternalTypes>,
) -> SemaResult<SemanticModule> {
    let mut analyzer = Analyzer::new(profile);
    if let Some(ext) = external_types {
        analyzer.register_external_types(ext);
    }
    analyzer.run(ast)
}
```

Add `register_external_types` method to `Analyzer`:
```rust
fn register_external_types(&mut self, ext: &ExternalTypes) {
    for (name, et) in ext.iter() {
        let kind = match et.kind {
            ExternalTypeKind::VarInt => DeclKind::VarInt,
            ExternalTypeKind::Packet => DeclKind::Packet,
            ExternalTypeKind::Frame => DeclKind::Frame,
            ExternalTypeKind::Capsule => DeclKind::Capsule,
            ExternalTypeKind::Enum => DeclKind::Enum,
            ExternalTypeKind::Flags => DeclKind::Flags,
            ExternalTypeKind::StateMachine => DeclKind::StateMachine,
        };
        self.registry.register_external(name, kind);
    }
}
```

Also update the `pub use` in `lib.rs` — need to re-export or import `ExternalTypes` from driver. BUT there's a circular dependency problem: `wirespec-sema` can't depend on `wirespec-driver`.

**Solution:** Define `ExternalTypes` and `ExternalType` in `wirespec-sema` instead of `wirespec-driver`. Move these types to a new or existing module in sema, then have driver re-export from sema.

Alternative: Define a minimal `ExternalTypeInfo` directly in sema (just name → DeclKind mapping). The driver wraps it with richer info.

**Simplest approach:** Pass a `HashMap<String, DeclKind>` to `analyze()` instead of `ExternalTypes`. The driver converts its `ExternalTypes` into this map before calling.

```rust
pub fn analyze(
    ast: &AstModule,
    profile: ComplianceProfile,
    external_types: &HashMap<String, DeclKind>,
) -> SemaResult<SemanticModule> {
```

- [ ] **Step 2: Update all callers**

All existing calls to `analyze()` pass `&HashMap::new()` or `&Default::default()`.

Update:
- All test files that call `analyze()`
- `wirespec-driver/src/pipeline.rs`

- [ ] **Step 3: Commit**

### Task 3: Wire ExternalTypes through pipeline

**Files:**
- Modify: `crates/wirespec-driver/src/pipeline.rs`

- [ ] **Step 1: Convert ExternalTypes to HashMap<String, DeclKind> and pass to analyze**

```rust
pub fn compile_module(
    source: &str,
    profile: ComplianceProfile,
    external_types: &ExternalTypes,
) -> Result<CodecModule, PipelineError> {
    let ast = wirespec_syntax::parse(source).map_err(...)?;

    // Convert ExternalTypes to sema's expected format
    let ext_map: HashMap<String, wirespec_sema::resolve::DeclKind> = external_types
        .iter()
        .map(|(name, et)| {
            let kind = match et.kind {
                ExternalTypeKind::VarInt => wirespec_sema::resolve::DeclKind::VarInt,
                ExternalTypeKind::Packet => wirespec_sema::resolve::DeclKind::Packet,
                // ... etc
            };
            (name.clone(), kind)
        })
        .collect();

    let sem = wirespec_sema::analyze(&ast, profile, &ext_map).map_err(...)?;
    // ... rest unchanged
}
```

- [ ] **Step 2: Add tests**

```rust
// In driver_tests.rs — multi-module with imported VarInt
#[test]
fn driver_multi_module_varint_import() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "quic/varint.wspec", r#"module quic.varint
type VarInt = {
    prefix: bits[2],
    value: match prefix { 0b00 => bits[6], 0b01 => bits[14], 0b10 => bits[30], 0b11 => bits[62] },
}"#);
    write_file(&dir, "quic/frames.wspec", r#"module quic.frames
import quic.varint.VarInt
packet LengthPrefixedCid { length: u8, value: bytes[length] }
frame QuicFrame = match frame_type: VarInt {
    0x06 => Crypto { offset: VarInt, length: VarInt, data: bytes[length] },
    _ => Unknown { data: bytes[remaining] },
}"#);
    let entry = dir.path().join("quic/frames.wspec");
    let result = compile(&CompileRequest {
        entry,
        include_paths: vec![dir.path().to_path_buf()],
        profile: ComplianceProfile::default(),
    }).unwrap();
    assert!(result.modules.len() >= 2);
    let frames = result.modules.last().unwrap();
    assert!(!frames.codec.frames.is_empty());
}
```

- [ ] **Step 3: Commit**

---

## Chunk 2: CLI Binary

### Task 4: CLI binary

**Files:**
- Create: `crates/wirespec-driver/src/bin/wirespec.rs`
- Modify: `crates/wirespec-driver/Cargo.toml`

- [ ] **Step 1: Add binary target and CLI arg parsing**

No external deps — use minimal argument parsing (std::env::args).

```rust
// crates/wirespec-driver/src/bin/wirespec.rs
use std::env;
use std::path::PathBuf;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "compile" => cmd_compile(&args[2..]),
        "check" => cmd_check(&args[2..]),
        "--help" | "-h" => print_usage(),
        _ => {
            eprintln!("unknown command: {}", args[1]);
            print_usage();
            process::exit(1);
        }
    }
}

fn cmd_compile(args: &[String]) {
    let mut input = None;
    let mut output = PathBuf::from("build");
    let mut target = "c".to_string();
    let mut include_paths = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => { i += 1; output = PathBuf::from(&args[i]); }
            "-t" | "--target" => { i += 1; target = args[i].clone(); }
            "-I" | "--include-path" => { i += 1; include_paths.push(PathBuf::from(&args[i])); }
            "--help" | "-h" => { print_compile_usage(); return; }
            arg if arg.starts_with('-') => {
                eprintln!("unknown option: {arg}");
                process::exit(1);
            }
            _ => { input = Some(PathBuf::from(&args[i])); }
        }
        i += 1;
    }

    let input = input.unwrap_or_else(|| {
        eprintln!("error: no input file specified");
        process::exit(1);
    });

    // Compile
    let result = wirespec_driver::compile(&wirespec_driver::CompileRequest {
        entry: input,
        include_paths,
        profile: wirespec_sema::ComplianceProfile::default(),
    });

    match result {
        Ok(result) => {
            std::fs::create_dir_all(&output).ok();

            // Get backend and emit for each module
            use wirespec_backend_api::*;
            use std::sync::Arc;

            for module in &result.modules {
                let (backend, ctx): (Box<dyn BackendDyn>, BackendContext) = match target.as_str() {
                    "c" => {
                        let b = wirespec_backend_c::CBackend;
                        let ctx = BackendContext {
                            module_name: module.module_name.clone(),
                            module_prefix: module.source_prefix.clone(),
                            source_prefixes: Default::default(),
                            compliance_profile: "phase2_extended_current".into(),
                            common_options: CommonOptions::default(),
                            target_options: TargetOptions::C(CBackendOptions::default()),
                            checksum_bindings: Arc::new(wirespec_backend_c::checksum_binding::CChecksumBindings),
                            is_entry_module: module.module_name == result.modules.last().unwrap().module_name,
                        };
                        (Box::new(b), ctx)
                    }
                    "rust" => {
                        let b = wirespec_backend_rust::RustBackend;
                        let ctx = BackendContext {
                            module_name: module.module_name.clone(),
                            module_prefix: module.source_prefix.clone(),
                            source_prefixes: Default::default(),
                            compliance_profile: "phase2_extended_current".into(),
                            common_options: CommonOptions::default(),
                            target_options: TargetOptions::Rust(RustBackendOptions::default()),
                            checksum_bindings: Arc::new(wirespec_backend_rust::checksum_binding::RustChecksumBindings),
                            is_entry_module: module.module_name == result.modules.last().unwrap().module_name,
                        };
                        (Box::new(b), ctx)
                    }
                    _ => {
                        eprintln!("unknown target: {target}");
                        process::exit(1);
                    }
                };

                let mut sink = wirespec_backend_api::MemorySink::new();
                match backend.lower_and_emit(&module.codec, &ctx, &mut sink) {
                    Ok(out) => {
                        for (meta, contents) in &sink.artifacts {
                            let path = output.join(&meta.relative_path);
                            std::fs::write(&path, contents).unwrap();
                            eprintln!("  wrote {}", path.display());
                        }
                    }
                    Err(e) => {
                        eprintln!("backend error: {e}");
                        process::exit(1);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

fn cmd_check(args: &[String]) {
    // Parse-only check (no codegen)
    let input = args.first().unwrap_or_else(|| {
        eprintln!("error: no input file specified");
        process::exit(1);
    });

    let source = std::fs::read_to_string(input).unwrap_or_else(|e| {
        eprintln!("error: cannot read {input}: {e}");
        process::exit(1);
    });

    match wirespec_driver::compile_module(&source, wirespec_sema::ComplianceProfile::default(), &Default::default()) {
        Ok(_) => eprintln!("ok"),
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

fn print_usage() {
    eprintln!("wirespec compiler\n");
    eprintln!("Usage: wirespec <command> [options]\n");
    eprintln!("Commands:");
    eprintln!("  compile  Compile .wspec files to C or Rust");
    eprintln!("  check    Parse and type-check without code generation");
}

fn print_compile_usage() {
    eprintln!("Usage: wirespec compile <input.wspec> [options]\n");
    eprintln!("Options:");
    eprintln!("  -o, --output <dir>      Output directory (default: build)");
    eprintln!("  -t, --target <c|rust>   Target language (default: c)");
    eprintln!("  -I, --include-path <dir>  Module search path (repeatable)");
}
```

Cargo.toml additions:
```toml
[[bin]]
name = "wirespec"
path = "src/bin/wirespec.rs"

[dependencies]
wirespec-backend-c = { path = "../wirespec-backend-c" }
wirespec-backend-rust = { path = "../wirespec-backend-rust" }
```

- [ ] **Step 2: Build and test CLI**

```bash
cargo build -p wirespec-driver --bin wirespec
./target/debug/wirespec compile protospec/examples/net/udp.wire -o /tmp/wirespec-out -t c
gcc -Wall -Wextra -Werror -std=c11 -fsyntax-only -I /tmp/wirespec-out -I protospec/runtime /tmp/wirespec-out/net_udp.c
```

- [ ] **Step 3: Commit**

---

## Summary

| Chunk | Tasks | What it delivers |
|-------|-------|-----------------|
| 1 | Tasks 1-3 | Cross-module type resolution: imported types registered in sema |
| 2 | Task 4 | CLI binary: `wirespec compile` and `wirespec check` |
