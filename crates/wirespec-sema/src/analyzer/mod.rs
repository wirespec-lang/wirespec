// crates/wirespec-sema/src/analyzer/mod.rs
//!
//! Core semantic analyzer for wirespec.
//! Takes an `AstModule` and a `ComplianceProfile` and produces a `SemanticModule`.
//!
//! Two-pass approach:
//! - Pass 1: register all top-level names in the TypeRegistry
//! - Pass 2: lower each AST item to Semantic IR

mod lower;
mod register;
mod validate;

use wirespec_syntax::ast::*;

use crate::error::*;
use crate::expr::*;
use crate::ir::*;
use crate::profile::ComplianceProfile;
use crate::resolve::*;
use crate::types::*;
use crate::validate::*;

use self::validate::{resolve_event_params, resolve_guard_sm_names};

/// Entry point: analyze an AST module and produce semantic IR.
///
/// `external_types` provides names and declaration kinds from previously-compiled
/// modules. These are registered in the `TypeRegistry` before Pass 1 so that
/// imported types resolve correctly.
pub fn analyze(
    ast: &AstModule,
    profile: ComplianceProfile,
    external_types: &std::collections::HashMap<String, DeclKind>,
) -> SemaResult<SemanticModule> {
    let mut analyzer = Analyzer::new(profile);
    analyzer.register_external_types(external_types);
    analyzer.run(ast)
}

struct Analyzer {
    registry: TypeRegistry,
    profile: ComplianceProfile,
    errors: Vec<SemaError>,
    warnings: Vec<SemaWarning>,
    /// External type names and kinds from previously-compiled modules.
    external_types: std::collections::HashMap<String, DeclKind>,
    /// ASN.1 extern declarations collected during Pass 1.
    asn1_externs: Vec<Asn1ExternDecl>,
    /// Pending ASN.1 hint from the most recent `resolve_type_expr` call.
    pending_asn1_hint: Option<Asn1Hint>,
}

impl Analyzer {
    fn new(profile: ComplianceProfile) -> Self {
        // Default endianness is Big; will be overridden by @endian annotation
        Self {
            registry: TypeRegistry::new(Endianness::Big),
            profile,
            errors: Vec::new(),
            warnings: Vec::new(),
            external_types: std::collections::HashMap::new(),
            asn1_externs: Vec::new(),
            pending_asn1_hint: None,
        }
    }

    fn register_external_types(&mut self, ext: &std::collections::HashMap<String, DeclKind>) {
        self.external_types = ext.clone();
    }

