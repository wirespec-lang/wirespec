# wirespec-sema Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement semantic analysis that lowers parsed AST into a fully name-resolved, type-checked Semantic IR — the first backend-neutral meaning-resolved stage of the wirespec compiler pipeline.

**Architecture:** Two-pass analysis (type registry → lowering) consuming `wirespec_syntax::ast::AstModule` and producing `SemanticModule`. Pass 1 registers all type declarations. Pass 2 resolves names, checks types, validates constraints, and emits canonical Semantic IR with stable IDs. Compliance profiles gate checksum algorithm/scope legality at the semantic boundary.

**Tech Stack:** Rust (edition 2024), `wirespec-syntax` crate (AST + Span types)

**Normative specs:**
- `docs/ref/SEMANTIC_IR_SPEC.md` — IR types, stable IDs, expression model
- `docs/ref/COMPLIANCE_PROFILE_SPEC.md` — profile IDs, enforcement
- `docs/ref/CHECKSUM_ARCHITECTURE_SPEC.md` — checksum validation
- `docs/ref/wirespec_spec_v1.0.md` §3–§4 — language rules

**Reference implementation:**
- `protospec/wirespec/semantic_ir.py` — Python IR types
- `protospec/wirespec/semantic_analyzer.py` — Python analyzer (two-pass)

---

## File Structure

```
crates/wirespec-sema/
├── Cargo.toml                      # Add no new deps beyond wirespec-syntax
├── src/
│   ├── lib.rs                      # Crate root, public API: analyze()
│   ├── ir.rs                       # Semantic IR types (SemanticModule, SemanticField, etc.)
│   ├── types.rs                    # SemanticType, PrimitiveWireType, Endianness, etc.
│   ├── expr.rs                     # SemanticExpr, ValueRef, SemanticLiteral
│   ├── profile.rs                  # ComplianceProfile, checksum algorithm catalog
│   ├── error.rs                    # SemaError, ErrorKind enum
│   ├── analyzer.rs                 # Main two-pass analyzer: registry + lowering
│   ├── resolve.rs                  # Name resolution: type registry, field scope tracking
│   └── validate.rs                 # Validation rules: forward refs, remaining-is-last, checksums
└── tests/
    ├── analyze_tests.rs            # Integration tests: full AST → SemanticModule
    ├── type_resolution_tests.rs    # Type resolution edge cases
    ├── validation_tests.rs         # Error case tests
    └── checksum_tests.rs           # Checksum + profile gating tests
```

**Responsibilities per file:**

| File | Responsibility |
|------|---------------|
| `ir.rs` | All Semantic IR struct/enum types from SEMANTIC_IR_SPEC §6–§12 |
| `types.rs` | `SemanticType`, `PrimitiveWireType`, `Endianness`, `SemanticBytesKind`, `FieldPresence` |
| `expr.rs` | `SemanticExpr`, `ValueRef`, `TransitionPeerRef`, `SemanticLiteral` |
| `profile.rs` | `ComplianceProfile` enum, checksum algorithm table, scope legality |
| `error.rs` | `SemaError` with `ErrorKind`, span, context stack |
| `analyzer.rs` | `Analyzer` struct with pass 1 (registry) + pass 2 (lowering) |
| `resolve.rs` | `TypeRegistry` (name → DeclKind), field scope tracker, alias resolution |
| `validate.rs` | `validate_remaining_is_last`, `validate_forward_refs`, `validate_single_checksum`, `validate_lor_field` |

---

## Chunk 1: IR Types and Error Infrastructure

This chunk defines all data types that the analyzer will produce and the error types it will emit. No analysis logic yet — pure type definitions.

### Task 1: Semantic Type Model

**Files:**
- Create: `crates/wirespec-sema/src/types.rs`
- Test: `crates/wirespec-sema/tests/type_resolution_tests.rs`

- [ ] **Step 1: Write the types module**

```rust
// crates/wirespec-sema/src/types.rs
use wirespec_syntax::span::Span;

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
            Self::Primitive { wire } => wire.is_integer_like(),
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
```

- [ ] **Step 2: Write a basic compilation test**

```rust
// crates/wirespec-sema/tests/type_resolution_tests.rs
use wirespec_sema::types::*;

#[test]
fn primitive_byte_widths() {
    assert_eq!(PrimitiveWireType::U8.byte_width(), 1);
    assert_eq!(PrimitiveWireType::U16.byte_width(), 2);
    assert_eq!(PrimitiveWireType::U32.byte_width(), 4);
    assert_eq!(PrimitiveWireType::U64.byte_width(), 8);
    assert_eq!(PrimitiveWireType::Bit.byte_width(), 0);
}

#[test]
fn integer_like_checks() {
    assert!(PrimitiveWireType::U8.is_integer_like());
    assert!(PrimitiveWireType::U64.is_integer_like());
    assert!(!PrimitiveWireType::Bool.is_integer_like());
    assert!(!PrimitiveWireType::Bit.is_integer_like());
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p wirespec-sema`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/wirespec-sema/src/types.rs crates/wirespec-sema/tests/type_resolution_tests.rs
git commit -m "feat(sema): add SemanticType, PrimitiveWireType, Endianness types"
```

---

### Task 2: Semantic Expression Model

**Files:**
- Create: `crates/wirespec-sema/src/expr.rs`

- [ ] **Step 1: Write the expression types**

```rust
// crates/wirespec-sema/src/expr.rs

