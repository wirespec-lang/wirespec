# Adding a New Backend to wirespec

This guide explains how to add a new code generation backend (e.g., Go, Swift, Zig) to the wirespec compiler.

## Architecture Overview

```
.wspec source
  → wirespec-syntax (parse)       → AST
  → wirespec-sema (analyze)       → Semantic IR
  → wirespec-layout (lower)       → Layout IR
  → wirespec-codec (lower)        → Codec IR  ← your backend consumes this
  → wirespec-backend-XXX (lower)  → target code (.go, .swift, etc.)
```

All backends consume `CodecModule` from `wirespec-codec`. You do NOT need to modify any upstream crate (syntax, sema, layout, codec).

## What You Need to Create

A single new crate: `crates/wirespec-backend-xxx/`

### 1. Cargo.toml

```toml
[package]
name = "wirespec-backend-xxx"
version.workspace = true
edition.workspace = true

[dependencies]
wirespec-backend-api = { path = "../wirespec-backend-api" }
wirespec-codec = { path = "../wirespec-codec" }
wirespec-sema = { path = "../wirespec-sema" }  # for Endianness, SemanticVarInt, etc.
```

### 2. Implement the `Backend` Trait

```rust
use wirespec_backend_api::*;
use wirespec_codec::CodecModule;

pub const TARGET_XXX: TargetId = TargetId("xxx");

pub struct XxxBackendOptions {
    // target-specific options
}

impl Default for XxxBackendOptions {
    fn default() -> Self { Self {} }
}

pub struct XxxBackend;

impl Backend for XxxBackend {
    type LoweredModule = XxxLoweredModule;

    fn id(&self) -> TargetId { TARGET_XXX }

    fn lower(
        &self,
        module: &CodecModule,
        ctx: &BackendContext,
    ) -> Result<Self::LoweredModule, BackendError> {
        // Downcast target options
        let _opts = ctx.target_options.downcast_ref::<XxxBackendOptions>()
            .ok_or_else(|| BackendError::UnsupportedOption {
                target: TARGET_XXX,
                option: "target_options".into(),
                reason: "expected XxxBackendOptions".into(),
            })?;

        // Generate code from CodecModule
        let source = emit_source(module, &ctx.module_prefix);
        Ok(XxxLoweredModule { source, prefix: ctx.module_prefix.clone() })
    }

    fn emit(
        &self,
        lowered: &Self::LoweredModule,
        sink: &mut dyn ArtifactSink,
    ) -> Result<BackendOutput, BackendError> {
        sink.write(Artifact {
            target: TARGET_XXX,
            kind: ArtifactKind("xxx-source"),
            module_name: lowered.prefix.clone(),
            module_prefix: lowered.prefix.clone(),
            relative_path: format!("{}.xxx", lowered.prefix).into(),
            contents: lowered.source.as_bytes().to_vec(),
        })?;
        Ok(BackendOutput {
            target: TARGET_XXX,
            artifacts: vec![ArtifactMeta {
                kind: ArtifactKind("xxx-source"),
                relative_path: format!("{}.xxx", lowered.prefix).into(),
                byte_len: lowered.source.len(),
            }],
        })
    }
}

// Required for registry dispatch
impl BackendDyn for XxxBackend {
    fn id(&self) -> TargetId { TARGET_XXX }
    fn lower_and_emit(
        &self, module: &CodecModule, ctx: &BackendContext, sink: &mut dyn ArtifactSink,
    ) -> Result<BackendOutput, BackendError> {
        let lowered = Backend::lower(self, module, ctx)?;
        Backend::emit(self, &lowered, sink)
    }
}

pub struct XxxLoweredModule {
    pub source: String,
    pub prefix: String,
}
```

### 3. Implement Checksum Bindings (if your target has checksum support)

