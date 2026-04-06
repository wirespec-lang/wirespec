// Integration tests for the wirespec CLI binary.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

fn wirespec_bin() -> Command {
    // The wirespec binary is built from the root crate, not wirespec-driver.
    // Walk up from CARGO_MANIFEST_DIR (crates/wirespec-driver) to the workspace root.
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../target/debug/wirespec");
    let mut cmd = Command::new(path);
    cmd.env("RUST_BACKTRACE", "1");
    cmd
}

fn write_file(dir: &TempDir, rel_path: &str, content: &str) {
    let path = dir.path().join(rel_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn cli_help() {
    let output = wirespec_bin().arg("--help").output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("wirespec"),
        "help output should mention wirespec: {stderr}"
    );
    assert!(
        stderr.contains("compile"),
        "help output should mention compile command: {stderr}"
    );
    assert!(
        stderr.contains("check"),
        "help output should mention check command: {stderr}"
    );
}

#[test]
fn cli_no_args_exits_with_error() {
    let output = wirespec_bin().output().unwrap();
    assert!(!output.status.success());
}

#[test]
fn cli_unknown_command_exits_with_error() {
    let output = wirespec_bin().arg("frobnicate").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown command"));
}

#[test]
fn cli_compile_c_target() {
    let dir = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "test.wspec",
        "module test\n@endian big\npacket Pkt { x: u8, y: u16 }",
    );
    let input = dir.path().join("test.wspec");

    let output = wirespec_bin()
        .args([
            "compile",
            input.to_str().unwrap(),
            "-o",
            out_dir.path().to_str().unwrap(),
            "-t",
            "c",
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "compile should succeed: {stderr}");
    assert!(
        stderr.contains("wrote"),
        "should print wrote messages: {stderr}"
    );

    // Verify output files exist
    let h_path = out_dir.path().join("test.h");
    let c_path = out_dir.path().join("test.c");
    assert!(
        h_path.exists(),
        "header file should exist: {}",
        h_path.display()
    );
    assert!(
        c_path.exists(),
        "source file should exist: {}",
        c_path.display()
    );

    // Verify files are non-empty
    let h_content = fs::read_to_string(&h_path).unwrap();
    assert!(!h_content.is_empty(), "header should be non-empty");
    let c_content = fs::read_to_string(&c_path).unwrap();
    assert!(!c_content.is_empty(), "source should be non-empty");
}

#[test]
fn cli_compile_rust_target() {
    let dir = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "test.wspec",
        "module test\n@endian big\npacket Pkt { x: u8, y: u16 }",
    );
    let input = dir.path().join("test.wspec");

    let output = wirespec_bin()
        .args([
            "compile",
            input.to_str().unwrap(),
            "-o",
            out_dir.path().to_str().unwrap(),
            "-t",
            "rust",
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "compile should succeed for rust target: {stderr}"
    );

    let rs_path = out_dir.path().join("test.rs");
    assert!(
        rs_path.exists(),
        "rust source should exist: {}",
        rs_path.display()
    );
    let rs_content = fs::read_to_string(&rs_path).unwrap();
    assert!(!rs_content.is_empty(), "rust source should be non-empty");
}

#[test]
fn cli_compile_default_target_is_c() {
    let dir = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "test.wspec",
        "module test\n@endian big\npacket Pkt { x: u8 }",
    );
    let input = dir.path().join("test.wspec");

    let output = wirespec_bin()
        .args([
            "compile",
            input.to_str().unwrap(),
            "-o",
            out_dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "compile with default target should succeed: {stderr}"
    );

    // Default target is C
    assert!(
        out_dir.path().join("test.h").exists(),
        "should produce .h (default target is C)"
    );
    assert!(
        out_dir.path().join("test.c").exists(),
        "should produce .c (default target is C)"
    );
}

#[test]
fn cli_compile_unknown_target_fails() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "test.wspec", "module test\npacket Pkt { x: u8 }");
    let input = dir.path().join("test.wspec");

    let output = wirespec_bin()
        .args(["compile", input.to_str().unwrap(), "-t", "go"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown target"),
        "should report unknown target: {stderr}"
    );
}