/// Fully name-resolved reference to a value (field, derived, const, state field).
#[derive(Debug, Clone, PartialEq)]
pub struct ValueRef {
    pub value_id: String,
    pub kind: ValueRefKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueRefKind {
    Field,
    Derived,
    Const,
    StateField,
}

/// Reference to src/dst/event-param in a state machine transition.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionPeerRef {
    pub peer: TransitionPeerKind,
    pub event_param_id: Option<String>,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionPeerKind {
    Src,
    Dst,
    EventParam,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SemanticLiteral {
    Int(i64),
    Bool(bool),
    Null,
}

/// Fully name-resolved semantic expression.
/// Bare field names are forbidden — all references use ValueRef or TransitionPeerRef.
#[derive(Debug, Clone, PartialEq)]
pub enum SemanticExpr {
    Literal {
        value: SemanticLiteral,
    },
    ValueRef {
        reference: ValueRef,
    },
    TransitionPeerRef {
        reference: TransitionPeerRef,
    },
    Binary {
        op: String,
        left: Box<SemanticExpr>,
        right: Box<SemanticExpr>,
    },
    Unary {
        op: String,
        operand: Box<SemanticExpr>,
    },
    Coalesce {
        expr: Box<SemanticExpr>,
        default: Box<SemanticExpr>,
    },
    InState {
        expr: Box<SemanticExpr>,
        sm_id: String,
        sm_name: String,
        state_id: String,
        state_name: String,
    },
    Subscript {
        base: Box<SemanticExpr>,
        index: Box<SemanticExpr>,
    },
    StateConstructor {
        sm_id: String,
        sm_name: String,
        state_id: String,
        state_name: String,
        args: Vec<SemanticExpr>,
    },
    Fill {
        value: Box<SemanticExpr>,
        count: Box<SemanticExpr>,
    },
    Slice {
        base: Box<SemanticExpr>,
        start: Box<SemanticExpr>,
        end: Box<SemanticExpr>,
    },
    All {
        collection: Box<SemanticExpr>,
        sm_id: String,
        sm_name: String,
        state_id: String,
        state_name: String,
    },
}

// Note: op strings match AST BinOp/UnaryOp display forms ("+", "-", "==", "!", etc.)
// This follows SEMANTIC_IR_SPEC §10 which uses String for operator representation.
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p wirespec-sema`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add crates/wirespec-sema/src/expr.rs
git commit -m "feat(sema): add SemanticExpr, ValueRef, TransitionPeerRef types"
```

---

### Task 3: Semantic IR Structs

**Files:**
- Create: `crates/wirespec-sema/src/ir.rs`

- [ ] **Step 1: Write the IR module**

All the container types from SEMANTIC_IR_SPEC §6–§12: `SemanticModule`, `SemanticPacket`, `SemanticFrame`, `SemanticCapsule`, `SemanticStateMachine`, etc. Key types:

```rust
// crates/wirespec-sema/src/ir.rs
use wirespec_syntax::span::Span;
use crate::expr::*;
use crate::types::*;

// ── Root ──

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticModule {
    pub schema_version: String,         // Fixed: "semantic-ir/v1"
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
    /// All items in declaration order, by ID.
    pub item_order: Vec<String>,
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
    pub span: Option<Span>,
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
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p wirespec-sema`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add crates/wirespec-sema/src/ir.rs
git commit -m "feat(sema): add Semantic IR struct types (module, packet, frame, capsule, SM)"
```

---

### Task 4: Error Types

**Files:**
- Create: `crates/wirespec-sema/src/error.rs`

- [ ] **Step 1: Write the error module**

```rust
// crates/wirespec-sema/src/error.rs
use wirespec_syntax::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    UndefinedType,
    UndefinedField,
    UndefinedState,
    UndefinedEvent,
    ForwardReference,
    TypeMismatch,
    RemainingNotLast,
    InvalidAnnotation,
    DuplicateDefinition,
    InvalidChecksumType,
    DuplicateChecksum,
    ChecksumProfileViolation,
    InvalidLengthOrRemaining,
    InvalidArrayCount,
    InvalidBytesLength,
    SmUndefinedState,
    SmInvalidInitial,
    SmUnhandledEvent,
    SmDuplicateTransition,
    SmMissingAssignment,
    SmDelegateNotSelfTransition,
    SmDelegateWithAction,
    CyclicDependency,
    ReservedIdentifier,
}

#[derive(Debug, Clone)]
pub struct SemaError {
    pub kind: ErrorKind,
    pub msg: String,
    pub span: Option<Span>,
    pub context: Vec<String>,
    pub hint: Option<String>,
}

impl SemaError {
    pub fn new(kind: ErrorKind, msg: impl Into<String>) -> Self {
        Self {
            kind,
            msg: msg.into(),
            span: None,
            context: Vec::new(),
            hint: None,
        }
    }

