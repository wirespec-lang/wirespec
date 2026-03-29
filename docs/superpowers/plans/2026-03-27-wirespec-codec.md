# wirespec-codec Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the Codec IR stage that lowers Layout IR into a self-contained, backend-neutral parse/serialize plan — assigning field strategies, memory tiers, checksum plans, and flattened execution items that backends consume without reading upstream IRs.

**Architecture:** Single-pass lowering consuming `LayoutModule` (plus upstream semantic data carried by value) and producing `CodecModule`. For each scope, the pass: (1) assigns field strategy (priority-based), (2) assigns memory tier, (3) flattens field data into `CodecField` with all needed info, (4) builds ordered `CodecItem` list, (5) synthesizes `ChecksumPlan` if any checksum field exists. The Codec IR is fully self-contained — no backreferences to Layout/Semantic IR.

**Tech Stack:** Rust (edition 2024), `wirespec-layout` crate (Layout IR types)

**Normative spec:** `docs/ref/CODEC_IR_SPEC.md`

---

## File Structure

```
crates/wirespec-codec/
├── Cargo.toml                      # Depends on wirespec-layout (which re-exports sema types)
├── src/
│   ├── lib.rs                      # Crate root, public API: lower()
│   ├── ir.rs                       # Codec IR types (CodecModule, CodecField, CodecExpr, etc.)
│   ├── strategy.rs                 # FieldStrategy + MemoryTier assignment logic
│   ├── lower.rs                    # Main lowering: LayoutModule → CodecModule
│   └── checksum.rs                 # ChecksumPlan synthesis
└── tests/
    ├── codec_tests.rs              # Integration tests: full pipeline → CodecModule
    ├── strategy_tests.rs           # Strategy assignment edge cases
    └── corpus_codec_tests.rs       # Real .wspec files through full pipeline
```

| File | Responsibility |
|------|---------------|
| `ir.rs` | All Codec IR types from spec §8–§13: `CodecModule`, `CodecField`, `CodecExpr`, `FieldStrategy`, `MemoryTier`, `ChecksumPlan`, etc. |
| `strategy.rs` | `assign_strategy()` + `assign_memory_tier()` — priority-based per spec §14–§15 |
| `lower.rs` | `lower()` entry, scope lowering, field flattening, expression conversion |
| `checksum.rs` | `synthesize_checksum_plan()` — builds `ChecksumPlan` from checksum-annotated fields |

---

## Chunk 1: Codec IR Types

### Task 1: All Codec IR type definitions

**Files:**
- Create: `crates/wirespec-codec/src/ir.rs`

- [ ] **Step 1: Write the types**

The Codec IR is "flattened" — each `CodecField` carries all info a backend needs directly, without backreferences. Key types from spec §7–§13:

```rust
// crates/wirespec-codec/src/ir.rs
use wirespec_layout::ir::LayoutBitGroupMember;
use wirespec_sema::ir::*;
use wirespec_sema::types::*;
use wirespec_syntax::span::Span;

// ── Enums (spec §7) ──

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

// ── Wire type for flattened fields (spec §10) ──

#[derive(Debug, Clone, PartialEq)]
pub enum WireType {
    U8, U16, U24, U32, U64,
    I8, I16, I32, I64,
    Bool, Bit,
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

// ── Expressions (spec §13) ──

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
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CodecExpr {
    ValueRef { reference: ValueRef },
    Literal { value: LiteralValue },
    Binary { op: String, left: Box<CodecExpr>, right: Box<CodecExpr> },
    Unary { op: String, operand: Box<CodecExpr> },
    Coalesce { expr: Box<CodecExpr>, default: Box<CodecExpr> },
    InState { expr: Box<CodecExpr>, sm_name: String, state_name: String },
    Subscript { base: Box<CodecExpr>, index: Box<CodecExpr> },
    StateConstructor { sm_name: String, state_name: String, args: Vec<CodecExpr> },
    Fill { value: Box<CodecExpr>, count: Box<CodecExpr> },
    Slice { base: Box<CodecExpr>, start: Box<CodecExpr>, end: Box<CodecExpr> },
    All { collection: Box<CodecExpr>, sm_name: String, state_name: String },
}

// ── Field substructures (spec §11) ──

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
}

// ── Variant pattern ──

#[derive(Debug, Clone, PartialEq)]
pub enum VariantPattern {
    Exact { value: i64 },
    RangeInclusive { start: i64, end: i64 },
    Wildcard,
}

// ── Checksum (spec §16) ──

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

// ── Ordered items (spec §12) ──

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

// ── Field (spec §10) ──

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

// ── Scopes (spec §9) ──

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

// ── Root (spec §8) ──

#[derive(Debug, Clone, PartialEq)]
pub struct CodecModule {
    pub schema_version: String,          // "codec-ir/v1"
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
```

