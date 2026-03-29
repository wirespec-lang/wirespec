# wirespec-backend-rust Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the Rust code generator that lowers Codec IR into `.rs` source files producing idiomatic Rust structs with `parse()`, `serialize()`, and `serialized_len()` methods — using the `wirespec-rt` cursor/writer API with zero-copy byte slices and lifetime tracking.

**Architecture:** Mirrors the C backend structure: (1) Rust-local naming/type mapping. (2) Source emitter generates a single `.rs` file with struct definitions, impl blocks for parse/serialize, and frame enums. Key differences from C: Rust enums for frames (not tag+union), `Option<T>` for conditionals (not has_X+X), `&'a [u8]` for bytes (not wirespec_bytes_t), `Result<T>` with `?` propagation (not error code checking).

**Tech Stack:** Rust (edition 2024), `wirespec-backend-api` + `wirespec-codec` crates

**Reference implementation:** `protospec/wirespec/codegen_rust.py`, `protospec/runtime/rust/wirespec-rt/`

---

## File Structure

```
crates/wirespec-backend-rust/
├── Cargo.toml
├── src/
│   ├── lib.rs                      # Backend trait impl, public API
│   ├── names.rs                    # Rust naming: PascalCase types, snake_case fields
│   ├── type_map.rs                 # WireType → Rust type string, cursor read/write methods
│   ├── expr.rs                     # CodecExpr → Rust expression string
│   ├── emit.rs                     # .rs file emission: structs, enums, impl blocks
│   ├── parse_emit.rs              # Parse method body generation
│   ├── serialize_emit.rs          # Serialize method body generation
│   └── checksum_binding.rs        # Rust checksum runtime bindings
└── tests/
    └── codegen_tests.rs            # Integration tests
```

---

## Chunk 1: Naming, Type Mapping, Expressions

### Task 1: All foundational modules

**Files:** Create `names.rs`, `type_map.rs`, `expr.rs`, `checksum_binding.rs`

- [ ] **Step 1: Write implementations**

**names.rs** — Rust naming conventions:
- `to_pascal_case(prefix, name)` → `"QuicVarintVarInt"` (PascalCase)
- `to_snake_case(name)` → `"var_int"` (snake_case for fields/functions)
- `rust_module_prefix(prefix, name)` → `"quic_varint_var_int"` (function prefix)

**type_map.rs** — WireType → Rust type:
- `U8 → "u8"`, `U16 → "u16"`, `U24 → "u32"`, `U32 → "u32"`, `U64 → "u64"`
- `I8 → "i8"`, `I16 → "i16"`, `I32 → "i32"`, `I64 → "i64"`
- `Bool → "bool"`, `Bit → "u8"`, `Bits(n) → "u8"` (or u16/u32/u64)
- `VarInt/ContVarInt → "u64"`
- `Bytes → "&'a [u8]"` (zero-copy)
- `Struct(name)/Frame(name)/Capsule(name) → pascal_case(name)` with optional `<'a>`
- `Enum(name) → pascal_case(name)`
- Cursor methods: `read_u16be`, `read_u16le`, `read_u32be`, etc.
- Writer methods: `write_u16be`, `write_u16le`, etc.

**expr.rs** — CodecExpr → Rust expression:
- `ValueRef(Field)` → `self.field_name` (serialize) or `field_name` (parse, local variable)
- `ValueRef(Const)` → `CONST_NAME`
- `Literal(Int)` → integer literal
- `Binary` → `(left op right)` with `and→&&`, `or→||`
- `Coalesce` → `field_name.unwrap_or(default)` (Rust Option)
- `Unary("!")` → `!operand`

**checksum_binding.rs** — Rust checksum bindings using `wirespec_rt`:
- `internet` → `wirespec_rt::internet_checksum` / `wirespec_rt::internet_checksum_compute`
- `crc32` → `wirespec_rt::crc32_verify` / `wirespec_rt::crc32_compute`
- `crc32c` → `wirespec_rt::crc32c_verify` / `wirespec_rt::crc32c_compute`
- `fletcher16` → `wirespec_rt::fletcher16_verify` / `wirespec_rt::fletcher16_compute`

- [ ] **Step 2: Verify build, commit**

---

## Chunk 2: Source Emission

### Task 2: Parse and serialize emission

**Files:** Create `parse_emit.rs`, `serialize_emit.rs`

