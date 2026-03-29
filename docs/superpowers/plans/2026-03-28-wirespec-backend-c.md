# wirespec-backend-c Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the C code generator that lowers Codec IR into `.h` header and `.c` source files producing `_parse()`, `_serialize()`, and `_serialized_len()` functions for all wirespec constructs — using the existing `wirespec_runtime.h` cursor API with zero heap allocation.

**Architecture:** Three layers: (1) C-local lowering converts Codec IR types/fields/expressions into C-specific names and patterns (type mapping, naming conventions, helper selection). (2) Header emitter generates struct/union typedefs, enum defs, and function declarations. (3) Source emitter generates parse/serialize implementations using the wirespec_runtime cursor API. The backend implements the `Backend` trait from `wirespec-backend-api`.

**Tech Stack:** Rust (edition 2024), `wirespec-backend-api` + `wirespec-codec` crates

**Reference implementation:** `protospec/wirespec/codegen_c.py` (4300 lines), `protospec/runtime/wirespec_runtime.h`

**Design constraints:**
- Generated C must compile with `gcc -Wall -Wextra -Werror -O2 -std=c11`
- No heap allocation in generated code (stack + caller buffers only)
- Memory tiers: A=zero-copy `wirespec_bytes_t`, B=materialized scalar array, C=materialized struct array
- Bitgroups: single read/write + shift+mask per field
- Checksum: verify after parse, auto-compute after serialize

---

## File Structure

```
crates/wirespec-backend-c/
├── Cargo.toml
├── src/
│   ├── lib.rs                      # Backend trait impl, public API
│   ├── names.rs                    # C naming conventions: type names, function names, field accessors
│   ├── type_map.rs                 # WireType → C type string mapping
│   ├── header.rs                   # .h emission: structs, enums, function declarations
│   ├── source.rs                   # .c emission: parse/serialize implementations
│   ├── expr.rs                     # CodecExpr → C expression string conversion
│   ├── parse_emit.rs              # Parse function body generation (cursor reads)
│   ├── serialize_emit.rs          # Serialize function body generation (buffer writes)
│   └── checksum_binding.rs        # C checksum runtime bindings
└── tests/
    ├── names_tests.rs              # Naming convention tests
    ├── codegen_tests.rs            # Full codegen output tests
    └── compile_tests.rs            # Compile generated C with gcc (if available)
```

| File | Responsibility |
|------|---------------|
| `names.rs` | `{prefix}_{snake_case}_t` type names, `{prefix}_{snake_case}_parse` function names |
| `type_map.rs` | `WireType::U16` → `"uint16_t"`, field struct member types |
| `header.rs` | `#include`, struct typedefs, tagged union for frames, enum defs, function decls |
| `source.rs` | Top-level `.c` file assembly: includes, static parse helpers, public API functions |
| `expr.rs` | `CodecExpr` → C code string (field refs, binary ops, literals, coalesce) |
| `parse_emit.rs` | Per-field parse code: cursor reads, bitgroup reads, conditional, arrays, bytes, varint |
| `serialize_emit.rs` | Per-field serialize code: buffer writes, bitgroup writes, conditional, arrays |
| `checksum_binding.rs` | `ChecksumBindingProvider` impl for C runtime symbols |

---

## Chunk 1: Naming and Type Mapping

### Task 1: C naming conventions

**Files:**
- Create: `crates/wirespec-backend-c/src/names.rs`
- Test: `crates/wirespec-backend-c/tests/names_tests.rs`

- [ ] **Step 1: Write tests and implementation**

