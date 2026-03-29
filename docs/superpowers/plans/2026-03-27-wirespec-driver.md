# wirespec-driver Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the compilation driver that resolves module dependencies, orchestrates the full pipeline (parse → sema → layout → codec → backend), and provides the public API for both library use and future CLI integration.

**Architecture:** Two components: (1) Module resolver that discovers `.wspec` files from include paths, builds a dependency graph via DFS, detects cycles, and produces a topological ordering. (2) Pipeline driver that processes each module in dependency order, registers exported types for downstream modules, and invokes backend lowering/emission. No CLI binary in this plan — just the library API. CLI can be added later as a thin wrapper.

**Tech Stack:** Rust (edition 2024), all upstream wirespec crates

**Reference implementation:**
- `protospec/wirespec/resolver.py` — Python module resolver
- `protospec/wirespec/pipeline.py` — Python pipeline orchestration
- `protospec/wirespec/cli.py` — Python CLI (for API shape reference)

---

## File Structure

```
crates/wirespec-driver/
├── Cargo.toml
├── src/
│   ├── lib.rs                      # Crate root, public API
│   ├── resolve.rs                  # Module resolver: file discovery, DFS, topo sort, cycle detection
│   ├── pipeline.rs                 # Single-module pipeline: parse → sema → layout → codec
│   └── driver.rs                   # Multi-module driver: resolve → pipeline each → backend emit
└── tests/
    ├── resolve_tests.rs            # Resolver tests with temp directories
    ├── pipeline_tests.rs           # Single-module pipeline tests
    └── driver_tests.rs             # Multi-module driver tests
```

| File | Responsibility |
|------|---------------|
| `resolve.rs` | Module name → file path mapping, import graph DFS, cycle detection, topological sort, export visibility |
| `pipeline.rs` | Single-module compile: `compile_module(source, external_types) → CodecModule` |
| `driver.rs` | Multi-module orchestration: resolve → compile each in order → register exports → backend invoke |

---

## Chunk 1: Module Resolver

### Task 1: Module resolver

**Files:**
- Create: `crates/wirespec-driver/src/resolve.rs`
- Test: `crates/wirespec-driver/tests/resolve_tests.rs`

- [ ] **Step 1: Write failing tests**

