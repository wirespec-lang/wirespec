# wirespec

**Type-safe protocol description language for network binary formats.**

Declaratively describe your binary protocol and get safe, zero-allocation C and Rust parsers/serializers — no hand-written byte manipulation, no buffer overreads, no endianness bugs.

## Quick Example

```
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
```

```bash
wirespec compile udp.wspec -t c -o build/    # generates .h + .c
wirespec compile udp.wspec -t rust -o build/  # generates .rs
```

## Install

```bash
cargo build --release
# Binary: target/release/wirespec
```

## Usage

```bash
wirespec compile <input.wspec> -t <c|rust> -o <dir>  # compile
wirespec check <input.wspec>                          # type-check only
```

Options: `-I <dir>` (include path), `--recursive` (emit dependencies), `--fuzz` (libFuzzer harness, C only).

## Protocol Examples

QUIC, TLS 1.3, MQTT, BLE, IPv4, TCP, Ethernet, V2X (ASN.1/UPER) — all defined and tested in `examples/`.

## ASN.1 Integration

wirespec can embed ASN.1-encoded fields in binary protocol descriptions. With the `asn1` feature, `.asn1` files are automatically compiled via [rasn](https://github.com/librasn/rasn).

```
extern asn1 "etsi_its_cdd.asn1" { CAM }

packet ItsCamPacket {
    version: u8,
    length: u16,
    cam: asn1(CAM, encoding: uper, length: length),
}
```

```bash
cargo run --features asn1 -- compile its.wspec -t rust -o build/
# Outputs: build/etsi_its_cdd.rs (rasn types) + build/its.rs (wirespec codec)
```

Generated Rust code decodes ASN.1 payloads automatically — `pkt.cam` is a fully typed struct, not raw bytes.

## Documentation

Full language guide, reference, and cookbook: [docs.wirespec.org](https://docs.wirespec.org)

## Roadmap

| Version | Feature |
|---------|---------|
| v0.1.0 | Wire formats, state machines, C/Rust codegen |
| v0.2.0 | ASN.1 / rasn integration |
| v0.3.0 | TLA+ bounded verification |

## License

Apache-2.0

Copyright (c) 2026 mp0rta