```rust
// crates/wirespec-backend-c/src/names.rs

/// Convert PascalCase/camelCase to snake_case.
pub fn to_snake_case(name: &str) -> String {
    let mut result = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            let prev = name.chars().nth(i - 1).unwrap_or('_');
            if prev.is_lowercase() || prev.is_ascii_digit() {
                result.push('_');
            }
        }
        result.push(ch.to_lowercase().next().unwrap());
    }
    result
}

/// Generate C type name: {prefix}_{snake_name}_t
pub fn c_type_name(prefix: &str, name: &str) -> String {
    format!("{prefix}_{}_t", to_snake_case(name))
}

/// Generate C function name: {prefix}_{snake_name}_{suffix}
pub fn c_func_name(prefix: &str, name: &str, suffix: &str) -> String {
    format!("{prefix}_{}_{suffix}", to_snake_case(name))
}

/// Generate C enum tag name: {PREFIX}_{SNAKE_NAME}_{MEMBER}
pub fn c_enum_member(prefix: &str, enum_name: &str, member: &str) -> String {
    format!(
        "{}_{}_{member}",
        prefix.to_uppercase(),
        to_snake_case(enum_name).to_uppercase()
    )
}

/// Generate frame tag enum name: {prefix}_{frame_snake}_tag_t
pub fn c_frame_tag_type(prefix: &str, frame_name: &str) -> String {
    format!("{prefix}_{}_tag_t", to_snake_case(frame_name))
}

/// Generate frame tag enum value: {PREFIX}_{FRAME}_{VARIANT}
pub fn c_frame_tag_value(prefix: &str, frame_name: &str, variant: &str) -> String {
    format!(
        "{}_{}_{}",
        prefix.to_uppercase(),
        to_snake_case(frame_name).to_uppercase(),
        to_snake_case(variant).to_uppercase()
    )
}
```

Tests:
```rust
// crates/wirespec-backend-c/tests/names_tests.rs
use wirespec_backend_c::names::*;

#[test]
fn snake_case_simple() {
    assert_eq!(to_snake_case("UdpDatagram"), "udp_datagram");
    assert_eq!(to_snake_case("IPv4Header"), "i_pv4_header");
    assert_eq!(to_snake_case("VarInt"), "var_int");
    assert_eq!(to_snake_case("simple"), "simple");
}

#[test]
fn type_name() {
    assert_eq!(c_type_name("quic", "VarInt"), "quic_var_int_t");
    assert_eq!(c_type_name("net_udp", "UdpDatagram"), "net_udp_udp_datagram_t");
}

#[test]
fn func_name() {
    assert_eq!(c_func_name("quic", "VarInt", "parse"), "quic_var_int_parse");
    assert_eq!(c_func_name("net_udp", "UdpDatagram", "serialize"), "net_udp_udp_datagram_serialize");
}

#[test]
fn enum_member_name() {
    assert_eq!(c_enum_member("quic", "FrameType", "Padding"), "QUIC_FRAME_TYPE_Padding");
}

#[test]
fn frame_tag_names() {
    assert_eq!(c_frame_tag_type("ble_att", "AttPdu"), "ble_att_att_pdu_tag_t");
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p wirespec-backend-c --test names_tests`

- [ ] **Step 3: Commit**

---

### Task 2: WireType → C type string mapping

**Files:**
- Create: `crates/wirespec-backend-c/src/type_map.rs`

- [ ] **Step 1: Write implementation**

