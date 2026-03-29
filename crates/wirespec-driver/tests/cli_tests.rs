// Integration tests for the wirespec CLI binary.

use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn wirespec_bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wirespec"));
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
        .args(["compile", input.to_str().unwrap(), "-o", out_dir.path().to_str().unwrap(), "-t", "c"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "compile should succeed: {stderr}"
    );
    assert!(stderr.contains("wrote"), "should print wrote messages: {stderr}");

    // Verify output files exist
    let h_path = out_dir.path().join("test.h");
    let c_path = out_dir.path().join("test.c");
    assert!(h_path.exists(), "header file should exist: {}", h_path.display());
    assert!(c_path.exists(), "source file should exist: {}", c_path.display());

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
        .args(["compile", input.to_str().unwrap(), "-o", out_dir.path().to_str().unwrap(), "-t", "rust"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "compile should succeed for rust target: {stderr}"
    );

    let rs_path = out_dir.path().join("test.rs");
    assert!(rs_path.exists(), "rust source should exist: {}", rs_path.display());
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
        .args(["compile", input.to_str().unwrap(), "-o", out_dir.path().to_str().unwrap()])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "compile with default target should succeed: {stderr}"
    );

    // Default target is C
    assert!(out_dir.path().join("test.h").exists(), "should produce .h (default target is C)");
    assert!(out_dir.path().join("test.c").exists(), "should produce .c (default target is C)");
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
    assert!(stderr.contains("unknown target"), "should report unknown target: {stderr}");
}

#[test]
fn cli_compile_missing_input_fails() {
    let output = wirespec_bin()
        .args(["compile"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no input file"), "should report no input file: {stderr}");
}

#[test]
fn cli_compile_nonexistent_file_fails() {
    let output = wirespec_bin()
        .args(["compile", "/nonexistent/file.wspec", "-o", "/tmp/wirespec-cli-test-nonexist"])
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
    let output = wirespec_bin()
        .args(["check"])
        .output()
        .unwrap();

    assert!(!output.status.success());
}

#[test]
fn cli_compile_help() {
    let output = wirespec_bin()
        .args(["compile", "--help"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--output"), "compile help should mention --output: {stderr}");
    assert!(stderr.contains("--target"), "compile help should mention --target: {stderr}");
    assert!(stderr.contains("--include-path"), "compile help should mention --include-path: {stderr}");
}

#[test]
fn cli_check_help() {
    let output = wirespec_bin()
        .args(["check", "--help"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("check"), "check help should mention check: {stderr}");
}

#[test]
fn cli_compile_wire_extension() {
    // Test that .wire files also work (not just .wspec)
    let dir = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "proto.wire",
        "module proto\n@endian big\npacket Msg { tag: u8 }",
    );
    let input = dir.path().join("proto.wire");

    let output = wirespec_bin()
        .args(["compile", input.to_str().unwrap(), "-o", out_dir.path().to_str().unwrap(), "-t", "c"])
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
