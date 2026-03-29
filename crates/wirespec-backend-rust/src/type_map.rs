// crates/wirespec-backend-rust/src/type_map.rs
//
// WireType -> Rust type string mapping, cursor read/write method names.

use wirespec_codec::ir::*;
use wirespec_sema::types::Endianness;

use crate::names::to_pascal_case;

/// Map Codec WireType to Rust type string for struct field declaration.
/// `needs_lifetime` is set to true if the type requires `<'a>`.
pub fn wire_type_to_rust(wt: &WireType) -> &'static str {
    match wt {
        WireType::U8 => "u8",
        WireType::U16 => "u16",
        WireType::U24 => "u32",
        WireType::U32 => "u32",
        WireType::U64 => "u64",
        WireType::I8 => "i8",
        WireType::I16 => "i16",
        WireType::I32 => "i32",
        WireType::I64 => "i64",
        WireType::Bool => "bool",
        WireType::Bit => "u8",
        WireType::Bits(n) => {
            if *n <= 8 {
                "u8"
            } else if *n <= 16 {
                "u16"
            } else if *n <= 32 {
                "u32"
            } else {
                "u64"
            }
        }
        WireType::VarInt | WireType::ContVarInt => "u64",
        WireType::Bytes => "BYTES", // placeholder; handled specially
        WireType::Array => "ARRAY", // placeholder; handled specially
        // Struct/Frame/Capsule/Enum are handled by wire_type_to_rust_named
        _ => "UNKNOWN",
    }
}

/// Get a Rust type name for named types (Struct, Frame, Capsule, Enum).
pub fn wire_type_to_rust_named(wt: &WireType) -> Option<String> {
    match wt {
        WireType::Struct(name)
        | WireType::Frame(name)
        | WireType::Capsule(name)
        | WireType::Enum(name) => Some(to_pascal_case(name)),
        _ => None,
    }
}

/// Whether a wire type needs a lifetime parameter.
pub fn wire_type_needs_lifetime(wt: &WireType) -> bool {
    matches!(wt, WireType::Bytes)
}

/// Check if a named type (Struct/Frame/Capsule) might need a lifetime.
/// This is a conservative check; the caller should track actual fields.
pub fn wire_type_is_named(wt: &WireType) -> bool {
    matches!(
        wt,
        WireType::Struct(_) | WireType::Frame(_) | WireType::Capsule(_)
    )
}

/// Cursor read method call for a primitive/endian combo.
/// Returns the method name on the Cursor type (e.g., "read_u16be").
pub fn cursor_read_method(wt: &WireType, endianness: Option<Endianness>) -> &'static str {
    match (wt, endianness) {
        (WireType::U8, _) | (WireType::Bool, _) | (WireType::Bit, _) => "read_u8",
        (WireType::I8, _) => "read_i8",
        (WireType::U16, Some(Endianness::Little)) => "read_u16le",
        (WireType::I16, Some(Endianness::Little)) => "read_i16le",
        (WireType::U16, _) => "read_u16be",
        (WireType::I16, _) => "read_i16be",
        (WireType::U24, Some(Endianness::Little)) => "read_u24le",
        (WireType::U24, _) => "read_u24be",
        (WireType::U32, Some(Endianness::Little)) => "read_u32le",
        (WireType::I32, Some(Endianness::Little)) => "read_i32le",
        (WireType::U32, _) => "read_u32be",
        (WireType::I32, _) => "read_i32be",
        (WireType::U64, Some(Endianness::Little)) => "read_u64le",
        (WireType::I64, Some(Endianness::Little)) => "read_i64le",
        (WireType::U64, _) => "read_u64be",
        (WireType::I64, _) => "read_i64be",
        _ => unreachable!("unexpected wire type for cursor_read_method: {:?}", wt),
    }
}

/// Writer write method call for a primitive/endian combo.
/// Returns the method name on the Writer type (e.g., "write_u16be").
pub fn writer_write_method(wt: &WireType, endianness: Option<Endianness>) -> &'static str {
    match (wt, endianness) {
        (WireType::U8, _) | (WireType::Bool, _) | (WireType::Bit, _) => "write_u8",
        (WireType::I8, _) => "write_i8",
        (WireType::U16, Some(Endianness::Little)) => "write_u16le",
        (WireType::I16, Some(Endianness::Little)) => "write_i16le",
        (WireType::U16, _) => "write_u16be",
        (WireType::I16, _) => "write_i16be",
        (WireType::U24, Some(Endianness::Little)) => "write_u24le",
        (WireType::U24, _) => "write_u24be",
        (WireType::U32, Some(Endianness::Little)) => "write_u32le",
        (WireType::I32, Some(Endianness::Little)) => "write_i32le",
        (WireType::U32, _) => "write_u32be",
        (WireType::I32, _) => "write_i32be",
        (WireType::U64, Some(Endianness::Little)) => "write_u64le",
        (WireType::I64, Some(Endianness::Little)) => "write_i64le",
        (WireType::U64, _) => "write_u64be",
        (WireType::I64, _) => "write_i64be",
        _ => unreachable!("unexpected wire type for writer_write_method: {:?}", wt),
    }
}

/// Bitgroup read method name.
pub fn bitgroup_read_method(total_bits: u16, endianness: Endianness) -> &'static str {
    match (total_bits, endianness) {
        (1..=8, _) => "read_u8",
        (9..=16, Endianness::Little) => "read_u16le",
        (9..=16, Endianness::Big) => "read_u16be",
        (17..=32, Endianness::Little) => "read_u32le",
        (17..=32, Endianness::Big) => "read_u32be",
        _ => unreachable!("unexpected bitgroup size for read: {}", total_bits),
    }
}

/// Bitgroup write method name.
pub fn bitgroup_write_method(total_bits: u16, endianness: Endianness) -> &'static str {
    match (total_bits, endianness) {
        (1..=8, _) => "write_u8",
        (9..=16, Endianness::Little) => "write_u16le",
        (9..=16, Endianness::Big) => "write_u16be",
        (17..=32, Endianness::Little) => "write_u32le",
        (17..=32, Endianness::Big) => "write_u32be",
        _ => unreachable!("unexpected bitgroup size for write: {}", total_bits),
    }
}

/// Rust type for the bitgroup container variable.
pub fn bitgroup_rust_type(total_bits: u16) -> &'static str {
    match total_bits {
        1..=8 => "u8",
        9..=16 => "u16",
        17..=32 => "u32",
        _ => "u64",
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