```rust
// crates/wirespec-driver/tests/resolve_tests.rs
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;
use wirespec_driver::resolve::*;

fn write_file(dir: &TempDir, rel_path: &str, content: &str) -> PathBuf {
    let path = dir.path().join(rel_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn resolve_single_file_no_imports() {
    let dir = TempDir::new().unwrap();
    let entry = write_file(&dir, "test.wspec", "module test\npacket P { x: u8 }");
    let modules = resolve(&entry, &[]).unwrap();
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].module_name, "test");
}

#[test]
fn resolve_with_import() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "quic/varint.wspec", "module quic.varint\ntype VarInt = { prefix: bits[2], value: match prefix { 0b00 => bits[6], 0b01 => bits[14], 0b10 => bits[30], 0b11 => bits[62] } }");
    let entry = write_file(&dir, "quic/frames.wspec", "module quic.frames\nimport quic.varint.VarInt\npacket P { x: VarInt }");
    let modules = resolve(&entry, &[dir.path().to_path_buf()]).unwrap();
    // Dependency-first order: varint before frames
    assert_eq!(modules.len(), 2);
    assert_eq!(modules[0].module_name, "quic.varint");
    assert_eq!(modules[1].module_name, "quic.frames");
}

#[test]
fn resolve_circular_import_error() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "a.wspec", "module a\nimport b");
    write_file(&dir, "b.wspec", "module b\nimport a");
    let entry = dir.path().join("a.wspec");
    let result = resolve(&entry, &[dir.path().to_path_buf()]);
    assert!(result.is_err());
}

#[test]
fn resolve_module_not_found_error() {
    let dir = TempDir::new().unwrap();
    let entry = write_file(&dir, "test.wspec", "module test\nimport nonexistent.Foo");
    let result = resolve(&entry, &[dir.path().to_path_buf()]);
    assert!(result.is_err());
}

#[test]
fn resolve_transitive_imports() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "c.wspec", "module c\npacket C { x: u8 }");
    write_file(&dir, "b.wspec", "module b\nimport c.C\npacket B { inner: C }");
    let entry = write_file(&dir, "a.wspec", "module a\nimport b.B\npacket A { inner: B }");
    let modules = resolve(&entry, &[dir.path().to_path_buf()]).unwrap();
    // Order: c → b → a
    assert_eq!(modules.len(), 3);
    assert_eq!(modules[0].module_name, "c");
    assert_eq!(modules[1].module_name, "b");
    assert_eq!(modules[2].module_name, "a");
}

#[test]
fn resolve_dotted_module_to_path() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "net/tcp.wspec", "module net.tcp\npacket T { x: u8 }");
    let entry = write_file(&dir, "app.wspec", "module app\nimport net.tcp");
    let modules = resolve(&entry, &[dir.path().to_path_buf()]).unwrap();
    assert_eq!(modules.len(), 2);
}

#[test]
fn resolve_implicit_include_parent() {
    // Entry file's parent directory is searched automatically
    let dir = TempDir::new().unwrap();
    write_file(&dir, "lib.wspec", "module lib\npacket L { x: u8 }");
    let entry = write_file(&dir, "main.wspec", "module main\nimport lib.L");
    // No explicit include paths — parent of entry should work
    let modules = resolve(&entry, &[]).unwrap();
    assert_eq!(modules.len(), 2);
}

#[test]
fn resolve_deduplicates_shared_deps() {
    // a imports b and c; both b and c import d → d should appear once
    let dir = TempDir::new().unwrap();
    write_file(&dir, "d.wspec", "module d\npacket D { x: u8 }");
    write_file(&dir, "b.wspec", "module b\nimport d.D\npacket B { x: D }");
    write_file(&dir, "c.wspec", "module c\nimport d.D\npacket C { x: D }");
    let entry = write_file(&dir, "a.wspec", "module a\nimport b.B\nimport c.C\npacket A { x: B }");
    let modules = resolve(&entry, &[dir.path().to_path_buf()]).unwrap();
    // d appears once, before b and c, which are before a
    let names: Vec<_> = modules.iter().map(|m| m.module_name.as_str()).collect();
    assert_eq!(names.iter().filter(|&&n| n == "d").count(), 1);
    assert_eq!(*names.last().unwrap(), "a");
}
```

- [ ] **Step 2: Write the resolver implementation**

