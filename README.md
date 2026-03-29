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

QUIC, TLS 1.3, MQTT, BLE, IPv4, TCP, Ethernet — all defined and tested in `examples/`.

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