```rust
// crates/wirespec-backend-c/src/type_map.rs
use wirespec_codec::ir::*;

/// Map Codec WireType to C type string for struct member declaration.
pub fn wire_type_to_c(wt: &WireType, prefix: &str) -> String {
    match wt {
        WireType::U8 => "uint8_t".into(),
        WireType::U16 => "uint16_t".into(),
        WireType::U24 => "uint32_t".into(), // stored as u32 in C
        WireType::U32 => "uint32_t".into(),
        WireType::U64 => "uint64_t".into(),
        WireType::I8 => "int8_t".into(),
        WireType::I16 => "int16_t".into(),
        WireType::I32 => "int32_t".into(),
        WireType::I64 => "int64_t".into(),
        WireType::Bool => "bool".into(),
        WireType::Bit => "uint8_t".into(),
        WireType::Bits(n) => {
            if *n <= 8 { "uint8_t".into() }
            else if *n <= 16 { "uint16_t".into() }
            else if *n <= 32 { "uint32_t".into() }
            else { "uint64_t".into() }
        }
        WireType::VarInt | WireType::ContVarInt => "uint64_t".into(),
        WireType::Bytes => "wirespec_bytes_t".into(),
        WireType::Struct(name) | WireType::Frame(name) | WireType::Capsule(name) => {
            crate::names::c_type_name(prefix, name)
        }
        WireType::Enum(name) => crate::names::c_type_name(prefix, name),
        WireType::Array => "/* array */".into(), // handled specially
    }
}

/// C type for a field (handles optional: bool has_X + T X).
pub fn field_c_type(field: &CodecField, prefix: &str) -> String {
    if field.is_optional {
        if let Some(ref inner) = field.inner_wire_type {
            wire_type_to_c(inner, prefix)
        } else {
            wire_type_to_c(&field.wire_type, prefix)
        }
    } else {
        wire_type_to_c(&field.wire_type, prefix)
    }
}

/// C read function for a primitive/endian combo.
pub fn cursor_read_fn(wt: &WireType, endianness: Option<wirespec_sema::types::Endianness>) -> &'static str {
    use wirespec_sema::types::Endianness;
    match (wt, endianness) {
        (WireType::U8, _) => "wirespec_cursor_read_u8",
        (WireType::U16, Some(Endianness::Big)) => "wirespec_cursor_read_u16be",
        (WireType::U16, Some(Endianness::Little)) => "wirespec_cursor_read_u16le",
        (WireType::U16, None) => "wirespec_cursor_read_u16be",
        (WireType::U24, Some(Endianness::Big)) => "wirespec_cursor_read_u24be",
        (WireType::U24, Some(Endianness::Little)) => "wirespec_cursor_read_u24le",
        (WireType::U24, None) => "wirespec_cursor_read_u24be",
        (WireType::U32, Some(Endianness::Big)) => "wirespec_cursor_read_u32be",
        (WireType::U32, Some(Endianness::Little)) => "wirespec_cursor_read_u32le",
        (WireType::U32, None) => "wirespec_cursor_read_u32be",
        (WireType::U64, Some(Endianness::Big)) => "wirespec_cursor_read_u64be",
        (WireType::U64, Some(Endianness::Little)) => "wirespec_cursor_read_u64le",
        (WireType::U64, None) => "wirespec_cursor_read_u64be",
        (WireType::I8, _) => "wirespec_cursor_read_u8", // read as u8, cast
        _ => "/* unsupported read */",
    }
}

/// C write function for a primitive/endian combo.
pub fn write_fn(wt: &WireType, endianness: Option<wirespec_sema::types::Endianness>) -> &'static str {
    use wirespec_sema::types::Endianness;
    match (wt, endianness) {
        (WireType::U8, _) => "wirespec_write_u8",
        (WireType::U16, Some(Endianness::Big)) => "wirespec_write_u16be",
        (WireType::U16, Some(Endianness::Little)) => "wirespec_write_u16le",
        (WireType::U16, None) => "wirespec_write_u16be",
        (WireType::U24, Some(Endianness::Big)) => "wirespec_write_u24be",
        (WireType::U24, Some(Endianness::Little)) => "wirespec_write_u24le",
        (WireType::U24, None) => "wirespec_write_u24be",
        (WireType::U32, Some(Endianness::Big)) => "wirespec_write_u32be",
        (WireType::U32, Some(Endianness::Little)) => "wirespec_write_u32le",
        (WireType::U32, None) => "wirespec_write_u32be",
        (WireType::U64, Some(Endianness::Big)) => "wirespec_write_u64be",
        (WireType::U64, Some(Endianness::Little)) => "wirespec_write_u64le",
        (WireType::U64, None) => "wirespec_write_u64be",
        (WireType::I8, _) => "wirespec_write_u8",
        _ => "/* unsupported write */",
    }
}
```

- [ ] **Step 2: Verify build, commit**

---

## Chunk 2: Expression Codegen + Checksum Bindings

### Task 3: CodecExpr → C expression string

**Files:**
- Create: `crates/wirespec-backend-c/src/expr.rs`

- [ ] **Step 1: Write implementation**

Convert codec expressions to C code strings. Key patterns:
- `ValueRef(Field)` → `out->field_name` (parse) or `val->field_name` (serialize)
- `ValueRef(Const)` → `CONST_NAME` (uppercase)
- `Literal(Int)` → integer literal
- `Binary` → `(left op right)` with parentheses
- `Coalesce` → `out->has_X ? out->X : default`