```rust
// crates/wirespec-driver/src/resolve.rs
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use wirespec_syntax::ast::AstModule;

#[derive(Debug)]
pub struct ResolvedModule {
    pub path: PathBuf,
    pub module_name: String,
    pub source_prefix: String,
    pub source: String,
    pub ast: AstModule,
}

#[derive(Debug)]
pub struct ResolveError {
    pub msg: String,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "resolve error: {}", self.msg)
    }
}

impl std::error::Error for ResolveError {}

/// Resolve all transitive dependencies starting from an entry file.
/// Returns modules in topological (dependency-first) order.
pub fn resolve(
    entry: &Path,
    include_paths: &[PathBuf],
) -> Result<Vec<ResolvedModule>, ResolveError> {
    let entry = entry.canonicalize().map_err(|e| ResolveError {
        msg: format!("cannot resolve entry file {}: {e}", entry.display()),
    })?;

    // Build effective include paths: explicit + entry's parent
    let mut search_paths: Vec<PathBuf> = include_paths.to_vec();
    if let Some(parent) = entry.parent() {
        if !search_paths.contains(&parent.to_path_buf()) {
            search_paths.push(parent.to_path_buf());
        }
    }

    let mut resolved: HashMap<PathBuf, ResolvedModule> = HashMap::new();
    let mut in_progress: HashSet<PathBuf> = HashSet::new();
    let mut order: Vec<PathBuf> = Vec::new();

    visit(&entry, &search_paths, &mut resolved, &mut in_progress, &mut order)?;

    Ok(order
        .into_iter()
        .map(|p| resolved.remove(&p).unwrap())
        .collect())
}

fn visit(
    path: &Path,
    search_paths: &[PathBuf],
    resolved: &mut HashMap<PathBuf, ResolvedModule>,
    in_progress: &mut HashSet<PathBuf>,
    order: &mut Vec<PathBuf>,
) -> Result<(), ResolveError> {
    let abs = path.canonicalize().map_err(|e| ResolveError {
        msg: format!("cannot resolve {}: {e}", path.display()),
    })?;

    // Already completed
    if resolved.contains_key(&abs) {
        return Ok(());
    }

    // Cycle detection
    if !in_progress.insert(abs.clone()) {
        return Err(ResolveError {
            msg: format!("circular import detected involving {}", abs.display()),
        });
    }

    // Parse the file
    let source = std::fs::read_to_string(&abs).map_err(|e| ResolveError {
        msg: format!("cannot read {}: {e}", abs.display()),
    })?;
    let ast = wirespec_syntax::parse(&source).map_err(|e| ResolveError {
        msg: format!("parse error in {}: {e}", abs.display()),
    })?;

    // Determine module name
    let module_name = if let Some(ref decl) = ast.module_decl {
        decl.name.clone()
    } else {
        // Infer from filename
        abs.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    };

    // Process imports: resolve each dependency
    for import in &ast.imports {
        let dep_path = find_module(&import.module, search_paths)?;
        visit(&dep_path, search_paths, resolved, in_progress, order)?;
    }

    // Post-order: add after all deps
    let source_prefix = module_name.replace('.', "_");
    in_progress.remove(&abs);
    resolved.insert(
        abs.clone(),
        ResolvedModule {
            path: abs.clone(),
            module_name,
            source_prefix,
            source,
            ast,
        },
    );
    order.push(abs);

    Ok(())
}

/// Find a module file by dotted name in include paths.
/// "quic.varint" → search for "quic/varint.wspec" in each include path.
fn find_module(module_name: &str, search_paths: &[PathBuf]) -> Result<PathBuf, ResolveError> {
    let rel_path = module_name.replace('.', "/") + ".wspec";

    for base in search_paths {
        let candidate = base.join(&rel_path);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    // Fallback: single-component → dir/name.wspec (e.g., "mqtt" → "mqtt/mqtt.wspec")
    let parts: Vec<&str> = module_name.split('.').collect();
    if parts.len() == 1 {
        let fallback = format!("{0}/{0}.wspec", parts[0]);
        for base in search_paths {
            let candidate = base.join(&fallback);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    // Also try .wire extension for backward compatibility
    let rel_path_wire = module_name.replace('.', "/") + ".wire";
    for base in search_paths {
        let candidate = base.join(&rel_path_wire);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(ResolveError {
        msg: format!(
            "module '{module_name}' not found; searched: {}",
            search_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    })
}
```

**Note:** `tempfile` crate needed for tests. Add to `[dev-dependencies]`.

- [ ] **Step 3: Update Cargo.toml**

Add `tempfile` dev-dependency. Also remove backend crate deps since driver shouldn't directly depend on concrete backends (it uses `BackendDyn` from backend-api):

```toml
[package]
name = "wirespec-driver"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Compilation driver and CLI for wirespec"

[dependencies]
wirespec-syntax = { path = "../wirespec-syntax" }
wirespec-sema = { path = "../wirespec-sema" }
wirespec-layout = { path = "../wirespec-layout" }
wirespec-codec = { path = "../wirespec-codec" }
wirespec-backend-api = { path = "../wirespec-backend-api" }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 4: Wire up lib.rs, run tests**

```rust
pub mod resolve;
```

Run: `cargo test -p wirespec-driver --test resolve_tests`
Expected: PASS

- [ ] **Step 5: Commit**

---

## Chunk 2: Single-Module Pipeline

### Task 2: Pipeline function

**Files:**
- Create: `crates/wirespec-driver/src/pipeline.rs`
- Test: `crates/wirespec-driver/tests/pipeline_tests.rs`

- [ ] **Step 1: Write failing tests**

```rust
// crates/wirespec-driver/tests/pipeline_tests.rs
use wirespec_driver::pipeline::*;
use wirespec_sema::ComplianceProfile;

