# wirespec

**Type-safe protocol description language for network binary formats.**

Declaratively describe your binary protocol and get safe, zero-allocation C and Rust parsers/serializers — no hand-written byte manipulation, no buffer overreads, no endianness bugs. State machines are verified with TLA+ model checking.

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
wirespec compile <input.wspec> -t <c|rust> -o <dir>  # compile to C or Rust
wirespec check <input.wspec>                          # type-check only
wirespec verify <input.wspec> -o <dir>                # generate TLA+ spec
wirespec verify <input.wspec> --run-tlc               # run TLC model checker
```

Options: `-I <dir>` (include path), `--recursive` (emit dependencies), `--fuzz` (libFuzzer harness, C only), `--bound N` (TLA+ model checking bound).

## State Machine Verification

Define state machines with guards, actions, and delegates. wirespec statically verifies them and generates TLA+ specs for model checking.

```
@verify(bound = 3)
state machine PathState {
    state Init       { path_id: u8 }
    state Active     { path_id: u8, rtt: u8 = 0 }
    state Closed [terminal]
    initial Init

    transition Init -> Active {
        on activate(id: u8)
        action { dst.path_id = src.path_id; }
    }
    transition Active -> Closed { on close }
    transition * -> Closed { on abort }

    verify NoDeadlock
    verify AllReachClosed
    verify property AbandonIsFinal:
        in_state(Closing) -> [] not in_state(Active)
}
```

```bash
wirespec verify path.wspec --run-tlc
# PASS: All properties verified for PathState (bound = 3)
```

**Static analysis (compile-time):** deadlock-free terminal states (S2), delegate acyclicity (S4), structural reachability (S5), exhaustive transitions (S6), wildcard priority (S7).

**Model checking (TLA+):** NoDeadlock, AllReachClosed (liveness), user-defined safety/liveness properties, guard mutual exclusivity.

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

Supported encodings: UPER, BER, DER, APER, OER, COER. C backend transparently treats ASN.1 fields as raw bytes.

## Protocol Examples

QUIC, TLS 1.3, MQTT, BLE, IPv4, TCP, Ethernet, V2X (ASN.1/UPER) — all defined and tested in `examples/`.

## Editor Support

VS Code extension with syntax highlighting, completion, hover, and diagnostics: [wirespec-language-tools](https://github.com/wirespec-lang/wirespec-language-tools)

## Roadmap

| Version | Feature |
|---------|---------|
| ~~v0.1.0~~ | Wire formats, state machines, C/Rust codegen |
| ~~v0.2.0~~ | ASN.1 / rasn integration |
| ~~v0.3.0~~ | TLA+ bounded verification |
| v0.4.0 | Delegate SM TLA+ support, crates.io publish |

## License

Apache-2.0

Copyright (c) 2026 mp0rta