- [ ] **Step 2: Wire up lib.rs, verify it compiles**

```rust
pub mod ir;
```

Run: `cargo build -p wirespec-codec`

- [ ] **Step 3: Commit**

---

## Chunk 2: Strategy Assignment + Checksum Plans

### Task 2: FieldStrategy and MemoryTier assignment

**Files:**
- Create: `crates/wirespec-codec/src/strategy.rs`
- Test: `crates/wirespec-codec/tests/strategy_tests.rs`

- [ ] **Step 1: Write failing tests**

```rust
// crates/wirespec-codec/tests/strategy_tests.rs
use wirespec_codec::strategy::*;
use wirespec_codec::ir::*;
use wirespec_sema::types::*;

#[test]
fn strategy_primitive_u8() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Primitive { wire: PrimitiveWireType::U8 },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Primitive);
}

#[test]
fn strategy_bitgroup_overrides_primitive() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Bits { width_bits: 4 },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: true,
    });
    assert_eq!(s, FieldStrategy::BitGroup);
}

#[test]
fn strategy_checksum_overrides_primitive() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Primitive { wire: PrimitiveWireType::U16 },
        is_optional: false,
        has_checksum: true,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Checksum);
}

#[test]
fn strategy_conditional() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Primitive { wire: PrimitiveWireType::U16 },
        is_optional: true,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Conditional);
}

#[test]
fn strategy_varint_ref() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::VarIntRef { varint_id: "varint:V".into(), name: "V".into() },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    // VarInt strategy (prefix-match or continuation) — assign_strategy returns VarInt
    assert_eq!(s, FieldStrategy::VarInt);
}

#[test]
fn strategy_bytes_remaining() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Bytes { bytes_kind: SemanticBytesKind::Remaining, fixed_size: None, size_expr: None },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::BytesRemaining);
}

#[test]
fn strategy_bytes_fixed() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Bytes { bytes_kind: SemanticBytesKind::Fixed, fixed_size: Some(6), size_expr: None },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::BytesFixed);
}

#[test]
fn strategy_struct_ref() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::PacketRef { packet_id: "packet:P".into(), name: "P".into() },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Struct);
}

#[test]
fn strategy_array() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Array { element_type: Box::new(SemanticType::Primitive { wire: PrimitiveWireType::U8 }), count_expr: None, within_expr: None },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Array);
}

#[test]
fn tier_bytes_is_a() {
    assert_eq!(assign_memory_tier(FieldStrategy::BytesFixed), Some(MemoryTier::A));
    assert_eq!(assign_memory_tier(FieldStrategy::BytesLength), Some(MemoryTier::A));
    assert_eq!(assign_memory_tier(FieldStrategy::BytesRemaining), Some(MemoryTier::A));
    assert_eq!(assign_memory_tier(FieldStrategy::BytesLor), Some(MemoryTier::A));
}

#[test]
fn tier_primitive_is_none() {
    assert_eq!(assign_memory_tier(FieldStrategy::Primitive), None);
    assert_eq!(assign_memory_tier(FieldStrategy::VarInt), None);
}
```

- [ ] **Step 2: Write implementation**

