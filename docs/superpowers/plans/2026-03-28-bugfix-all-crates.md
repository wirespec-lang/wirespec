# Bugfix All Crates Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all bugs and spec compliance issues found during the comprehensive code review — across sema, codec, and backend-c crates.

**Architecture:** Per-crate fixes applied in dependency order. Sema fixes first (upstream), then codec (depends on sema), then backend-c (depends on codec). Each fix includes a regression test.

**Tech Stack:** Rust (edition 2024)

**Already fixed (by background agent):** Syntax bugs 12-14 (String literal, named annotation args, annotations on const/enum/flags)

---

## Chunk 1: wirespec-sema Critical Fixes

### Task 1: Wire in dead validation functions

**Files:**
- Modify: `crates/wirespec-sema/src/analyzer.rs`
- Modify: `crates/wirespec-sema/tests/error_tests.rs` (add tests)

**Fixes:**
1. Call `validate_remaining_is_last()` at end of `lower_packet()`, `lower_variant_scope()`
2. Call `validate_single_checksum()` at end of `lower_packet()`, `lower_variant_scope()`
3. Call `validate_checksum_field_type()` in `lower_field()` when checksum annotation is found

**Tests to add:**
```rust
#[test]
fn error_remaining_not_last_in_packet() {
    let ast = parse("packet P { data: bytes[remaining], x: u8 }").unwrap();
    assert!(analyze(&ast, default_profile()).is_err());
}

#[test]
fn error_fill_not_last_in_packet() {
    let ast = parse("packet P { items: [u8; fill], x: u8 }").unwrap();
    assert!(analyze(&ast, default_profile()).is_err());
}

#[test]
fn error_duplicate_checksum() {
    let src = "packet P { @checksum(internet) c1: u16, @checksum(crc32) c2: u32 }";
    assert!(analyze(&parse(src).unwrap(), default_profile()).is_err());
}

#[test]
fn error_checksum_wrong_field_type() {
    let src = "packet P { @checksum(internet) c: u32 }"; // internet requires u16
    assert!(analyze(&parse(src).unwrap(), default_profile()).is_err());
}
```

---

### Task 2: Add endianness to SemanticType::Primitive

**Files:**
- Modify: `crates/wirespec-sema/src/types.rs` — add `endianness: Option<Endianness>` to `Primitive`
- Modify: `crates/wirespec-sema/src/analyzer.rs` — propagate endianness from `resolve_named_type()`
- Modify: `crates/wirespec-layout/src/lower.rs` — use field's endianness when available

**Tests:**
```rust
#[test]
fn endianness_explicit_override() {
    // u16le in @endian big module should resolve to Little
    let ast = parse("@endian big\nmodule test\npacket P { x: u16le }").unwrap();
    let sem = analyze(&ast, default_profile()).unwrap();
    // verify the field type has Little endianness
}
```

---

### Task 3: Fix cyclic alias stack overflow

**Files:**
- Modify: `crates/wirespec-sema/src/resolve.rs` — add cycle detection to `resolve_type_name()`

**Fix:** Add a depth counter or visited set to `resolve_type_name()`:
```rust
pub fn resolve_type_name(&self, name: &str) -> Option<ResolvedType> {
    self.resolve_type_name_inner(name, 0)
}

fn resolve_type_name_inner(&self, name: &str, depth: usize) -> Option<ResolvedType> {
    if depth > 32 { return None; } // cycle guard
    if let Some(target) = self.aliases.get(name) {
        return self.resolve_type_name_inner(target, depth + 1);
    }
    // ... rest unchanged
}
```

**Tests:**
```rust
#[test]
fn error_cyclic_type_alias() {
    let ast = parse("type A = B\ntype B = A\npacket P { x: A }").unwrap();
    assert!(analyze(&ast, default_profile()).is_err());
}
```

---

### Task 4: Fix NameRef bare name (emit proper value IDs)

**Files:**
- Modify: `crates/wirespec-sema/src/analyzer.rs` — in `lower_expr()`, produce scope-qualified field IDs

**Fix:** Pass `scope_id` to `lower_expr()` and construct proper value_id:
```rust
// When resolving a field ref:
if let Some(idx) = declared.iter().position(|d| d == &name) {
    SemanticExpr::ValueRef {
        reference: ValueRef {
            value_id: format!("{scope_id}.field[{idx}]"), // NOT bare name
            kind: ValueRefKind::Field,
        },
    }
}
```