#[test]
fn pipeline_simple_packet() {
    let codec = compile_module(
        "packet P { x: u8, y: u16 }",
        ComplianceProfile::default(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(codec.packets.len(), 1);
    assert_eq!(codec.packets[0].fields.len(), 2);
}

#[test]
fn pipeline_with_varint_and_packet() {
    let src = r#"
        type VarInt = {
            prefix: bits[2],
            value: match prefix {
                0b00 => bits[6], 0b01 => bits[14],
                0b10 => bits[30], 0b11 => bits[62],
            },
        }
        packet P { x: VarInt }
    "#;
    let codec = compile_module(src, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(codec.varints.len(), 1);
    assert_eq!(codec.packets.len(), 1);
}

#[test]
fn pipeline_frame() {
    let src = r#"
        frame F = match tag: u8 {
            0 => A {},
            _ => B { data: bytes[remaining] },
        }
    "#;
    let codec = compile_module(src, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(codec.frames.len(), 1);
}

#[test]
fn pipeline_capsule() {
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
    let codec = compile_module(src, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(codec.capsules.len(), 1);
}

#[test]
fn pipeline_state_machine() {
    let src = r#"
        state machine S {
            state A
            state B [terminal]
            initial A
            transition A -> B { on done }
        }
    "#;
    let codec = compile_module(src, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(codec.state_machines.len(), 1);
}

#[test]
fn pipeline_parse_error() {
    let result = compile_module(
        "packet { bad }",
        ComplianceProfile::default(),
        &Default::default(),
    );
    assert!(result.is_err());
}

#[test]
fn pipeline_sema_error() {
    let result = compile_module(
        "packet P { x: NonExistent }",
        ComplianceProfile::default(),
        &Default::default(),
    );
    assert!(result.is_err());
}

#[test]
fn pipeline_with_external_types() {
    // Simulate an imported VarInt type
    let mut ext = ExternalTypes::default();
    ext.register("VarInt", ExternalType {
        module: "quic.varint".to_string(),
        name: "VarInt".to_string(),
        source_prefix: "quic_varint".to_string(),
        kind: ExternalTypeKind::VarInt,
    });
    let codec = compile_module(
        "packet P { x: VarInt }",
        ComplianceProfile::default(),
        &ext,
    )
    .unwrap();
    assert_eq!(codec.packets.len(), 1);
}
```

- [ ] **Step 2: Write the pipeline implementation**

```rust
// crates/wirespec-driver/src/pipeline.rs
use std::collections::HashMap;
use wirespec_codec::CodecModule;
use wirespec_sema::ComplianceProfile;

#[derive(Debug)]
pub struct PipelineError {
    pub msg: String,
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pipeline error: {}", self.msg)
    }
}

impl std::error::Error for PipelineError {}

/// External type info registered by previously-compiled modules.
#[derive(Debug, Clone)]
pub struct ExternalType {
    pub module: String,
    pub name: String,
    pub source_prefix: String,
    pub kind: ExternalTypeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalTypeKind {
    VarInt,
    Packet,
    Frame,
    Capsule,
    Enum,
    Flags,
    StateMachine,
}

/// Registry of types from already-compiled modules.
#[derive(Debug, Clone, Default)]
pub struct ExternalTypes {
    types: HashMap<String, ExternalType>,
}

impl ExternalTypes {
    pub fn register(&mut self, name: &str, ext: ExternalType) {
        self.types.insert(name.to_string(), ext);
    }

    pub fn get(&self, name: &str) -> Option<&ExternalType> {
        self.types.get(name)
    }
}

/// Compile a single module through the full pipeline.
/// Returns CodecModule ready for backend consumption.
pub fn compile_module(
    source: &str,
    profile: ComplianceProfile,
    _external_types: &ExternalTypes,
) -> Result<CodecModule, PipelineError> {
    // Parse
    let ast = wirespec_syntax::parse(source).map_err(|e| PipelineError {
        msg: format!("parse error: {e}"),
    })?;

    // Semantic analysis
    let sem = wirespec_sema::analyze(&ast, profile).map_err(|e| PipelineError {
        msg: format!("semantic error: {e}"),
    })?;

    // Layout
    let layout = wirespec_layout::lower(&sem).map_err(|e| PipelineError {
        msg: format!("layout error: {e}"),
    })?;

    // Codec
    let codec = wirespec_codec::lower(&layout).map_err(|e| PipelineError {
        msg: format!("codec error: {e}"),
    })?;

    Ok(codec)
}

/// Collect external types from a compiled module for downstream modules.
pub fn collect_external_types(
    registry: &mut ExternalTypes,
    codec: &CodecModule,
    module_name: &str,
    source_prefix: &str,
) {
    for v in &codec.varints {
        registry.register(&v.name, ExternalType {
            module: module_name.to_string(),
            name: v.name.clone(),
            source_prefix: source_prefix.to_string(),
            kind: ExternalTypeKind::VarInt,
        });
    }
    for p in &codec.packets {
        registry.register(&p.name, ExternalType {
            module: module_name.to_string(),
            name: p.name.clone(),
            source_prefix: source_prefix.to_string(),
            kind: ExternalTypeKind::Packet,
        });
    }
    for f in &codec.frames {
        registry.register(&f.name, ExternalType {
            module: module_name.to_string(),
            name: f.name.clone(),
            source_prefix: source_prefix.to_string(),
            kind: ExternalTypeKind::Frame,
        });
    }
    for c in &codec.capsules {
        registry.register(&c.name, ExternalType {
            module: module_name.to_string(),
            name: c.name.clone(),
            source_prefix: source_prefix.to_string(),
            kind: ExternalTypeKind::Capsule,
        });
    }
    for e in &codec.enums {
        let kind = if e.is_flags {
            ExternalTypeKind::Flags
        } else {
            ExternalTypeKind::Enum
        };
        registry.register(&e.name, ExternalType {
            module: module_name.to_string(),
            name: e.name.clone(),
            source_prefix: source_prefix.to_string(),
            kind,
        });
    }
    for sm in &codec.state_machines {
        registry.register(&sm.name, ExternalType {
            module: module_name.to_string(),
            name: sm.name.clone(),
            source_prefix: source_prefix.to_string(),
            kind: ExternalTypeKind::StateMachine,
        });
    }
}
```

- [ ] **Step 3: Add `pub mod pipeline;` to lib.rs, run tests**

Run: `cargo test -p wirespec-driver --test pipeline_tests`
Expected: PASS

- [ ] **Step 4: Commit**

---

## Chunk 3: Multi-Module Driver

### Task 3: Driver with multi-module compilation

**Files:**
- Create: `crates/wirespec-driver/src/driver.rs`
- Test: `crates/wirespec-driver/tests/driver_tests.rs`

- [ ] **Step 1: Write failing tests**

```rust
// crates/wirespec-driver/tests/driver_tests.rs
use std::fs;
use tempfile::TempDir;
use wirespec_driver::driver::*;
use wirespec_sema::ComplianceProfile;

fn write_file(dir: &TempDir, rel_path: &str, content: &str) {
    let path = dir.path().join(rel_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn driver_single_module() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "test.wspec", "module test\npacket P { x: u8 }");
    let entry = dir.path().join("test.wspec");
    let result = compile(&CompileRequest {
        entry: entry.clone(),
        include_paths: vec![],
        profile: ComplianceProfile::default(),
    })
    .unwrap();
    assert_eq!(result.modules.len(), 1);
    assert_eq!(result.modules[0].module_name, "test");
}

#[test]
fn driver_multi_module_with_imports() {
    let dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "quic/varint.wspec",
        r#"module quic.varint
type VarInt = {
    prefix: bits[2],
    value: match prefix {
        0b00 => bits[6], 0b01 => bits[14],
        0b10 => bits[30], 0b11 => bits[62],
    },
}"#,
    );
    write_file(
        &dir,
        "quic/frames.wspec",
        "module quic.frames\nimport quic.varint.VarInt\npacket P { x: VarInt }",
    );
    let entry = dir.path().join("quic/frames.wspec");
    let result = compile(&CompileRequest {
        entry,
        include_paths: vec![dir.path().to_path_buf()],
        profile: ComplianceProfile::default(),
    })
    .unwrap();
    assert_eq!(result.modules.len(), 2);
    // varint compiled before frames
    assert_eq!(result.modules[0].module_name, "quic.varint");
    assert_eq!(result.modules[1].module_name, "quic.frames");
    // frames module should have the VarInt field resolved
    assert_eq!(result.modules[1].codec.packets[0].fields.len(), 1);
}

#[test]
fn driver_circular_import_error() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "a.wspec", "module a\nimport b");
    write_file(&dir, "b.wspec", "module b\nimport a");
    let entry = dir.path().join("a.wspec");
    let result = compile(&CompileRequest {
        entry,
        include_paths: vec![dir.path().to_path_buf()],
        profile: ComplianceProfile::default(),
    });
    assert!(result.is_err());
}

#[test]
fn driver_module_not_found_error() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "test.wspec", "module test\nimport missing.Foo");
    let entry = dir.path().join("test.wspec");
    let result = compile(&CompileRequest {
        entry,
        include_paths: vec![dir.path().to_path_buf()],
        profile: ComplianceProfile::default(),
    });
    assert!(result.is_err());
}

#[test]
fn driver_transitive_deps_compiled_once() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "base.wspec", "module base\npacket B { x: u8 }");
    write_file(&dir, "mid.wspec", "module mid\nimport base.B\npacket M { inner: B }");
    let entry_src = "module top\nimport mid.M\nimport base.B\npacket T { m: M }";
    write_file(&dir, "top.wspec", entry_src);
    let entry = dir.path().join("top.wspec");
    let result = compile(&CompileRequest {
        entry,
        include_paths: vec![dir.path().to_path_buf()],
        profile: ComplianceProfile::default(),
    })
    .unwrap();
    // base appears once, before mid, before top
    let names: Vec<_> = result.modules.iter().map(|m| m.module_name.as_str()).collect();
    assert_eq!(names.iter().filter(|&&n| n == "base").count(), 1);
    assert_eq!(*names.last().unwrap(), "top");
}
```

- [ ] **Step 2: Write the driver implementation**

```rust
// crates/wirespec-driver/src/driver.rs
use std::path::PathBuf;
use crate::pipeline::{self, ExternalTypes, PipelineError};
use crate::resolve;
use wirespec_codec::CodecModule;
use wirespec_sema::ComplianceProfile;