```rust
// crates/wirespec-codec/src/strategy.rs
use crate::ir::*;
use wirespec_sema::types::*;

pub struct StrategyInput<'a> {
    pub ty: &'a SemanticType,
    pub is_optional: bool,
    pub has_checksum: bool,
    pub has_bitgroup: bool,
}

/// Assign field strategy per spec §14 priority order.
pub fn assign_strategy(input: &StrategyInput) -> FieldStrategy {
    // Priority 1: bitgroup
    if input.has_bitgroup {
        return FieldStrategy::BitGroup;
    }
    // Priority 2: checksum
    if input.has_checksum {
        return FieldStrategy::Checksum;
    }
    // Priority 3: conditional
    if input.is_optional {
        return FieldStrategy::Conditional;
    }
    // Priority 4-9: type-based
    match input.ty {
        SemanticType::VarIntRef { .. } => FieldStrategy::VarInt,
        SemanticType::Bytes { bytes_kind, .. } => match bytes_kind {
            SemanticBytesKind::Fixed => FieldStrategy::BytesFixed,
            SemanticBytesKind::Length => FieldStrategy::BytesLength,
            SemanticBytesKind::Remaining => FieldStrategy::BytesRemaining,
            SemanticBytesKind::LengthOrRemaining => FieldStrategy::BytesLor,
        },
        SemanticType::PacketRef { .. }
        | SemanticType::FrameRef { .. }
        | SemanticType::CapsuleRef { .. }
        | SemanticType::EnumRef { .. } => FieldStrategy::Struct,
        SemanticType::Array { .. } => FieldStrategy::Array,
        SemanticType::Primitive { .. } | SemanticType::Bits { .. } => FieldStrategy::Primitive,
    }
}

/// Assign memory tier per spec §15.
pub fn assign_memory_tier(strategy: FieldStrategy) -> Option<MemoryTier> {
    match strategy {
        FieldStrategy::BytesFixed
        | FieldStrategy::BytesLength
        | FieldStrategy::BytesRemaining
        | FieldStrategy::BytesLor => Some(MemoryTier::A),
        // Array tier is determined by element type (B for scalar, C for composite)
        // — handled in lower.rs where element type is known
        _ => None,
    }
}

/// Assign array memory tier based on element type.
pub fn assign_array_memory_tier(element_type: &SemanticType) -> MemoryTier {
    match element_type {
        SemanticType::PacketRef { .. }
        | SemanticType::FrameRef { .. }
        | SemanticType::CapsuleRef { .. }
        | SemanticType::EnumRef { .. } => MemoryTier::C,
        _ => MemoryTier::B,
    }
}
```

- [ ] **Step 3: Add `pub mod strategy;` to lib.rs, run tests**

Run: `cargo test -p wirespec-codec --test strategy_tests`
Expected: PASS

- [ ] **Step 4: Commit**

---

### Task 3: Checksum plan synthesis

**Files:**
- Create: `crates/wirespec-codec/src/checksum.rs`

- [ ] **Step 1: Write implementation**

