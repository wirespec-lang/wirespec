// crates/wirespec-driver/src/resolve.rs
//!
//! Module resolver: file discovery, DFS with cycle detection, topological sort.
//!
//! Given an entry `.wspec`/`.wire` file, discovers all transitive dependencies
//! via `import` declarations and returns modules in dependency-first (topological) order.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use wirespec_syntax::ast::{AstModule, AstTopItem};

/// A module that has been resolved (parsed + positioned in the dependency graph).
#[derive(Debug)]
pub struct ResolvedModule {
    pub path: PathBuf,
    pub module_name: String,
    pub source_prefix: String,
    pub source: String,
    pub ast: AstModule,
}

/// Error during module resolution.
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
        let parent = parent.to_path_buf();
        if !search_paths.contains(&parent) {
            search_paths.push(parent);
        }
    }

    let mut resolved: HashMap<PathBuf, ResolvedModule> = HashMap::new();
    let mut in_progress: HashSet<PathBuf> = HashSet::new();
    let mut order: Vec<PathBuf> = Vec::new();

    visit(
        &entry,
        &search_paths,
        &mut resolved,
        &mut in_progress,
        &mut order,
    )?;

    Ok(order
        .into_iter()
        .map(|p| {
            resolved
                .remove(&p)
                .expect("every path in order must be present in resolved map")
        })
        .collect())
}

/// Compute the set of names that are importable from a module.
///
/// If any item in the module has `exported == true`, only exported items are importable.
/// If no items are exported, all named items are importable (backward compatibility).
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

    // Read and parse the file
    let source = std::fs::read_to_string(&abs).map_err(|e| ResolveError {
        msg: format!("cannot read {}: {e}", abs.display()),
    })?;
    let ast = wirespec_syntax::parse(&source).map_err(|e| ResolveError {
        msg: format!("parse error in {}: {e}", abs.display()),
    })?;

    // Determine module name from `module` declaration or infer from filename
    let module_name = if let Some(ref decl) = ast.module_decl {
        decl.name.clone()
    } else {
        abs.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    };

    // Process imports: resolve each dependency.
    // An import like `import quic.varint.VarInt` means the module is `quic.varint`.
    // An import like `import quic.varint` (no name) means the module is `quic.varint`.
    for import in &ast.imports {
        let dep_module = extract_module_from_import(&import.module, import.name.is_some());
        let dep_path = find_module(&dep_module, search_paths)?;
        visit(&dep_path, search_paths, resolved, in_progress, order)?;

        // Validate export visibility: check that imported name is exported by the dependency
        if let Some(item_name) = &import.name {
            let dep_abs = dep_path.canonicalize().map_err(|e| ResolveError {
                msg: format!("cannot resolve {}: {e}", dep_path.display()),
            })?;
            if let Some(dep_module_resolved) = resolved.get(&dep_abs) {
                let exportable = get_exportable_names(&dep_module_resolved.ast);
                if !exportable.contains(item_name) {
                    return Err(ResolveError {
                        msg: format!(
                            "module '{}' does not export '{}'",
                            dep_module_resolved.module_name, item_name
                        ),
                    });
                }
            }
        }
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

/// Extract the module path from an import declaration.
///
/// Import syntax: `import quic.varint.VarInt` where:
/// - `module` field = "quic.varint" (the module part)
/// - `name` field = Some("VarInt") (the type name)
///
/// But also: `import quic.varint` where:
/// - `module` field = "quic.varint" (this IS the module)
/// - `name` field = None
///
/// The parser already splits these, so `import.module` is always the module path.
fn extract_module_from_import(module_path: &str, _has_name: bool) -> String {
    module_path.to_string()
}

/// Find a module file by dotted name in include paths.
/// `quic.varint` -> search for `quic/varint.wspec` then `quic/varint.wire` in each path.
pub fn find_module(
    module_name: &str,
    search_paths: &[PathBuf],
) -> Result<PathBuf, ResolveError> {
    let rel_path_wspec = module_name.replace('.', "/") + ".wspec";
    let rel_path_wire = module_name.replace('.', "/") + ".wire";

    // Search .wspec first, then .wire, across all paths
    for base in search_paths {
        let candidate = base.join(&rel_path_wspec);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    for base in search_paths {
        let candidate = base.join(&rel_path_wire);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    // Fallback: single-component name -> dir/name.wspec or dir/name.wire
    let parts: Vec<&str> = module_name.split('.').collect();
    if parts.len() == 1 {
        let fallback_wspec = format!("{0}/{0}.wspec", parts[0]);
        let fallback_wire = format!("{0}/{0}.wire", parts[0]);
        for base in search_paths {
            let candidate = base.join(&fallback_wspec);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
        for base in search_paths {
            let candidate = base.join(&fallback_wire);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    Err(ResolveError {
        msg: format!(
            "module '{}' not found; searched: {}",
            module_name,
            search_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    })
}