    pub fn with_span(mut self, span: Option<Span>) -> Self {
        self.span = span;
        self
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context.push(ctx.into());
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl std::fmt::Display for SemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(span) = &self.span {
            write!(f, "error at offset {}: ", span.offset)?;
        }
        write!(f, "{}", self.msg)?;
        for ctx in &self.context {
            write!(f, "\n  in {ctx}")?;
        }
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {hint}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SemaError {}

pub type SemaResult<T> = Result<T, SemaError>;
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p wirespec-sema`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add crates/wirespec-sema/src/error.rs
git commit -m "feat(sema): add SemaError with ErrorKind enum and builder pattern"
```

---

### Task 5: Compliance Profile

**Files:**
- Create: `crates/wirespec-sema/src/profile.rs`

- [ ] **Step 1: Write the profile module**

```rust
// crates/wirespec-sema/src/profile.rs

/// Compliance profile per COMPLIANCE_PROFILE_SPEC.md
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplianceProfile {
    Phase2StrictV1_0,
    Phase2ExtendedCurrent,
}

impl ComplianceProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Phase2StrictV1_0 => "phase2_strict_v1_0",
            Self::Phase2ExtendedCurrent => "phase2_extended_current",
        }
    }

    /// Checksum algorithms allowed under this profile.
    pub fn allowed_checksum_algorithms(self) -> &'static [&'static str] {
        match self {
            Self::Phase2StrictV1_0 => &["internet", "crc32", "crc32c"],
            Self::Phase2ExtendedCurrent => &["internet", "crc32", "crc32c", "fletcher16"],
        }
    }

    /// Whether capsule-scope checksums are allowed.
    pub fn allows_capsule_checksum(self) -> bool {
        match self {
            Self::Phase2StrictV1_0 => false,
            Self::Phase2ExtendedCurrent => true,
        }
    }
}

impl Default for ComplianceProfile {
    fn default() -> Self {
        // Migration default: extended current
        Self::Phase2ExtendedCurrent
    }
}

/// Expected field type for a checksum algorithm.
pub fn checksum_required_type(algorithm: &str) -> Option<&'static str> {
    match algorithm {
        "internet" | "fletcher16" => Some("u16"),
        "crc32" | "crc32c" => Some("u32"),
        _ => None,
    }
}

/// Field width in bytes for a checksum algorithm.
pub fn checksum_field_width(algorithm: &str) -> Option<u8> {
    match algorithm {
        "internet" | "fletcher16" => Some(2),
        "crc32" | "crc32c" => Some(4),
        _ => None,
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p wirespec-sema`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add crates/wirespec-sema/src/profile.rs
git commit -m "feat(sema): add ComplianceProfile with checksum algorithm/scope gating"
```

---

### Task 6: Wire up lib.rs with all modules

**Files:**
- Modify: `crates/wirespec-sema/src/lib.rs`

- [ ] **Step 1: Update lib.rs**

```rust
// crates/wirespec-sema/src/lib.rs
pub mod error;
pub mod expr;
pub mod ir;
pub mod profile;
pub mod types;
```

- [ ] **Step 2: Verify full crate compiles**

Run: `cargo build -p wirespec-sema`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add crates/wirespec-sema/src/lib.rs
git commit -m "feat(sema): wire up all type modules in lib.rs"
```

---

## Chunk 2: Name Resolution and Type Registry

The core of Pass 1: scan all declarations, build a registry of what names exist and what kind they are, and resolve type aliases.

### Task 7: Type Registry (resolve.rs)

**Files:**
- Create: `crates/wirespec-sema/src/resolve.rs`
- Test: `crates/wirespec-sema/tests/type_resolution_tests.rs` (append)

- [ ] **Step 1: Write the failing test**

```rust
// append to crates/wirespec-sema/tests/type_resolution_tests.rs
use wirespec_sema::resolve::*;
use wirespec_sema::types::*;

#[test]
fn registry_resolves_primitives() {
    let reg = TypeRegistry::new(Endianness::Big);
    let resolved = reg.resolve_type_name("u16").unwrap();
    assert_eq!(resolved, ResolvedType::Primitive(PrimitiveWireType::U16, None));
}

#[test]
fn registry_resolves_explicit_endian() {
    let reg = TypeRegistry::new(Endianness::Big);
    let resolved = reg.resolve_type_name("u16le").unwrap();
    assert_eq!(resolved, ResolvedType::Primitive(PrimitiveWireType::U16, Some(Endianness::Little)));
}

#[test]
fn registry_user_type() {
    let mut reg = TypeRegistry::new(Endianness::Big);
    reg.register("VarInt", DeclKind::VarInt);
    let resolved = reg.resolve_type_name("VarInt").unwrap();
    assert_eq!(resolved, ResolvedType::UserDefined("VarInt".into(), DeclKind::VarInt));
}

#[test]
fn registry_unknown_type() {
    let reg = TypeRegistry::new(Endianness::Big);
    assert!(reg.resolve_type_name("Unknown").is_none());
}

#[test]
fn registry_alias_resolution() {
    let mut reg = TypeRegistry::new(Endianness::Little);
    reg.register_alias("AttHandle", "u16le");
    let resolved = reg.resolve_type_name("AttHandle").unwrap();
    assert_eq!(resolved, ResolvedType::Primitive(PrimitiveWireType::U16, Some(Endianness::Little)));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wirespec-sema`
Expected: FAIL (module `resolve` not found)

- [ ] **Step 3: Write the implementation**

```rust
// crates/wirespec-sema/src/resolve.rs
use std::collections::HashMap;
use crate::types::*;

/// What kind of declaration a name refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    VarInt,
    Packet,
    Frame,
    Capsule,
    Enum,
    Flags,
    StateMachine,
    Const,
}

/// Result of resolving a type name.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedType {
    Primitive(PrimitiveWireType, Option<Endianness>),
    UserDefined(String, DeclKind),
}

/// Registry of all known type names in a module.
pub struct TypeRegistry {
    module_endianness: Endianness,
    /// user-defined name → DeclKind
    declarations: HashMap<String, DeclKind>,
    /// alias name → target name (resolved transitively)
    aliases: HashMap<String, String>,
    /// const name → value (for compile-time evaluation)
    const_values: HashMap<String, i64>,
}

impl TypeRegistry {
    pub fn new(module_endianness: Endianness) -> Self {
        Self {
            module_endianness,
            declarations: HashMap::new(),
            aliases: HashMap::new(),
            const_values: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: &str, kind: DeclKind) {
        self.declarations.insert(name.to_string(), kind);
    }

    pub fn register_alias(&mut self, alias: &str, target: &str) {
        self.aliases.insert(alias.to_string(), target.to_string());
    }

    pub fn register_const(&mut self, name: &str, value: i64) {
        self.const_values.insert(name.to_string(), value);
    }

    pub fn get_const_value(&self, name: &str) -> Option<i64> {
        self.const_values.get(name).copied()
    }

    pub fn module_endianness(&self) -> Endianness {
        self.module_endianness
    }

    pub fn contains(&self, name: &str) -> bool {
        self.declarations.contains_key(name)
            || self.aliases.contains_key(name)
            || self.resolve_primitive(name).is_some()
    }

    pub fn get_decl_kind(&self, name: &str) -> Option<DeclKind> {
        self.declarations.get(name).copied()
    }

    /// Resolve a type name to its meaning.
    pub fn resolve_type_name(&self, name: &str) -> Option<ResolvedType> {
        // Check alias first (transitive)
        if let Some(target) = self.aliases.get(name) {
            return self.resolve_type_name(target);
        }

        // Check primitives
        if let Some((prim, endian)) = self.resolve_primitive(name) {
            return Some(ResolvedType::Primitive(prim, endian));
        }

        // Check user declarations
        if let Some(kind) = self.declarations.get(name) {
            return Some(ResolvedType::UserDefined(name.to_string(), *kind));
        }

        None
    }

    fn resolve_primitive(&self, name: &str) -> Option<(PrimitiveWireType, Option<Endianness>)> {
        let (prim, endian) = match name {
            "u8" => (PrimitiveWireType::U8, None),
            "u16" => (PrimitiveWireType::U16, Some(self.module_endianness)),
            "u24" => (PrimitiveWireType::U24, Some(self.module_endianness)),
            "u32" => (PrimitiveWireType::U32, Some(self.module_endianness)),
            "u64" => (PrimitiveWireType::U64, Some(self.module_endianness)),
            "i8" => (PrimitiveWireType::I8, None),
            "i16" => (PrimitiveWireType::I16, Some(self.module_endianness)),
            "i32" => (PrimitiveWireType::I32, Some(self.module_endianness)),
            "i64" => (PrimitiveWireType::I64, Some(self.module_endianness)),
            "u16be" => (PrimitiveWireType::U16, Some(Endianness::Big)),
            "u16le" => (PrimitiveWireType::U16, Some(Endianness::Little)),
            "u24be" => (PrimitiveWireType::U24, Some(Endianness::Big)),
            "u24le" => (PrimitiveWireType::U24, Some(Endianness::Little)),
            "u32be" => (PrimitiveWireType::U32, Some(Endianness::Big)),
            "u32le" => (PrimitiveWireType::U32, Some(Endianness::Little)),
            "u64be" => (PrimitiveWireType::U64, Some(Endianness::Big)),
            "u64le" => (PrimitiveWireType::U64, Some(Endianness::Little)),
            "i16be" => (PrimitiveWireType::I16, Some(Endianness::Big)),
            "i16le" => (PrimitiveWireType::I16, Some(Endianness::Little)),
            "i32be" => (PrimitiveWireType::I32, Some(Endianness::Big)),
            "i32le" => (PrimitiveWireType::I32, Some(Endianness::Little)),
            "i64be" => (PrimitiveWireType::I64, Some(Endianness::Big)),
            "i64le" => (PrimitiveWireType::I64, Some(Endianness::Little)),
            "bool" => (PrimitiveWireType::Bool, None),
            "bit" => (PrimitiveWireType::Bit, None),
            _ => return None,
        };
        Some((prim, endian))
    }
}
```

- [ ] **Step 4: Add `pub mod resolve;` to lib.rs and run tests**

Run: `cargo test -p wirespec-sema`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/wirespec-sema/src/resolve.rs crates/wirespec-sema/src/lib.rs crates/wirespec-sema/tests/type_resolution_tests.rs
git commit -m "feat(sema): add TypeRegistry with primitive/alias/user-type resolution"
```

---

## Chunk 3: Validation Rules

Pure validation functions that operate on AST nodes and registry state.

### Task 8: Validation Functions (validate.rs)

**Files:**
- Create: `crates/wirespec-sema/src/validate.rs`
- Test: `crates/wirespec-sema/tests/validation_tests.rs`

- [ ] **Step 1: Write failing tests for validation rules**

```rust
// crates/wirespec-sema/tests/validation_tests.rs
use wirespec_sema::validate::*;
use wirespec_sema::error::ErrorKind;
use wirespec_syntax::ast::*;

#[test]
fn remaining_not_last_ok() {
    // bytes[remaining] as last wire field — should pass
    let fields = vec![
        mock_wire_field("x", false, false),
        mock_wire_field("data", true, false), // remaining
    ];
    assert!(validate_remaining_is_last(&fields).is_ok());
}

#[test]
fn remaining_not_last_err() {
    // bytes[remaining] not last — should fail
    let fields = vec![
        mock_wire_field("data", true, false), // remaining
        mock_wire_field("x", false, false),
    ];
    let err = validate_remaining_is_last(&fields).unwrap_err();
    assert_eq!(err.kind, ErrorKind::RemainingNotLast);
}

#[test]
fn fill_not_last_ok() {
    let fields = vec![
        mock_wire_field("x", false, false),
        mock_wire_field("items", false, true), // fill
    ];
    assert!(validate_remaining_is_last(&fields).is_ok());
}

#[test]
fn single_checksum_ok() {
    let checksums = vec!["checksum"];
    assert!(validate_single_checksum(&checksums, "packet Foo").is_ok());
}

#[test]
fn duplicate_checksum_err() {
    let checksums = vec!["checksum1", "checksum2"];
    let err = validate_single_checksum(&checksums, "packet Foo").unwrap_err();
    assert_eq!(err.kind, ErrorKind::DuplicateChecksum);
}

#[test]
fn forward_ref_detected() {
    // Field "data" references "length" which hasn't been declared yet
    let declared = vec!["src_port".to_string()];
    let refs = vec!["length".to_string()];
    let err = validate_no_forward_refs(&refs, &declared, "data", None).unwrap_err();
    assert_eq!(err.kind, ErrorKind::ForwardReference);
}

#[test]
fn forward_ref_ok() {
    let declared = vec!["length".to_string()];
    let refs = vec!["length".to_string()];
    assert!(validate_no_forward_refs(&refs, &declared, "data", None).is_ok());
}

// Helper to create a simplified field descriptor for validation
fn mock_wire_field(name: &str, is_remaining: bool, is_fill: bool) -> FieldDescriptor {
    FieldDescriptor {
        name: name.to_string(),
        is_remaining,
        is_fill,
        is_wire: true,
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wirespec-sema --test validation_tests`
Expected: FAIL

- [ ] **Step 3: Write the implementation**

```rust
// crates/wirespec-sema/src/validate.rs
use crate::error::*;
use wirespec_syntax::span::Span;

/// Simplified field descriptor for scope-level validation.
pub struct FieldDescriptor {
    pub name: String,
    pub is_remaining: bool,
    pub is_fill: bool,
    pub is_wire: bool,
}

/// Spec §3.15: bytes[remaining] and [T; fill] must be the last wire field in scope.
pub fn validate_remaining_is_last(fields: &[FieldDescriptor]) -> SemaResult<()> {
    let wire_fields: Vec<_> = fields.iter().filter(|f| f.is_wire).collect();
    for (i, field) in wire_fields.iter().enumerate() {
        if (field.is_remaining || field.is_fill) && i < wire_fields.len() - 1 {
            return Err(SemaError::new(
                ErrorKind::RemainingNotLast,
                format!(
                    "field '{}' consumes remaining scope but is not the last wire field",
                    field.name
                ),
            ));
        }
    }
    Ok(())
}

/// Spec §3.11: at most one @checksum per scope.
pub fn validate_single_checksum(
    checksum_fields: &[&str],
    scope_desc: &str,
) -> SemaResult<()> {
    if checksum_fields.len() > 1 {
        return Err(SemaError::new(
            ErrorKind::DuplicateChecksum,
            format!(
                "multiple @checksum fields in {scope_desc}: {}",
                checksum_fields.join(", ")
            ),
        ));
    }
    Ok(())
}

/// Spec §3.14: fields may only reference previously declared fields.
pub fn validate_no_forward_refs(
    referenced: &[String],
    declared: &[String],
    field_name: &str,
    span: Option<Span>,
) -> SemaResult<()> {
    for name in referenced {
        if !declared.contains(name) {
            return Err(
                SemaError::new(
                    ErrorKind::ForwardReference,
                    format!("field '{field_name}' references undeclared '{name}'"),
                )
                .with_span(span)
                .with_hint(format!("'{name}' must be declared before '{field_name}'")),
            );
        }
    }
    Ok(())
}

/// Validate checksum field type matches algorithm requirement.
pub fn validate_checksum_field_type(
    algorithm: &str,
    field_type_name: &str,
    field_name: &str,
) -> SemaResult<()> {
    let required = crate::profile::checksum_required_type(algorithm);
    if let Some(req) = required {
        if field_type_name != req {
            return Err(SemaError::new(
                ErrorKind::InvalidChecksumType,
                format!(
                    "@checksum({algorithm}) requires field type '{req}', but '{field_name}' has type '{field_type_name}'"
                ),
            ));
        }
    }
    Ok(())
}

/// Validate checksum algorithm is allowed under profile.
pub fn validate_checksum_profile(
    algorithm: &str,
    profile: crate::profile::ComplianceProfile,
) -> SemaResult<()> {
    if !profile
        .allowed_checksum_algorithms()
        .contains(&algorithm)
    {
        return Err(SemaError::new(
            ErrorKind::ChecksumProfileViolation,
            format!(
                "@checksum({algorithm}) is not available in profile {}",
                profile.as_str()
            ),
        )
        .with_hint(format!(
            "use --profile phase2_extended_current to enable extension algorithms"
        )));
    }
    Ok(())
}
```

- [ ] **Step 4: Add `pub mod validate;` to lib.rs and run tests**

Run: `cargo test -p wirespec-sema --test validation_tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/wirespec-sema/src/validate.rs crates/wirespec-sema/src/lib.rs crates/wirespec-sema/tests/validation_tests.rs
git commit -m "feat(sema): add validation rules — remaining-is-last, forward refs, checksums"
```

---

## Chunk 4: Analyzer — Pass 1 (Registry) and Pass 2 (Lowering)

The main `Analyzer` struct. Pass 1 scans all top-level items and populates the `TypeRegistry`. Pass 2 lowers each item into Semantic IR.

### Task 9: Analyzer skeleton + Pass 1 (registry)

**Files:**
- Create: `crates/wirespec-sema/src/analyzer.rs`
- Test: `crates/wirespec-sema/tests/analyze_tests.rs`

- [ ] **Step 1: Write failing tests for pass 1 + simple lowering**

```rust
// crates/wirespec-sema/tests/analyze_tests.rs
use wirespec_sema::analyzer::analyze;
use wirespec_sema::profile::ComplianceProfile;
use wirespec_syntax::parse;

#[test]
fn analyze_empty_module() {
    let ast = parse("module test").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.module_name, "test");
    assert_eq!(sem.module_endianness, wirespec_sema::types::Endianness::Big);
}

#[test]
fn analyze_const() {
    let ast = parse("const MAX: u8 = 20").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.consts.len(), 1);
    assert_eq!(sem.consts[0].name, "MAX");
    assert_eq!(sem.consts[0].const_id, "const:MAX");
}

#[test]
fn analyze_enum() {
    let ast = parse("enum E: u8 { A = 0, B = 1 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.enums.len(), 1);
    assert_eq!(sem.enums[0].name, "E");
    assert_eq!(sem.enums[0].members.len(), 2);
    assert!(!sem.enums[0].is_flags);
}

#[test]
fn analyze_flags() {
    let ast = parse("flags F: u8 { A = 0x01, B = 0x02 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.enums.len(), 1);
    assert!(sem.enums[0].is_flags);
}

#[test]
fn analyze_endianness_from_annotation() {
    let ast = parse("@endian little\nmodule test\npacket P { x: u16 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.module_endianness, wirespec_sema::types::Endianness::Little);
}

#[test]
fn analyze_simple_packet() {
    let ast = parse("packet Foo { x: u8, y: u16 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.packets.len(), 1);
    assert_eq!(sem.packets[0].name, "Foo");
    assert_eq!(sem.packets[0].fields.len(), 2);
    assert_eq!(sem.packets[0].fields[0].name, "x");
}

#[test]
fn analyze_packet_with_require() {
    let ast = parse("packet P { length: u16, require length >= 8 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.packets[0].requires.len(), 1);
    assert_eq!(sem.packets[0].items.len(), 2); // field + require
}

#[test]
fn analyze_packet_with_derived() {
    let ast = parse("packet P { flags: u8, let is_set: bool = (flags & 0x01) != 0 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.packets[0].derived.len(), 1);
    assert_eq!(sem.packets[0].derived[0].name, "is_set");
}

#[test]
fn analyze_type_alias() {
    let ast = parse("type Handle = u16le\npacket P { h: Handle }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    // Alias should resolve — field type should be Primitive U16 Little
    assert_eq!(sem.packets[0].fields[0].name, "h");
}

#[test]
fn analyze_undefined_type_error() {
    let ast = parse("packet P { x: Unknown }").unwrap();
    let result = analyze(&ast, ComplianceProfile::default());
    assert!(result.is_err());
}

#[test]
fn analyze_forward_reference_error() {
    let ast = parse("packet P { data: bytes[length: length], length: u16 }").unwrap();
    let result = analyze(&ast, ComplianceProfile::default());
    assert!(result.is_err());
}

#[test]
fn analyze_static_assert() {
    let ast = parse("const MAX: u8 = 20\nstatic_assert MAX <= 255").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.static_asserts.len(), 1);
}

#[test]
fn analyze_frame() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A {},
            0x01 => B { x: u8 },
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.frames.len(), 1);
    assert_eq!(sem.frames[0].variants.len(), 3);
    assert_eq!(sem.frames[0].tag_name, "tag");
}

#[test]
fn analyze_capsule() {
    let src = r#"
        capsule C {
            type_field: u8,
            length: u16,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                _ => Unknown { data: bytes[remaining] },
            },
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.capsules.len(), 1);
    assert_eq!(sem.capsules[0].header_fields.len(), 2);
    assert_eq!(sem.capsules[0].variants.len(), 2);
}

#[test]
fn analyze_varint_prefix_match() {
    let src = r#"
        type VarInt = {
            prefix: bits[2],
            value: match prefix {
                0b00 => bits[6],
                0b01 => bits[14],
                0b10 => bits[30],
                0b11 => bits[62],
            },
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.varints.len(), 1);
    assert_eq!(sem.varints[0].name, "VarInt");
}

#[test]
fn analyze_continuation_varint() {
    let src = r#"
        type MqttLen = varint {
            continuation_bit: msb,
            value_bits: 7,
            max_bytes: 4,
            byte_order: little,
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.varints.len(), 1);
    assert_eq!(sem.varints[0].name, "MqttLen");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wirespec-sema --test analyze_tests`
Expected: FAIL

- [ ] **Step 3: Write the analyzer implementation**

The `analyzer.rs` implements the main `analyze()` function with:
- Pass 1: scan all top-level items, register names in `TypeRegistry`
- Pass 2: lower each item to Semantic IR
- Expression lowering (`lower_expr`)
- Type expression resolution (`resolve_type_expr`)
- Field lowering with scope tracking
- Variant scope lowering (shared for frame + capsule)

This is the largest single file. Key sections:
1. `analyze()` → creates `Analyzer`, runs pass 1 + pass 2
2. `Analyzer::pass1_register()` → registers consts, enums, types, packets, etc.
3. `Analyzer::pass2_lower()` → lowers each item
4. `Analyzer::lower_packet()` → fields + derived + requires
5. `Analyzer::lower_frame()` → tag + variants
6. `Analyzer::lower_capsule()` → header + payload variants
7. `Analyzer::lower_field()` → resolve type, check annotations
8. `Analyzer::lower_expr()` → recursive expression resolution
9. `Analyzer::resolve_type_expr()` → AstTypeExpr → SemanticType
10. `Analyzer::lower_varint()` → prefix-match and continuation-bit

Full implementation code is intentionally not inlined here due to size (~800 lines). The implementation should follow the Python `semantic_analyzer.py` two-pass structure, using the `TypeRegistry` from `resolve.rs` and validation functions from `validate.rs`.

Key implementation notes:
- `lower_expr`: map `AstExpr::NameRef` → look up in field scope (`declared`) and const registry, produce `SemanticExpr::ValueRef` with resolved `value_id`
- `resolve_type_expr`: map `AstTypeExpr::Named` through `TypeRegistry::resolve_type_name()`, produce appropriate `SemanticType` variant
- Field scope: maintain `declared: Vec<String>` per scope, appending as fields are lowered
- Forward ref check: before lowering a field's type expr, collect all `NameRef`s and validate against `declared`
- VarInt detection: `AstTypeDeclBody::Fields` with exactly 2 fields where field[0] is bits[N] and field[1] is match on field[0] → `SemanticVarInt` with `PrefixMatch` encoding

- [ ] **Step 4: Add `pub mod analyzer;` to lib.rs and run tests**

Run: `cargo test -p wirespec-sema --test analyze_tests`
Expected: PASS for all tests

- [ ] **Step 5: Commit**

```bash
git add crates/wirespec-sema/src/analyzer.rs crates/wirespec-sema/src/lib.rs crates/wirespec-sema/tests/analyze_tests.rs
git commit -m "feat(sema): implement two-pass analyzer — registry, type resolution, lowering"
```

---

### Task 10: State Machine Analysis

**Files:**
- Modify: `crates/wirespec-sema/src/analyzer.rs` (add SM lowering)
- Test: `crates/wirespec-sema/tests/analyze_tests.rs` (append)

- [ ] **Step 1: Write failing SM tests**

```rust
// append to analyze_tests.rs
#[test]
fn analyze_state_machine_basic() {
    let src = r#"
        state machine S {
            state Init { count: u8 = 0 }
            state Done [terminal]
            initial Init
            transition Init -> Done { on finish }
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    assert_eq!(sem.state_machines.len(), 1);
    let sm = &sem.state_machines[0];
    assert_eq!(sm.name, "S");
    assert_eq!(sm.states.len(), 2);
    assert_eq!(sm.events.len(), 1);
    assert_eq!(sm.transitions.len(), 1);
    assert!(sm.states[1].is_terminal);
}

#[test]
fn analyze_sm_wildcard_transition() {
    let src = r#"
        state machine S {
            state A
            state B [terminal]
            initial A
            transition A -> B { on close }
            transition * -> B { on error }
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    let sm = &sem.state_machines[0];
    // * expands to all non-terminal states not already covered
    assert!(sm.transitions.len() >= 2);
}

#[test]
fn analyze_sm_with_guard_and_action() {
    let src = r#"
        state machine S {
            state A { count: u8 = 0 }
            state B [terminal]
            initial A
            transition A -> A {
                on tick
                guard src.count < 10
                action { dst.count = src.count + 1; }
            }
            transition A -> B { on done }
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    let sm = &sem.state_machines[0];
    let t = &sm.transitions[0];
    assert!(t.guard.is_some());
    assert_eq!(t.actions.len(), 1);
}

#[test]
fn analyze_sm_undefined_state_error() {
    let src = r#"
        state machine S {
            state A
            initial A
            transition A -> Nonexistent { on go }
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, ComplianceProfile::default());
    assert!(result.is_err());
}
```

- [ ] **Step 2: Implement SM lowering in analyzer**

SM lowering logic:
1. Collect all state names → state IDs
2. Collect all unique events → event IDs (dedup by name)
3. Normalize multi-event transitions into separate semantic transitions
4. Expand wildcard `*` src to all non-terminal states
5. Validate: initial state exists, all referenced states exist, no duplicate (state, event) pairs
6. Lower guard/action/delegate expressions using `lower_sm_expr()`
7. `lower_sm_expr()` extends `lower_expr()` with `TransitionPeerRef` for `src.*`/`dst.*` access

- [ ] **Step 3: Run tests**

Run: `cargo test -p wirespec-sema --test analyze_tests`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/wirespec-sema/src/analyzer.rs crates/wirespec-sema/tests/analyze_tests.rs
git commit -m "feat(sema): add state machine analysis — states, events, transitions, guard/action"
```

---

## Chunk 5: Checksum Validation and Corpus Tests

### Task 11: Checksum Profile Validation

**Files:**
- Test: `crates/wirespec-sema/tests/checksum_tests.rs`

- [ ] **Step 1: Write checksum tests**

```rust
// crates/wirespec-sema/tests/checksum_tests.rs
use wirespec_sema::analyzer::analyze;
use wirespec_sema::profile::ComplianceProfile;
use wirespec_syntax::parse;

#[test]
fn checksum_internet_accepted() {
    let src = r#"
        @checksum(internet)
        packet P {
            data: u32,
            @checksum(internet)
            checksum: u16,
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::Phase2StrictV1_0).unwrap();
    assert_eq!(sem.packets[0].fields[1].checksum_algorithm.as_deref(), Some("internet"));
}

#[test]
fn checksum_fletcher16_rejected_under_strict() {
    let src = r#"
        packet P {
            data: u32,
            @checksum(fletcher16)
            checksum: u16,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, ComplianceProfile::Phase2StrictV1_0);
    assert!(result.is_err());
}

#[test]
fn checksum_fletcher16_accepted_under_extended() {
    let src = r#"
        packet P {
            data: u32,
            @checksum(fletcher16)
            checksum: u16,
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::Phase2ExtendedCurrent).unwrap();
    assert_eq!(sem.packets[0].fields[1].checksum_algorithm.as_deref(), Some("fletcher16"));
}

#[test]
fn checksum_wrong_field_type_error() {
    let src = r#"
        packet P {
            data: u32,
            @checksum(internet)
            checksum: u32,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, ComplianceProfile::default());
    assert!(result.is_err()); // internet requires u16, not u32
}

#[test]
fn duplicate_checksum_error() {
    let src = r#"
        packet P {
            @checksum(internet)
            c1: u16,
            @checksum(crc32)
            c2: u32,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, ComplianceProfile::default());
    assert!(result.is_err());
}

#[test]
fn capsule_checksum_rejected_under_strict() {
    let src = r#"
        capsule C {
            type_field: u8,
            @checksum(internet)
            checksum: u16,
            length: u16,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                _ => Unknown { data: bytes[remaining] },
            },
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, ComplianceProfile::Phase2StrictV1_0);
    assert!(result.is_err()); // capsule checksum not allowed under strict profile
}

#[test]
fn capsule_checksum_accepted_under_extended() {
    let src = r#"
        capsule C {
            type_field: u8,
            @checksum(internet)
            checksum: u16,
            length: u16,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                _ => Unknown { data: bytes[remaining] },
            },
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::Phase2ExtendedCurrent).unwrap();
    assert_eq!(sem.capsules[0].header_fields[1].checksum_algorithm.as_deref(), Some("internet"));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p wirespec-sema --test checksum_tests`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/wirespec-sema/tests/checksum_tests.rs
git commit -m "test(sema): add checksum validation tests — profile gating, type checks"
```

---

### Task 12: Corpus Integration Tests

**Files:**
- Create: `crates/wirespec-sema/tests/corpus_sema_tests.rs`

Test that semantic analysis succeeds on all real `.wspec` corpus files.

- [ ] **Step 1: Write corpus tests**

```rust
// crates/wirespec-sema/tests/corpus_sema_tests.rs
use wirespec_sema::analyzer::analyze;
use wirespec_sema::profile::ComplianceProfile;
use wirespec_syntax::parse;

fn analyze_file(path: &str) {
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
    let ast = parse(&source)
        .unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"));
    let _sem = analyze(&ast, ComplianceProfile::Phase2ExtendedCurrent)
        .unwrap_or_else(|e| panic!("Failed to analyze {path}: {e}"));
}

#[test]
fn corpus_quic_varint() {
    analyze_file("../../protospec/examples/quic/varint.wire");
}

#[test]
fn corpus_udp() {
    analyze_file("../../protospec/examples/net/udp.wire");
}

#[test]
fn corpus_tcp() {
    analyze_file("../../protospec/examples/net/tcp.wire");
}

#[test]
fn corpus_ethernet() {
    analyze_file("../../protospec/examples/net/ethernet.wire");
}

#[test]
fn corpus_ble_att() {
    analyze_file("../../protospec/examples/ble/att.wire");
}

#[test]
fn corpus_mqtt() {
    analyze_file("../../protospec/examples/mqtt/mqtt.wire");
}

#[test]
fn corpus_bits_groups() {
    analyze_file("../../protospec/examples/test/bits_groups.wire");
}

// Note: quic/frames.wire and mpquic/path.wire require import resolution
// which depends on wirespec-driver/resolver — skip for now.
```

- [ ] **Step 2: Run corpus tests**

Run: `cargo test -p wirespec-sema --test corpus_sema_tests`
Expected: PASS (for files that don't use imports — quic/varint, udp, tcp, ethernet, bits_groups)
Expected: Some may fail on import — mark those `#[ignore]` for now

- [ ] **Step 3: Commit**

```bash
git add crates/wirespec-sema/tests/corpus_sema_tests.rs
git commit -m "test(sema): add corpus integration tests for semantic analysis"
```

---

### Task 13: Final lib.rs Public API

**Files:**
- Modify: `crates/wirespec-sema/src/lib.rs`

- [ ] **Step 1: Finalize the public API**

```rust
// crates/wirespec-sema/src/lib.rs
pub mod analyzer;
pub mod error;
pub mod expr;
pub mod ir;
pub mod profile;
pub mod resolve;
pub mod types;
pub mod validate;

pub use analyzer::analyze;
pub use ir::SemanticModule;
pub use profile::ComplianceProfile;
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests pass across all crates

- [ ] **Step 3: Commit**

```bash
git add crates/wirespec-sema/src/lib.rs
git commit -m "feat(sema): finalize public API — analyze(), SemanticModule, ComplianceProfile"
```

---

## Summary

| Chunk | Tasks | What it delivers |
|-------|-------|-----------------|
| 1 | Tasks 1–6 | All IR types, error types, profile, lib.rs wiring |
| 2 | Task 7 | TypeRegistry with primitive/alias/user-type resolution |
| 3 | Task 8 | Validation rules (remaining-is-last, forward refs, checksums) |
| 4 | Tasks 9–10 | Full two-pass analyzer + state machine analysis |
| 5 | Tasks 11–13 | Checksum profile tests, corpus tests, public API |

**Total test count target:** ~50+ tests covering:
- Type resolution (primitives, endianness, aliases)
- Validation rules (remaining, forward refs, checksums)
- Full analysis (const, enum, flags, packet, frame, capsule, varint, SM)
- Error cases (undefined type, forward ref, checksum violations)
- Corpus files (real .wspec examples)
- Profile gating (strict vs extended)