    fn first_error(&mut self) -> SemaResult<()> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.remove(0))
        }
    }

    fn run(&mut self, ast: &AstModule) -> SemaResult<SemanticModule> {
        // Determine module name
        let module_name = ast
            .module_decl
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_default();

        // Determine endianness from @endian annotation
        let endianness = self.parse_endianness(ast);
        self.registry = TypeRegistry::new(endianness);

        // Register external types (from previously-compiled modules)
        for (name, kind) in &self.external_types {
            self.registry.register_external(name, *kind);
        }

        // ── Pass 1: register all declarations ──
        self.register_all(ast)?;

        // ── Pass 2: lower each item ──
        let mut consts = Vec::new();
        let mut enums = Vec::new();
        let mut varints = Vec::new();
        let mut packets = Vec::new();
        let mut frames = Vec::new();
        let mut capsules = Vec::new();
        let mut state_machines = Vec::new();
        let mut static_asserts = Vec::new();
        let mut item_order = Vec::new();
        let mut assert_index: usize = 0;

        for item in &ast.items {
            match item {
                AstTopItem::Const(c) => {
                    let sem = self.lower_const(c)?;
                    item_order.push(sem.const_id.clone());
                    consts.push(sem);
                }
                AstTopItem::Enum(e) => {
                    let sem = self.lower_enum_decl(e, false)?;
                    item_order.push(sem.enum_id.clone());
                    enums.push(sem);
                }
                AstTopItem::Flags(f) => {
                    let sem = self.lower_flags_decl(f)?;
                    item_order.push(sem.enum_id.clone());
                    enums.push(sem);
                }
                AstTopItem::Type(td) => match &td.body {
                    AstTypeDeclBody::Alias { .. } => {
                        // Type aliases are resolved through the registry; no IR node
                    }
                    AstTypeDeclBody::Fields { fields } => {
                        let sem = self.lower_varint_prefix_match(
                            &td.name,
                            fields,
                            &td.annotations,
                            td.span,
                        )?;
                        item_order.push(sem.varint_id.clone());
                        varints.push(sem);
                    }
                },
                AstTopItem::ContinuationVarInt(cv) => {
                    let sem = self.lower_continuation_varint(cv);
                    item_order.push(sem.varint_id.clone());
                    varints.push(sem);
                }
                AstTopItem::Packet(p) => {
                    let sem = self.lower_packet(p)?;
                    item_order.push(sem.packet_id.clone());
                    packets.push(sem);
                }
                AstTopItem::Frame(f) => {
                    let sem = self.lower_frame(f)?;
                    item_order.push(sem.frame_id.clone());
                    frames.push(sem);
                }
                AstTopItem::Capsule(c) => {
                    let sem = self.lower_capsule(c)?;
                    item_order.push(sem.capsule_id.clone());
                    capsules.push(sem);
                }
                AstTopItem::StateMachine(sm) => {
                    let sem = self.lower_state_machine(sm)?;
                    item_order.push(sem.sm_id.clone());
                    state_machines.push(sem);
                }
                AstTopItem::StaticAssert(sa) => {
                    let expr = self.lower_expr(&sa.expr, &[], &[]);
                    let id = format!("assert:{}", assert_index);
                    assert_index += 1;
                    static_asserts.push(SemanticStaticAssert {
                        assert_id: id.clone(),
                        expr,
                        span: sa.span,
                    });
                    item_order.push(id);
                }
                AstTopItem::ExternAsn1(_) => {
                    // Already registered in Pass 1
                }
            }
        }

        self.first_error()?;

        // S4: Delegate acyclicity — detect cyclic SM delegation chains
        {
            use std::collections::{HashMap, HashSet, VecDeque};
            let mut edges: HashMap<&str, HashSet<&str>> = HashMap::new();
            let mut all_names: HashSet<&str> = HashSet::new();
            for sm in &state_machines {
                all_names.insert(&sm.name);
                for state in &sm.states {
                    for field in &state.fields {
                        if let Some(ref child) = field.child_sm_name {
                            edges
                                .entry(sm.name.as_str())
                                .or_default()
                                .insert(child.as_str());
                        }
                    }
                }
            }
            let mut in_deg: HashMap<&str, usize> = all_names.iter().map(|&n| (n, 0)).collect();
            for targets in edges.values() {
                for &t in targets {
                    if let Some(d) = in_deg.get_mut(t) {
                        *d += 1;
                    }
                }
            }
            let mut queue: VecDeque<&str> = in_deg
                .iter()
                .filter(|&(_, &d)| d == 0)
                .map(|(&n, _)| n)
                .collect();
            let mut count = 0;
            while let Some(node) = queue.pop_front() {
                count += 1;
                if let Some(targets) = edges.get(node) {
                    for &t in targets {
                        if let Some(d) = in_deg.get_mut(t) {
                            *d -= 1;
                            if *d == 0 {
                                queue.push_back(t);
                            }
                        }
                    }
                }
            }
            if count < all_names.len() {
                let cycle: Vec<&str> = in_deg
                    .iter()
                    .filter(|&(_, &d)| d > 0)
                    .map(|(&n, _)| n)
                    .collect();
                return Err(SemaError::new(
                    ErrorKind::CyclicDependency,
                    format!(
                        "cyclic delegate dependency detected: {}",
                        cycle.join(" -> ")
                    ),
                )
                .with_hint("state machines cannot form a circular delegation chain"));
            }
        }

        Ok(SemanticModule {
            schema_version: "semantic-ir/v1".to_string(),
            compliance_profile: self.profile.as_str().to_string(),
            module_name,
            module_endianness: endianness,
            imports: Vec::new(),
            varints,
            consts,
            enums,
            packets,
            frames,
            capsules,
            state_machines,
            static_asserts,
            asn1_externs: self.asn1_externs.clone(),
            item_order,
            warnings: std::mem::take(&mut self.warnings),
        })
    }

    // ── Endianness ──

    fn parse_endianness(&self, ast: &AstModule) -> Endianness {
        // Check module-level annotations
        for ann in &ast.annotations {
            if ann.name == "endian"
                && let Some(AstAnnotationArg::Identifier(val)) = ann.args.first()
            {
                return match val.as_str() {
                    "little" => Endianness::Little,
                    _ => Endianness::Big,
                };
            }
        }
        // Check item-level annotations (pre-module)
        for item in &ast.items {
            if let AstTopItem::Packet(p) = item {
                for ann in &p.annotations {
                    if ann.name == "endian"
                        && let Some(AstAnnotationArg::Identifier(val)) = ann.args.first()
                    {
                        return match val.as_str() {
                            "little" => Endianness::Little,
                            _ => Endianness::Big,
                        };
                    }
                }
            }
        }
        Endianness::Big
    }
}

// ── Helper functions ──

fn binop_to_string(op: &BinOp) -> String {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "and",
        BinOp::Or => "or",
    }
    .to_string()
}

fn unaryop_to_string(op: &UnaryOp) -> String {
    match op {
        UnaryOp::Not => "!",
        UnaryOp::Neg => "-",
    }
    .to_string()
}

/// Check if a SemanticType is an integer primitive suitable as an enum/flags underlying type.
fn is_integer_underlying(ty: &SemanticType) -> bool {
    matches!(
        ty,
        SemanticType::Primitive {
            wire: PrimitiveWireType::U8
                | PrimitiveWireType::U16
                | PrimitiveWireType::U24
                | PrimitiveWireType::U32
                | PrimitiveWireType::U64
                | PrimitiveWireType::I8
                | PrimitiveWireType::I16
                | PrimitiveWireType::I32
                | PrimitiveWireType::I64,
            ..
        }
    )
}

