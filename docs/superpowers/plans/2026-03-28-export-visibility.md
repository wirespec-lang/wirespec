# Export Visibility Enforcement Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enforce export visibility so that only explicitly exported symbols are importable when a module uses `export` — matching the Python implementation's behavior.

**Architecture:** Enforce at the **resolver level** (driver), not in the IR pipeline. The resolver already parses each module's AST. After parsing, collect the set of importable names using the Python's rule: if any item has `exported=true`, only exported items are importable; if none are exported, all items are importable (backward compat). When resolving an import `import module.Item`, check that `Item` is in the importable set.

**Tech Stack:** Rust, `wirespec-driver` crate only. No IR changes needed.

---

### Task 1: Add export visibility checking to resolver

**Files:**
- Modify: `crates/wirespec-driver/src/resolve.rs`
- Test: `crates/wirespec-driver/tests/resolve_tests.rs`

- [ ] **Step 1: Add `get_exportable_names()` helper**

After parsing a module's AST, extract the set of names that are importable:

```rust
fn get_exportable_names(ast: &AstModule) -> HashSet<String> {
    let mut has_exports = false;
    let mut all_names = Vec::new();
    let mut exported_names = Vec::new();

    for item in &ast.items {
        let (name, exported) = match item {
            AstTopItem::Const(c) => (c.name.clone(), c.exported),
            AstTopItem::Enum(e) => (e.name.clone(), e.exported),
            AstTopItem::Flags(f) => (f.name.clone(), f.exported),
            AstTopItem::Type(t) => (t.name.clone(), t.exported),
            AstTopItem::Packet(p) => (p.name.clone(), p.exported),
            AstTopItem::Frame(f) => (f.name.clone(), f.exported),
            AstTopItem::Capsule(c) => (c.name.clone(), c.exported),
            AstTopItem::ContinuationVarInt(v) => (v.name.clone(), v.exported),
            AstTopItem::StateMachine(s) => (s.name.clone(), s.exported),
            AstTopItem::StaticAssert(_) => continue,
        };
        all_names.push(name.clone());
        if exported {
            has_exports = true;
            exported_names.push(name);
        }
    }

    if has_exports {
        exported_names.into_iter().collect()
    } else {
        all_names.into_iter().collect()
    }
}
```

- [ ] **Step 2: Validate imports against exportable names**

In the `visit()` function, after parsing a dependency module, validate that each imported item name exists in the dependency's exportable names:

```rust
// After parsing dep module and getting its AST:
let dep_exportable = get_exportable_names(&dep_ast);

// For the current module's import that references this dep:
if let Some(item_name) = &import.name {
    if !dep_exportable.contains(item_name) {
        return Err(ResolveError {
            msg: format!("module '{}' does not export '{}'", dep_module_name, item_name),
        });
    }
}
```

This requires tracking which import triggered each dependency visit. Adjust the `visit()` function to validate imports after dependencies are resolved.

- [ ] **Step 3: Add tests**

```rust
#[test]
fn resolve_export_visibility_enforced() {
    let dir = TempDir::new().unwrap();
    // Module with selective exports
    write_file(&dir, "lib.wspec", "module lib\nexport packet Pub { x: u8 }\npacket Priv { y: u16 }");
    // Importing exported item — should work
    let entry = write_file(&dir, "ok.wspec", "module ok\nimport lib.Pub\npacket P { inner: Pub }");
    assert!(resolve(&entry, &[dir.path().to_path_buf()]).is_ok());
}

#[test]
fn resolve_export_visibility_rejected() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "lib.wspec", "module lib\nexport packet Pub { x: u8 }\npacket Priv { y: u16 }");
    // Importing non-exported item — should fail
    let entry = write_file(&dir, "bad.wspec", "module bad\nimport lib.Priv\npacket P { inner: Priv }");
    assert!(resolve(&entry, &[dir.path().to_path_buf()]).is_err());
}

#[test]
fn resolve_no_exports_all_public() {
    let dir = TempDir::new().unwrap();
    // No export keyword — all items importable (backward compat)
    write_file(&dir, "lib.wspec", "module lib\npacket A { x: u8 }\npacket B { y: u16 }");
    let entry = write_file(&dir, "ok.wspec", "module ok\nimport lib.A\nimport lib.B\npacket P { a: A }");
    assert!(resolve(&entry, &[dir.path().to_path_buf()]).is_ok());
}
```

- [ ] **Step 4: Run tests, commit**

Run: `cargo test --workspace`

---

## Summary

Single task, resolver-only change. No IR propagation needed. ~3 tests.