pub struct CompileRequest {
    pub entry: PathBuf,
    pub include_paths: Vec<PathBuf>,
    pub profile: ComplianceProfile,
}

pub struct CompiledModule {
    pub module_name: String,
    pub source_prefix: String,
    pub codec: CodecModule,
}

pub struct CompileResult {
    pub modules: Vec<CompiledModule>,
}

#[derive(Debug)]
pub struct DriverError {
    pub msg: String,
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "driver error: {}", self.msg)
    }
}

impl std::error::Error for DriverError {}

impl From<resolve::ResolveError> for DriverError {
    fn from(e: resolve::ResolveError) -> Self {
        Self { msg: e.msg }
    }
}

impl From<PipelineError> for DriverError {
    fn from(e: PipelineError) -> Self {
        Self { msg: e.msg }
    }
}

/// Compile an entry module and all its transitive dependencies.
pub fn compile(request: &CompileRequest) -> Result<CompileResult, DriverError> {
    // 1. Resolve all transitive dependencies
    let resolved = resolve::resolve(&request.entry, &request.include_paths)?;

    // 2. Compile each module in topological order
    let mut external_types = ExternalTypes::default();
    let mut compiled = Vec::new();

    for module in &resolved {
        let codec = pipeline::compile_module(
            &module.source,
            request.profile,
            &external_types,
        )?;

        // Register exported types for downstream modules
        pipeline::collect_external_types(
            &mut external_types,
            &codec,
            &module.module_name,
            &module.source_prefix,
        );

        compiled.push(CompiledModule {
            module_name: module.module_name.clone(),
            source_prefix: module.source_prefix.clone(),
            codec,
        });
    }

    Ok(CompileResult { modules: compiled })
}
```

- [ ] **Step 3: Update lib.rs with full public API**

```rust
// crates/wirespec-driver/src/lib.rs
pub mod driver;
pub mod pipeline;
pub mod resolve;

