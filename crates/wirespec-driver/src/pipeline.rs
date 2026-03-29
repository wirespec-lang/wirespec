// crates/wirespec-driver/src/pipeline.rs
//!
//! Single-module compilation pipeline: parse -> sema -> layout -> codec.
//!
//! Processes one module at a time, with an `ExternalTypes` registry
//! for cross-module type information.

use std::collections::HashMap;
use wirespec_codec::CodecModule;
use wirespec_sema::ComplianceProfile;

/// Error during pipeline processing.
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

/// What kind of type an external type is.
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

    pub fn iter(&self) -> impl Iterator<Item = (&String, &ExternalType)> {
        self.types.iter()
    }
}

/// Compile a single module through the full pipeline.
/// Returns a CodecModule ready for backend consumption.
///
/// `external_types` provides cross-module type info from previously-compiled
/// modules. These are converted to sema's format and passed to the analyzer
/// so that imported types resolve correctly.
pub fn compile_module(
    source: &str,
    profile: ComplianceProfile,
    external_types: &ExternalTypes,
) -> Result<CodecModule, PipelineError> {
    // Parse
    let ast = wirespec_syntax::parse(source).map_err(|e| PipelineError {
        msg: format!("parse error: {e}"),
    })?;

    // Convert ExternalTypes to sema's expected format
    let ext_map: HashMap<String, wirespec_sema::resolve::DeclKind> = external_types
        .iter()
        .map(|(name, et)| {
            let kind = match et.kind {
                ExternalTypeKind::VarInt => wirespec_sema::resolve::DeclKind::VarInt,
                ExternalTypeKind::Packet => wirespec_sema::resolve::DeclKind::Packet,
                ExternalTypeKind::Frame => wirespec_sema::resolve::DeclKind::Frame,
                ExternalTypeKind::Capsule => wirespec_sema::resolve::DeclKind::Capsule,
                ExternalTypeKind::Enum => wirespec_sema::resolve::DeclKind::Enum,
                ExternalTypeKind::Flags => wirespec_sema::resolve::DeclKind::Flags,
                ExternalTypeKind::StateMachine => wirespec_sema::resolve::DeclKind::StateMachine,
            };
            (name.clone(), kind)
        })
        .collect();

    // Semantic analysis
    let mut sem = wirespec_sema::analyze(&ast, profile, &ext_map).map_err(|e| PipelineError {
        msg: format!("semantic error: {e}"),
    })?;

    // Populate imports from AST import declarations + external type registry
    for imp in &ast.imports {
        if let Some(ref item_name) = imp.name {
            if let Some(ext) = external_types.get(item_name) {
                let kind = match ext.kind {
                    ExternalTypeKind::VarInt => wirespec_sema::ir::ImportedDeclKind::VarInt,
                    ExternalTypeKind::Packet => wirespec_sema::ir::ImportedDeclKind::Packet,
                    ExternalTypeKind::Frame => wirespec_sema::ir::ImportedDeclKind::Frame,
                    ExternalTypeKind::Capsule => wirespec_sema::ir::ImportedDeclKind::Capsule,
                    ExternalTypeKind::Enum => wirespec_sema::ir::ImportedDeclKind::Enum,
                    ExternalTypeKind::Flags => wirespec_sema::ir::ImportedDeclKind::Flags,
                    ExternalTypeKind::StateMachine => wirespec_sema::ir::ImportedDeclKind::Packet,
                };
                sem.imports.push(wirespec_sema::ir::ImportedTypeRef {
                    import_id: format!("import:{}", item_name),
                    name: item_name.clone(),
                    source_module: ext.module.clone(),
                    source_prefix: ext.source_prefix.clone(),
                    decl_kind: kind,
                });
            }
        }
    }

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

/// Collect external types from a compiled module for use by downstream modules.
pub fn collect_external_types(
    registry: &mut ExternalTypes,
    codec: &CodecModule,
    module_name: &str,
    source_prefix: &str,
) {
    for v in &codec.varints {
        registry.register(
            &v.name,
            ExternalType {
                module: module_name.to_string(),
                name: v.name.clone(),
                source_prefix: source_prefix.to_string(),
                kind: ExternalTypeKind::VarInt,
            },
        );
    }
    for p in &codec.packets {
        registry.register(
            &p.name,
            ExternalType {
                module: module_name.to_string(),
                name: p.name.clone(),
                source_prefix: source_prefix.to_string(),
                kind: ExternalTypeKind::Packet,
            },
        );
    }
    for f in &codec.frames {
        registry.register(
            &f.name,
            ExternalType {
                module: module_name.to_string(),
                name: f.name.clone(),
                source_prefix: source_prefix.to_string(),
                kind: ExternalTypeKind::Frame,
            },
        );
    }
    for c in &codec.capsules {
        registry.register(
            &c.name,
            ExternalType {
                module: module_name.to_string(),
                name: c.name.clone(),
                source_prefix: source_prefix.to_string(),
                kind: ExternalTypeKind::Capsule,
            },
        );
    }
    for e in &codec.enums {
        let kind = if e.is_flags {
            ExternalTypeKind::Flags
        } else {
            ExternalTypeKind::Enum
        };
        registry.register(
            &e.name,
            ExternalType {
                module: module_name.to_string(),
                name: e.name.clone(),
                source_prefix: source_prefix.to_string(),
                kind,
            },
        );
    }
    for sm in &codec.state_machines {
        registry.register(
            &sm.name,
            ExternalType {
                module: module_name.to_string(),
                name: sm.name.clone(),
                source_prefix: source_prefix.to_string(),
                kind: ExternalTypeKind::StateMachine,
            },
        );
    }
}