- [ ] **Step 1: Write parse_emit.rs**

Per-field parse code. Each field creates a local `let` binding:
- `Primitive` → `let name = cur.read_u16be()?;`
- `BitGroup` → read group, shift+mask extract:
  ```rust
  let _bg0 = cur.read_u8()?;
  let version = ((_bg0 >> 4) & 0xf) as u8;
  let ihl = ((_bg0 >> 0) & 0xf) as u8;
  ```
- `BytesFixed` → `let mac = cur.read_bytes(6)?;`
- `BytesLength` → `let data = cur.read_bytes(len as usize)?;`
- `BytesRemaining` → `let data = cur.read_remaining();`
- `BytesLor` → `let data = if let Some(l) = len { cur.read_bytes(l as usize)? } else { cur.read_remaining() };`
- `Conditional` → `let extra = if condition { Some(cur.read_u16be()?) } else { None };`
- `Array` → loop with `let mut items = [Default; CAP]; let mut items_count = 0;`
- `Struct` → `let inner = Type::parse(cur)?;`
- `VarInt` → `let x = prefix_varint_parse(cur)?;`
- `Checksum` → same as primitive (verify post-parse)

- [ ] **Step 2: Write serialize_emit.rs**

Per-field serialize code:
- `Primitive` → `w.write_u16be(self.name)?;`
- `BitGroup` → combine with shift+OR, write
- `Bytes` → `w.write_bytes(self.data)?;`
- `Conditional` → `if let Some(ref val) = self.extra { w.write_u16be(*val)?; }`
- `Array` → loop
- `Struct` → `self.inner.serialize(w)?;`

Also `serialized_len()`:
- `Primitive` → fixed sizes
- `Bytes` → `self.data.len()`
- `Conditional` → `if self.extra.is_some() { size } else { 0 }`
- `Array` → element_size * count (or loop for variable-size)

- [ ] **Step 3: Commit**

---

### Task 3: Main source file emission

**Files:** Create `emit.rs`

- [ ] **Step 1: Write implementation**

Generates the complete `.rs` file:

```rust
// Auto-generated by wirespec. Do not edit.
#![allow(unused_imports, unused_variables, dead_code)]
use wirespec_rt::{Cursor, Writer, Error, Result};

// Constants
pub const MAX_CID_LENGTH: u8 = 20;

// Enums
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameType { Padding = 0, Ping = 1, Crypto = 6 }

// Packet structs
#[derive(Debug, Clone, PartialEq)]
pub struct UdpDatagram<'a> {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,
    pub checksum: u16,
    pub data: &'a [u8],
}

impl<'a> UdpDatagram<'a> {
    pub fn parse(cur: &mut Cursor<'a>) -> Result<Self> { ... }
    pub fn serialize(&self, w: &mut Writer<'_>) -> Result<()> { ... }
    pub fn serialized_len(&self) -> usize { ... }
}

// Frame enums
#[derive(Debug, Clone, PartialEq)]
pub enum AttPdu<'a> {
    ErrorRsp { request_opcode: u8, handle: u16, error_code: u8 },
    ReadRsp { value: &'a [u8] },
    WriteRsp,
    Unknown { data: &'a [u8] },
}

impl<'a> AttPdu<'a> {
    pub fn parse(cur: &mut Cursor<'a>) -> Result<(Self, u8)> { ... }
    pub fn serialize(&self, tag: u8, w: &mut Writer<'_>) -> Result<()> { ... }
    pub fn serialized_len(&self) -> usize { ... }
}
```

Lifetime tracking: if any field uses `&'a [u8]`, the struct/enum gets `<'a>`.

- [ ] **Step 2: Commit**

---

## Chunk 3: Backend Integration + Tests

### Task 4: Backend trait implementation

**Files:** Modify `lib.rs`

- [ ] **Step 1: Implement RustBackend**