fn extract_derive_traits(annotations: &[AstAnnotation]) -> Vec<DeriveTrait> {
    let mut traits = Vec::new();
    for ann in annotations {
        if ann.name == "derive" {
            for arg in &ann.args {
                if let AstAnnotationArg::Identifier(name) = arg {
                    match name.as_str() {
                        "debug" => traits.push(DeriveTrait::Debug),
                        "compare" => traits.push(DeriveTrait::Compare),
                        _ => {}
                    }
                }
            }
        }
    }
    traits
}

fn lower_pattern(pat: &AstPattern) -> SemanticPattern {
    match pat {
        AstPattern::Value { value, .. } => SemanticPattern::Exact { value: *value },
        AstPattern::RangeInclusive { start, end, .. } => SemanticPattern::RangeInclusive {
            start: *start,
            end: *end,
        },
        AstPattern::Wildcard { .. } => SemanticPattern::Wildcard,
    }
}

/// Extract the "name" string from a type expression for alias registration.
fn type_expr_name(texpr: &AstTypeExpr) -> String {
    match texpr {
        AstTypeExpr::Named { name, .. } => name.clone(),
        AstTypeExpr::Bits { width, .. } => format!("bits[{}]", width),
        AstTypeExpr::Bytes { .. } => "bytes".to_string(),
        AstTypeExpr::Array { element_type, .. } => {
            format!("[{}]", type_expr_name(element_type))
        }
        AstTypeExpr::Optional { inner_type, .. } => type_expr_name(inner_type),
        AstTypeExpr::Match { .. } => "match".to_string(),
        AstTypeExpr::Asn1 { type_name, .. } => format!("asn1({})", type_name),
    }
}

/// Extract base name from an expression (for MemberAccess)
fn extract_base_name(expr: &AstExpr) -> String {
    match expr {
        AstExpr::NameRef { name, .. } => name.clone(),
        AstExpr::MemberAccess { base, field, .. } => {
            format!("{}.{}", extract_base_name(base), field)
        }
        _ => "_".to_string(),
    }
}

/// Recursively collect all NameRef names from a type expression (for forward ref checking).
fn collect_type_expr_refs(texpr: &AstTypeExpr, out: &mut Vec<String>) {
    match texpr {
        AstTypeExpr::Named { .. } => {
            // Named types are resolved through the registry, not field refs
        }
        AstTypeExpr::Bits { .. } => {}
        AstTypeExpr::Bytes { size_expr, .. } => {
            if let Some(expr) = size_expr {
                collect_expr_refs(expr, out);
            }
        }
        AstTypeExpr::Array {
            element_type,
            count,
            within_expr,
            ..
        } => {
            collect_type_expr_refs(element_type, out);
            match count {
                AstArrayCount::Expr(e) => collect_expr_refs(e, out),
                AstArrayCount::Fill => {}
            }
            if let Some(w) = within_expr {
                collect_expr_refs(w, out);
            }
        }
        AstTypeExpr::Optional {
            condition,
            inner_type,
            ..
        } => {
            collect_expr_refs(condition, out);
            collect_type_expr_refs(inner_type, out);
        }
        AstTypeExpr::Match { branches, .. } => {
            for b in branches {
                collect_type_expr_refs(&b.result_type, out);
            }
        }
        AstTypeExpr::Asn1 { length, .. } => {
            if let Asn1Length::Expr(expr) = length {
                collect_expr_refs(expr, out);
            }
        }
    }
}

/// Recursively collect all NameRef names from an expression.
fn collect_expr_refs(expr: &AstExpr, out: &mut Vec<String>) {
    match expr {
        AstExpr::NameRef { name, .. } => {
            out.push(name.clone());
        }
        AstExpr::Binary { left, right, .. } => {
            collect_expr_refs(left, out);
            collect_expr_refs(right, out);
        }
        AstExpr::Unary { operand, .. } => {
            collect_expr_refs(operand, out);
        }
        AstExpr::MemberAccess { base, .. } => {
            collect_expr_refs(base, out);
        }
        AstExpr::Coalesce { expr, default, .. } => {
            collect_expr_refs(expr, out);
            collect_expr_refs(default, out);
        }
        AstExpr::Subscript { base, index, .. } => {
            collect_expr_refs(base, out);
            collect_expr_refs(index, out);
        }
        AstExpr::Fill { value, count, .. } => {
            collect_expr_refs(value, out);
            collect_expr_refs(count, out);
        }
        AstExpr::Slice {
            base, start, end, ..
        } => {
            collect_expr_refs(base, out);
            collect_expr_refs(start, out);
            collect_expr_refs(end, out);
        }
        AstExpr::InState { expr, .. } => {
            collect_expr_refs(expr, out);
        }
        AstExpr::All { collection, .. } => {
            collect_expr_refs(collection, out);
        }
        AstExpr::StateConstructor { args, .. } => {
            for a in args {
                collect_expr_refs(a, out);
            }
        }
        AstExpr::Int { .. } | AstExpr::Bool { .. } | AstExpr::Null { .. } => {}
    }
}