#[test]
fn cli_compile_missing_input_fails() {
    let output = wirespec_bin().args(["compile"]).output().unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no input file"),
        "should report no input file: {stderr}"
    );
}

#[test]
fn cli_compile_nonexistent_file_fails() {
    let output = wirespec_bin()
        .args([
            "compile",
            "/nonexistent/file.wspec",
            "-o",
            "/tmp/wirespec-cli-test-nonexist",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
}

#[test]
fn cli_check_valid_file() {
    let dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "good.wspec",
        "module good\n@endian big\npacket Pkt { x: u8, y: u16 }",
    );
    let input = dir.path().join("good.wspec");

    let output = wirespec_bin()
        .args(["check", input.to_str().unwrap()])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "check of valid file should succeed: {stderr}"
    );
    assert!(stderr.contains("ok"), "should print ok: {stderr}");
}

#[test]
fn cli_check_invalid_syntax_fails() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "bad.wspec", "this is not valid wirespec syntax !!!");
    let input = dir.path().join("bad.wspec");

    let output = wirespec_bin()
        .args(["check", input.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "check of invalid file should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error"), "should print error: {stderr}");
}

#[test]
fn cli_check_missing_input_fails() {
    let output = wirespec_bin().args(["check"]).output().unwrap();

    assert!(!output.status.success());
}

#[test]
fn cli_compile_help() {
    let output = wirespec_bin().args(["compile", "--help"]).output().unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--output"),
        "compile help should mention --output: {stderr}"
    );
    assert!(
        stderr.contains("--target"),
        "compile help should mention --target: {stderr}"
    );
    assert!(
        stderr.contains("--include-path"),
        "compile help should mention --include-path: {stderr}"
    );
}

#[test]
fn cli_check_help() {
    let output = wirespec_bin().args(["check", "--help"]).output().unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("check"),
        "check help should mention check: {stderr}"
    );
}

#[test]
fn cli_compile_wire_extension() {
    // Test that .wspec files also work (not just .wspec)
    let dir = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "proto.wspec",
        "module proto\n@endian big\npacket Msg { tag: u8 }",
    );
    let input = dir.path().join("proto.wspec");

    let output = wirespec_bin()
        .args([
            "compile",
            input.to_str().unwrap(),
            "-o",
            out_dir.path().to_str().unwrap(),
            "-t",
            "c",
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "compile of .wire file should succeed: {stderr}"
    );
    assert!(out_dir.path().join("proto.h").exists());
    assert!(out_dir.path().join("proto.c").exists());
}

// ── Crash Resilience Tests ──