```rust
// crates/wirespec-codec/src/checksum.rs
use crate::ir::*;
use wirespec_sema::types::Endianness;

/// Synthesize a ChecksumPlan from a scope's fields.
/// Returns None if no field has a checksum annotation.
pub fn synthesize_checksum_plan(
    fields: &[CodecField],
    scope_kind: ScopeKind,
    scope_name: &str,
) -> Option<ChecksumPlan> {
    for (i, field) in fields.iter().enumerate() {
        if let Some(ref algorithm) = field.checksum_algorithm {
            let (verify_mode, input_model) = checksum_modes(algorithm);
            let field_width = checksum_field_width(algorithm);

            return Some(ChecksumPlan {
                scope_kind,
                scope_name: scope_name.to_string(),
                field_name: field.name.clone(),
                field_index_in_scope: i as u32,
                algorithm_id: algorithm.clone(),
                verify_mode,
                input_model,
                field_width_bytes: field_width,
                field_endianness: field.endianness,
            });
        }
    }
    None
}

fn checksum_modes(algorithm: &str) -> (ChecksumVerifyMode, ChecksumInputModel) {
    match algorithm {
        "internet" => (
            ChecksumVerifyMode::ZeroSum,
            ChecksumInputModel::ZeroSumWholeScope,
        ),
        "crc32" | "crc32c" | "fletcher16" => (
            ChecksumVerifyMode::RecomputeCompare,
            ChecksumInputModel::RecomputeWithSkippedField,
        ),
        _ => (
            ChecksumVerifyMode::RecomputeCompare,
            ChecksumInputModel::RecomputeWithSkippedField,
        ),
    }
}

fn checksum_field_width(algorithm: &str) -> u8 {
    match algorithm {
        "internet" | "fletcher16" => 2,
        "crc32" | "crc32c" => 4,
        _ => 0,
    }
}
```

- [ ] **Step 2: Add `pub mod checksum;` to lib.rs, verify build**

Run: `cargo build -p wirespec-codec`

- [ ] **Step 3: Commit**

---

## Chunk 3: Lowering Pass

### Task 4: Main lowering pass

**Files:**
- Create: `crates/wirespec-codec/src/lower.rs`
- Test: `crates/wirespec-codec/tests/codec_tests.rs`

- [ ] **Step 1: Write failing tests**

