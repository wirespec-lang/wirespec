# Backend Codegen Gaps Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement 4 missing codegen features in both C and Rust backends: VarInt functions, checksum verify/compute, const/enum/import emission, and state machine dispatch — closing the gap with the Python reference implementation.

**Architecture:** Each feature adds new emission functions to the existing backend modules. VarInt and SM generate standalone functions/structs. Checksum hooks into the existing parse/serialize wrappers. Const/import adds to header emission. All patterns follow the Python reference `codegen_c.py` / `codegen_rust.py` exactly.

**Tech Stack:** Rust (edition 2024), `wirespec-backend-c` and `wirespec-backend-rust` crates

**Reference:** `protospec/wirespec/codegen_c.py`, `protospec/wirespec/codegen_rust.py`, `protospec/build/*.{c,h}` (generated examples)

---

## File Modifications

| File | Additions |
|------|-----------|
| `backend-c/src/header.rs` | VarInt typedefs+decls, const `#define`, enum `typedef`+`#define`, import `#include`, SM enums+structs+dispatch decl |
| `backend-c/src/source.rs` | VarInt parse/serialize/wire_size functions, checksum verify/compute in packet wrappers, SM dispatch function |
| `backend-c/src/parse_emit.rs` | Checksum offset tracking, verify call after scope |
| `backend-c/src/serialize_emit.rs` | Checksum offset tracking, compute+patch after scope |
| `backend-rust/src/emit.rs` | VarInt parse/serialize/wire_size functions, const declarations, enum type aliases, SM enums+impl |

---

## Chunk 1: VarInt Codegen (C + Rust)

### Task 1: VarInt C code generation

**Files:**
- Modify: `crates/wirespec-backend-c/src/header.rs`
- Modify: `crates/wirespec-backend-c/src/source.rs`
- Test: `crates/wirespec-backend-c/tests/codegen_tests.rs`

- [ ] **Step 1: Add VarInt header emission**

In `header.rs`, add `emit_varint_header()` that generates for each VarInt:
```c
typedef uint64_t {prefix}_{snake_name}_t;

static wirespec_result_t {prefix}_{snake_name}_parse_cursor(
    wirespec_cursor_t *cur, {prefix}_{snake_name}_t *out);
wirespec_result_t {prefix}_{snake_name}_parse(
    const uint8_t *buf, size_t len,
    {prefix}_{snake_name}_t *out, size_t *consumed);
wirespec_result_t {prefix}_{snake_name}_serialize(
    {prefix}_{snake_name}_t val,
    uint8_t *buf, size_t cap, size_t *written);
size_t {prefix}_{snake_name}_wire_size({prefix}_{snake_name}_t val);
```

Call this from `emit_header()` before packet/frame/capsule sections.

- [ ] **Step 2: Add VarInt source emission for prefix-match encoding**

In `source.rs`, add `emit_varint_source()` generating:

**Parse cursor function:** Read first byte, extract prefix bits, switch on prefix value. Each branch reads remaining bytes and assembles the value with shift+OR. If `@strict`, check noncanonical encoding (value fits in smaller branch).

**Public parse wrapper:** Init cursor, call parse_cursor, set consumed.

**Serialize function:** If/else chain from smallest to largest branch. Each branch checks capacity, writes prefix bits + value bytes.

**Wire_size function:** If/else chain returning total_bytes per branch.

- [ ] **Step 3: Add VarInt source emission for continuation-bit encoding**

Similar but different algorithm: loop reading bytes, extract value_bits per byte, check continuation bit. Serialize: emit bytes with continuation bits set, clear on last byte.

- [ ] **Step 4: Add tests**

```rust
#[test]
fn codegen_varint_header() {
    let src = r#"type VarInt = { prefix: bits[2], value: match prefix { 0b00 => bits[6], 0b01 => bits[14], 0b10 => bits[30], 0b11 => bits[62] } }"#;
    let (header, source) = generate_c(src);
    assert!(header.contains("typedef uint64_t"));
    assert!(header.contains("_parse_cursor"));
    assert!(header.contains("_serialize"));
    assert!(header.contains("_wire_size"));
    assert!(source.contains("switch"));
    assert!(source.contains("prefix"));
}

#[test]
fn codegen_cont_varint() {
    let src = r#"type MqttLen = varint { continuation_bit: msb, value_bits: 7, max_bytes: 4, byte_order: little }"#;
    let (header, source) = generate_c(src);
    assert!(header.contains("typedef uint64_t"));
    assert!(source.contains("0x80") || source.contains("0x7f")); // continuation bit mask
}
```

