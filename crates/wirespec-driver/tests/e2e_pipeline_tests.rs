// crates/wirespec-driver/tests/e2e_pipeline_tests.rs
//
// End-to-end pipeline tests: parse -> sema -> layout -> codec -> backend.

use std::sync::Arc;

use wirespec_backend_api::*;
use wirespec_sema::ComplianceProfile;
use wirespec_syntax::parse;

fn pipeline_to_c(src: &str) -> (String, String) {
    let ast = parse(src).unwrap();
    let sem = wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = wirespec_backend_c::CBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(CBackendOptions::default()),
        checksum_bindings: Arc::new(wirespec_backend_c::checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };
    let lowered = Backend::lower(&backend, &codec, &ctx).unwrap();
    (lowered.header_content.clone(), lowered.source_content.clone())
}

fn pipeline_to_rust(src: &str) -> String {
    let ast = parse(src).unwrap();
    let sem = wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = wirespec_backend_rust::RustBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(RustBackendOptions::default()),
        checksum_bindings: Arc::new(wirespec_backend_rust::checksum_binding::RustChecksumBindings),
        is_entry_module: true,
    };
    let lowered = Backend::lower(&backend, &codec, &ctx).unwrap();
    lowered.source.clone()
}

#[test]
fn e2e_udp_packet_c() {
    let src = r#"
        @endian big
        module net.udp
        packet UdpDatagram {
            src_port: u16,
            dst_port: u16,
            length: u16,
            checksum: u16,
            require length >= 8,
            data: bytes[length: length - 8],
        }
    "#;
    let (header, source) = pipeline_to_c(src);
    // Header has struct + function decls
    assert!(header.contains("typedef struct"));
    assert!(header.contains("uint16_t src_port"));
    assert!(header.contains("wirespec_result_t test_udp_datagram_parse"));
    assert!(header.contains("wirespec_result_t test_udp_datagram_serialize"));
    assert!(header.contains("wirespec_bytes_t data")); // bytes field
    // Source has parse/serialize impl
    assert!(source.contains("wirespec_cursor_read_u16be"));
    assert!(source.contains("WIRESPEC_ERR_CONSTRAINT")); // require
    assert!(source.contains("(void)r;")); // suppression
}

#[test]
fn e2e_udp_packet_rust() {
    let src = r#"
        @endian big
        module net.udp
        packet UdpDatagram {
            src_port: u16,
            dst_port: u16,
            length: u16,
            checksum: u16,
            require length >= 8,
            data: bytes[length: length - 8],
        }
    "#;
    let rs = pipeline_to_rust(src);
    assert!(rs.contains("pub struct"));
    assert!(rs.contains("pub src_port: u16"));
    assert!(rs.contains("&'a [u8]")); // bytes -> zero-copy slice
    assert!(rs.contains("fn parse"));
    assert!(rs.contains("fn serialize"));
    assert!(rs.contains("Error::Constraint")); // require
}

#[test]
fn e2e_bitgroup_c() {
    let src = r#"
        @endian big
        packet TcpHeader {
            src_port: u16,
            dst_port: u16,
            data_offset: bits[4],
            reserved: bits[4],
            flags: u8,
        }
    "#;
    let (_, source) = pipeline_to_c(src);
    assert!(source.contains(">>")); // bitgroup shift
    assert!(source.contains("& 0xf")); // 4-bit mask
}

#[test]
fn e2e_frame_c() {
    let src = r#"
        frame AttPdu = match opcode: u8 {
            0x01 => ErrorRsp { code: u8 },
            0x0b => ReadRsp { value: bytes[remaining] },
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    let (header, source) = pipeline_to_c(src);
    assert!(header.contains("tag_t")); // tag enum
    assert!(header.contains("union")); // variant union
    assert!(source.contains("switch")); // dispatch
    assert!(source.contains("frame_type")); // raw tag field
}

#[test]
fn e2e_capsule_c() {
    let src = r#"
        capsule TlvPacket {
            type_field: u8,
            length: u16,
            payload: match type_field within length {
                0 => Data { content: bytes[remaining] },
                _ => Unknown { data: bytes[remaining] },
            },
        }
    "#;
    let (_, source) = pipeline_to_c(src);
    assert!(source.contains("wirespec_cursor_sub")); // within sub-cursor
}

#[test]
fn e2e_optional_field_c() {
    let src = "packet P { flags: u8, extra: if flags & 0x01 { u16 } }";
    let (header, source) = pipeline_to_c(src);
    assert!(header.contains("bool has_extra"));
    assert!(source.contains("has_extra"));
}

#[test]
fn e2e_optional_field_rust() {
    let src = "packet P { flags: u8, extra: if flags & 0x01 { u16 } }";
    let rs = pipeline_to_rust(src);
    assert!(rs.contains("Option<u16>"));
}

#[test]
fn e2e_array_c() {
    let src = "packet P { count: u8, items: [u8; count] }";
    let (header, source) = pipeline_to_c(src);
    assert!(header.contains("items[")); // array
    assert!(header.contains("items_count")); // count
    assert!(source.contains("for")); // loop
    assert!(source.contains("WIRESPEC_ERR_CAPACITY")); // bounds check
}

#[test]
fn e2e_enum_c() {
    let src = "enum ErrorCode: u8 { InvalidHandle = 0x01, ReadNotPermitted = 0x02 }";
    let (header, _) = pipeline_to_c(src);
    assert!(header.contains("0x01") || header.contains("1"));
}

#[test]
fn e2e_state_machine_preserved() {
    let src = r#"
        state machine S {
            state A { count: u8 = 0 }
            state B [terminal]
            initial A
            transition A -> B { on done }
        }
    "#;
    // SM should at least not crash the pipeline
    let ast = parse(src).unwrap();
    let sem = wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.state_machines.len(), 1);
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    assert_eq!(codec.state_machines.len(), 1);
}

#[test]
fn e2e_artifact_c_produces_two_files() {
    let ast = parse("packet P { x: u8 }").unwrap();
    let sem = wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = wirespec_backend_c::CBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(CBackendOptions::default()),
        checksum_bindings: Arc::new(wirespec_backend_c::checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };
    let mut sink = MemorySink::new();
    let output = backend.lower_and_emit(&codec, &ctx, &mut sink).unwrap();
    assert_eq!(output.artifacts.len(), 2);
    assert!(output.artifacts[0]
        .relative_path
        .to_string_lossy()
        .ends_with(".h"));
    assert!(output.artifacts[1]
        .relative_path
        .to_string_lossy()
        .ends_with(".c"));
}

#[test]
fn e2e_artifact_rust_produces_one_file() {
    let ast = parse("packet P { x: u8 }").unwrap();
    let sem = wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = wirespec_backend_rust::RustBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(RustBackendOptions::default()),
        checksum_bindings: Arc::new(wirespec_backend_rust::checksum_binding::RustChecksumBindings),
        is_entry_module: true,
    };
    let mut sink = MemorySink::new();
    let output = backend.lower_and_emit(&codec, &ctx, &mut sink).unwrap();
    assert_eq!(output.artifacts.len(), 1);
    assert!(output.artifacts[0]
        .relative_path
        .to_string_lossy()
        .ends_with(".rs"));
}