```rust
// crates/wirespec-backend-c/src/expr.rs
use wirespec_codec::ir::*;

pub enum ExprContext {
    Parse,      // field refs use "out->"
    Serialize,  // field refs use "val->"
}

pub fn expr_to_c(expr: &CodecExpr, ctx: &ExprContext) -> String {
    match expr {
        CodecExpr::ValueRef { reference } => {
            let prefix = match ctx {
                ExprContext::Parse => "out->",
                ExprContext::Serialize => "val->",
            };
            match reference.kind {
                ValueRefKind::Field | ValueRefKind::Derived => {
                    // Extract field name from value_id
                    let name = extract_name_from_id(&reference.value_id);
                    format!("{prefix}{name}")
                }
                ValueRefKind::Const => {
                    reference.value_id.to_uppercase()
                }
            }
        }
        CodecExpr::Literal { value } => match value {
            LiteralValue::Int(n) => {
                if *n < 0 { format!("({n})") }
                else if *n > 0xFFFF { format!("{n}UL") }
                else { format!("{n}") }
            }
            LiteralValue::Bool(b) => if *b { "true" } else { "false" }.into(),
            LiteralValue::Null => "0".into(),
        },
        CodecExpr::Binary { op, left, right } => {
            let l = expr_to_c(left, ctx);
            let r = expr_to_c(right, ctx);
            let c_op = match op.as_str() {
                "and" => "&&",
                "or" => "||",
                o => o,
            };
            format!("({l} {c_op} {r})")
        }
        CodecExpr::Unary { op, operand } => {
            let o = expr_to_c(operand, ctx);
            format!("({op}{o})")
        }
        CodecExpr::Coalesce { expr: e, default: d } => {
            // For optional field coalesce: has_X ? X : default
            let e_str = expr_to_c(e, ctx);
            let d_str = expr_to_c(d, ctx);
            // Extract field name for has_ prefix
            if let CodecExpr::ValueRef { reference } = e.as_ref() {
                let name = extract_name_from_id(&reference.value_id);
                let prefix = match ctx {
                    ExprContext::Parse => "out->",
                    ExprContext::Serialize => "val->",
                };
                format!("({prefix}has_{name} ? {e_str} : {d_str})")
            } else {
                format!("({e_str} ? {e_str} : {d_str})")
            }
        }
        CodecExpr::Subscript { base, index } => {
            let b = expr_to_c(base, ctx);
            let i = expr_to_c(index, ctx);
            format!("{b}[{i}]")
        }
        _ => "/* unsupported expr */".into(),
    }
}

fn extract_name_from_id(id: &str) -> &str {
    // IDs like "packet:P.field[0]" or just field names
    // For simple cases, extract the last meaningful part
    if let Some(dot_pos) = id.rfind('.') {
        let after = &id[dot_pos + 1..];
        if let Some(bracket) = after.find('[') {
            &after[..bracket]
        } else {
            after
        }
    } else {
        id
    }
}
```

- [ ] **Step 2: Commit**

---

### Task 4: Checksum binding provider for C

**Files:**
- Create: `crates/wirespec-backend-c/src/checksum_binding.rs`

- [ ] **Step 1: Write implementation**

```rust
// crates/wirespec-backend-c/src/checksum_binding.rs
use wirespec_backend_api::*;

pub struct CChecksumBindings;

impl ChecksumBindingProvider for CChecksumBindings {
    fn binding_for(&self, algorithm: &str) -> Result<ChecksumBackendBinding, BackendError> {
        match algorithm {
            "internet" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("wirespec_internet_checksum".into()),
                compute_symbol: "wirespec_internet_checksum_compute".into(),
                compute_style: ComputeStyle::PatchInPlace,
            }),
            "crc32" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("wirespec_crc32_verify".into()),
                compute_symbol: "wirespec_crc32_compute".into(),
                compute_style: ComputeStyle::ReturnValue,
            }),
            "crc32c" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("wirespec_crc32c_verify".into()),
                compute_symbol: "wirespec_crc32c_compute".into(),
                compute_style: ComputeStyle::ReturnValue,
            }),
            "fletcher16" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("wirespec_fletcher16_verify".into()),
                compute_symbol: "wirespec_fletcher16_compute".into(),
                compute_style: ComputeStyle::ReturnValue,
            }),
            _ => Err(BackendError::MissingChecksumBinding {
                target: TARGET_C,
                algorithm: algorithm.to_string(),
            }),
        }
    }
}
```

- [ ] **Step 2: Commit**

---

## Chunk 3: Header Emission

### Task 5: .h header file generation

**Files:**
- Create: `crates/wirespec-backend-c/src/header.rs`

- [ ] **Step 1: Write implementation**

Generates the `.h` file with:
- Include guards
- `#include <stdint.h>`, `#include <stdbool.h>`, `#include <stddef.h>`, `#include "wirespec_runtime.h"`
- Forward declarations
- Enum/flags typedefs
- Struct typedefs for packets
- Tagged union typedefs for frames (tag enum + union + wrapper struct)
- Capsule struct typedefs
- Function declarations: `_parse()`, `_serialize()`, `_serialized_len()`

