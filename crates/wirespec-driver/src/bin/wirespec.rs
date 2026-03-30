// crates/wirespec-driver/src/bin/wirespec.rs
//
// CLI binary for the wirespec compiler.
//
// Usage:
//   wirespec compile <input.wspec> -o <dir> -t <c|rust> -I <include-path>
//   wirespec check <input.wspec>
//   wirespec --help

use std::env;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;

use wirespec_backend_api::*;

// ── Backend Factories ──

struct CBackendFactory;

impl BackendFactory for CBackendFactory {
    fn id(&self) -> TargetId {
        wirespec_backend_c::TARGET_C
    }

    fn create(&self) -> Box<dyn BackendDyn> {
        Box::new(wirespec_backend_c::CBackend)
    }

    fn default_options(&self) -> Box<dyn std::any::Any + Send + Sync> {
        Box::new(CBackendOptions::default())
    }
}

struct RustBackendFactory;

impl BackendFactory for RustBackendFactory {
    fn id(&self) -> TargetId {
        wirespec_backend_rust::TARGET_RUST
    }

    fn create(&self) -> Box<dyn BackendDyn> {
        Box::new(wirespec_backend_rust::RustBackend)
    }

    fn default_options(&self) -> Box<dyn std::any::Any + Send + Sync> {
        Box::new(RustBackendOptions::default())
    }
}

fn build_registry() -> BackendRegistry {
    let mut reg = BackendRegistry::new();
    reg.register(Box::new(CBackendFactory));
    reg.register(Box::new(RustBackendFactory));
    reg
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "compile" => cmd_compile(&args[2..]),
        "check" => cmd_check(&args[2..]),
        "--help" | "-h" => print_usage(),
        other => {
            eprintln!("error: unknown command: {other}");
            eprintln!();
            print_usage();
            process::exit(1);
        }
    }
}

