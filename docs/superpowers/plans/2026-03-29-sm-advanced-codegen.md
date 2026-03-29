# SM Advanced Codegen Plan (M9-M11)

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement advanced state machine code generation (child dispatch, in_state(), fill(), state constructors, all(), +=, slice) in both C and Rust backends, closing the gap with the Python reference.

**Architecture:** Extend the existing SM expression codegen (`sema_expr_to_c` in C backend, `sm_expr_to_rust` in Rust backend) to handle all SM expression types. Add child dispatch with `child_state_changed` re-dispatch in the delegate transition handler. All patterns follow the Python reference exactly.

**Tech Stack:** Rust, `wirespec-backend-c` and `wirespec-backend-rust` crates

---

## Chunk 1: SM Expression Codegen (Both Backends)

### Task 1: C backend — complete SM expression codegen

**Files:** Modify `crates/wirespec-backend-c/src/expr.rs`, `crates/wirespec-backend-c/src/source.rs`

Currently `sema_expr_to_c()` handles basic TransitionPeerRef, Literal, Binary, Unary, Subscript. Add:

- **`InState`**: `({expr_c}.tag == {PREFIX}_{SM}_{STATE})`
- **`StateConstructor`** (no args): `(({child_type}_t){ .tag = {TAG} })`
- **`StateConstructor`** (with args): `(({child_type}_t){ .tag = {TAG}, .{state_snake} = { .f1 = v1, .f2 = v2 } })`
- **`Fill`**: emit as inline — caller handles the for-loop
- **`Slice`**: not directly an expression — used inside `All` collection
- **`All`**: emit as block with bool accumulator loop

For `fill()` and `all()`, these are STATEMENT-level constructs, not pure expressions. They need special handling in the action/guard emission rather than in `sema_expr_to_c`.

**In source.rs** `emit_sm_dispatch()`, extend action emission:
- When action value is `Fill { value, count }` → emit for-loop
- When action value is `StateConstructor` → emit compound literal
- When guard contains `All` → emit accumulator loop block
- Support `+=` operator in actions

### Task 2: Rust backend — complete SM expression codegen

**Files:** Modify `crates/wirespec-backend-rust/src/emit.rs`

Same patterns but Rust syntax:
- **`InState`**: `matches!({expr_rs}, {SmName}::{State} { .. })`
- **`StateConstructor`** (no args): `{SmName}::{State}`
- **`StateConstructor`** (with args): `{SmName}::{State} { f1: v1, f2: v2 }`
- **`Fill`**: `for i in 0..{count} { arr[i] = {value}; }`
- **`All`**: `(start..end).all(|i| matches!(collection[i], ...))`
- **`+=`**: `field += value;`

---

## Chunk 2: Child Dispatch (delegate codegen)

### Task 3: C backend — child SM dispatch in delegate

**Files:** Modify `crates/wirespec-backend-c/src/source.rs`

In `emit_sm_dispatch()`, when a transition has `delegate`:

1. Auto-copy dst from src (already done)
2. Build child event struct from the delegate's event parameter
3. Save child's old tag
4. Call child SM's dispatch function
5. If child tag changed AND parent has `child_state_changed`, re-dispatch parent

```c
// delegate src.paths[path_id] <- event
{
    {child_sm_type}_tag_t _old_tag = dst.{src_state}.{target}[{idx}].tag;
    {child_event_type}_t _child_ev;
    _child_ev.tag = /* map event param */;
    wirespec_result_t _rc = {child_prefix}_dispatch(
        &dst.{src_state}.{target}[{idx}], &_child_ev);
    if (_rc != WIRESPEC_OK) return _rc;

    if (dst.{src_state}.{target}[{idx}].tag != _old_tag) {
        {parent_event_type}_t _csc_ev;
        _csc_ev.tag = {PREFIX}_CHILD_STATE_CHANGED;
        wirespec_result_t _csc_rc = {parent_prefix}_dispatch(sm, &_csc_ev);
        if (_csc_rc != WIRESPEC_OK && _csc_rc != WIRESPEC_ERR_INVALID_STATE)
            return _csc_rc;
    }
}
```

### Task 4: Rust backend — child SM dispatch in delegate

Same logic but Rust:
```rust
// delegate
let old_tag = std::mem::discriminant(&self.{field});
self.{field}.dispatch(&child_event)?;
if std::mem::discriminant(&self.{field}) != old_tag {
    // child_state_changed re-dispatch
    let _ = self.dispatch(&{SmName}Event::ChildStateChanged);
}
```

---

## Chunk 3: Tests

### Task 5: GCC round-trip tests for advanced SM

**Files:** Append to `crates/wirespec-driver/tests/roundtrip_tests.rs`

Test with the PathState example from spec §3.10:

```rust
#[test]
fn roundtrip_sm_state_constructor() {
    // SM with state constructor in action
}

#[test]
fn roundtrip_sm_fill() {
    // SM with fill() in action
}

#[test]
fn roundtrip_sm_in_state_guard() {
    // SM with in_state() in guard
}
```

### Task 6: Codegen output tests

Append to C and Rust backend codegen_tests:
```rust
#[test]
fn codegen_sm_in_state() { ... }
#[test]
fn codegen_sm_state_constructor() { ... }
#[test]
fn codegen_sm_fill() { ... }
#[test]
fn codegen_sm_all_quantifier() { ... }
#[test]
fn codegen_sm_plus_assign() { ... }
```

---

## Summary

| Task | Backend | Features |
|------|---------|----------|
| 1 | C | in_state, state constructor, fill, all, +=, slice expressions |
| 2 | Rust | Same |
| 3 | C | Child dispatch + child_state_changed re-dispatch |
| 4 | Rust | Same |
| 5-6 | Both | Tests |
