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
    String(String),
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
