// crates/wirespec-backend-c/src/type_map.rs
//
// WireType -> C type string mapping, cursor read/write function names.

use wirespec_codec::ir::*;
use wirespec_sema::types::Endianness;

/// Map Codec WireType to C type string for struct member declaration.
pub fn wire_type_to_c(wt: &WireType, prefix: &str) -> String {
    match wt {
        WireType::U8 => "uint8_t".into(),
        WireType::U16 => "uint16_t".into(),
        WireType::U24 => "uint32_t".into(),
        WireType::U32 => "uint32_t".into(),
        WireType::U64 => "uint64_t".into(),
        WireType::I8 => "int8_t".into(),
        WireType::I16 => "int16_t".into(),
        WireType::I32 => "int32_t".into(),
        WireType::I64 => "int64_t".into(),
        WireType::Bool => "bool".into(),
        WireType::Bit => "uint8_t".into(),
        WireType::Bits(n) => {
            if *n <= 8 {
                "uint8_t".into()
            } else if *n <= 16 {
                "uint16_t".into()
            } else if *n <= 32 {
                "uint32_t".into()
            } else {
                "uint64_t".into()
            }
        }
        WireType::VarInt | WireType::ContVarInt => "uint64_t".into(),
        WireType::Bytes => "wirespec_bytes_t".into(),
        WireType::Struct(name) | WireType::Frame(name) | WireType::Capsule(name) => {
            crate::names::c_type_name(prefix, name)
        }
        WireType::Enum(name) => crate::names::c_type_name(prefix, name),
        WireType::Array => "/* array */".into(),
    }
}

/// C cursor-read function for a primitive/endian combo.
pub fn cursor_read_fn(wt: &WireType, endianness: Option<Endianness>) -> &'static str {
    match (wt, endianness) {
        (WireType::U8, _) | (WireType::Bool, _) | (WireType::Bit, _) => "wirespec_cursor_read_u8",
        (WireType::U16, Some(Endianness::Little)) => "wirespec_cursor_read_u16le",
        (WireType::U16, _) => "wirespec_cursor_read_u16be",
        (WireType::U24, Some(Endianness::Little)) => "wirespec_cursor_read_u24le",
        (WireType::U24, _) => "wirespec_cursor_read_u24be",
        (WireType::U32, Some(Endianness::Little)) => "wirespec_cursor_read_u32le",
        (WireType::U32, _) => "wirespec_cursor_read_u32be",
        (WireType::U64, Some(Endianness::Little)) => "wirespec_cursor_read_u64le",
        (WireType::U64, _) => "wirespec_cursor_read_u64be",
        (WireType::I8, _) => "wirespec_cursor_read_u8",
        (WireType::I16, Some(Endianness::Little)) => "wirespec_cursor_read_u16le",
        (WireType::I16, _) => "wirespec_cursor_read_u16be",
        (WireType::I32, Some(Endianness::Little)) => "wirespec_cursor_read_u32le",
        (WireType::I32, _) => "wirespec_cursor_read_u32be",
        (WireType::I64, Some(Endianness::Little)) => "wirespec_cursor_read_u64le",
        (WireType::I64, _) => "wirespec_cursor_read_u64be",
        _ => unreachable!("unexpected wire type for cursor_read_fn: {:?}", wt),
    }
}

/// C buffer-write function for a primitive/endian combo.
pub fn write_fn(wt: &WireType, endianness: Option<Endianness>) -> &'static str {
    match (wt, endianness) {
        (WireType::U8, _) | (WireType::Bool, _) | (WireType::Bit, _) => "wirespec_write_u8",
        (WireType::U16, Some(Endianness::Little)) => "wirespec_write_u16le",
        (WireType::U16, _) => "wirespec_write_u16be",
        (WireType::U24, Some(Endianness::Little)) => "wirespec_write_u24le",
        (WireType::U24, _) => "wirespec_write_u24be",
        (WireType::U32, Some(Endianness::Little)) => "wirespec_write_u32le",
        (WireType::U32, _) => "wirespec_write_u32be",
        (WireType::U64, Some(Endianness::Little)) => "wirespec_write_u64le",
        (WireType::U64, _) => "wirespec_write_u64be",
        (WireType::I8, _) => "wirespec_write_u8",
        (WireType::I16, Some(Endianness::Little)) => "wirespec_write_u16le",
        (WireType::I16, _) => "wirespec_write_u16be",
        (WireType::I32, Some(Endianness::Little)) => "wirespec_write_u32le",
        (WireType::I32, _) => "wirespec_write_u32be",
        (WireType::I64, Some(Endianness::Little)) => "wirespec_write_u64le",
        (WireType::I64, _) => "wirespec_write_u64be",
        _ => unreachable!("unexpected wire type for write_fn: {:?}", wt),
    }
}

/// Return the byte width of a WireType for the bitgroup read function.
pub fn bitgroup_read_fn(total_bits: u16, endianness: Endianness) -> &'static str {
    match (total_bits, endianness) {
        (1..=8, _) => "wirespec_cursor_read_u8",
        (9..=16, Endianness::Little) => "wirespec_cursor_read_u16le",
        (9..=16, Endianness::Big) => "wirespec_cursor_read_u16be",
        (17..=32, Endianness::Little) => "wirespec_cursor_read_u32le",
        (17..=32, Endianness::Big) => "wirespec_cursor_read_u32be",
        _ => unreachable!("unexpected bitgroup size for read: {}", total_bits),
    }
}

/// Return the write function for a bitgroup.
pub fn bitgroup_write_fn(total_bits: u16, endianness: Endianness) -> &'static str {
    match (total_bits, endianness) {
        (1..=8, _) => "wirespec_write_u8",
        (9..=16, Endianness::Little) => "wirespec_write_u16le",
        (9..=16, Endianness::Big) => "wirespec_write_u16be",
        (17..=32, Endianness::Little) => "wirespec_write_u32le",
        (17..=32, Endianness::Big) => "wirespec_write_u32be",
        _ => unreachable!("unexpected bitgroup size for write: {}", total_bits),
    }
}

/// C type for the bitgroup container variable.
pub fn bitgroup_c_type(total_bits: u16) -> &'static str {
    match total_bits {
        1..=8 => "uint8_t",
        9..=16 => "uint16_t",
        17..=32 => "uint32_t",
        _ => "uint64_t",
    }
}

/// Byte width of a wire type (for serialized_len calculation).
pub fn wire_type_byte_width(wt: &WireType) -> Option<usize> {
    match wt {
        WireType::U8 | WireType::I8 | WireType::Bool | WireType::Bit => Some(1),
        WireType::U16 | WireType::I16 => Some(2),
        WireType::U24 => Some(3),
        WireType::U32 | WireType::I32 => Some(4),
        WireType::U64 | WireType::I64 => Some(8),
        _ => None,
    }
}
