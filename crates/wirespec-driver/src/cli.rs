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

pub fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "compile" => cmd_compile(&args[2..]),
        "check" => cmd_check(&args[2..]),
        "verify" => cmd_verify(&args[2..]),
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
    let result = crate::compile(&crate::CompileRequest {
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
    module: &crate::CompiledModule,
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

// TODO: ASN.1 preprocessing currently only scans the entry .wspec file for
// `extern asn1` declarations. Imported modules that also reference ASN.1 files
// are not discovered here. Fixing this would require either a multi-pass
// pipeline or moving ASN.1 discovery into the main compilation loop.
fn preprocess_asn1(
    input: &std::path::Path,
    include_paths: &[PathBuf],
    output_dir: &std::path::Path,
) -> crate::Asn1ModuleMap {
    #[cfg(not(feature = "asn1"))]
    {
        let _ = (input, include_paths, output_dir);
        crate::Asn1ModuleMap::default()
    }

    #[cfg(feature = "asn1")]
    {
        use crate::asn1_compile;
        use crate::pipeline::{Asn1ModuleInfo, Asn1ModuleMap};

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

    match crate::compile_module(
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

fn cmd_verify(args: &[String]) {
    let mut input = None;
    let mut output = None;
    let mut run_tlc = false;
    let mut tlc_path = std::env::var("TLC_JAR").unwrap_or_else(|_| "tla2tools.jar".to_string());
    let mut bound: Option<u32> = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: -o requires a directory argument");
                    process::exit(1);
                }
                output = Some(PathBuf::from(&args[i]));
            }
            "--run-tlc" => {
                run_tlc = true;
            }
            "--tlc-path" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --tlc-path requires an argument");
                    process::exit(1);
                }
                tlc_path = args[i].clone();
            }
            "--bound" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --bound requires a numeric argument");
                    process::exit(1);
                }
                bound = Some(args[i].parse().unwrap_or_else(|_| {
                    eprintln!("error: invalid value for --bound: '{}'", args[i]);
                    process::exit(1);
                }));
            }
            "--help" | "-h" => {
                print_verify_usage();
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
        print_verify_usage();
        process::exit(1);
    });

    // Read and parse the .wspec file
    let source = std::fs::read_to_string(&input).unwrap_or_else(|e| {
        eprintln!("error: cannot read {}: {e}", input.display());
        process::exit(1);
    });

    let ast = wirespec_syntax::parse(&source).unwrap_or_else(|e| {
        eprintln!("error: parse error: {}", e.msg);
        process::exit(1);
    });

    let sem = wirespec_sema::analyze(
        &ast,
        wirespec_sema::ComplianceProfile::default(),
        &Default::default(),
    )
    .unwrap_or_else(|e| {
        eprintln!("error: {}", e.msg);
        process::exit(1);
    });

    if sem.state_machines.is_empty() {
        eprintln!("error: no state machines found in {}", input.display());
        process::exit(1);
    }

    // Determine output directory
    let out_dir = output.unwrap_or_else(|| std::env::temp_dir().join("wirespec-verify"));
    std::fs::create_dir_all(&out_dir).unwrap_or_else(|e| {
        eprintln!("error: cannot create output directory: {e}");
        process::exit(1);
    });

    // Generate TLA+ for each state machine
    for sm in &sem.state_machines {
        let result = wirespec_backend_tlaplus::generate_tlaplus(sm, &sem.state_machines, bound);
        match result {
            Ok(output_tla) => {
                let tla_path = out_dir.join(format!("{}.tla", sm.name));
                let cfg_path = out_dir.join(format!("{}.cfg", sm.name));
                std::fs::write(&tla_path, &output_tla.spec).unwrap_or_else(|e| {
                    eprintln!("error: cannot write {}: {e}", tla_path.display());
                    process::exit(1);
                });
                std::fs::write(&cfg_path, &output_tla.config).unwrap_or_else(|e| {
                    eprintln!("error: cannot write {}: {e}", cfg_path.display());
                    process::exit(1);
                });
                eprintln!("  wrote {}", tla_path.display());
                eprintln!("  wrote {}", cfg_path.display());

                // Run built-in model checker (default)
                if !run_tlc {
                    let effective_bound = bound.or(sm.verify_bound).unwrap_or(3);
                    run_builtin_check(
                        &output_tla.spec,
                        &output_tla.config,
                        &sm.name,
                        effective_bound,
                    );
                }

                // Run external TLC if requested
                if run_tlc {
                    run_tlc_check(&tla_path, &cfg_path, &tlc_path);
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
        }
    }
}

fn run_builtin_check(tla_spec: &str, tla_config: &str, sm_name: &str, bound: u32) {
    use tla_checker::ast::{Env, Expr};
    use tla_checker::checker::{CheckResult, CheckerConfig, check};
    use tla_checker::config::{apply_config, parse_cfg};
    use tla_checker::parser::parse as parse_tla;

    // Parse TLA+ spec
    let mut spec = match parse_tla(tla_spec) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("  error: TLA+ parse error: {:?}", e);
            process::exit(1);
        }
    };

    // Parse config
    let cfg = match parse_cfg(tla_config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("  error: TLA+ config error: {:?}", e);
            process::exit(1);
        }
    };

    // Apply config to spec
    let mut domains = Env::new();
    let mut checker_config = CheckerConfig {
        allow_deadlock: true, // terminal states have no transitions
        quiet: true,
        ..Default::default()
    };

    if let Err(e) = apply_config(
        &cfg,
        &mut spec,
        &mut domains,
        &mut checker_config,
        &[],
        &[],
        false,
    ) {
        eprintln!("  error: config apply error: {}", e);
        process::exit(1);
    }

    // Unwrap Expr::Eventually wrappers on liveness properties — tla-checker's
    // check already wraps properties in <> semantics, so double-wrapping
    // would cause a "temporal operator <> cannot be evaluated" error.
    spec.liveness_properties = spec
        .liveness_properties
        .into_iter()
        .map(|p| match p {
            Expr::Eventually(inner) => *inner,
            other => other,
        })
        .collect();

    // Run model checker
    eprintln!("  checking {} (bound = {})...", sm_name, bound);
    let result = check(&spec, &domains, &checker_config);

    // Report results
    match &result {
        CheckResult::Ok(stats) => {
            eprintln!(
                "  ok: {} verified ({} states, {} transitions)",
                sm_name, stats.states_explored, stats.transitions
            );
        }
        CheckResult::InvariantViolation(ce, _) => {
            let inv_name = spec
                .invariant_names
                .get(ce.violated_invariant)
                .and_then(|n| n.as_ref())
                .map(|n| n.to_string())
                .unwrap_or_else(|| format!("#{}", ce.violated_invariant));
            eprintln!("  FAIL: invariant '{}' violated for {}", inv_name, sm_name);
            for (i, state) in ce.trace.iter().enumerate() {
                let action = ce
                    .actions
                    .get(i)
                    .and_then(|a| a.as_ref())
                    .map(|a| format!(" ({})", a))
                    .unwrap_or_default();
                let vars: Vec<String> = spec
                    .vars
                    .iter()
                    .zip(state.values.iter())
                    .map(|(name, val)| format!("{} = {:?}", name, val))
                    .collect();
                eprintln!("    step {}{}: {}", i + 1, action, vars.join(", "));
            }
            process::exit(1);
        }
        CheckResult::LivenessViolation(lv, _) => {
            eprintln!(
                "  FAIL: liveness property '{}' violated for {}",
                lv.property, sm_name
            );
            eprintln!("    prefix ({} states):", lv.prefix.len());
            for (i, state) in lv.prefix.iter().enumerate() {
                eprintln!("      step {}: {:?}", i + 1, state);
            }
            eprintln!("    cycle ({} states):", lv.cycle.len());
            for (i, state) in lv.cycle.iter().enumerate() {
                eprintln!("      step {}: {:?}", i + 1, state);
            }
            process::exit(1);
        }
        CheckResult::Deadlock(states, _, _) => {
            eprintln!("  FAIL: deadlock detected for {}", sm_name);
            if let Some(last) = states.last() {
                eprintln!("    final state: {:?}", last);
            }
            process::exit(1);
        }
        CheckResult::InitError(e) => {
            eprintln!("  error: Init evaluation error: {:?}", e);
            process::exit(1);
        }
        CheckResult::NextError(e, _, _) => {
            eprintln!("  error: Next evaluation error: {:?}", e);
            process::exit(1);
        }
        CheckResult::InvariantError(e, _, _) => {
            eprintln!("  error: Invariant evaluation error: {:?}", e);
            process::exit(1);
        }
        CheckResult::AssumeViolation(idx) => {
            eprintln!("  error: ASSUME violation (index {})", idx);
            process::exit(1);
        }
        CheckResult::AssumeError(idx, e) => {
            eprintln!("  error: ASSUME evaluation error (index {}): {:?}", idx, e);
            process::exit(1);
        }
        CheckResult::MaxStatesExceeded(stats) => {
            eprintln!(
                "  error: max states exceeded ({} explored) for {}",
                stats.states_explored, sm_name
            );
            process::exit(1);
        }
        CheckResult::MaxDepthExceeded(stats) => {
            eprintln!(
                "  error: max depth exceeded ({} explored) for {}",
                stats.states_explored, sm_name
            );
            process::exit(1);
        }
        CheckResult::NoInitialStates => {
            eprintln!("  error: no initial states for {}", sm_name);
            process::exit(1);
        }
        CheckResult::MissingConstants(names) => {
            let names: Vec<&str> = names.iter().map(|n| n.as_ref()).collect();
            eprintln!(
                "  error: missing constants for {}: {}",
                sm_name,
                names.join(", ")
            );
            process::exit(1);
        }
    }
}

