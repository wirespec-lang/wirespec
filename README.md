# wirespec

**Type-safe protocol description language for network binary formats.**

wirespec is a DSL for defining network protocol wire formats. You write `.wspec` (or `.wire`) files describing packets, frames, and state machines, and the wirespec compiler generates C or Rust parser/serializer code -- zero-copy where possible, no heap allocation, no runtime dependencies.

## Quick Example

Define a UDP datagram in `udp.wspec`:

```
module net.udp
@endian big

packet UdpDatagram {
    src_port: u16,
    dst_port: u16,
    length: u16,
    checksum: u16,
    require length >= 8,
    data: bytes[length: length - 8],
}
```

Compile to C:

```bash
wirespec compile udp.wspec -t c -o build/
```

This generates `build/net_udp.h` and `build/net_udp.c` with:

```c
wirespec_result_t net_udp_udp_datagram_parse(
    const uint8_t *buf, size_t len,
    net_udp_udp_datagram_t *out, size_t *consumed);

wirespec_result_t net_udp_udp_datagram_serialize(
    const net_udp_udp_datagram_t *frame,
    uint8_t *buf, size_t cap, size_t *written);

size_t net_udp_udp_datagram_serialized_len(
    const net_udp_udp_datagram_t *frame);
```

Compile to Rust instead:

```bash
wirespec compile udp.wspec -t rust -o build/
```

## Installation / Build

Requires Rust (edition 2024).

```bash
# Build all crates
cargo build --release

# The compiler binary is at:
target/release/wirespec
```

## Usage

### `wirespec compile`

Compile a `.wspec` file to C or Rust source code.

```bash
wirespec compile <input.wspec> [options]

Options:
  -o, --output <dir>          Output directory (default: build)
  -t, --target <c|rust>       Target language (default: c)
  -I, --include-path <dir>    Module search path (repeatable)
  --fuzz                      Generate libFuzzer harness (C target only)
  --recursive                 Also emit code for all dependencies
```

Examples:

```bash
# Compile QUIC frames with dependencies
wirespec compile examples/quic/frames.wire -t c -o build/ -I examples/ --recursive

# Generate Rust code
wirespec compile examples/net/udp.wire -t rust -o build/

# Generate C code with fuzz harness
wirespec compile examples/quic/varint.wire -t c --fuzz -o build/
```

### `wirespec check`

Parse and type-check a file without generating code.

```bash
wirespec check examples/quic/frames.wire
```

## Language Features

### Primitives and Endianness

```
u8, u16, u32, u64              # unsigned integers
i8, i16, i32, i64              # signed integers
u16be, u16le, u32be, u32le     # explicit endianness
u24, u24be, u24le              # 24-bit integers
bit                            # single bit
bits[N]                        # N-bit unsigned integer
bytes[N]                       # fixed-length bytes
bytes[length: EXPR]            # length-prefixed bytes
bytes[remaining]               # consume rest of scope
```

Module-level endianness control:

```
@endian big       # QUIC, IP, TCP
@endian little    # BLE, USB
```

### Packets

Fixed-layout structures with validation:

```
packet AckRange {
    gap: VarInt,
    ack_range: VarInt,
}
```

### Frames (Tagged Unions)

Dispatch on a tag field to different variants:

```
frame QuicFrame = match frame_type: VarInt {
    0x00 => Padding {},
    0x01 => Ping {},
    0x06 => Crypto {
        offset: VarInt,
        length: VarInt,
        data: bytes[length],
    },
    _ => Unknown { data: bytes[remaining] },
}
```

### Capsules (TLV Containers)

Type-length-value with scoped parsing via `within`:

```
capsule MqttPacket {
    type_and_flags: u8,
    remaining_length: MqttLength,
    payload: match (type_and_flags >> 4) within remaining_length {
        1 => Connect { ... },
        3 => Publish { ... },
        _ => Unknown { data: bytes[remaining] },
    },
}
```

### Sub-byte Fields

Consecutive `bits[N]` fields are automatically grouped and packed:

```
packet IPv4Header {
    version: bits[4],
    ihl: bits[4],
    dscp: bits[6],
    ecn: bits[2],
    total_length: u16,
    ...
}
```

### VarInt (Computed Types)

Prefix-match (QUIC) and continuation-bit (MQTT/Protobuf) variable-length integers:

```
type VarInt = {
    prefix: bits[2],
    value: match prefix {
        0b00 => bits[6],  0b01 => bits[14],
        0b10 => bits[30], 0b11 => bits[62],
    },
}

type MqttLength = varint {
    continuation_bit: msb,
    value_bits: 7,
    max_bytes: 4,
    byte_order: little,
}
```

### Conditional Fields, Derived Fields, Validation

```
packet Stream {
    stream_id: VarInt,
    offset_raw: if frame_type & 0x04 { VarInt },     # optional
    let offset: u64 = offset_raw ?? 0,                # derived
    require stream_id < MAX_STREAMS,                   # validation
}
```

### Constants, Enums, Flags

```
const QUIC_VERSION_1: u32 = 0x00000001

enum FrameType: VarInt {
    Padding = 0x00, Ping = 0x01, Crypto = 0x06,
}

flags PacketFlags: u8 {
    KeyPhase = 0x04, SpinBit = 0x20,
}
```

### Checksums

Automatic verification on parse, auto-computation on serialize:

```
packet IPv4Header {
    ...
    @checksum(internet)
    header_checksum: u16,
    ...
}
```

Supported algorithms: `internet` (RFC 1071), `crc32` (IEEE 802.3), `crc32c` (Castagnoli), `fletcher16` (RFC 1146).

### State Machines

First-class declarative state machines with guards, actions, and hierarchical delegation:

```
state machine PathState {
    state Init       { path_id: VarInt }
    state Active     { path_id: VarInt, rtt: u64 = 0 }
    state Closed [terminal]

    initial Init

    transition Init -> Active {
        on path_validated
        action { dst.path_id = src.path_id; }
    }

    transition Active -> Closed { on abandon }
    transition * -> Closed { on connection_closed }
}
```

### Modules and Imports

```
module quic.varint
export type VarInt = { ... }

module quic.frames
import quic.varint.VarInt
```

Cycle detection, topological sorting, and selective visibility via `export`.

## Compiler Architecture

wirespec uses a 4-stage pipeline of progressively lower-level, backend-independent intermediate representations:

```
.wspec source
  --> wirespec-syntax   (parse)   --> AST
  --> wirespec-sema     (analyze) --> Semantic IR
  --> wirespec-layout   (lower)   --> Layout IR
  --> wirespec-codec    (lower)   --> Codec IR  <-- backends consume this
  --> wirespec-backend-{c,rust}   --> target code
```

| Stage | Purpose |
|-------|---------|
| **AST** | Direct representation of source syntax |
| **Semantic IR** | Type-checked, name-resolved; all types and imports fully resolved |
| **Layout IR** | Wire shape: field order, bit packing, endianness, scope boundaries |
| **Codec IR** | Parse/serialize strategy: zero-copy vs. materialized, error paths, capacity checks |

Backends only consume `CodecModule` from the codec stage. They never touch the parser or semantic analysis.

## Crate Structure

| Crate | Description |
|-------|-------------|
| `wirespec-syntax` | Hand-written lexer + recursive descent parser, AST node types, span tracking |
| `wirespec-sema` | Semantic analysis: name resolution, type checking, validation rules |
| `wirespec-layout` | Layout lowering: wire field ordering, bit group packing, endianness |
| `wirespec-codec` | Codec lowering: parse/serialize strategies, zero-copy decisions, capacity checks |
| `wirespec-backend-api` | Backend trait definitions (`Backend`, `BackendDyn`, `ArtifactSink`, checksum bindings) |
| `wirespec-backend-c` | C code generator: header + source, bitgroup shift/mask, checksum verify/compute |
| `wirespec-backend-rust` | Rust code generator: single `.rs` file, lifetime tracking, Rust enums for frames |
| `wirespec-driver` | Compilation driver: module resolution, dependency graph, multi-module pipeline, CLI binary |

## Supported Targets

| Target | Flag | Output | Notes |
|--------|------|--------|-------|
| **C** | `-t c` | `.h` + `.c` | C11, no heap, `-Werror` clean, zero-copy bytes, libFuzzer harness via `--fuzz` |
| **Rust** | `-t rust` | `.rs` | Lifetime-tracked zero-copy, Rust enums for frames |

Generated C compiles cleanly with `gcc -Wall -Wextra -Werror -O2 -std=c11`. The C runtime (`wirespec_runtime.h`) is a header-only library under 500 lines with no external dependencies.

## Testing

The project has **900+ `#[test]` functions** across 8 crates, covering:

- **Parser tests** -- syntax edge cases, boundary conditions, corpus tests
- **Semantic analysis tests** -- type errors, name resolution, checksum catalog
- **Layout and codec tests** -- bit group packing, lowering strategies, production protocols
- **Codegen tests** -- C and Rust output verification
- **GCC verification tests** -- generated C is compiled with GCC to verify it builds without warnings
- **Round-trip tests** -- parse-serialize-parse cycle for protocol examples
- **End-to-end pipeline tests** -- `.wire` input through full compilation pipeline
- **Differential tests** -- cross-checking between pipeline stages

Run the full test suite:

```bash
cargo test
```

## Adding a New Backend

To add a new code generation target (e.g., Go, Swift, Zig):

1. Create a new crate `crates/wirespec-backend-xxx/`
2. Depend on `wirespec-backend-api` and `wirespec-codec`
3. Implement the `Backend` and `BackendDyn` traits, consuming `CodecModule`
4. Register a factory in the CLI binary

You do not need to modify any upstream crate (syntax, sema, layout, codec).

See [docs/CONTRIBUTING_BACKEND.md](docs/CONTRIBUTING_BACKEND.md) for the full guide with code examples.

## Protocol Examples

The `examples/` directory contains complete wire definitions for real protocols:

| Protocol | File | Features |
|----------|------|----------|
| QUIC VarInt | `examples/quic/varint.wire` | Computed type, bits[N] match |
| QUIC Frames | `examples/quic/frames.wire` | frame, capsule, if, let, within, require, arrays |
| BLE ATT/L2CAP | `examples/ble/att.wire` | Little-endian, type alias, enum fields |
| MQTT 3.1.1 | `examples/mqtt/mqtt.wire` | Continuation-bit VarInt, expression capsule tag |
| TLS 1.3 | `examples/tls/tls13.wire` | Capsules, enum tags, u24, fill-within |
| IPv4 | `examples/ip/ipv4.wire` | `@checksum(internet)`, bits[N] |
| TCP | `examples/net/tcp.wire` | Bit flags, BitGroup auto-grouping |
| Ethernet | `examples/net/ethernet.wire` | Zero-copy bytes, bytes[remaining] |
| MPQUIC | `examples/mpquic/path.wire` | State machines, delegate, hierarchical dispatch |

## Roadmap

| Version | Feature | Description |
|---------|---------|-------------|
| v0.1.0 | Wire formats + C/Rust codegen | Phase 1-2 complete. Packets, frames, capsules, state machines, checksums, multi-module compilation. |
| v0.2.0 | ASN.1 / rasn integration | `extern asn1` type import and `asn1()` codec generation via rasn. |
| v0.3.0 | TLA+ verification | Bounded model checking of state machine properties via `@verify` annotations and TLC. |

## License

Apache-2.0