```rust
use wirespec_backend_api::*;

pub struct XxxChecksumBindings;

impl ChecksumBindingProvider for XxxChecksumBindings {
    fn binding_for(&self, algorithm: &str) -> Result<ChecksumBackendBinding, BackendError> {
        match algorithm {
            "internet" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("xxx_internet_checksum_verify".into()),
                compute_symbol: "xxx_internet_checksum_compute".into(),
                compute_style: ComputeStyle::PatchInPlace,
            }),
            _ => Err(BackendError::MissingChecksumBinding {
                target: TARGET_XXX,
                algorithm: algorithm.to_string(),
            }),
        }
    }
}
```

### 4. Code Generation

Your backend consumes `CodecModule` which contains everything needed:

| Field | What it provides |
|-------|-----------------|
| `module.packets` | Packet definitions with fields, items, checksum plans |
| `module.frames` | Tagged union definitions with variant scopes |
| `module.capsules` | TLV containers with header + payload variants |
| `module.varints` | VarInt definitions (prefix-match and continuation-bit) |
| `module.consts` | Named constants |
| `module.enums` | Enum/flags definitions |
| `module.state_machines` | State machine definitions |

Each `CodecField` has:
- `strategy` — how to parse/serialize (Primitive, VarInt, BytesLength, Array, BitGroup, etc.)
- `wire_type` — the wire-level type
- `endianness` — byte order
- `is_optional` / `condition` — conditional field info
- `bytes_spec` / `array_spec` / `bitgroup_member` — strategy-specific details
- `checksum_algorithm` — if this field has a checksum annotation

See `crates/wirespec-codec/src/ir.rs` for the complete `CodecModule` schema.

### 5. Register with the CLI

Add your crate to the workspace:

```toml
# crates/Cargo.toml
[workspace]
members = [
    # ... existing members ...
    "wirespec-backend-xxx",
]
```

Add the dependency and register the factory in the CLI binary:

```rust
// crates/wirespec-driver/src/bin/wirespec.rs

struct XxxBackendFactory;
impl BackendFactory for XxxBackendFactory {
    fn id(&self) -> TargetId { wirespec_backend_xxx::TARGET_XXX }
    fn create(&self) -> Box<dyn BackendDyn> { Box::new(wirespec_backend_xxx::XxxBackend) }
    fn default_options(&self) -> Box<dyn std::any::Any + Send + Sync> {
        Box::new(wirespec_backend_xxx::XxxBackendOptions::default())
    }
}

fn build_registry() -> BackendRegistry {
    let mut reg = BackendRegistry::new();
    reg.register(Box::new(CBackendFactory));
    reg.register(Box::new(RustBackendFactory));
    reg.register(Box::new(XxxBackendFactory));  // ← add this line
    reg
}
```

Then `wirespec compile input.wspec -t xxx` works.

## Testing

### Unit tests

Test your code generation output:

```rust
fn generate_xxx(src: &str) -> String {
    let ast = wirespec_syntax::parse(src).unwrap();
    let sem = wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    let backend = XxxBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(XxxBackendOptions::default()),
        checksum_bindings: Arc::new(XxxChecksumBindings),
        is_entry_module: true,
    };
    let lowered = Backend::lower(&backend, &codec, &ctx).unwrap();
    lowered.source
}

#[test]
fn codegen_simple_packet() {
    let src = generate_xxx("packet P { x: u8, y: u16 }");
    assert!(src.contains(/* expected pattern */));
}
```

### Using `MemorySink` for artifact tests

```rust
let mut sink = MemorySink::new();
backend.lower_and_emit(&codec, &ctx, &mut sink).unwrap();
assert_eq!(sink.artifacts.len(), 1);
```

## What You Do NOT Need to Modify

- `wirespec-syntax` — parser / AST
- `wirespec-sema` — semantic analysis
- `wirespec-layout` — layout IR
- `wirespec-codec` — codec IR
- `wirespec-backend-api` — backend traits (TargetId, ArtifactKind, etc. are all open)
- `wirespec-driver` — driver library (only the CLI binary needs the factory registration)

## Reference Backends

- `crates/wirespec-backend-c/` — C backend (header + source split, bitgroup shift/mask, checksum verify/compute)
- `crates/wirespec-backend-rust/` — Rust backend (single .rs file, lifetime tracking, Rust enums for frames)
