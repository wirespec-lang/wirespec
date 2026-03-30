// crates/wirespec-sema/src/types.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endianness {
    Big,
    Little,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveWireType {
    U8,
    U16,
    U24,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    Bool,
    Bit,
}

impl PrimitiveWireType {
    /// Byte width of this primitive (Bit returns 0 — it's sub-byte).
    pub fn byte_width(self) -> u8 {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U24 => 3,
            Self::U32 | Self::I32 => 4,
            Self::U64 | Self::I64 => 8,
            Self::Bool | Self::Bit => 0,
        }
    }

    pub fn is_integer_like(self) -> bool {
        matches!(
            self,
            Self::U8
                | Self::U16
                | Self::U24
                | Self::U32
                | Self::U64
                | Self::I8
                | Self::I16
                | Self::I32
                | Self::I64
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticBytesKind {
    Fixed,
    Length,
    Remaining,
    LengthOrRemaining,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SemanticType {
    Primitive {
        wire: PrimitiveWireType,
        endianness: Option<Endianness>,
    },
    Bits {
        width_bits: u16,
    },
    VarIntRef {
        varint_id: String,
        name: String,
    },
    Bytes {
        bytes_kind: SemanticBytesKind,
        fixed_size: Option<u64>,
        size_expr: Option<Box<crate::expr::SemanticExpr>>,
    },
    Array {
        element_type: Box<SemanticType>,
        count_expr: Option<Box<crate::expr::SemanticExpr>>,
        within_expr: Option<Box<crate::expr::SemanticExpr>>,
    },
    PacketRef {
        packet_id: String,
        name: String,
    },
    EnumRef {
        enum_id: String,
        name: String,
        is_flags: bool,
    },
    FrameRef {
        frame_id: String,
        name: String,
    },
    CapsuleRef {
        capsule_id: String,
        name: String,
    },
}

impl SemanticType {
    /// Returns true if this type is integer-like (valid for array counts, byte lengths).
    pub fn is_integer_like(&self) -> bool {
        match self {
            Self::Primitive { wire, .. } => wire.is_integer_like(),
            Self::VarIntRef { .. } => true,
            Self::Bits { .. } => true,
            Self::EnumRef { .. } => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldPresence {
    Always,
    Conditional {
        condition: crate::expr::SemanticExpr,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeriveTrait {
    Debug,
    Compare,
}

/// ASN.1 extern declaration (module-level).
#[derive(Debug, Clone, PartialEq)]
pub struct Asn1ExternDecl {
    pub path: String,
    pub type_names: Vec<String>,
    pub span: Option<wirespec_syntax::span::Span>,
}

/// ASN.1 hint attached to a field for backend codegen.
#[derive(Debug, Clone, PartialEq)]
pub struct Asn1Hint {
    pub type_name: String,
    pub encoding: String,
    pub extern_path: String,
}