Key patterns from Python reference:
```c
// Packet struct
typedef struct prefix_name prefix_name_t;
struct prefix_name {
    uint8_t field1;
    uint16_t field2;
    bool has_optional;      // for optional fields
    uint16_t optional;
    wirespec_bytes_t data;  // for bytes fields
    uint8_t items[64];      // for scalar arrays
    uint32_t items_count;   // array count
};

// Frame: tag enum + union
typedef enum { ... } prefix_frame_tag_t;
typedef struct prefix_frame prefix_frame_t;
struct prefix_frame {
    prefix_frame_tag_t tag;
    union { ... } value;
};

// Function declarations
wirespec_result_t prefix_name_parse(
    const uint8_t *buf, size_t len,
    prefix_name_t *out, size_t *consumed);
wirespec_result_t prefix_name_serialize(
    const prefix_name_t *val,
    uint8_t *buf, size_t cap, size_t *written);
size_t prefix_name_serialized_len(const prefix_name_t *val);
```

- [ ] **Step 2: Commit**

---

## Chunk 4: Parse and Serialize Emission

### Task 6: Parse function body generation

**Files:**
- Create: `crates/wirespec-backend-c/src/parse_emit.rs`

- [ ] **Step 1: Write implementation**

Per-field parse code generation. Each field strategy maps to a code pattern:

- `Primitive` → `r = wirespec_cursor_read_uXXbe(cur, &out->name);`
- `BitGroup` → read group as single value, shift+mask extract
- `BytesFixed` → `r = wirespec_cursor_read_bytes(cur, N, &out->name);`
- `BytesLength` → `r = wirespec_cursor_read_bytes(cur, out->len_field, &out->name);`
- `BytesRemaining` → `out->name.ptr = cur->base + cur->pos; out->name.len = wirespec_cursor_remaining(cur);`
- `Conditional` → `if (condition) { ... }`
- `Array` → loop with count
- `Struct` → `r = prefix_inner_parse_cursor(cur, &out->name);`
- `Checksum` → same as primitive read (verification done post-parse)
- `VarInt` → `r = prefix_varint_parse_cursor(cur, &out->name);`

Frame dispatch:
```c
r = prefix_varint_parse_cursor(cur, &tag_value);
switch (tag_value) {
    case 0x00: out->tag = TAG_A; /* parse A fields */ break;
    case 0x01: out->tag = TAG_B; /* parse B fields */ break;
    default: out->tag = TAG_UNKNOWN; /* parse Unknown fields */ break;
}
```

- [ ] **Step 2: Commit**

---

### Task 7: Serialize function body generation

**Files:**
- Create: `crates/wirespec-backend-c/src/serialize_emit.rs`

- [ ] **Step 1: Write implementation**

Mirror of parse but in reverse — writing to buffer instead of reading from cursor.

- `Primitive` → `r = wirespec_write_uXXbe(buf, cap, &pos, val->name);`
- `BitGroup` → combine fields with shift+OR, write single value
- `BytesFixed/Length/Remaining` → `r = wirespec_write_bytes(buf, cap, &pos, val->name.ptr, val->name.len);`
- `Conditional` → `if (val->has_name) { ... }`
- `Array` → loop
- `Struct` → `r = prefix_inner_serialize(val->name, buf + pos, cap - pos, &_written);`
- `Checksum` → write field normally, then patch after all fields written

Also: `_serialized_len()` function that computes output size without actually writing.

- [ ] **Step 2: Commit**

---

## Chunk 5: Source File Assembly + Backend Integration

### Task 8: .c source file assembly

**Files:**
- Create: `crates/wirespec-backend-c/src/source.rs`

- [ ] **Step 1: Write implementation**

Assembles the complete `.c` file:
1. `#include "{prefix}.h"`
2. Static cursor-based parse functions (internal)
3. Public parse functions (wrapping cursor + checksum verify)
4. Public serialize functions
5. Public serialized_len functions
6. Frame dispatch functions
7. Capsule parse/serialize with within sub-cursor

- [ ] **Step 2: Commit**

---

### Task 9: Backend trait implementation

**Files:**
- Modify: `crates/wirespec-backend-c/src/lib.rs`

- [ ] **Step 1: Write implementation**