Also: return error for unknown names instead of silent pass-through.

---

### Task 5: Fix @derive annotation processing

**Files:**
- Modify: `crates/wirespec-sema/src/analyzer.rs` — extract @derive from annotations

**Fix:** In `lower_packet`, `lower_frame`, etc., check annotations for `@derive` and populate `derive_traits`:
```rust
fn extract_derive_traits(annotations: &[AstAnnotation]) -> Vec<DeriveTrait> {
    let mut traits = Vec::new();
    for ann in annotations {
        if ann.name == "derive" {
            for arg in &ann.args {
                if let AstAnnotationArg::Identifier(name) = arg {
                    match name.as_str() {
                        "debug" => traits.push(DeriveTrait::Debug),
                        "compare" => traits.push(DeriveTrait::Compare),
                        _ => {}
                    }
                }
            }
        }
    }
    traits
}
```

---

### Task 6: Add reserved identifier checking

**Files:**
- Modify: `crates/wirespec-sema/src/analyzer.rs` — check in `register_all()`

**Fix:** Check each registered name against reserved identifiers:
```rust
const RESERVED: &[&str] = &["bool", "null", "src", "dst", "child_state_changed"];

fn check_reserved(name: &str) -> SemaResult<()> {
    if RESERVED.contains(&name) {
        return Err(SemaError::new(ErrorKind::ReservedIdentifier,
            format!("'{name}' is a reserved identifier")));
    }
    Ok(())
}
```

---

### Task 7: Add duplicate definition detection

**Files:**
- Modify: `crates/wirespec-sema/src/resolve.rs` — `register()` returns error on duplicate

---

### Task 8: Fix lower_derived error swallowing

**Files:**
- Modify: `crates/wirespec-sema/src/analyzer.rs` — propagate errors instead of `unwrap_or`

---

## Chunk 2: wirespec-codec Fixes

### Task 9: Add sm_id/state_id to CodecExpr::InState/StateConstructor/All

**Files:**
- Modify: `crates/wirespec-codec/src/ir.rs` — add `sm_id`, `state_id` fields
- Modify: `crates/wirespec-codec/src/lower.rs` — propagate from SemanticExpr

---

## Chunk 3: wirespec-backend-c Critical Compilation Fixes

### Task 10: Add `(void)r;` suppression

**Files:**
- Modify: `crates/wirespec-backend-c/src/source.rs` — add before return in all parse/serialize functions

---

### Task 11: Fix signed integer pointer type mismatch

**Files:**
- Modify: `crates/wirespec-backend-c/src/parse_emit.rs` — add cast for signed types

---

### Task 12: Add raw tag value field to frame struct

**Files:**
- Modify: `crates/wirespec-backend-c/src/header.rs` — add raw tag field
- Modify: `crates/wirespec-backend-c/src/parse_emit.rs` — store raw tag during parse
- Modify: `crates/wirespec-backend-c/src/serialize_emit.rs` — write raw tag before switch

---

### Task 13: Fix capsule variant cursor usage

**Files:**
- Modify: `crates/wirespec-backend-c/src/parse_emit.rs` — use sub cursor for all strategies

---

### Task 14: Add missing `default:` case for frames without wildcard

**Files:**
- Modify: `crates/wirespec-backend-c/src/parse_emit.rs` — add `default: return WIRESPEC_ERR_INVALID_TAG;`

---

## Summary

| Chunk | Tasks | Fixes |
|-------|-------|-------|
| 1 (sema) | Tasks 1-8 | Validation wiring, endianness, cyclic aliases, NameRef IDs, @derive, reserved ids, duplicates, error swallowing |
| 2 (codec) | Task 9 | sm_id/state_id in CodecExpr |
| 3 (backend-c) | Tasks 10-14 | (void)r, signed types, raw tag, capsule cursor, default case |

**Already fixed:** Syntax bugs 12-14 (String literal, named annotation, annotations on const/enum/flags)
**Deferred:** VarInt/SM/checksum C codegen, cross-module type resolution (these are new features, not bugs)
