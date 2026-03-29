// crates/wirespec-layout/src/ir.rs
use wirespec_sema::ir::*;
use wirespec_sema::types::*;
use wirespec_syntax::span::Span;

// ── Root ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutModule {
    pub schema_version: String,         // "layout-ir/v1"
    pub compliance_profile: String,
    pub module_name: String,
    pub module_endianness: Endianness,
    pub imports: Vec<ImportedTypeRef>,
    pub varints: Vec<SemanticVarInt>,
    pub consts: Vec<SemanticConst>,
    pub enums: Vec<SemanticEnum>,
    pub packets: Vec<LayoutPacket>,
    pub frames: Vec<LayoutFrame>,
    pub capsules: Vec<LayoutCapsule>,
    pub state_machines: Vec<SemanticStateMachine>,
}

// ── Packet ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutPacket {
    pub packet_id: String,
    pub name: String,
    pub derive_traits: Vec<DeriveTrait>,
    pub fields: Vec<LayoutField>,
    pub derived: Vec<SemanticDerived>,
    pub requires: Vec<SemanticRequire>,
    pub items: Vec<SemanticScopeItem>,
    pub bitgroups: Vec<LayoutBitGroup>,
    pub span: Option<Span>,
}

// ── Frame ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutFrame {
    pub frame_id: String,
    pub name: String,
    pub derive_traits: Vec<DeriveTrait>,
    pub tag_name: String,
    pub tag_type: SemanticType,
    pub tag_endianness: Option<Endianness>,
    pub variants: Vec<LayoutVariantScope>,
    pub span: Option<Span>,
}

// ── Capsule ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutCapsule {
    pub capsule_id: String,
    pub name: String,
    pub derive_traits: Vec<DeriveTrait>,
    pub tag_type: SemanticType,
    pub tag_selector: CapsuleTagSelector,
    pub within_field_id: String,
    pub within_field_name: String,
    pub header_fields: Vec<LayoutField>,
    pub header_derived: Vec<SemanticDerived>,
    pub header_requires: Vec<SemanticRequire>,
    pub header_items: Vec<SemanticScopeItem>,
    pub header_bitgroups: Vec<LayoutBitGroup>,
    pub variants: Vec<LayoutVariantScope>,
    pub span: Option<Span>,
}

// ── Variant Scope (shared by frame + capsule) ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutVariantScope {
    pub scope_id: String,
    pub owner: VariantOwner,
    pub variant_name: String,
    pub ordinal: u32,
    pub pattern: SemanticPattern,
    pub fields: Vec<LayoutField>,
    pub derived: Vec<SemanticDerived>,
    pub requires: Vec<SemanticRequire>,
    pub items: Vec<SemanticScopeItem>,
    pub bitgroups: Vec<LayoutBitGroup>,
    pub span: Option<Span>,
}

// ── Field ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutField {
    pub field_id: String,
    pub name: String,
    pub ty: SemanticType,
    pub presence: FieldPresence,
    pub max_elements: Option<u32>,
    pub checksum_algorithm: Option<String>,
    pub wire_width_bits: Option<u16>,
    pub endianness: Option<Endianness>,
    pub bitgroup_member: Option<LayoutBitGroupMemberRef>,
    pub span: Option<Span>,
}

// ── Bitgroup ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBitGroup {
    pub bitgroup_id: String,            // "<scope_id>.bitgroup[<index>]"
    pub scope_id: String,
    pub total_bits: u16,
    pub endianness: Endianness,
    pub members: Vec<LayoutBitGroupMember>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBitGroupMember {
    pub field_id: String,
    pub offset_bits: u16,
    pub width_bits: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBitGroupMemberRef {
    pub bitgroup_id: String,
    pub offset_bits: u16,
    pub width_bits: u16,
}