```rust
// crates/wirespec-backend-c/src/lib.rs
pub mod checksum_binding;
pub mod expr;
pub mod header;
pub mod names;
pub mod parse_emit;
pub mod serialize_emit;
pub mod source;
pub mod type_map;

use wirespec_backend_api::*;
use wirespec_codec::CodecModule;

pub struct CBackend;

impl Backend for CBackend {
    type LoweredModule = CLoweredModule;

    fn id(&self) -> TargetId { TARGET_C }

    fn lower(
        &self,
        module: &CodecModule,
        ctx: &BackendContext,
    ) -> Result<Self::LoweredModule, BackendError> {
        // Validate target options
        let c_opts = match &ctx.target_options {
            TargetOptions::C(opts) => opts,
            _ => return Err(BackendError::UnsupportedOption {
                target: TARGET_C,
                option: "target_options".into(),
                reason: "expected C backend options".into(),
            }),
        };

        let header = header::emit_header(module, &ctx.module_prefix);
        let source = source::emit_source(module, &ctx.module_prefix);

        Ok(CLoweredModule {
            header_content: header,
            source_content: source,
            prefix: ctx.module_prefix.clone(),
            emit_fuzz: c_opts.emit_fuzz_harness,
        })
    }

    fn emit(
        &self,
        lowered: &Self::LoweredModule,
        sink: &mut dyn ArtifactSink,
    ) -> Result<BackendOutput, BackendError> {
        let mut artifacts = Vec::new();

        // Header
        sink.write(Artifact {
            target: TARGET_C,
            kind: ArtifactKind::CHeader,
            module_name: lowered.prefix.clone(),
            module_prefix: lowered.prefix.clone(),
            relative_path: format!("{}.h", lowered.prefix).into(),
            contents: lowered.header_content.as_bytes().to_vec(),
        })?;
        artifacts.push(ArtifactMeta {
            kind: ArtifactKind::CHeader,
            relative_path: format!("{}.h", lowered.prefix).into(),
            byte_len: lowered.header_content.len(),
        });

        // Source
        sink.write(Artifact {
            target: TARGET_C,
            kind: ArtifactKind::CSource,
            module_name: lowered.prefix.clone(),
            module_prefix: lowered.prefix.clone(),
            relative_path: format!("{}.c", lowered.prefix).into(),
            contents: lowered.source_content.as_bytes().to_vec(),
        })?;
        artifacts.push(ArtifactMeta {
            kind: ArtifactKind::CSource,
            relative_path: format!("{}.c", lowered.prefix).into(),
            byte_len: lowered.source_content.len(),
        });

        Ok(BackendOutput {
            target: TARGET_C,
            artifacts,
        })
    }
}

pub struct CLoweredModule {
    pub header_content: String,
    pub source_content: String,
    pub prefix: String,
    pub emit_fuzz: bool,
}

// BackendDyn adapter
impl BackendDyn for CBackend {
    fn id(&self) -> TargetId { TARGET_C }

    fn lower_and_emit(
        &self,
        module: &CodecModule,
        ctx: &BackendContext,
        sink: &mut dyn ArtifactSink,
    ) -> Result<BackendOutput, BackendError> {
        let lowered = Backend::lower(self, module, ctx)?;
        Backend::emit(self, &lowered, sink)
    }
}
```

- [ ] **Step 2: Commit**

---

## Chunk 6: Integration Tests

### Task 10: Full codegen integration tests

**Files:**
- Create: `crates/wirespec-backend-c/tests/codegen_tests.rs`

- [ ] **Step 1: Write tests**