- [ ] **Step 5: Commit**

---

### Task 2: VarInt Rust code generation

**Files:**
- Modify: `crates/wirespec-backend-rust/src/emit.rs`
- Test: `crates/wirespec-backend-rust/tests/codegen_tests.rs`

- [ ] **Step 1: Add VarInt emission**

In `emit.rs`, add `emit_varint()` generating:

```rust
pub fn {prefix}_{snake_name}_parse(cur: &mut Cursor<'_>) -> Result<u64> {
    let first = cur.read_u8()?;
    let prefix = first >> {8 - prefix_bits};
    match prefix {
        {branch_value} => { /* assemble value */ Ok(val) }
        ...
    }
}

pub fn {prefix}_{snake_name}_serialize(val: u64, w: &mut Writer<'_>) -> Result<()> {
    if val <= {max_0} { w.write_u8(val as u8)?; }
    else if val <= {max_1} { /* multi-byte */ }
    ...
    Ok(())
}

pub fn {prefix}_{snake_name}_wire_size(val: u64) -> usize {
    if val <= {max_0} { 1 } else if val <= {max_1} { 2 } ... else { {max_bytes} }
}
```

- [ ] **Step 2: Add tests, commit**

---

## Chunk 2: Checksum Codegen

### Task 3: Checksum C code generation

**Files:**
- Modify: `crates/wirespec-backend-c/src/source.rs`
- Test: `crates/wirespec-backend-c/tests/codegen_tests.rs`

- [ ] **Step 1: Add checksum verify in parse wrapper**

In the public parse function (after `parse_cursor` call), if `checksum_plan` exists:

For `internet`:
```c
{ uint16_t _cksum = wirespec_internet_checksum(buf, *consumed);
  if (_cksum != 0) return WIRESPEC_ERR_CHECKSUM; }
```

For `crc32`/`crc32c`/`fletcher16`:
```c
{ uint32_t _computed = wirespec_{algo}_verify(buf, *consumed, _cksum_offset, {field_width});
  if (out->{field_name} != _computed) return WIRESPEC_ERR_CHECKSUM; }
```

Need to track `_cksum_offset` — the byte offset of the checksum field within the scope. This can be emitted as a local variable that's set when the checksum field is written during serialize, or computed from the field index.

- [ ] **Step 2: Add checksum auto-compute in serialize**

After all fields are written, if `checksum_plan` exists:

For `internet`:
```c
wirespec_internet_checksum_compute(buf, pos, _cksum_offset);
```

For `crc32`/`crc32c` (big-endian patch):
```c
{ uint32_t _crc = wirespec_{algo}_compute(buf, pos, _cksum_offset);
  buf[_cksum_offset]   = (uint8_t)((_crc >> 24) & 0xFF);
  buf[_cksum_offset+1] = (uint8_t)((_crc >> 16) & 0xFF);
  buf[_cksum_offset+2] = (uint8_t)((_crc >> 8) & 0xFF);
  buf[_cksum_offset+3] = (uint8_t)(_crc & 0xFF); }
```

- [ ] **Step 3: Add tests**

```rust
#[test]
fn codegen_checksum_internet_verify() {
    let src = "packet P { data: u32, @checksum(internet) checksum: u16 }";
    let (_, source) = generate_c(src);
    assert!(source.contains("wirespec_internet_checksum"));
    assert!(source.contains("WIRESPEC_ERR_CHECKSUM"));
}

#[test]
fn codegen_checksum_internet_compute() {
    let src = "packet P { data: u32, @checksum(internet) checksum: u16 }";
    let (_, source) = generate_c(src);
    assert!(source.contains("wirespec_internet_checksum_compute"));
}

#[test]
fn codegen_checksum_crc32() {
    let src = "packet P { data: u32, @checksum(crc32) checksum: u32 }";
    let (_, source) = generate_c(src);
    assert!(source.contains("wirespec_crc32_verify") || source.contains("wirespec_crc32_compute"));
}
```

