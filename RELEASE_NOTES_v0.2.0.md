# wirespec v0.2.0 — ASN.1 / rasn Integration

## Highlights

wirespec can now embed ASN.1-encoded fields in binary protocol descriptions. Generated Rust code automatically decodes/encodes ASN.1 payloads via [rasn](https://github.com/librasn/rasn), while C output transparently treats them as raw bytes.

```
extern asn1 "etsi_its_cdd.asn1" { CAM }

packet ItsCamPacket {
    version: u8,
    length: u16,
    cam: asn1(CAM, encoding: uper, length: length),
}
```

```bash
wirespec compile its.wspec -t rust -o build/
# pkt.cam is a fully typed CAM struct, not raw bytes
```

## New Features

### ASN.1 field type

- `asn1(TypeName, encoding: <codec>, length: <expr>)` — length-prefixed ASN.1 payload
- `asn1(TypeName, encoding: <codec>, remaining)` — consume remaining bytes as ASN.1
- Works in packets, frames, and capsules
- Supported encodings: **UPER, BER, DER, APER, OER, COER**

### `extern asn1` declarations

- `extern asn1 "schema.asn1" { TypeA, TypeB }` — declare ASN.1 types for use in fields
- Optional `use` clause for Rust import path: `extern asn1 "schema.asn1" use crate::my_types { TypeA }`
- Supports `::` paths: `use crate::generated::my_module`

### Automatic `.asn1` compilation (`asn1` feature)

- `cargo build --features asn1` enables rasn-compiler integration
- `wirespec compile` automatically compiles referenced `.asn1` files
- Generated Rust types output alongside wirespec output
- Type names validated against actual ASN.1 module contents
- `use` path auto-resolved from rasn-compiler output (no manual `use` clause needed)

### Rust backend — rasn codegen

- Parse: `rasn::{codec}::decode::<T>(bytes)` with `Error::Asn1Decode`
- Serialize: `rasn::{codec}::encode(&val)` with `Error::Asn1Encode`
- Struct fields use decoded ASN.1 type (not `&[u8]`)
- Length fields recomputed from encoded payload during serialization
- Dynamic `use rasn::{codec};` imports based on encodings used
- ASN.1 fields are owned — no lifetime parameter added

### C backend — transparent passthrough

- ASN.1 fields emitted as `const uint8_t *` with length
- No rasn dependency — users decode with asn1c or other C ASN.1 libraries
- Zero code changes to C backend

## Bug Fixes

### Capsule serialize (Rust backend)

- Fixed: capsule `serialize()` and `serialized_len()` now include payload variants (previously only wrote header fields)

### State machine codegen (Rust backend)

- Fixed: child SM fields now emit correct type names (was `u64`)
- Fixed: `Vec<T>` SM fields use `vec![]` initialization and `.clone()` (was array literal and `*deref`)
- Fixed: delegate transitions generate actual `child.dispatch()` calls with ordinal-to-event mapping (was TODO comments)
- Fixed: `in_state()` on terminal/unit states emits correct pattern without `{ .. }`
- Fixed: cross-SM `in_state` lookups work via state name fallback when state_id is empty
- Added: Rust reserved word escaping for identifiers (`r#type`, `r#match`, etc.)

### Sema validation

- Fixed: duplicate enum/flags member names now rejected
- Fixed: enum member values that overflow underlying type now rejected
- Removed: dead `ErrorKind` variants (`UndefinedField`, `UndefinedEvent`, `InvalidAnnotation`)

### wirespec-rt

- Added: `Error::Asn1Decode` and `Error::Asn1Encode` variants
- Added: signed type read/write methods (`read_i8`, `read_i16be/le`, etc.)

## New Examples

| Example | Protocol | Pattern |
|---|---|---|
| `examples/asn1/v2x_its.wspec` | ETSI ITS V2X | packet + UPER CAM |
| `examples/asn1/supl_example.wspec` | SUPL positioning | packet + UPER |
| `examples/asn1/lpp_transport.wspec` | 3GPP LPP | capsule + UPER |

All examples include real `.asn1` files and work with `--features asn1`.

## Testing

- 1020 tests (with `asn1` feature)
- 31 rustc compilation verifications (generated Rust compiles clean)
- 21 GCC compilation verifications (generated C compiles clean)
- End-to-end round-trip verified: parse(serialize(value)) == value with rasn

## Known Limitations

- C backend: indexed delegate dispatch (`src.paths[idx] <- ev`) not implemented
- `serialized_len()` re-encodes ASN.1 payload to measure size (performance tradeoff)
- Variant-internal length-prefixed ASN.1 length field recomputation not yet supported

## Upgrade Notes

- No breaking changes from v0.1.0
- New `asn1` feature flag on `wirespec-driver` (opt-in, off by default)
- `wirespec-rt` crate: two new `Error` variants added (non-breaking for match with `_` arm)

## Roadmap

| Version | Feature |
|---|---|
| ~~v0.1.0~~ | Wire formats, state machines, C/Rust codegen |
| **v0.2.0** | **ASN.1 / rasn integration** |
| v0.3.0 | TLA+ bounded verification |