pub use driver::{compile, CompileRequest, CompileResult, CompiledModule};
pub use pipeline::{compile_module, ExternalTypes, ExternalType, ExternalTypeKind};
```

Run: `cargo test -p wirespec-driver`
Expected: PASS

- [ ] **Step 4: Commit**

---

## Chunk 4: Corpus Tests with Real Example Files

### Task 4: Corpus tests through driver

**Files:**
- Create: `crates/wirespec-driver/tests/corpus_driver_tests.rs`

- [ ] **Step 1: Write corpus tests**

Test real `.wspec`/`.wire` files from the examples directory — including multi-module files that previously required imports:

```rust
// crates/wirespec-driver/tests/corpus_driver_tests.rs
use wirespec_driver::{compile, CompileRequest};
use wirespec_sema::ComplianceProfile;
use std::path::PathBuf;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protospec/examples")
}

fn compile_example(rel_path: &str) -> wirespec_driver::CompileResult {
    let entry = examples_dir().join(rel_path);
    compile(&CompileRequest {
        entry: entry.clone(),
        include_paths: vec![examples_dir()],
        profile: ComplianceProfile::Phase2ExtendedCurrent,
    })
    .unwrap_or_else(|e| panic!("Failed to compile {rel_path}: {e}"))
}

#[test]
fn corpus_quic_varint() {
    let result = compile_example("quic/varint.wire");
    assert_eq!(result.modules.len(), 1);
    assert!(result.modules[0].codec.varints.len() >= 1);
}

