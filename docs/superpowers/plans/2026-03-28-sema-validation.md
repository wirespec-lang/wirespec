# Sema Validation Strengthening Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire in all remaining semantic validations so the compiler rejects invalid programs per wirespec_spec_v1.0 §3.9-§4.2 — SM exhaustiveness, delegate rules, duplicate transitions, recursive types, and export visibility.

**Architecture:** Add validation functions to `validate.rs` and wire them into `analyzer.rs` at the appropriate points. Each validation is independent and testable. ErrorKind variants already exist for all cases.

**Tech Stack:** Rust (edition 2024), `wirespec-sema` crate only

**Spec references:** §3.9 rules 2/5/6/8, §3.13, §4.2

---

## Chunk 1: State Machine Validations

### Task 1: SM duplicate transition detection

**Files:** Modify `crates/wirespec-sema/src/analyzer.rs`

In `lower_state_machine()`, after building all transitions, check for duplicate `(src_state, event_name)` pairs:
```rust
let mut seen: HashSet<(String, String)> = HashSet::new();
for t in &transitions {
    let key = (t.src_state_name.clone(), t.event_name.clone());
    if !seen.insert(key) {
        return Err(SemaError::new(ErrorKind::SmDuplicateTransition, ...));
    }
}
```

Test:
```rust
#[test]
fn error_sm_duplicate_transition() {
    let src = r#"state machine S { state A state B [terminal] initial A
        transition A -> B { on go } transition A -> B { on go } }"#;
    assert!(analyze(&parse(src).unwrap(), default_profile(), &Default::default()).is_err());
}
```

### Task 2: SM delegate-only-self-transition

In `lower_state_machine()`, for transitions with delegate, check `src_state == dst_state`:
```rust
if t.delegate.is_some() && t.src_state_name != t.dst_state_name {
    return Err(SemaError::new(ErrorKind::SmDelegateNotSelfTransition, ...));
}
```

Test:
```rust
#[test]
fn error_sm_delegate_not_self() {
    let src = r#"state machine S { state A { c: u8 } state B [terminal] initial A
        transition A -> B { on ev(id: u8, e: u8) delegate src.c <- e } }"#;
    assert!(analyze(&parse(src).unwrap(), default_profile(), &Default::default()).is_err());
}
```

### Task 3: SM delegate-with-action mutual exclusion

Check that transitions with delegate don't have actions:
```rust
if t.delegate.is_some() && !t.actions.is_empty() {
    return Err(SemaError::new(ErrorKind::SmDelegateWithAction, ...));
}
```

Test:
```rust
#[test]
fn error_sm_delegate_with_action() {
    let src = r#"state machine S { state A { c: u8 } initial A
        transition A -> A { on ev(id: u8, e: u8) delegate src.c <- e action { dst.c = 0; } } }"#;
    assert!(analyze(&parse(src).unwrap(), default_profile(), &Default::default()).is_err());
}
```

### Task 4: SM invalid initial state

Check that initial_state exists in states list:
```rust
if !states.iter().any(|s| s.name == initial_state) {
    return Err(SemaError::new(ErrorKind::SmInvalidInitial, ...));
}
```

Test:
```rust
#[test]
fn error_sm_invalid_initial() {
    let src = r#"state machine S { state A state B [terminal] initial NonExistent
        transition A -> B { on go } }"#;
    assert!(analyze(&parse(src).unwrap(), default_profile(), &Default::default()).is_err());
}
```

---

## Chunk 2: Type System Validations

### Task 5: Recursive type / alias cycle detection

In `resolve.rs`, the depth guard (max 32) already prevents stack overflow. Now make it return a proper error:

```rust
fn resolve_type_name_inner(&self, name: &str, depth: usize) -> Option<ResolvedType> {
    if depth > 32 { return None; } // caller interprets None as undefined
    ...
}
```

The analyzer already errors on None (undefined type). Add a test to confirm:
```rust
#[test]
fn error_recursive_type_alias() {
    let src = "type A = B\ntype B = C\ntype C = A\npacket P { x: A }";
    assert!(analyze(&parse(src).unwrap(), default_profile(), &Default::default()).is_err());
}
```

### Task 6: Export visibility enforcement

In `analyzer.rs` `register_external_types()`, when processing AST imports, check that imported names are actually exported from their source module. This requires the driver to pass export info along with ExternalTypes.

**Simpler approach for now:** In the driver's `collect_external_types()`, only register exported items (check `exported` field on CodecModule items).

Modify `wirespec-driver/src/pipeline.rs` `collect_external_types()`:
```rust
// Only register exported items (or all if none are exported — backward compat)
let has_exports = codec.packets.iter().any(|p| /* check exported */);
// ... filter accordingly
```

**Problem:** CodecModule doesn't carry `exported` flags. The flag is on the AST and Semantic IR but gets lost during lowering. This requires propagating `exported` through the pipeline.

**Deferred:** This requires changes across sema→layout→codec to propagate `exported`. Mark as future work.

---

## Chunk 3: Additional Validations

### Task 7: bytes[length_or_remaining:] validation

When a field uses `bytes[length_or_remaining: EXPR]`, verify that `EXPR` references an optional field (per spec §3.5).

In `lower_field()`, when the type resolves to `Bytes { kind: LengthOrRemaining, size_expr }`, check the referenced field is optional.

Test:
```rust
#[test]
fn error_lor_non_optional_ref() {
    let src = "packet P { len: u16, data: bytes[length_or_remaining: len] }";
    assert!(analyze(&parse(src).unwrap(), default_profile(), &Default::default()).is_err());
}

#[test]
fn ok_lor_optional_ref() {
    let src = "packet P { flags: u8, len: if flags & 1 { u16 }, data: bytes[length_or_remaining: len] }";
    assert!(analyze(&parse(src).unwrap(), default_profile(), &Default::default()).is_ok());
}
```

### Task 8: Integer-like type validation for array counts and byte lengths

In `resolve_type_expr()`, when resolving `Array` count expressions or `Bytes` length expressions, validate that the referenced field type is integer-like.

This is complex to implement properly (requires type inference on expressions). **Deferred** — the current behavior accepts any expression.

---

## Summary

| Task | Validation | ErrorKind | Status |
|------|-----------|-----------|--------|
| 1 | SM duplicate (src, event) | SmDuplicateTransition | Implement |
| 2 | SM delegate not self-transition | SmDelegateNotSelfTransition | Implement |
| 3 | SM delegate + action mutual exclusion | SmDelegateWithAction | Implement |
| 4 | SM invalid initial state | SmInvalidInitial | Implement |
| 5 | Recursive type alias | (returns None → UndefinedType) | Test only |
| 6 | Export visibility | — | Deferred (needs `exported` propagation) |
| 7 | bytes[length_or_remaining:] optional check | InvalidLengthOrRemaining | Implement |
| 8 | Integer-like type for counts/lengths | — | Deferred |

**Test target:** ~8 new tests
