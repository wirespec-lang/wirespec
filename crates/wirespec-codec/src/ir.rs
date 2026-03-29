// crates/wirespec-codec/src/ir.rs
//
// Codec IR types -- flattened, self-contained, backend-neutral parse/serialize plan.
// No backreferences to Layout/Semantic IR.

use wirespec_sema::ir::*;
use wirespec_sema::types::*;
use wirespec_syntax::span::Span;

// -- Enums (spec S7) --

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldStrategy {
    Primitive,
    VarInt,
    ContVarInt,
    BytesFixed,
    BytesLength,
    BytesRemaining,
    BytesLor,
    Struct,
    Array,
    BitGroup,
    Conditional,
    Checksum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryTier {
    A, // zero-copy bytes view
    B, // materialized scalar array
    C, // materialized composite array
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Packet,
    FrameVariant,
    CapsuleHeader,
    CapsulePayloadVariant,
}

// -- Wire type for flattened fields (spec S10) --

#[derive(Debug, Clone, PartialEq)]
pub enum WireType {
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
    Bits(u16),
    VarInt,
    ContVarInt,
    Bytes,
    Struct(String),
    Enum(String),
    Frame(String),
    Capsule(String),
    Array,
}

// -- Expressions (spec S13) --

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueRefKind {
    Field,
    Derived,
    Const,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValueRef {
    pub value_id: String,
    pub kind: ValueRefKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiteralValue {
    Int(i64),
    Bool(bool),
    String(String),
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CodecExpr {
    ValueRef {
        reference: ValueRef,
    },
    Literal {
        value: LiteralValue,
    },
    Binary {
        op: String,
        left: Box<CodecExpr>,
        right: Box<CodecExpr>,
    },
    Unary {
        op: String,
        operand: Box<CodecExpr>,
    },
    Coalesce {
        expr: Box<CodecExpr>,
        default: Box<CodecExpr>,
    },
    InState {
        expr: Box<CodecExpr>,
        sm_id: String,
        sm_name: String,
        state_id: String,
        state_name: String,
    },
    Subscript {
        base: Box<CodecExpr>,
        index: Box<CodecExpr>,
    },
    StateConstructor {
        sm_id: String,
        sm_name: String,
        state_id: String,
        state_name: String,
        args: Vec<CodecExpr>,
    },
    Fill {
        value: Box<CodecExpr>,
        count: Box<CodecExpr>,
    },
    Slice {
        base: Box<CodecExpr>,
        start: Box<CodecExpr>,
        end: Box<CodecExpr>,
    },
    All {
        collection: Box<CodecExpr>,
        sm_id: String,
        sm_name: String,
        state_id: String,
        state_name: String,
    },
}

// -- Field substructures (spec S11) --

#[derive(Debug, Clone, PartialEq)]
pub enum BytesSpec {
    Fixed { size: u64 },
    Length { expr: CodecExpr },
    Remaining,
    LengthOrRemaining { expr: CodecExpr },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArraySpec {
    pub element_wire_type: WireType,
    pub element_strategy: FieldStrategy,
    pub element_ref_type_name: Option<String>,
    pub count_expr: Option<CodecExpr>,
    pub within_expr: Option<CodecExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BitgroupMember {
    pub group_id: u32,
    pub total_bits: u16,
    pub member_offset_bits: u16,
    pub member_width_bits: u16,
    pub group_endianness: Endianness,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodecTag {
    pub field_name: String,
    pub wire_type: WireType,
    pub endianness: Option<Endianness>,
    /// For VarInt/Struct tags: the referenced type name (e.g., "VarInt")
    pub ref_type_name: Option<String>,
}

// -- Variant pattern --

#[derive(Debug, Clone, PartialEq)]
pub enum VariantPattern {
    Exact { value: i64 },
    RangeInclusive { start: i64, end: i64 },
    Wildcard,
}

// -- Checksum (spec S16) --

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumVerifyMode {
    ZeroSum,
    RecomputeCompare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumInputModel {
    ZeroSumWholeScope,
    RecomputeWithSkippedField,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChecksumPlan {
    pub scope_kind: ScopeKind,
    pub scope_name: String,
    pub field_name: String,
    pub field_index_in_scope: u32,
    pub algorithm_id: String,
    pub verify_mode: ChecksumVerifyMode,
    pub input_model: ChecksumInputModel,
    pub field_width_bytes: u8,
    pub field_endianness: Option<Endianness>,
}

// -- Ordered items (spec S12) --

#[derive(Debug, Clone, PartialEq)]
pub enum CodecItem {
    Field { field_id: String },
    Derived(CodecDerivedItem),
    Require(CodecRequireItem),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodecDerivedItem {
    pub item_id: String,
    pub name: String,
    pub wire_type: WireType,
    pub expr: CodecExpr,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodecRequireItem {
    pub item_id: String,
    pub expr: CodecExpr,
    pub span: Option<Span>,
}

// -- Field (spec S10) --

#[derive(Debug, Clone, PartialEq)]
pub struct CodecField {
    pub field_id: String,
    pub scope_id: String,
    pub field_index: u32,
    pub name: String,
    pub wire_type: WireType,
    pub strategy: FieldStrategy,
    pub memory_tier: Option<MemoryTier>,
    pub endianness: Option<Endianness>,
    pub is_optional: bool,
    pub inner_wire_type: Option<WireType>,
    pub condition: Option<CodecExpr>,
    pub ref_type_name: Option<String>,
    pub bit_width: Option<u16>,
    pub bytes_spec: Option<BytesSpec>,
    pub array_spec: Option<ArraySpec>,
    pub bitgroup_member: Option<BitgroupMember>,
    pub max_elements: Option<u32>,
    pub checksum_algorithm: Option<String>,
    pub span: Option<Span>,
}

// -- Scopes (spec S9) --

#[derive(Debug, Clone, PartialEq)]
pub struct CodecPacket {
    pub scope_id: String,
    pub name: String,
    pub fields: Vec<CodecField>,
    pub items: Vec<CodecItem>,
    pub checksum_plan: Option<ChecksumPlan>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodecFrame {
    pub frame_id: String,
    pub name: String,
    pub tag: CodecTag,
    pub variants: Vec<CodecVariantScope>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodecVariantScope {
    pub scope_id: String,
    pub owner: VariantOwner,
    pub ordinal: u32,
    pub name: String,
    pub pattern: VariantPattern,
    pub fields: Vec<CodecField>,
    pub items: Vec<CodecItem>,
    pub checksum_plan: Option<ChecksumPlan>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodecCapsule {
    pub capsule_id: String,
    pub name: String,
    pub tag: CodecTag,
    pub within_field: String,
    pub tag_expr: Option<CodecExpr>,
    pub header_fields: Vec<CodecField>,
    pub header_items: Vec<CodecItem>,
    pub header_checksum_plan: Option<ChecksumPlan>,
    pub variants: Vec<CodecVariantScope>,
}

// -- Root (spec S8) --

#[derive(Debug, Clone, PartialEq)]
pub struct CodecModule {
    pub schema_version: String, // "codec-ir/v1"
    pub module_name: String,
    pub module_endianness: Endianness,
    pub compliance_profile: String,
    pub imports: Vec<ImportedTypeRef>,
    pub varints: Vec<SemanticVarInt>,
    pub consts: Vec<SemanticConst>,
    pub enums: Vec<SemanticEnum>,
    pub state_machines: Vec<SemanticStateMachine>,
    pub packets: Vec<CodecPacket>,
    pub frames: Vec<CodecFrame>,
    pub capsules: Vec<CodecCapsule>,
}
