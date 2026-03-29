//! AST node definitions for wirespec.
//!
//! These correspond to the canonical AST schema defined in `AST_SCHEMA_SPEC.md`.
//! The AST is syntax-oriented: it preserves source structure, declaration order,
//! and field order. It does NOT perform name resolution, type checking, or any
//! semantic analysis.

use crate::span::Span;

// ── Root ──

/// Root of a parsed `.wspec` file.
#[derive(Debug, Clone, PartialEq)]
pub struct AstModule {
    pub module_decl: Option<AstModuleDecl>,
    pub imports: Vec<AstImport>,
    pub annotations: Vec<AstAnnotation>,
    pub items: Vec<AstTopItem>,
    pub span: Option<Span>,
}

// ── Module / Import ──

#[derive(Debug, Clone, PartialEq)]
pub struct AstModuleDecl {
    pub name: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstImport {
    pub module: String,
    pub name: Option<String>,
    pub span: Option<Span>,
}

// ── Annotations ──

#[derive(Debug, Clone, PartialEq)]
pub struct AstAnnotation {
    pub name: String,
    pub args: Vec<AstAnnotationArg>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstAnnotationArg {
    Identifier(String),
    Int(i64),
    Bool(bool),
    String(String),
    NamedValue {
        name: String,
        value: AstLiteralValue,
    },
}

// ── Expressions ──

#[derive(Debug, Clone, PartialEq)]
pub enum AstExpr {
    Int {
        value: i64,
        span: Option<Span>,
    },
    Bool {
        value: bool,
        span: Option<Span>,
    },
    Null {
        span: Option<Span>,
    },
    NameRef {
        name: String,
        span: Option<Span>,
    },
    MemberAccess {
        base: Box<AstExpr>,
        field: String,
        span: Option<Span>,
    },
    Binary {
        op: BinOp,
        left: Box<AstExpr>,
        right: Box<AstExpr>,
        span: Option<Span>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<AstExpr>,
        span: Option<Span>,
    },
    Coalesce {
        expr: Box<AstExpr>,
        default: Box<AstExpr>,
        span: Option<Span>,
    },
    InState {
        expr: Box<AstExpr>,
        state_name: String,
        span: Option<Span>,
    },
    Subscript {
        base: Box<AstExpr>,
        index: Box<AstExpr>,
        span: Option<Span>,
    },
    StateConstructor {
        sm_name: String,
        state_name: String,
        args: Vec<AstExpr>,
        span: Option<Span>,
    },
    Fill {
        value: Box<AstExpr>,
        count: Box<AstExpr>,
        span: Option<Span>,
    },
    Slice {
        base: Box<AstExpr>,
        start: Box<AstExpr>,
        end: Box<AstExpr>,
        span: Option<Span>,
    },
    All {
        collection: Box<AstExpr>,
        state_name: String,
        span: Option<Span>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Not,
    Neg,
}

// ── Type Expressions ──

#[derive(Debug, Clone, PartialEq)]
pub enum AstTypeExpr {
    Named {
        name: String,
        span: Option<Span>,
    },
    Bits {
        width: u16,
        span: Option<Span>,
    },
    Match {
        field_name: String,
        branches: Vec<AstMatchBranch>,
        span: Option<Span>,
    },
    Bytes {
        kind: AstBytesKind,
        fixed_size: Option<u64>,
        size_expr: Option<Box<AstExpr>>,
        span: Option<Span>,
    },
    Array {
        element_type: Box<AstTypeExpr>,
        count: AstArrayCount,
        within_expr: Option<Box<AstExpr>>,
        span: Option<Span>,
    },
    Optional {
        condition: AstExpr,
        inner_type: Box<AstTypeExpr>,
        span: Option<Span>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstMatchBranch {
    pub pattern: AstPattern,
    pub result_type: AstTypeExpr,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstBytesKind {
    Fixed,
    Length,
    Remaining,
    LengthOrRemaining,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstArrayCount {
    Expr(AstExpr),
    Fill,
}

// ── Patterns ──

#[derive(Debug, Clone, PartialEq)]
pub enum AstPattern {
    Value {
        value: i64,
        span: Option<Span>,
    },
    RangeInclusive {
        start: i64,
        end: i64,
        span: Option<Span>,
    },
    Wildcard {
        span: Option<Span>,
    },
}

// ── Fields ──

#[derive(Debug, Clone, PartialEq)]
pub enum AstFieldItem {
    Field(AstFieldDef),
    Derived(AstDerivedField),
    Require(AstRequireClause),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstFieldDef {
    pub name: String,
    pub type_expr: AstTypeExpr,
    pub annotations: Vec<AstAnnotation>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstDerivedField {
    pub name: String,
    pub type_name: String,
    pub expr: AstExpr,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstRequireClause {
    pub expr: AstExpr,
    pub span: Option<Span>,
}

// ── Top-Level Items ──

#[derive(Debug, Clone, PartialEq)]
pub enum AstTopItem {
    Const(AstConstDecl),
    Enum(AstEnumDecl),
    Flags(AstFlagsDecl),
    StaticAssert(AstStaticAssertDecl),
    Type(AstTypeDecl),
    Packet(AstPacketDecl),
    Frame(AstFrameDecl),
    Capsule(AstCapsuleDecl),
    ContinuationVarInt(AstContinuationVarIntDecl),
    StateMachine(AstStateMachineDecl),
}

// ── Const / Enum / Flags ──

#[derive(Debug, Clone, PartialEq)]
pub struct AstConstDecl {
    pub name: String,
    pub type_name: String,
    pub value: AstLiteralValue,
    pub annotations: Vec<AstAnnotation>,
    pub exported: bool,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstEnumMember {
    pub name: String,
    pub value: i64,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstEnumDecl {
    pub name: String,
    pub underlying_type: String,
    pub members: Vec<AstEnumMember>,
    pub annotations: Vec<AstAnnotation>,
    pub exported: bool,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstFlagsDecl {
    pub name: String,
    pub underlying_type: String,
    pub members: Vec<AstEnumMember>,
    pub annotations: Vec<AstAnnotation>,
    pub exported: bool,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstStaticAssertDecl {
    pub expr: AstExpr,
    pub span: Option<Span>,
}

// ── Type / Packet ──

#[derive(Debug, Clone, PartialEq)]
pub struct AstTypeDecl {
    pub name: String,
    pub annotations: Vec<AstAnnotation>,
    pub body: AstTypeDeclBody,
    pub exported: bool,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstTypeDeclBody {
    Fields { fields: Vec<AstFieldDef> },
    Alias { target: AstTypeExpr },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstPacketDecl {
    pub name: String,
    pub fields: Vec<AstFieldItem>,
    pub annotations: Vec<AstAnnotation>,
    pub exported: bool,
    pub span: Option<Span>,
}

// ── Frame / Capsule ──

#[derive(Debug, Clone, PartialEq)]
pub struct AstFrameBranch {
    pub pattern: AstPattern,
    pub variant_name: String,
    pub fields: Vec<AstFieldItem>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstFrameDecl {
    pub name: String,
    pub tag_field: String,
    pub tag_type: String,
    pub branches: Vec<AstFrameBranch>,
    pub annotations: Vec<AstAnnotation>,
    pub exported: bool,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstCapsuleDecl {
    pub name: String,
    pub fields: Vec<AstFieldItem>,
    pub payload_tag: AstPayloadTagSelector,
    pub payload_within: String,
    pub branches: Vec<AstFrameBranch>,
    pub annotations: Vec<AstAnnotation>,
    pub exported: bool,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstPayloadTagSelector {
    Field { field_name: String },
    Expr { expr: AstExpr },
}

// ── Continuation VarInt ──

#[derive(Debug, Clone, PartialEq)]
pub struct AstContinuationVarIntDecl {
    pub name: String,
    pub annotations: Vec<AstAnnotation>,
    pub continuation_bit: String,
    pub value_bits: u8,
    pub max_bytes: u8,
    pub byte_order: String,
    pub exported: bool,
    pub span: Option<Span>,
}

// ── State Machine ──

#[derive(Debug, Clone, PartialEq)]
pub struct AstStateMachineDecl {
    pub name: String,
    pub states: Vec<AstStateDecl>,
    pub initial_state: String,
    pub transitions: Vec<AstTransitionDecl>,
    pub annotations: Vec<AstAnnotation>,
    pub exported: bool,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstStateDecl {
    pub name: String,
    pub fields: Vec<AstStateFieldDef>,
    pub is_terminal: bool,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstStateFieldDef {
    pub name: String,
    pub type_expr: AstTypeExpr,
    pub default_value: Option<AstLiteralValue>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstEventDecl {
    pub name: String,
    pub params: Vec<AstEventParam>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstEventParam {
    pub name: String,
    pub type_expr: AstTypeExpr,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstTransitionDecl {
    pub src_state: String,
    pub dst_state: String,
    pub events: Vec<AstEventDecl>,
    pub guard: Option<AstExpr>,
    pub actions: Vec<AstAssignment>,
    pub delegate: Option<AstDelegateClause>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstAssignment {
    pub target: AstExpr,
    pub op: String,
    pub value: AstExpr,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstDelegateClause {
    pub target: AstExpr,
    pub event_name: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstLiteralValue {
    Int(i64),
    Bool(bool),
    String(String),
    Null,
}
