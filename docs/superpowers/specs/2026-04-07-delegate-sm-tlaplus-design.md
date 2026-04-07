# Delegate SM TLA+ Support — Design Spec

## Goal

Generate TLA+ specs for state machines containing delegate transitions, enabling formal verification of hierarchical (parent-child) state machine compositions.

## Architecture: Inline Expansion

Child SM states and transitions are inlined directly into the parent SM's TLA+ spec. This matches the semantics of the Rust and C backends, where child SM dispatch is embedded in the parent's transition logic.

## Scope

- **1-level delegation only** — child SM must not itself contain delegates (error if it does)
- Simple delegates: `delegate src.child <- ev`
- Indexed delegates: `delegate src.paths[idx] <- ev`
- `child_state_changed` event with `in_state()` guard
- Bound applies to both parent and child state variables

## Data Flow

```
SemanticStateMachine (parent)
  → resolve child SM via state field's child_sm_name
  → SemanticStateMachine (child)
  → inline child variables, init, transitions into parent TLA+ spec
```

## TLA+ Generation Changes

### Variables

Parent state record gains child state field(s):

```tla
\* Single child field
state_Active == [data: Values, child_state: ChildStates]

\* Array child field (paths[N])
state_Running == [counter: Values, paths_states: Seq(PathStates)]
```

Where `ChildStates` / `PathStates` are the set of child SM state names.

### Initial State

Child fields initialized to child SM's `initial` state:

```tla
Init == state = [tag |-> "Active", data |-> 0, child_state |-> "Idle"]
```

### Delegate Transitions

A delegate transition:
1. Auto-copies src to dst (same as sema rule)
2. Maps event parameter to child event ordinal
3. Applies child SM's matching transition to `child_state`
4. If no matching child transition, the parent transition is disabled (guard fails)

```tla
Transition_Active_Active_child_event(ev) ==
  /\ state.tag = "Active"
  /\ LET child_state == state.child_state
         new_child_state == ChildDispatch(child_state, ev)
     IN new_child_state /= "INVALID"
        /\ state' = [state EXCEPT !.child_state = new_child_state]
```

### Child Dispatch Helper

Generated as a TLA+ operator:

```tla
ChildDispatch(child_state, ev_tag) ==
  CASE child_state = "Idle" /\ ev_tag = 0 -> "Active"
    [] child_state = "Active" /\ ev_tag = 1 -> "Done"
    [] OTHER -> "INVALID"
```

### child_state_changed

When `has_child_state_changed` is true, after a delegate transition changes the child state, an implicit `ChildStateChanged` event fires on the parent:

```tla
Transition_Active_Active_child_event_with_csc(ev) ==
  /\ state.tag = "Active"
  /\ LET old_child == state.child_state
         new_child == ChildDispatch(old_child, ev)
     IN new_child /= "INVALID"
        /\ LET state_after_delegate == [state EXCEPT !.child_state = new_child]
           IN IF old_child /= new_child
              THEN \* Apply child_state_changed transitions
                   ChildStateChangedNext(state_after_delegate)
              ELSE state' = state_after_delegate
```

### in_state() Guard

`in_state(Done)` on a child field becomes a TLA+ predicate:

```tla
/\ state.child_state = "Done"
```

For indexed children:
```tla
/\ state.paths_states[idx] = "Done"
```

### Config

Child state names added to the model's constant sets with bound applied.

## API Change

`generate_tlaplus` signature changes:

```rust
// Before
pub fn generate_tlaplus(sm: &SemanticStateMachine, cli_bound: Option<u32>) -> Result<TlaplusOutput, String>

// After
pub fn generate_tlaplus(
    sm: &SemanticStateMachine,
    all_sms: &[SemanticStateMachine],
    cli_bound: Option<u32>,
) -> Result<TlaplusOutput, String>
```

`all_sms` is needed to resolve child SM definitions by name.

## Error Cases

- Child SM contains delegate → error: "nested delegates not supported"
- Child SM not found in `all_sms` → error: "child state machine '{}' not found"

## Files Changed

| File | Change |
|------|--------|
| `crates/wirespec-backend-tlaplus/src/lib.rs` | Remove delegate rejection, add `all_sms` param, resolve child SM |
| `crates/wirespec-backend-tlaplus/src/emit.rs` | Add child variable/init/transition/dispatch generation |
| `crates/wirespec-driver/src/cli.rs` | Pass `sem.state_machines` to `generate_tlaplus` |
| Tests | Add delegate SM TLA+ generation + verification tests |
