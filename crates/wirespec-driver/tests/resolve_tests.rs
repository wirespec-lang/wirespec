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
    write_file(
        &dir,
        "quic/varint.wspec",
        "module quic.varint\ntype VarInt = {\n    prefix: bits[2],\n    value: match prefix {\n        0b00 => bits[6], 0b01 => bits[14],\n        0b10 => bits[30], 0b11 => bits[62],\n    },\n}",
    );
    let entry = write_file(
        &dir,
        "quic/frames.wspec",
        "module quic.frames\nimport quic.varint.VarInt\npacket P { x: VarInt }",
    );
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
    write_file(
        &dir,
        "b.wspec",
        "module b\nimport c.C\npacket B { inner: C }",
    );
    let entry = write_file(
        &dir,
        "a.wspec",
        "module a\nimport b.B\npacket A { inner: B }",
    );
    let modules = resolve(&entry, &[dir.path().to_path_buf()]).unwrap();
    // Order: c -> b -> a
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
    // No explicit include paths -- parent of entry should work
    let modules = resolve(&entry, &[]).unwrap();
    assert_eq!(modules.len(), 2);
}

#[test]
fn resolve_deduplicates_shared_deps() {
    // a imports b and c; both b and c import d -- d should appear once
    let dir = TempDir::new().unwrap();
    write_file(&dir, "d.wspec", "module d\npacket D { x: u8 }");
    write_file(&dir, "b.wspec", "module b\nimport d.D\npacket B { x: D }");
    write_file(&dir, "c.wspec", "module c\nimport d.D\npacket C { x: D }");
    let entry = write_file(
        &dir,
        "a.wspec",
        "module a\nimport b.B\nimport c.C\npacket A { x: B }",
    );
    let modules = resolve(&entry, &[dir.path().to_path_buf()]).unwrap();
    // d appears once, before b and c, which are before a
    let names: Vec<_> = modules.iter().map(|m| m.module_name.as_str()).collect();
    assert_eq!(names.iter().filter(|&&n| n == "d").count(), 1);
    assert_eq!(*names.last().unwrap(), "a");
}

#[test]
fn resolve_export_visibility_enforced() {
    let dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "lib.wspec",
        "module lib\nexport packet Pub { x: u8 }\npacket Priv { y: u16 }",
    );
    let entry = write_file(
        &dir,
        "ok.wspec",
        "module ok\nimport lib.Pub\npacket P { x: u8 }",
    );
    assert!(resolve(&entry, &[dir.path().to_path_buf()]).is_ok());
}

#[test]
fn resolve_export_visibility_rejected() {
    let dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "lib.wspec",
        "module lib\nexport packet Pub { x: u8 }\npacket Priv { y: u16 }",
    );
    let entry = write_file(
        &dir,
        "bad.wspec",
        "module bad\nimport lib.Priv\npacket P { x: u8 }",
    );
    let result = resolve(&entry, &[dir.path().to_path_buf()]);
    assert!(result.is_err());
    assert!(result.unwrap_err().msg.contains("does not export"));
}

#[test]
fn resolve_no_exports_all_public() {
    let dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "lib.wspec",
        "module lib\npacket A { x: u8 }\npacket B { y: u16 }",
    );
    let entry = write_file(
        &dir,
        "ok.wspec",
        "module ok\nimport lib.A\nimport lib.B\npacket P { x: u8 }",
    );
    assert!(resolve(&entry, &[dir.path().to_path_buf()]).is_ok());
}