```rust
use wirespec_backend_c::*;
use wirespec_backend_api::*;
use wirespec_sema::ComplianceProfile;
use std::sync::Arc;

fn generate_c(src: &str) -> (String, String) {
    let ast = wirespec_syntax::parse(src).unwrap();
    let sem = wirespec_sema::analyze(&ast, ComplianceProfile::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = CBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: TargetOptions::C(CBackendOptions::default()),
        checksum_bindings: Arc::new(wirespec_backend_c::checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };

    let lowered = backend.lower(&codec, &ctx).unwrap();
    (lowered.header_content.clone(), lowered.source_content.clone())
}

#[test]
fn codegen_simple_packet_header() {
    let (header, _) = generate_c("packet UdpDatagram { src_port: u16, dst_port: u16, length: u16, checksum: u16 }");
    assert!(header.contains("typedef struct"));
    assert!(header.contains("test_udp_datagram_t"));
    assert!(header.contains("uint16_t src_port"));
    assert!(header.contains("wirespec_result_t test_udp_datagram_parse"));
    assert!(header.contains("wirespec_result_t test_udp_datagram_serialize"));
}

#[test]
fn codegen_simple_packet_source() {
    let (_, source) = generate_c("packet P { x: u8, y: u16 }");
    assert!(source.contains("wirespec_cursor_read_u8"));
    assert!(source.contains("wirespec_cursor_read_u16be"));
    assert!(source.contains("wirespec_write_u8"));
    assert!(source.contains("wirespec_write_u16be"));
}

#[test]
fn codegen_packet_with_require() {
    let (_, source) = generate_c("packet P { length: u16, require length >= 8 }");
    assert!(source.contains("WIRESPEC_ERR_CONSTRAINT"));
}

#[test]
fn codegen_packet_with_optional() {
    let (header, source) = generate_c("packet P { flags: u8, extra: if flags & 0x01 { u16 } }");
    assert!(header.contains("bool has_extra"));
    assert!(source.contains("has_extra"));
}

#[test]
fn codegen_bytes_field() {
    let (header, _) = generate_c("packet P { data: bytes[remaining] }");
    assert!(header.contains("wirespec_bytes_t data"));
}

#[test]
fn codegen_array_field() {
    let (header, source) = generate_c("packet P { count: u8, items: [u8; count] }");
    assert!(header.contains("uint8_t items["));
    assert!(header.contains("uint32_t items_count"));
    assert!(source.contains("for"));
}

#[test]
fn codegen_bitgroup() {
    let (_, source) = generate_c("packet P { a: bits[4], b: bits[4], c: u16 }");
    assert!(source.contains(">>"));   // shift for extract
    assert!(source.contains("& 0xf")); // mask for 4-bit field
}

#[test]
fn codegen_frame() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A {},
            0x01 => B { x: u8 },
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    let (header, source) = generate_c(src);
    assert!(header.contains("tag_t")); // tag enum type
    assert!(header.contains("union")); // union for variants
    assert!(source.contains("switch"));
}

#[test]
fn codegen_capsule() {
    let src = r#"
        capsule C {
            type_field: u8,
            length: u16,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                _ => U { data: bytes[remaining] },
            },
        }
    "#;
    let (_, source) = generate_c(src);
    assert!(source.contains("wirespec_cursor_sub")); // sub-cursor for within
}

#[test]
fn codegen_derived_field() {
    let (header, _) = generate_c("packet P { flags: u8, let is_set: bool = (flags & 1) != 0 }");
    assert!(header.contains("bool is_set"));
}

#[test]
fn codegen_enum() {
    let (header, _) = generate_c("enum E: u8 { A = 0, B = 1 }");
    assert!(header.contains("typedef enum"));
}

#[test]
fn codegen_artifact_emission() {
    let ast = wirespec_syntax::parse("packet P { x: u8 }").unwrap();
    let sem = wirespec_sema::analyze(&ast, ComplianceProfile::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = CBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: TargetOptions::C(CBackendOptions::default()),
        checksum_bindings: Arc::new(wirespec_backend_c::checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };

    let mut sink = MemorySink::new();
    let output = backend.lower_and_emit(&codec, &ctx, &mut sink).unwrap();
    assert_eq!(output.artifacts.len(), 2); // .h + .c
    assert_eq!(sink.artifacts.len(), 2);
    assert!(output.artifacts[0].relative_path.to_string_lossy().ends_with(".h"));
    assert!(output.artifacts[1].relative_path.to_string_lossy().ends_with(".c"));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p wirespec-backend-c`
Expected: PASS

- [ ] **Step 3: Commit**

---

## Summary

| Chunk | Tasks | What it delivers |
|-------|-------|-----------------|
| 1 | Tasks 1-2 | C naming conventions + type mapping |
| 2 | Tasks 3-4 | Expression codegen + checksum bindings |
| 3 | Task 5 | .h header emission |
| 4 | Tasks 6-7 | Parse + serialize body generation |
| 5 | Tasks 8-9 | Source file assembly + Backend trait impl |
| 6 | Task 10 | Integration tests (12 codegen + 1 artifact) |

**Total test count target:** ~20 tests (naming: 5, codegen: 13)

**Scope notes:**
- State machine C codegen is deferred (complex dispatch/transition functions)
- Fuzz harness generation is deferred
- VarInt parse/serialize uses runtime helper calls (not inline expansion)
- Focus is on producing compilable `.h`/`.c` that match the Python reference output structure
