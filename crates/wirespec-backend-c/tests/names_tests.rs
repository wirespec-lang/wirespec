use wirespec_backend_c::names::*;

#[test]
fn snake_case_simple() {
    assert_eq!(to_snake_case("UdpDatagram"), "udp_datagram");
    assert_eq!(to_snake_case("IPv4Header"), "ipv4_header");
    assert_eq!(to_snake_case("VarInt"), "var_int");
    assert_eq!(to_snake_case("simple"), "simple");
}

#[test]
fn type_name() {
    assert_eq!(c_type_name("quic", "VarInt"), "quic_var_int_t");
    assert_eq!(
        c_type_name("net_udp", "UdpDatagram"),
        "net_udp_udp_datagram_t"
    );
}

#[test]
fn func_name() {
    assert_eq!(c_func_name("quic", "VarInt", "parse"), "quic_var_int_parse");
    assert_eq!(
        c_func_name("net_udp", "UdpDatagram", "serialize"),
        "net_udp_udp_datagram_serialize"
    );
}

#[test]
fn enum_member_name() {
    assert_eq!(
        c_enum_member("quic", "FrameType", "Padding"),
        "QUIC_FRAME_TYPE_Padding"
    );
}

#[test]
fn frame_tag_names() {
    assert_eq!(
        c_frame_tag_type("ble_att", "AttPdu"),
        "ble_att_att_pdu_tag_t"
    );
    assert_eq!(
        c_frame_tag_value("ble_att", "AttPdu", "ReadReq"),
        "BLE_ATT_ATT_PDU_READ_REQ"
    );
}