- [ ] **Step 4: Commit**

---

## Chunk 3: Const / Enum / Import Codegen

### Task 4: Const and Enum C header emission

**Files:**
- Modify: `crates/wirespec-backend-c/src/header.rs`
- Test: `crates/wirespec-backend-c/tests/codegen_tests.rs`

- [ ] **Step 1: Add const #define emission**

For each const in `CodecModule.consts`:
```c
#define {PREFIX}_{CONST_NAME} (({c_type}){value})
```

- [ ] **Step 2: Fix enum emission to use typedef + #define pattern**

Replace current C11 enum emission with Python's pattern:
```c
typedef {underlying_c_type} {type_name};
#define {PREFIX}_{ENUM_NAME}_{MEMBER} (({type_name}){value})
```

- [ ] **Step 3: Add import #include emission**

For each import in `CodecModule.imports`:
```c
#include "{source_prefix}.h"
```

- [ ] **Step 4: Add tests, commit**

---

### Task 5: Const and Enum Rust emission

**Files:**
- Modify: `crates/wirespec-backend-rust/src/emit.rs`
- Test: `crates/wirespec-backend-rust/tests/codegen_tests.rs`

- [ ] **Step 1: Add const and enum emission**

```rust
pub const {NAME}: {rust_type} = {value};

pub type {TypeName} = {underlying_rust_type};
pub const {PREFIX}_{MEMBER}: {TypeName} = {value};
```

- [ ] **Step 2: Add tests, commit**

---

## Chunk 4: State Machine Codegen

### Task 6: State Machine C code generation

**Files:**
- Modify: `crates/wirespec-backend-c/src/header.rs`
- Modify: `crates/wirespec-backend-c/src/source.rs`
- Test: `crates/wirespec-backend-c/tests/codegen_tests.rs`

- [ ] **Step 1: Add SM header emission**

Generate for each state machine:
- State tag enum
- State data struct (tagged union with per-state field structs)
- Event tag enum
- Event data struct (tagged union with per-event param structs)
- Dispatch function declaration
- Init helper (static inline)

- [ ] **Step 2: Add SM dispatch function emission**

Generate `{prefix}_{sm}_dispatch()`:
- Switch on current state tag
- For each transition from that state: if/else on event tag
- Guard check → return ERR_INVALID_STATE if fails
- Build dst state, copy fields from src per actions
- `*sm = dst;` to apply transition
- Wildcard transitions as final fallback

- [ ] **Step 3: Add tests**

```rust
#[test]
fn codegen_state_machine_header() {
    let src = r#"
        state machine S {
            state Init { count: u8 = 0 }
            state Done [terminal]
            initial Init
            transition Init -> Done { on finish }
        }
    "#;
    let (header, source) = generate_c(src);
    assert!(header.contains("_tag_t")); // state tag enum
    assert!(header.contains("_dispatch")); // dispatch function
    assert!(source.contains("src_tag")); // dispatch impl
}
```

- [ ] **Step 4: Commit**

---

### Task 7: State Machine Rust code generation

**Files:**
- Modify: `crates/wirespec-backend-rust/src/emit.rs`
- Test: `crates/wirespec-backend-rust/tests/codegen_tests.rs`

- [ ] **Step 1: Add SM emission**

Generate Rust enum for state + event, dispatch method.

- [ ] **Step 2: Add tests, commit**

---

## Summary

| Chunk | Tasks | Feature |
|-------|-------|---------|
| 1 | Tasks 1-2 | VarInt codegen (C + Rust) |
| 2 | Task 3 | Checksum codegen (C) |
| 3 | Tasks 4-5 | Const/Enum/Import codegen (C + Rust) |
| 4 | Tasks 6-7 | State Machine codegen (C + Rust) |

**Test target:** ~15 new tests across both backends