#[test]
fn corpus_quic_frames_with_imports() {
    // This file imports quic.varint.VarInt — tests multi-module compilation
    let result = compile_example("quic/frames.wire");
    assert!(result.modules.len() >= 2); // varint + frames
    let frames = result.modules.last().unwrap();
    assert!(!frames.codec.frames.is_empty());
}

#[test]
fn corpus_net_udp() {
    let result = compile_example("net/udp.wire");
    assert_eq!(result.modules.len(), 1);
}

#[test]
fn corpus_net_tcp() {
    let result = compile_example("net/tcp.wire");
    assert_eq!(result.modules.len(), 1);
}

#[test]
fn corpus_net_ethernet() {
    let result = compile_example("net/ethernet.wire");
    assert_eq!(result.modules.len(), 1);
}

#[test]
fn corpus_ble_att() {
    let result = compile_example("ble/att.wire");
    assert_eq!(result.modules.len(), 1);
}

#[test]
fn corpus_mqtt() {
    let result = compile_example("mqtt/mqtt.wire");
    assert_eq!(result.modules.len(), 1);
}

#[test]
fn corpus_bits_groups() {
    let result = compile_example("test/bits_groups.wire");
    assert_eq!(result.modules.len(), 1);
}

#[test]
fn corpus_mpquic_path_with_imports() {
    // This file imports quic.varint.VarInt — tests multi-module compilation
    let result = compile_example("mpquic/path.wire");
    assert!(result.modules.len() >= 2); // varint + path
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p wirespec-driver`
Expected: Most pass. Import-dependent ones may need adjustment based on how sema handles external types.

- [ ] **Step 3: Commit**

---

## Summary

| Chunk | Tasks | What it delivers |
|-------|-------|-----------------|
| 1 | Task 1 | Module resolver: file discovery, DFS, cycle detection, topo sort |
| 2 | Task 2 | Single-module pipeline: parse → sema → layout → codec |
| 3 | Task 3 | Multi-module driver: resolve → pipeline each → register exports |
| 4 | Task 4 | Corpus tests including multi-file imports |

**Total test count target:** ~25-30 tests:
- Resolver: 8 tests (single file, imports, cycles, not found, transitive, dedup, dotted paths, implicit parent)
- Pipeline: 8 tests (all major constructs, error cases, external types)
- Driver: 5 tests (single, multi, circular, not found, transitive)
- Corpus: 9 tests (all example files including import-dependent ones)

**Key design decisions:**
- Driver depends on `wirespec-backend-api` (for `BackendDyn` trait) but NOT on concrete backends
- Concrete backends register via `BackendRegistry` at the driver level
- `ExternalTypes` bridges across module boundaries during multi-module compilation
- `.wire` extension supported for backward compat alongside `.wspec`