fn run_tlc_check(tla_path: &std::path::Path, cfg_path: &std::path::Path, tlc_jar: &str) {
    let tla_name = tla_path.file_stem().unwrap().to_string_lossy();
    let cfg_name = cfg_path.file_name().unwrap().to_string_lossy();

    eprintln!("\n  running TLC on {}...", tla_name);

    let output = match std::process::Command::new("java")
        .args([
            "-jar",
            tlc_jar,
            "-config",
            &cfg_name,
            &format!("{}.tla", tla_name),
            "-deadlock",
        ])
        .current_dir(tla_path.parent().unwrap())
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("\nError: TLC not found ({}). Install TLA+ tools:", e);
            eprintln!("  https://github.com/tlaplus/tlaplus/releases");
            eprintln!("\nSet TLC_JAR environment variable to the path of tla2tools.jar,");
            eprintln!("or use --tlc-path option.\n");
            eprintln!("Generated files:");
            eprintln!("  {}", tla_path.display());
            eprintln!("  {}", cfg_path.display());
            eprintln!("\nYou can run TLC manually:");
            eprintln!(
                "  java -jar tla2tools.jar -config {} {}.tla -deadlock",
                cfg_name, tla_name
            );
            return;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() && stdout.contains("Model checking completed. No error found") {
        eprintln!("  PASS: {} — no errors found", tla_name);
    } else if stdout.contains("Error:") || stderr.contains("Error:") {
        eprintln!("  FAIL: {} — TLC found errors:\n", tla_name);
        // Print relevant output
        for line in stdout.lines() {
            if line.contains("Error")
                || line.contains("Invariant")
                || line.contains("violated")
                || line.contains("State")
                || line.starts_with("  ")
            {
                eprintln!("    {}", line);
            }
        }
    } else {
        eprintln!("  TLC output:\n{}", stdout);
    }
}

fn print_verify_usage() {
    eprintln!("Usage: wirespec verify <input.wspec> [options]");
    eprintln!();
    eprintln!("Generate TLA+ spec from state machines and verify with built-in model checker.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -o, --output <dir>       Output directory (default: temp dir)");
    eprintln!("  --bound <N>              Value domain bound (default: 3)");
    eprintln!("  --run-tlc                Use external TLC (Java) instead of built-in checker");
    eprintln!(
        "  --tlc-path <path>        Path to tla2tools.jar (default: $TLC_JAR or tla2tools.jar)"
    );
    eprintln!("  -h, --help               Show this help message");
}

fn print_usage() {
    eprintln!("wirespec - protocol description language compiler");
    eprintln!();
    eprintln!("Usage: wirespec <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  compile    Compile .wspec files to C or Rust");
    eprintln!("  check      Parse and type-check a file (no code generation)");
    eprintln!("  verify     Generate TLA+ and verify state machines");
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