#[test]
fn cli_compile_empty_file() {
    let dir = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    write_file(&dir, "empty.wspec", "");
    let input = dir.path().join("empty.wspec");

    // Should not panic regardless of exit code
    let output = wirespec_bin()
        .args([
            "compile",
            input.to_str().unwrap(),
            "-o",
            out_dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    // We only care that the process didn't crash/panic
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "should not panic on empty file: {stderr}"
    );
}

#[test]
fn cli_compile_binary_file() {
    let dir = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    let bin_path = dir.path().join("binary.wspec");
    fs::write(&bin_path, [0xFF_u8, 0x00, 0xFE]).unwrap();

    let output = wirespec_bin()
        .args([
            "compile",
            bin_path.to_str().unwrap(),
            "-o",
            out_dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "should not panic on binary file: {stderr}"
    );
    assert!(
        !output.status.success(),
        "binary file should not compile successfully"
    );
}

#[test]
fn cli_verify_no_state_machine() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "no_sm.wspec", "packet P { x: u8 }");
    let input = dir.path().join("no_sm.wspec");

    let output = wirespec_bin()
        .args(["verify", input.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no state machines"),
        "should report no state machines: {stderr}"
    );
}

#[test]
fn cli_compile_syntax_error_shows_message() {
    let dir = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    write_file(&dir, "bad.wspec", "packet { broken");
    let input = dir.path().join("bad.wspec");

    let output = wirespec_bin()
        .args([
            "compile",
            input.to_str().unwrap(),
            "-o",
            out_dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error"),
        "should report an error in stderr: {stderr}"
    );
}

// ── Verify Command Tests ──

#[test]
fn cli_verify_generates_tla_files() {
    let dir = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "sm.wspec",
        r#"
        state machine S {
            state A {}
            state B [terminal]
            initial A
            transition A -> B { on go }
            verify NoDeadlock
        }
        "#,
    );
    let input = dir.path().join("sm.wspec");

    let output = wirespec_bin()
        .args([
            "verify",
            input.to_str().unwrap(),
            "-o",
            out_dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "verify should succeed: {stderr}");
    assert!(
        out_dir.path().join("S.tla").exists(),
        "should produce .tla file"
    );
    assert!(
        out_dir.path().join("S.cfg").exists(),
        "should produce .cfg file"
    );
}

#[test]
fn cli_verify_bound_option() {
    let dir = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "sm.wspec",
        r#"
        state machine S {
            state A {}
            state B [terminal]
            initial A
            transition A -> B { on go }
            verify NoDeadlock
        }
        "#,
    );
    let input = dir.path().join("sm.wspec");

    let output = wirespec_bin()
        .args([
            "verify",
            input.to_str().unwrap(),
            "-o",
            out_dir.path().to_str().unwrap(),
            "--bound",
            "5",
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "verify with --bound should succeed: {stderr}"
    );

    let cfg_content = fs::read_to_string(out_dir.path().join("S.cfg")).unwrap();
    assert!(
        cfg_content.contains("5"),
        "cfg should contain bound value 5: {cfg_content}"
    );
}

#[test]
fn cli_verify_nonexistent_file() {
    let output = wirespec_bin()
        .args(["verify", "/nonexistent/path/missing.wspec"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "verify of nonexistent file should fail"
    );
}

#[test]
fn cli_verify_no_sm_error() {
    let dir = TempDir::new().unwrap();
    write_file(&dir, "pkt.wspec", "packet P { x: u8 }");
    let input = dir.path().join("pkt.wspec");

    let output = wirespec_bin()
        .args(["verify", input.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no state machines"),
        "should report no state machines: {stderr}"
    );
}

// ── Verify argument error tests ──

#[test]
fn test_cli_verify_bound_missing_value() {
    // --bound with no value after it should exit with error
    let dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "sm.wspec",
        r#"
        state machine S {
            state A {}
            state B [terminal]
            initial A
            transition A -> B { on go }
            verify NoDeadlock
        }
        "#,
    );
    let input = dir.path().join("sm.wspec");

    let output = wirespec_bin()
        .args(["verify", input.to_str().unwrap(), "--bound"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "verify with --bound but no value should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error"), "should report an error: {stderr}");
}

#[test]
fn test_cli_verify_output_missing_value() {
    // -o with no value after it should exit with error
    let dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "sm.wspec",
        r#"
        state machine S {
            state A {}
            state B [terminal]
            initial A
            transition A -> B { on go }
            verify NoDeadlock
        }
        "#,
    );
    let input = dir.path().join("sm.wspec");

    let output = wirespec_bin()
        .args(["verify", input.to_str().unwrap(), "-o"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "verify with -o but no value should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error"), "should report an error: {stderr}");
}

#[test]
fn test_cli_verify_bound_invalid_value() {
    // --bound abc (non-numeric) should exit with error
    let dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "sm.wspec",
        r#"
        state machine S {
            state A {}
            state B [terminal]
            initial A
            transition A -> B { on go }
            verify NoDeadlock
        }
        "#,
    );
    let input = dir.path().join("sm.wspec");

    let output = wirespec_bin()
        .args(["verify", input.to_str().unwrap(), "--bound", "abc"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "verify with --bound abc should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid value for --bound"),
        "should report invalid value for --bound: {stderr}"
    );
}