```rust
// crates/wirespec-codec/tests/codec_tests.rs
use wirespec_codec::lower::lower;
use wirespec_codec::ir::*;
use wirespec_sema::ComplianceProfile;
use wirespec_syntax::parse;

fn full_pipeline(src: &str) -> CodecModule {
    let ast = parse(src).unwrap();
    let sem = wirespec_sema::analyze(&ast, ComplianceProfile::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    lower(&layout).unwrap()
}

#[test]
fn codec_empty_module() {
    let c = full_pipeline("module test");
    assert_eq!(c.schema_version, "codec-ir/v1");
    assert_eq!(c.module_name, "test");
}

#[test]
fn codec_simple_packet() {
    let c = full_pipeline("packet P { x: u8, y: u16 }");
    assert_eq!(c.packets.len(), 1);
    assert_eq!(c.packets[0].fields.len(), 2);
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::Primitive);
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::U8);
    assert_eq!(c.packets[0].fields[1].wire_type, WireType::U16);
}

#[test]
fn codec_field_ids_stable() {
    let c = full_pipeline("packet P { x: u8, y: u16 }");
    assert_eq!(c.packets[0].fields[0].field_index, 0);
    assert_eq!(c.packets[0].fields[1].field_index, 1);
    assert!(c.packets[0].fields[0].field_id.contains("field[0]"));
}

#[test]
fn codec_bytes_strategies() {
    let c = full_pipeline("packet P { a: bytes[6], b: bytes[remaining] }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::BytesFixed);
    assert_eq!(c.packets[0].fields[0].memory_tier, Some(MemoryTier::A));
    assert_eq!(c.packets[0].fields[1].strategy, FieldStrategy::BytesRemaining);
}

#[test]
fn codec_bitgroup_strategy() {
    let c = full_pipeline("packet P { a: bits[4], b: bits[4], c: u16 }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::BitGroup);
    assert_eq!(c.packets[0].fields[1].strategy, FieldStrategy::BitGroup);
    assert_eq!(c.packets[0].fields[2].strategy, FieldStrategy::Primitive);
    assert!(c.packets[0].fields[0].bitgroup_member.is_some());
}

#[test]
fn codec_conditional_strategy() {
    let c = full_pipeline("packet P { flags: u8, x: if flags & 0x01 { u16 } }");
    assert_eq!(c.packets[0].fields[1].strategy, FieldStrategy::Conditional);
    assert!(c.packets[0].fields[1].is_optional);
    assert!(c.packets[0].fields[1].condition.is_some());
}

#[test]
fn codec_array_strategy() {
    let c = full_pipeline("packet P { count: u16, items: [u8; count] }");
    assert_eq!(c.packets[0].fields[1].strategy, FieldStrategy::Array);
    assert!(c.packets[0].fields[1].array_spec.is_some());
    assert_eq!(c.packets[0].fields[1].memory_tier, Some(MemoryTier::B));
}

#[test]
fn codec_items_order() {
    let c = full_pipeline("packet P { x: u8, require x > 0, let y: bool = x != 0 }");
    assert_eq!(c.packets[0].items.len(), 3);
    assert!(matches!(&c.packets[0].items[0], CodecItem::Field { .. }));
    assert!(matches!(&c.packets[0].items[1], CodecItem::Require(_)));
    assert!(matches!(&c.packets[0].items[2], CodecItem::Derived(_)));
}

#[test]
fn codec_frame() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A {},
            0x01 => B { x: u16 },
            _ => C { data: bytes[remaining] },
        }
    "#;
    let c = full_pipeline(src);
    assert_eq!(c.frames.len(), 1);
    assert_eq!(c.frames[0].tag.field_name, "tag");
    assert_eq!(c.frames[0].variants.len(), 3);
}

#[test]
fn codec_capsule() {
    let src = r#"
        capsule C {
            type_field: u8,
            length: u16,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                _ => U { data: bytes[remaining] },
            },
        }
    "#;
    let c = full_pipeline(src);
    assert_eq!(c.capsules.len(), 1);
    assert_eq!(c.capsules[0].header_fields.len(), 2);
}

#[test]
fn codec_consts_enums_pass_through() {
    let c = full_pipeline("const MAX: u8 = 20\nenum E: u8 { A = 0 }");
    assert_eq!(c.consts.len(), 1);
    assert_eq!(c.enums.len(), 1);
}

#[test]
fn codec_checksum_plan() {
    let src = r#"
        packet P {
            data: u32,
            @checksum(internet)
            checksum: u16,
        }
    "#;
    let c = full_pipeline(src);
    let plan = c.packets[0].checksum_plan.as_ref().unwrap();
    assert_eq!(plan.algorithm_id, "internet");
    assert_eq!(plan.verify_mode, ChecksumVerifyMode::ZeroSum);
    assert_eq!(plan.field_width_bytes, 2);
}

#[test]
fn codec_varint() {
    let src = r#"
        type VarInt = {
            prefix: bits[2],
            value: match prefix {
                0b00 => bits[6], 0b01 => bits[14],
                0b10 => bits[30], 0b11 => bits[62],
            },
        }
        packet P { x: VarInt }
    "#;
    let c = full_pipeline(src);
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::VarInt);
}
```

- [ ] **Step 2: Write the lowering implementation**

The `lower.rs` converts `LayoutModule` → `CodecModule`. Key operations:
1. For each packet/frame/capsule scope: flatten fields into `CodecField`
2. Each `CodecField` gets: `wire_type` (from `SemanticType`), `strategy` (from `assign_strategy`), `memory_tier`, `endianness`, flattened substructures
3. Convert `LayoutField.ty` → `WireType` (flattened enum)
4. Convert `SemanticExpr` → `CodecExpr` (structural clone with ID preservation)
5. Build `CodecItem` list from scope's `items` (the `SemanticScopeItem` list)
6. For derived: convert `SemanticDerived` → `CodecDerivedItem`
7. For require: convert `SemanticRequire` → `CodecRequireItem`
8. Synthesize `ChecksumPlan` per scope

Implementation should be ~300 lines following these patterns. Each scope follows the same flatten-fields → build-items → checksum-plan pattern.

- [ ] **Step 3: Add `pub mod lower;` + re-exports to lib.rs, run tests**

```rust
pub mod checksum;
pub mod ir;
pub mod lower;
pub mod strategy;

pub use lower::lower;
pub use ir::CodecModule;
```

Run: `cargo test -p wirespec-codec`
Expected: PASS

- [ ] **Step 4: Commit**

