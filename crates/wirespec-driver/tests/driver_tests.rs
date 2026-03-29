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
        entry,
        include_paths: vec![],
        profile: ComplianceProfile::default(),
    })
    .unwrap();
    assert_eq!(result.modules.len(), 1);
    assert_eq!(result.modules[0].module_name, "test");
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
    write_file(
        &dir,
        "mid.wspec",
        "module mid\nimport base.B\npacket M { inner: B }",
    );
    let entry_src = "module top\nimport mid.M\nimport base.B\npacket T { m: M }";
    write_file(&dir, "top.wspec", entry_src);
    let entry = dir.path().join("top.wspec");
    let result = compile(&CompileRequest {
        entry,
        include_paths: vec![dir.path().to_path_buf()],
        profile: ComplianceProfile::default(),
    })
    .unwrap();
    // Cross-module type resolution is now wired: external types from
    // previously-compiled modules are registered in sema's TypeRegistry.
    let names: Vec<_> = result
        .modules
        .iter()
        .map(|m| m.module_name.as_str())
        .collect();
    assert_eq!(names.iter().filter(|&&n| n == "base").count(), 1);
    assert_eq!(*names.last().unwrap(), "top");
}

#[test]
fn driver_multi_module_self_contained() {
    // Each module is self-contained (no cross-module type references in sema)
    let dir = TempDir::new().unwrap();
    write_file(&dir, "base.wspec", "module base\npacket B { x: u8 }");
    // app imports base but doesn't actually use the imported type in fields
    write_file(
        &dir,
        "app.wspec",
        "module app\nimport base\npacket A { y: u16 }",
    );
    let entry = dir.path().join("app.wspec");
    let result = compile(&CompileRequest {
        entry,
        include_paths: vec![dir.path().to_path_buf()],
        profile: ComplianceProfile::default(),
    })
    .unwrap();
    assert_eq!(result.modules.len(), 2);
    assert_eq!(result.modules[0].module_name, "base");
    assert_eq!(result.modules[1].module_name, "app");
}

#[test]
fn driver_multi_module_varint_import() {
    // End-to-end test: VarInt defined in one module, imported and used in another.
    let dir = TempDir::new().unwrap();
    write_file(
        &dir,
        "quic/varint.wspec",
        r#"module quic.varint
@endian big
type VarInt = {
    prefix: bits[2],
    value: match prefix {
        0b00 => bits[6],
        0b01 => bits[14],
        0b10 => bits[30],
        0b11 => bits[62],
    },
}"#,
    );
    write_file(
        &dir,
        "quic/frames.wspec",
        r#"module quic.frames
@endian big
import quic.varint.VarInt

packet AckRange {
    gap: VarInt,
    ack_range: VarInt,
}

frame QuicFrame = match frame_type: VarInt {
    0x06 => Crypto {
        offset: VarInt,
        length: VarInt,
        data: bytes[length],
    },
    _ => Unknown { data: bytes[remaining] },
}"#,
    );
    let entry = dir.path().join("quic/frames.wspec");
    let result = compile(&CompileRequest {
        entry,
        include_paths: vec![dir.path().to_path_buf()],
        profile: ComplianceProfile::default(),
    })
    .unwrap();
    // Should compile two modules: varint first, then frames
    assert!(
        result.modules.len() >= 2,
        "expected at least 2 modules, got {}",
        result.modules.len()
    );
    let frames = result.modules.last().unwrap();
    assert_eq!(frames.module_name, "quic.frames");
    assert!(
        !frames.codec.frames.is_empty(),
        "frames module should contain at least one frame"
    );
    assert!(
        !frames.codec.packets.is_empty(),
        "frames module should contain at least one packet (AckRange)"
    );
}