fn cmd_compile(args: &[String]) {
    let mut input = None;
    let mut output = PathBuf::from("build");
    let mut target = "c".to_string();
    let mut include_paths = Vec::new();
    let mut fuzz = false;
    let mut recursive = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: -o requires a directory argument");
                    process::exit(1);
                }
                output = PathBuf::from(&args[i]);
            }
            "-t" | "--target" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: -t requires a target argument (c or rust)");
                    process::exit(1);
                }
                target = args[i].clone();
            }
            "-I" | "--include-path" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: -I requires a directory argument");
                    process::exit(1);
                }
                include_paths.push(PathBuf::from(&args[i]));
            }
            "--fuzz" => {
                fuzz = true;
            }
            "--recursive" => {
                recursive = true;
            }
            "--help" | "-h" => {
                print_compile_usage();
                return;
            }
            arg if arg.starts_with('-') => {
                eprintln!("error: unknown option: {arg}");
                process::exit(1);
            }
            _ => {
                input = Some(PathBuf::from(&args[i]));
            }
        }
        i += 1;
    }

    // Validate: --fuzz only valid with C target
    if fuzz && target != "c" {
        eprintln!("error: --fuzz is only supported with --target c");
        process::exit(1);
    }

    let input = input.unwrap_or_else(|| {
        eprintln!("error: no input file specified");
        eprintln!();
        print_compile_usage();
        process::exit(1);
    });

    // Build registry and look up the backend
    let registry = build_registry();
    let target_id = TargetId(leak_str(&target));
    let factory = match registry.get_factory(target_id) {
        Ok(f) => f,
        Err(_) => {
            let available: Vec<String> = registry
                .available_targets()
                .iter()
                .map(|t| t.to_string())
                .collect();
            eprintln!("error: unknown target: {target}");
            eprintln!("  supported targets: {}", available.join(", "));
            process::exit(1);
        }
    };
    let backend = factory.create();

    // Pre-process ASN.1 declarations (when asn1 feature is enabled)
    let asn1_modules = preprocess_asn1(&input, &include_paths, &output);

    // Compile via the driver
    let result = wirespec_driver::compile(&wirespec_driver::CompileRequest {
        entry: input.clone(),
        include_paths,
        profile: wirespec_sema::ComplianceProfile::default(),
        asn1_modules,
    });

    match result {
        Ok(result) => {
            if let Err(e) = std::fs::create_dir_all(&output) {
                eprintln!(
                    "error: cannot create output directory {}: {e}",
                    output.display()
                );
                process::exit(1);
            }

            for compiled_module in &result.modules {
                let is_entry =
                    compiled_module.module_name == result.modules.last().unwrap().module_name;

                // Without --recursive, only emit the entry module
                if !is_entry && !recursive {
                    continue;
                }

                emit_module(
                    compiled_module,
                    backend.as_ref(),
                    factory,
                    &output,
                    is_entry,
                    fuzz,
                );
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

/// Leak a string to get a `&'static str` for use as a TargetId.
/// This is fine for a CLI binary — the process exits soon after.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// Get the appropriate checksum bindings for the given target.
fn checksum_bindings_for(target: TargetId) -> Arc<dyn ChecksumBindingProvider> {
    match target.0 {
        "c" => Arc::new(wirespec_backend_c::checksum_binding::CChecksumBindings),
        "rust" => Arc::new(wirespec_backend_rust::checksum_binding::RustChecksumBindings),
        _ => Arc::new(NoChecksumBindings),
    }
}

fn emit_module(
    module: &wirespec_driver::CompiledModule,
    backend: &dyn BackendDyn,
    factory: &dyn BackendFactory,
    output: &std::path::Path,
    is_entry: bool,
    fuzz: bool,
) {
    // Build target options, applying --fuzz flag for C backend
    let target_options: Box<dyn std::any::Any + Send + Sync> = if backend.id().0 == "c" && fuzz {
        Box::new(CBackendOptions {
            emit_fuzz_harness: true,
        })
    } else {
        factory.default_options()
    };

    let ctx = BackendContext {
        module_name: module.module_name.clone(),
        module_prefix: module.source_prefix.clone(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options,
        checksum_bindings: checksum_bindings_for(backend.id()),
        is_entry_module: is_entry,
    };

    let mut sink = MemorySink::new();
    match backend.lower_and_emit(&module.codec, &ctx, &mut sink) {
        Ok(_) => {
            for (meta, contents) in &sink.artifacts {
                let path = output.join(&meta.relative_path);
                if let Err(e) = std::fs::write(&path, contents) {
                    eprintln!("error: cannot write {}: {e}", path.display());
                    std::process::exit(1);
                }
                eprintln!("  wrote {}", path.display());
            }
        }
        Err(e) => {
            eprintln!(
                "error: backend error for module '{}': {e}",
                module.module_name
            );
            process::exit(1);
        }
    }
}

fn preprocess_asn1(
    input: &std::path::Path,
    include_paths: &[PathBuf],
    output_dir: &std::path::Path,
) -> wirespec_driver::Asn1ModuleMap {
    #[cfg(not(feature = "asn1"))]
    {
        let _ = (input, include_paths, output_dir);
        wirespec_driver::Asn1ModuleMap::default()
    }

    #[cfg(feature = "asn1")]
    {
        use wirespec_driver::asn1_compile;
        use wirespec_driver::pipeline::{Asn1ModuleInfo, Asn1ModuleMap};

        // Read and parse the .wspec source to find extern asn1 declarations
        let source = match std::fs::read_to_string(input) {
            Ok(s) => s,
            Err(_) => return Asn1ModuleMap::default(), // will fail later in compile()
        };
        let ast = match wirespec_syntax::parse(&source) {
            Ok(a) => a,
            Err(_) => return Asn1ModuleMap::default(), // will fail later in compile()
        };

        let wspec_dir = input.parent().unwrap_or(std::path::Path::new("."));
        let mut map = Asn1ModuleMap::default();

        for item in &ast.items {
            if let wirespec_syntax::ast::AstTopItem::ExternAsn1(ext) = item {
                // Skip if user already provided use clause
                if ext.rust_module.is_some() {
                    continue;
                }

                // Resolve .asn1 path
                let asn1_path = resolve_asn1_path(&ext.path, wspec_dir, include_paths);
                let asn1_path = match asn1_path {
                    Some(p) => p,
                    None => {
                        eprintln!("error: ASN.1 file '{}' not found", ext.path);
                        process::exit(1);
                    }
                };

                // Compile with rasn-compiler
                let result = match asn1_compile::compile_asn1(&asn1_path) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("error: {}", e);
                        process::exit(1);
                    }
                };

                // Validate declared types
                if let Err(e) =
                    asn1_compile::validate_types(&ext.type_names, &result.type_names, &ext.path)
                {
                    eprintln!("error: {}", e);
                    process::exit(1);
                }

                // Write generated ASN.1 Rust file to output directory
                if let Err(e) = std::fs::create_dir_all(output_dir) {
                    eprintln!("error: cannot create output directory: {e}");
                    process::exit(1);
                }
                let out_file = output_dir.join(format!("{}.rs", result.module_name));
                if let Err(e) = std::fs::write(&out_file, &result.source) {
                    eprintln!("error: cannot write {}: {e}", out_file.display());
                    process::exit(1);
                }
                eprintln!("  wrote {}", out_file.display());

                // Store in map for pipeline injection
                map.modules.insert(
                    ext.path.clone(),
                    Asn1ModuleInfo {
                        module_name: result.module_name.clone(),
                        source: result.source,
                    },
                );
            }
        }

        map
    }
}

#[cfg(feature = "asn1")]
fn resolve_asn1_path(
    asn1_path: &str,
    wspec_dir: &std::path::Path,
    include_paths: &[PathBuf],
) -> Option<PathBuf> {
    // Try relative to .wspec file first
    let candidate = wspec_dir.join(asn1_path);
    if candidate.exists() {
        return Some(candidate);
    }
    // Try include paths
    for inc in include_paths {
        let candidate = inc.join(asn1_path);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn cmd_check(args: &[String]) {
    let mut input = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_check_usage();
                return;
            }
            arg if arg.starts_with('-') => {
                eprintln!("error: unknown option: {arg}");
                process::exit(1);
            }
            _ => {
                input = Some(PathBuf::from(&args[i]));
            }
        }
        i += 1;
    }

    let input = input.unwrap_or_else(|| {
        eprintln!("error: no input file specified");
        eprintln!();
        print_check_usage();
        process::exit(1);
    });

    let source = std::fs::read_to_string(&input).unwrap_or_else(|e| {
        eprintln!("error: cannot read {}: {e}", input.display());
        process::exit(1);
    });

    match wirespec_driver::compile_module(
        &source,
        wirespec_sema::ComplianceProfile::default(),
        &Default::default(),
        &Default::default(),
    ) {
        Ok(_) => {
            eprintln!("ok: {}", input.display());
        }
        Err(e) => {
            eprintln!(
                "{}",
                wirespec_sema::error::format_error_simple(
                    &e.to_string(),
                    &source,
                    &input.to_string_lossy()
                )
            );
            process::exit(1);
        }
    }
}

fn print_usage() {
    eprintln!("wirespec - protocol description language compiler");
    eprintln!();
    eprintln!("Usage: wirespec <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  compile    Compile .wspec/.wspec files to C or Rust");
    eprintln!("  check      Parse and type-check a file (no code generation)");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -h, --help    Show this help message");
    eprintln!();
    eprintln!("Run 'wirespec <command> --help' for command-specific options.");
}

fn print_compile_usage() {
    eprintln!("Usage: wirespec compile <input.wspec> [options]");
    eprintln!();
    eprintln!("Compile a wirespec file and its dependencies to C or Rust source code.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -o, --output <dir>          Output directory (default: build)");
    eprintln!("  -t, --target <c|rust>       Target language (default: c)");
    eprintln!("  -I, --include-path <dir>    Module search path (repeatable)");
    eprintln!("  --fuzz                      Generate libFuzzer harness (C target only)");
    eprintln!("  --recursive                 Also emit code for all dependencies");
    eprintln!("  -h, --help                  Show this help message");
}

fn print_check_usage() {
    eprintln!("Usage: wirespec check <input.wspec>");
    eprintln!();
    eprintln!("Parse and type-check a wirespec file without generating code.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -h, --help    Show this help message");
}