---

## Chunk 4: Corpus Tests

### Task 5: Corpus integration tests

**Files:**
- Create: `crates/wirespec-codec/tests/corpus_codec_tests.rs`

- [ ] **Step 1: Write corpus tests**

```rust
// crates/wirespec-codec/tests/corpus_codec_tests.rs
use wirespec_codec::lower;
use wirespec_sema::{analyze, ComplianceProfile};
use wirespec_syntax::parse;

fn codec_file(path: &str) -> wirespec_codec::ir::CodecModule {
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
    let ast = parse(&source).unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"));
    let sem = analyze(&ast, ComplianceProfile::Phase2ExtendedCurrent)
        .unwrap_or_else(|e| panic!("Failed to analyze {path}: {e}"));
    let layout = wirespec_layout::lower(&sem)
        .unwrap_or_else(|e| panic!("Failed to layout {path}: {e}"));
    lower(&layout).unwrap_or_else(|e| panic!("Failed to codec {path}: {e}"))
}

#[test]
fn corpus_quic_varint() {
    let c = codec_file("../../protospec/examples/quic/varint.wire");
    assert!(!c.varints.is_empty());
}

#[test]
fn corpus_udp() {
    let c = codec_file("../../protospec/examples/net/udp.wire");
    assert_eq!(c.packets.len(), 1);
    // UDP has require → items should include it
    assert!(c.packets[0].items.len() >= 5);
}

#[test]
fn corpus_tcp() {
    let c = codec_file("../../protospec/examples/net/tcp.wire");
    assert_eq!(c.packets.len(), 1);
    // TCP has bitgroup fields
    assert!(c.packets[0].fields.iter().any(|f| f.strategy == wirespec_codec::ir::FieldStrategy::BitGroup));
}

#[test]
fn corpus_ethernet() {
    let c = codec_file("../../protospec/examples/net/ethernet.wire");
    assert_eq!(c.packets.len(), 1);
}

#[test]
fn corpus_bits_groups() {
    let c = codec_file("../../protospec/examples/test/bits_groups.wire");
    assert_eq!(c.packets.len(), 2);
}

#[test]
fn corpus_ble_att() {
    let c = codec_file("../../protospec/examples/ble/att.wire");
    assert!(!c.frames.is_empty());
}

#[test]
fn corpus_mqtt() {
    let c = codec_file("../../protospec/examples/mqtt/mqtt.wire");
    assert!(!c.capsules.is_empty());
}
```

- [ ] **Step 2: Run corpus tests + full workspace**

Run: `cargo test --workspace`
Expected: All pass

- [ ] **Step 3: Commit**

---

## Summary

| Chunk | Tasks | What it delivers |
|-------|-------|-----------------|
| 1 | Task 1 | All Codec IR type definitions |
| 2 | Tasks 2-3 | Strategy assignment + checksum plan synthesis |
| 3 | Task 4 | Full lowering pass with 13 integration tests |
| 4 | Task 5 | 7 corpus tests through complete pipeline |

**Total test count target:** ~30 tests:
- Strategy assignment: 11 tests
- Integration (codec): 13 tests
- Corpus: 7 tests

**Pre-implementation fixes needed:**
1. **Add `state_machines` to `LayoutModule`** — `LayoutModule` currently lacks state machines. Add `pub state_machines: Vec<SemanticStateMachine>` to `LayoutModule` in `wirespec-layout/src/ir.rs` and propagate in `wirespec-layout/src/lower.rs`.
2. **Add Cargo.toml deps** — `wirespec-codec/Cargo.toml` must add `wirespec-sema` and `wirespec-syntax` dependencies since `ir.rs` imports their types.
3. **`ContVarInt` strategy** — The lowering pass must look up VarInt definitions from the module's `varints` list to determine encoding type. `VarIntEncoding::PrefixMatch` → `FieldStrategy::VarInt`, `VarIntEncoding::ContinuationBit` → `FieldStrategy::ContVarInt`.
