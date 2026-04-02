// crates/wirespec-sema/src/ir.rs
use crate::expr::*;
use crate::types::*;
use wirespec_syntax::span::Span;

// ── Warnings ──

#[derive(Debug, Clone, PartialEq)]
pub struct SemaWarning {
    pub kind: SemaWarningKind,
    pub msg: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemaWarningKind {
    SmUnreachableTerminal,
    SmUnreachableFromInitial,
}

// ── Root ──

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticModule {
    pub schema_version: String, // Fixed: "semantic-ir/v1"
    pub compliance_profile: String,
    pub module_name: String,
    pub module_endianness: Endianness,
    pub imports: Vec<ImportedTypeRef>,
    pub varints: Vec<SemanticVarInt>,
    pub consts: Vec<SemanticConst>,
    pub enums: Vec<SemanticEnum>,
    pub packets: Vec<SemanticPacket>,
    pub frames: Vec<SemanticFrame>,
    pub capsules: Vec<SemanticCapsule>,
    pub state_machines: Vec<SemanticStateMachine>,
    pub static_asserts: Vec<SemanticStaticAssert>,
    pub asn1_externs: Vec<crate::types::Asn1ExternDecl>,
    /// All items in declaration order, by ID.
    pub item_order: Vec<String>,
    pub warnings: Vec<SemaWarning>,
}

// ── Imports ──

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedTypeRef {
    pub import_id: String,
    pub name: String,
    pub source_module: String,
    pub source_prefix: String,
    pub decl_kind: ImportedDeclKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportedDeclKind {
    VarInt,
    Enum,
    Flags,
    Packet,
    Frame,
    Capsule,
}

// ── VarInt ──

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticVarInt {
    pub varint_id: String,
    pub name: String,
    pub encoding: VarIntEncoding,
    pub prefix_bits: Option<u8>,
    pub branches: Vec<SemanticVarIntBranch>,
    pub value_bits_per_byte: Option<u8>,
    pub max_bytes: u8,
    pub byte_order: Endianness,
    pub strict: bool,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticVarIntBranch {
    pub prefix_value: u64,
    pub prefix_bits: u8,
    pub value_bits: u8,
    pub total_bytes: u8,
    pub max_value: u64,
    pub prefix_mask: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarIntEncoding {
    PrefixMatch,
    ContinuationBit,
}

// ── Const / Enum ──

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticConst {
    pub const_id: String,
    pub name: String,
    pub ty: SemanticType,
    pub value: SemanticLiteral,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticEnum {
    pub enum_id: String,
    pub name: String,
    pub underlying_type: SemanticType,
    pub is_flags: bool,
    pub derive_traits: Vec<DeriveTrait>,
    pub members: Vec<SemanticEnumMember>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticEnumMember {
    pub member_id: String,
    pub name: String,
    pub value: i64,
    pub span: Option<Span>,
}

// ── Scope Items ──

#[derive(Debug, Clone, PartialEq)]
pub enum SemanticScopeItem {
    Field { field_id: String },
    Derived { derived_id: String },
    Require { require_id: String },
}

// ── Fields ──

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticField {
    pub field_id: String,
    pub name: String,
    pub ty: SemanticType,
    pub presence: FieldPresence,
    pub max_elements: Option<u32>,
    pub checksum_algorithm: Option<String>,
    pub asn1_hint: Option<crate::types::Asn1Hint>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticDerived {
    pub derived_id: String,
    pub name: String,
    pub ty: SemanticType,
    pub expr: SemanticExpr,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticRequire {
    pub require_id: String,
    pub expr: SemanticExpr,
    pub span: Option<Span>,
}

// ── Packet ──

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticPacket {
    pub packet_id: String,
    pub name: String,
    pub derive_traits: Vec<DeriveTrait>,
    pub fields: Vec<SemanticField>,
    pub derived: Vec<SemanticDerived>,
    pub requires: Vec<SemanticRequire>,
    pub items: Vec<SemanticScopeItem>,
    pub span: Option<Span>,
}

// ── Frame / Variant ──

#[derive(Debug, Clone, PartialEq)]
pub enum SemanticPattern {
    Exact { value: i64 },
    RangeInclusive { start: i64, end: i64 },
    Wildcard,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariantOwner {
    Frame { frame_id: String },
    CapsulePayload { capsule_id: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticVariantScope {
    pub scope_id: String,
    pub owner: VariantOwner,
    pub variant_name: String,
    pub ordinal: u32,
    pub pattern: SemanticPattern,
    pub fields: Vec<SemanticField>,
    pub derived: Vec<SemanticDerived>,
    pub requires: Vec<SemanticRequire>,
    pub items: Vec<SemanticScopeItem>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticFrame {
    pub frame_id: String,
    pub name: String,
    pub derive_traits: Vec<DeriveTrait>,
    pub tag_name: String,
    pub tag_type: SemanticType,
    pub variants: Vec<SemanticVariantScope>,
    pub span: Option<Span>,
}

// ── Capsule ──

#[derive(Debug, Clone, PartialEq)]
pub enum CapsuleTagSelector {
    Field {
        field_id: String,
        field_name: String,
    },
    Expr {
        expr: SemanticExpr,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticCapsule {
    pub capsule_id: String,
    pub name: String,
    pub derive_traits: Vec<DeriveTrait>,
    pub tag_type: SemanticType,
    pub tag_selector: CapsuleTagSelector,
    pub within_field_id: String,
    pub within_field_name: String,
    pub header_fields: Vec<SemanticField>,
    pub header_derived: Vec<SemanticDerived>,
    pub header_requires: Vec<SemanticRequire>,
    pub header_items: Vec<SemanticScopeItem>,
    pub variants: Vec<SemanticVariantScope>,
    pub span: Option<Span>,
}

// ── Static Assert ──

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticStaticAssert {
    pub assert_id: String,
    pub expr: SemanticExpr,
    pub span: Option<Span>,
}

// ── State Machine ──

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticStateMachine {
    pub sm_id: String,
    pub name: String,
    pub derive_traits: Vec<DeriveTrait>,
    pub states: Vec<SemanticState>,
    pub events: Vec<SemanticEvent>,
    pub initial_state_id: String,
    pub transitions: Vec<SemanticTransition>,
    pub has_child_state_changed: bool,
    pub verify_bound: Option<u32>,
    pub verify_declarations: Vec<SemanticVerifyDecl>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SemanticVerifyDecl {
    NoDeadlock,
    AllReachClosed,
    Property {
        name: String,
        formula: SemanticVerifyFormula,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum SemanticVerifyFormula {
    InState {
        state_name: String,
    },
    Not {
        inner: Box<SemanticVerifyFormula>,
    },
    And {
        left: Box<SemanticVerifyFormula>,
        right: Box<SemanticVerifyFormula>,
    },
    Or {
        left: Box<SemanticVerifyFormula>,
        right: Box<SemanticVerifyFormula>,
    },
    Implies {
        left: Box<SemanticVerifyFormula>,
        right: Box<SemanticVerifyFormula>,
    },
    Always {
        inner: Box<SemanticVerifyFormula>,
    },
    Eventually {
        inner: Box<SemanticVerifyFormula>,
    },
    LeadsTo {
        left: Box<SemanticVerifyFormula>,
        right: Box<SemanticVerifyFormula>,
    },
    FieldRef {
        field_name: String,
    },
    Literal {
        value: i64,
    },
    BoolLiteral {
        value: bool,
    },
    Compare {
        left: Box<SemanticVerifyFormula>,
        op: String,
        right: Box<SemanticVerifyFormula>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticState {
    pub state_id: String,
    pub name: String,
    pub fields: Vec<SemanticStateField>,
    pub is_terminal: bool,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticStateField {
    pub field_id: String,
    pub name: String,
    pub ty: SemanticType,
    pub default_value: Option<SemanticLiteral>,
    pub child_sm_id: Option<String>,
    pub child_sm_name: Option<String>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticEvent {
    pub event_id: String,
    pub name: String,
    pub params: Vec<SemanticEventParam>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticEventParam {
    pub param_id: String,
    pub name: String,
    pub ty: SemanticType,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticTransition {
    pub transition_id: String,
    pub src_state_id: String,
    pub src_state_name: String,
    pub dst_state_id: String,
    pub dst_state_name: String,
    pub event_id: String,
    pub event_name: String,
    pub guard: Option<SemanticExpr>,
    pub actions: Vec<SemanticAction>,
    pub delegate: Option<SemanticDelegate>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticAction {
    pub action_id: String,
    pub target: SemanticExpr,
    pub op: String,
    pub value: SemanticExpr,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticDelegate {
    pub target: SemanticExpr,
    pub event_id: String,
    pub event_name: String,
    pub span: Option<Span>,
}