```rust
pub struct RustBackend;

impl Backend for RustBackend {
    type LoweredModule = RustLoweredModule;

    fn id(&self) -> TargetId { TARGET_RUST }

    fn lower(&self, module: &CodecModule, ctx: &BackendContext) -> Result<Self::LoweredModule, BackendError> {
        match &ctx.target_options {
            TargetOptions::Rust(_) => {}
            _ => return Err(BackendError::UnsupportedOption { ... }),
        }
        let source = emit::emit_source(module, &ctx.module_prefix);
        Ok(RustLoweredModule { source, prefix: ctx.module_prefix.clone() })
    }

    fn emit(&self, lowered: &Self::LoweredModule, sink: &mut dyn ArtifactSink) -> Result<BackendOutput, BackendError> {
        sink.write(Artifact {
            target: TARGET_RUST,
            kind: ArtifactKind::RustSource,
            relative_path: format!("{}.rs", lowered.prefix).into(),
            contents: lowered.source.as_bytes().to_vec(),
            ...
        })?;
        Ok(BackendOutput { target: TARGET_RUST, artifacts: vec![...] })
    }
}

impl BackendDyn for RustBackend { ... }
```

- [ ] **Step 2: Commit**

---

### Task 5: Integration tests

**Files:** Create `tests/codegen_tests.rs`

- [ ] **Step 1: Write tests**

```rust
fn generate_rust(src: &str) -> String {
    let ast = wirespec_syntax::parse(src).unwrap();
    let sem = wirespec_sema::analyze(&ast, ComplianceProfile::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    let backend = RustBackend;
    let ctx = BackendContext { target_options: TargetOptions::Rust(RustBackendOptions::default()), ... };
    let lowered = backend.lower(&codec, &ctx).unwrap();
    lowered.source
}

#[test]
fn codegen_simple_packet() {
    let rs = generate_rust("packet P { x: u8, y: u16 }");
    assert!(rs.contains("pub struct"));
    assert!(rs.contains("pub x: u8"));
    assert!(rs.contains("pub y: u16"));
    assert!(rs.contains("fn parse"));
    assert!(rs.contains("fn serialize"));
}

#[test]
fn codegen_bytes_field() {
    let rs = generate_rust("packet P { data: bytes[remaining] }");
    assert!(rs.contains("&'a [u8]"));
    assert!(rs.contains("<'a>"));
}

#[test]
fn codegen_optional_field() {
    let rs = generate_rust("packet P { flags: u8, extra: if flags & 1 { u16 } }");
    assert!(rs.contains("Option<u16>"));
}

#[test]
fn codegen_frame() {
    let src = "frame F = match tag: u8 { 0 => A {}, 1 => B { x: u8 }, _ => C { data: bytes[remaining] } }";
    let rs = generate_rust(src);
    assert!(rs.contains("pub enum"));
    assert!(rs.contains("match"));
}

#[test]
fn codegen_bitgroup() {
    let rs = generate_rust("packet P { a: bits[4], b: bits[4], c: u16 }");
    assert!(rs.contains(">>")); // shift
    assert!(rs.contains("& 0xf")); // mask
}

#[test]
fn codegen_array() {
    let rs = generate_rust("packet P { count: u8, items: [u8; count] }");
    assert!(rs.contains("items_count"));
}

#[test]
fn codegen_enum_def() {
    let rs = generate_rust("enum E: u8 { A = 0, B = 1 }");
    assert!(rs.contains("pub const"));
}

#[test]
fn codegen_derived_field() {
    let rs = generate_rust("packet P { flags: u8, let is_set: bool = (flags & 1) != 0 }");
    assert!(rs.contains("is_set"));
}

#[test]
fn codegen_require() {
    let rs = generate_rust("packet P { length: u16, require length >= 8 }");
    assert!(rs.contains("Error::Constraint"));
}

#[test]
fn codegen_artifact_emission() {
    // Test artifact through MemorySink
    let mut sink = MemorySink::new();
    // ... lower_and_emit ...
    assert_eq!(sink.artifacts.len(), 1); // single .rs file
}
```

- [ ] **Step 2: Run tests, commit**

Run: `cargo test --workspace`

---

## Summary

| Chunk | Tasks | What it delivers |
|-------|-------|-----------------|
| 1 | Task 1 | Naming, type mapping, expression codegen, checksum bindings |
| 2 | Tasks 2-3 | Parse/serialize emission + full .rs file assembly |
| 3 | Tasks 4-5 | Backend trait impl + 10 integration tests |

**Total test count target:** ~10 tests

**Key differences from C backend:**
- Single `.rs` file output (no header/source split)
- `Option<T>` for optionals (not `has_X` + `X`)
- `&'a [u8]` for bytes (not `wirespec_bytes_t`)
- Rust enums for frames (not C tagged union)
- `Result<T>` with `?` (not error code checking)
- Lifetime `<'a>` annotation when bytes fields present
