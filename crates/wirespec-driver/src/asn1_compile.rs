//! ASN.1 compilation via rasn-compiler.
//!
//! Compiles .asn1 files to Rust source code, extracts module names
//! and type names for integration with wirespec's extern asn1 declarations.

use std::path::Path;

/// Result of compiling an ASN.1 file.
pub struct Asn1CompileResult {
    /// Generated Rust source code.
    pub source: String,
    /// Module name extracted from generated code (e.g., "test_module").
    pub module_name: String,
    /// All public type names found in generated code.
    pub type_names: Vec<String>,
}

/// Compile an ASN.1 file using rasn-compiler and return the result.
pub fn compile_asn1(path: &Path) -> Result<Asn1CompileResult, String> {
    use rasn_compiler::prelude::*;

    if !path.exists() {
        return Err(format!("ASN.1 file not found: {}", path.display()));
    }

    let result = Compiler::<RasnBackend, _>::new()
        .add_asn_by_path(path)
        .compile_to_string()
        .map_err(|e| format!("rasn-compiler error for '{}': {}", path.display(), e))?;

    let source = result.generated;
    let module_name = extract_module_name(&source).ok_or_else(|| {
        format!(
            "could not find 'pub mod' in rasn output for '{}'",
            path.display()
        )
    })?;
    let type_names = extract_type_names(&source);

    Ok(Asn1CompileResult {
        source,
        module_name,
        type_names,
    })
}

/// Extract the module name from rasn-compiler output.
/// Looks for `pub mod <name> {` pattern.
fn extract_module_name(source: &str) -> Option<String> {
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("pub mod ")
            && let Some(name) = rest.split_whitespace().next()
        {
            return Some(name.trim_end_matches('{').trim().to_string());
        }
    }
    None
}

/// Extract all public type names (struct and enum) from rasn-compiler output.
fn extract_type_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        for prefix in &["pub struct ", "pub enum "] {
            if let Some(rest) = trimmed.strip_prefix(prefix)
                && let Some(name) = rest
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .filter(|n| !n.is_empty())
            {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// Validate that all declared type names exist in the available types.
pub fn validate_types(
    declared: &[String],
    available: &[String],
    asn1_path: &str,
) -> Result<(), String> {
    for name in declared {
        if !available.contains(name) {
            return Err(format!(
                "type '{}' not found in ASN.1 module '{}'. Available types: {}",
                name,
                asn1_path,
                available.join(", ")
            ));
        }
    }
    Ok(())
}
