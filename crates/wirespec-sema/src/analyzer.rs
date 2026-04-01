// crates/wirespec-sema/src/analyzer.rs
//!
//! Core semantic analyzer for wirespec.
//! Takes an `AstModule` and a `ComplianceProfile` and produces a `SemanticModule`.
//!
//! Two-pass approach:
//! - Pass 1: register all top-level names in the TypeRegistry
//! - Pass 2: lower each AST item to Semantic IR

use wirespec_syntax::ast::*;

use crate::error::*;
use crate::expr::*;
use crate::ir::*;
use crate::profile::ComplianceProfile;
use crate::resolve::*;
use crate::types::*;
use crate::validate::*;

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
            external_types: std::collections::HashMap::new(),
            asn1_externs: Vec::new(),
            pending_asn1_hint: None,
        }
    }

    fn register_external_types(&mut self, ext: &std::collections::HashMap<String, DeclKind>) {
        self.external_types = ext.clone();
    }

    #[allow(dead_code)]
    fn emit(&mut self, err: SemaError) {
        self.errors.push(err);
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

    // ── Pass 1: registration ──

    fn check_reserved(name: &str) -> SemaResult<()> {
        const RESERVED_IDENTIFIERS: &[&str] = &[
            "bool",
            "null",
            "fill",
            "remaining",
            "in_state",
            "all",
            "child_state_changed",
            "src",
            "dst",
        ];
        if RESERVED_IDENTIFIERS.contains(&name) {
            return Err(SemaError::new(
                ErrorKind::ReservedIdentifier,
                format!("'{name}' is a reserved identifier"),
            ));
        }
        Ok(())
    }

    fn try_register(&mut self, name: &str, kind: DeclKind) -> SemaResult<()> {
        Self::check_reserved(name)?;
        self.registry
            .register(name, kind)
            .map_err(|msg| SemaError::new(ErrorKind::DuplicateDefinition, msg))
    }

    fn register_all(&mut self, ast: &AstModule) -> SemaResult<()> {
        for item in &ast.items {
            match item {
                AstTopItem::Const(c) => {
                    let val = match &c.value {
                        AstLiteralValue::Int(v) => *v,
                        AstLiteralValue::Bool(b) => {
                            if *b {
                                1
                            } else {
                                0
                            }
                        }
                        AstLiteralValue::String(_) => 0,
                        AstLiteralValue::Null => 0,
                    };
                    self.try_register(&c.name, DeclKind::Const)?;
                    self.registry.register_const(&c.name, val);
                }
                AstTopItem::Enum(e) => {
                    self.try_register(&e.name, DeclKind::Enum)?;
                }
                AstTopItem::Flags(f) => {
                    self.try_register(&f.name, DeclKind::Flags)?;
                }
                AstTopItem::Type(td) => match &td.body {
                    AstTypeDeclBody::Alias { target } => {
                        Self::check_reserved(&td.name)?;
                        let target_name = type_expr_name(target);
                        self.registry.register_alias(&td.name, &target_name);
                    }
                    AstTypeDeclBody::Fields { .. } => {
                        // VarInt pattern
                        self.try_register(&td.name, DeclKind::VarInt)?;
                    }
                },
                AstTopItem::ContinuationVarInt(cv) => {
                    self.try_register(&cv.name, DeclKind::VarInt)?;
                }
                AstTopItem::Packet(p) => {
                    self.try_register(&p.name, DeclKind::Packet)?;
                }
                AstTopItem::Frame(f) => {
                    self.try_register(&f.name, DeclKind::Frame)?;
                }
                AstTopItem::Capsule(c) => {
                    self.try_register(&c.name, DeclKind::Capsule)?;
                }
                AstTopItem::StateMachine(sm) => {
                    self.try_register(&sm.name, DeclKind::StateMachine)?;
                }
                AstTopItem::StaticAssert(_) => {
                    // Nothing to register
                }
                AstTopItem::ExternAsn1(e) => {
                    self.asn1_externs.push(Asn1ExternDecl {
                        path: e.path.clone(),
                        rust_module: e.rust_module.clone(),
                        type_names: e.type_names.clone(),
                        span: e.span,
                    });
                }
            }
        }
        Ok(())
    }

    // ── Pass 2: lowering ──

    fn lower_const(&mut self, c: &AstConstDecl) -> SemaResult<SemanticConst> {
        let ty = self.resolve_named_type(&c.type_name, c.span)?;
        let value = match &c.value {
            AstLiteralValue::Int(v) => SemanticLiteral::Int(*v),
            AstLiteralValue::Bool(b) => SemanticLiteral::Bool(*b),
            AstLiteralValue::String(s) => SemanticLiteral::String(s.clone()),
            AstLiteralValue::Null => SemanticLiteral::Null,
        };
        Ok(SemanticConst {
            const_id: format!("const:{}", c.name),
            name: c.name.clone(),
            ty,
            value,
            span: c.span,
        })
    }

    fn lower_enum_decl(&mut self, e: &AstEnumDecl, is_flags: bool) -> SemaResult<SemanticEnum> {
        let underlying_type = self.resolve_named_type(&e.underlying_type, e.span)?;
        let enum_id = format!("enum:{}", e.name);
        // Check for duplicate member names
        let mut seen_names = std::collections::HashSet::new();
        for m in &e.members {
            if !seen_names.insert(&m.name) {
                return Err(SemaError::new(
                    ErrorKind::DuplicateDefinition,
                    format!("duplicate member '{}' in enum '{}'", m.name, e.name),
                )
                .with_span(m.span));
            }
        }
        // Check member values fit underlying type
        use crate::types::PrimitiveWireType;
        let max_value = match &underlying_type {
            SemanticType::Primitive { wire, .. } => match wire {
                PrimitiveWireType::U8 => Some(u8::MAX as i128),
                PrimitiveWireType::U16 => Some(u16::MAX as i128),
                PrimitiveWireType::U32 => Some(u32::MAX as i128),
                PrimitiveWireType::U64 => Some(u64::MAX as i128),
                PrimitiveWireType::I8 => Some(i8::MAX as i128),
                PrimitiveWireType::I16 => Some(i16::MAX as i128),
                PrimitiveWireType::I32 => Some(i32::MAX as i128),
                _ => None,
            },
            _ => None,
        };
        if let Some(max) = max_value {
            for m in &e.members {
                if (m.value as i128) < 0 || (m.value as i128) > max {
                    return Err(SemaError::new(
                        ErrorKind::TypeMismatch,
                        format!(
                            "member '{}' value {} does not fit underlying type in '{}'",
                            m.name, m.value, e.name
                        ),
                    )
                    .with_span(m.span));
                }
            }
        }
        let members = e
            .members
            .iter()
            .map(|m| SemanticEnumMember {
                member_id: format!("{}/member:{}", enum_id, m.name),
                name: m.name.clone(),
                value: m.value,
                span: m.span,
            })
            .collect();
        Ok(SemanticEnum {
            enum_id,
            name: e.name.clone(),
            underlying_type,
            is_flags,
            derive_traits: extract_derive_traits(&e.annotations),
            members,
            span: e.span,
        })
    }

    fn lower_flags_decl(&mut self, f: &AstFlagsDecl) -> SemaResult<SemanticEnum> {
        let underlying_type = self.resolve_named_type(&f.underlying_type, f.span)?;
        let enum_id = format!("enum:{}", f.name);
        // Check for duplicate member names
        let mut seen_names = std::collections::HashSet::new();
        for m in &f.members {
            if !seen_names.insert(&m.name) {
                return Err(SemaError::new(
                    ErrorKind::DuplicateDefinition,
                    format!("duplicate member '{}' in flags '{}'", m.name, f.name),
                )
                .with_span(m.span));
            }
        }
        // Check member values fit underlying type
        use crate::types::PrimitiveWireType;
        let max_value = match &underlying_type {
            SemanticType::Primitive { wire, .. } => match wire {
                PrimitiveWireType::U8 => Some(u8::MAX as i128),
                PrimitiveWireType::U16 => Some(u16::MAX as i128),
                PrimitiveWireType::U32 => Some(u32::MAX as i128),
                PrimitiveWireType::U64 => Some(u64::MAX as i128),
                PrimitiveWireType::I8 => Some(i8::MAX as i128),
                PrimitiveWireType::I16 => Some(i16::MAX as i128),
                PrimitiveWireType::I32 => Some(i32::MAX as i128),
                _ => None,
            },
            _ => None,
        };
        if let Some(max) = max_value {
            for m in &f.members {
                if (m.value as i128) < 0 || (m.value as i128) > max {
                    return Err(SemaError::new(
                        ErrorKind::TypeMismatch,
                        format!(
                            "member '{}' value {} does not fit underlying type in '{}'",
                            m.name, m.value, f.name
                        ),
                    )
                    .with_span(m.span));
                }
            }
        }
        let members = f
            .members
            .iter()
            .map(|m| SemanticEnumMember {
                member_id: format!("{}/member:{}", enum_id, m.name),
                name: m.name.clone(),
                value: m.value,
                span: m.span,
            })
            .collect();
        Ok(SemanticEnum {
            enum_id,
            name: f.name.clone(),
            underlying_type,
            is_flags: true,
            derive_traits: extract_derive_traits(&f.annotations),
            members,
            span: f.span,
        })
    }

    fn lower_varint_prefix_match(
        &mut self,
        name: &str,
        fields: &[AstFieldDef],
        annotations: &[AstAnnotation],
        span: Option<wirespec_syntax::span::Span>,
    ) -> SemaResult<SemanticVarInt> {
        // Expected pattern:
        // field[0]: prefix: bits[N]
        // field[1]: value: match prefix { ... => bits[M], ... }
        let varint_id = format!("varint:{}", name);

        if fields.len() < 2 {
            return Err(SemaError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "type '{}' has too few fields for VarInt prefix-match pattern",
                    name
                ),
            ));
        }

        // Extract prefix bits
        let prefix_bits = match &fields[0].type_expr {
            AstTypeExpr::Bits { width, .. } => *width as u8,
            _ => {
                return Err(SemaError::new(
                    ErrorKind::TypeMismatch,
                    format!("VarInt '{}' first field must be bits[N]", name),
                ));
            }
        };

        // Extract match branches
        let match_branches = match &fields[1].type_expr {
            AstTypeExpr::Match { branches, .. } => branches,
            _ => {
                return Err(SemaError::new(
                    ErrorKind::TypeMismatch,
                    format!("VarInt '{}' second field must be match", name),
                ));
            }
        };

        let mut branches = Vec::new();
        for branch in match_branches {
            let prefix_value = match &branch.pattern {
                AstPattern::Value { value, .. } => *value as u64,
                AstPattern::Wildcard { .. } => continue,
                AstPattern::RangeInclusive { start, .. } => *start as u64,
            };
            let value_bits = match &branch.result_type {
                AstTypeExpr::Bits { width, .. } => *width as u8,
                _ => 0,
            };
            let total_bits = prefix_bits as u16 + value_bits as u16;
            let total_bytes = total_bits.div_ceil(8) as u8;
            let max_value = if value_bits >= 64 {
                u64::MAX
            } else {
                (1u64 << value_bits) - 1
            };
            let prefix_mask = if prefix_bits >= 64 {
                u64::MAX
            } else {
                (1u64 << prefix_bits) - 1
            };
            branches.push(SemanticVarIntBranch {
                prefix_value,
                prefix_bits,
                value_bits,
                total_bytes,
                max_value,
                prefix_mask,
            });
        }

        let max_bytes = branches.iter().map(|b| b.total_bytes).max().unwrap_or(1);

        // Check for @strict annotation
        let strict = annotations.iter().any(|a| a.name == "strict");

        // Check for byte_order
        let byte_order = self.registry.module_endianness();

        Ok(SemanticVarInt {
            varint_id,
            name: name.to_string(),
            encoding: VarIntEncoding::PrefixMatch,
            prefix_bits: Some(prefix_bits),
            branches,
            value_bits_per_byte: None,
            max_bytes,
            byte_order,
            strict,
            span,
        })
    }

    fn lower_continuation_varint(&self, cv: &AstContinuationVarIntDecl) -> SemanticVarInt {
        let byte_order = match cv.byte_order.as_str() {
            "little" => Endianness::Little,
            _ => Endianness::Big,
        };
        let strict = cv.annotations.iter().any(|a| a.name == "strict");
        SemanticVarInt {
            varint_id: format!("varint:{}", cv.name),
            name: cv.name.clone(),
            encoding: VarIntEncoding::ContinuationBit,
            prefix_bits: None,
            branches: Vec::new(),
            value_bits_per_byte: Some(cv.value_bits),
            max_bytes: cv.max_bytes,
            byte_order,
            strict,
            span: cv.span,
        }
    }

    fn lower_packet(&mut self, p: &AstPacketDecl) -> SemaResult<SemanticPacket> {
        let packet_id = format!("packet:{}", p.name);
        let scope_id = packet_id.clone();

        let mut fields = Vec::new();
        let mut derived = Vec::new();
        let mut requires = Vec::new();
        let mut items = Vec::new();
        let mut declared: Vec<String> = Vec::new();
        let mut optional_fields: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut field_idx: usize = 0;
        let mut derived_idx: usize = 0;
        let mut require_idx: usize = 0;

        for fi in &p.fields {
            match fi {
                AstFieldItem::Field(f) => {
                    // Task 7: track optional fields
                    if matches!(&f.type_expr, AstTypeExpr::Optional { .. }) {
                        optional_fields.insert(f.name.clone());
                    }
                    // Task 7: validate bytes[length_or_remaining:] references an optional field
                    if let AstTypeExpr::Bytes {
                        kind: AstBytesKind::LengthOrRemaining,
                        size_expr: Some(expr),
                        ..
                    } = &f.type_expr
                        && let AstExpr::NameRef { name, .. } = &**expr
                        && !optional_fields.contains(name.as_str())
                    {
                        return Err(SemaError::new(
                                    ErrorKind::InvalidLengthOrRemaining,
                                    format!(
                                        "bytes[length_or_remaining: {name}]: '{name}' must be an optional field"
                                    ),
                                )
                                .with_span(f.span));
                    }
                    let sem = self.lower_field(f, &scope_id, field_idx, &declared)?;
                    Self::validate_integer_like_size_ref(&f.type_expr, &fields)?;
                    declared.push(f.name.clone());
                    items.push(SemanticScopeItem::Field {
                        field_id: sem.field_id.clone(),
                    });
                    fields.push(sem);
                    field_idx += 1;
                }
                AstFieldItem::Derived(d) => {
                    let sem = self.lower_derived(d, &scope_id, derived_idx, &declared)?;
                    declared.push(d.name.clone());
                    items.push(SemanticScopeItem::Derived {
                        derived_id: sem.derived_id.clone(),
                    });
                    derived.push(sem);
                    derived_idx += 1;
                }
                AstFieldItem::Require(r) => {
                    let sem = self.lower_require(r, &scope_id, require_idx, &declared);
                    items.push(SemanticScopeItem::Require {
                        require_id: sem.require_id.clone(),
                    });
                    requires.push(sem);
                    require_idx += 1;
                }
            }
        }

        self.first_error()?;

        // Scope-level validations
        let field_descriptors: Vec<FieldDescriptor> = fields
            .iter()
            .map(|f| FieldDescriptor {
                name: f.name.clone(),
                is_remaining: matches!(
                    &f.ty,
                    SemanticType::Bytes {
                        bytes_kind: SemanticBytesKind::Remaining,
                        ..
                    }
                ),
                is_fill: matches!(
                    &f.ty,
                    SemanticType::Array {
                        count_expr: None,
                        ..
                    }
                ),
                is_wire: true,
            })
            .collect();
        validate_remaining_is_last(&field_descriptors)?;

        let checksum_fields: Vec<&str> = fields
            .iter()
            .filter_map(|f| f.checksum_algorithm.as_deref())
            .collect();
        validate_single_checksum(&checksum_fields, &format!("packet '{}'", p.name))?;

        Ok(SemanticPacket {
            packet_id,
            name: p.name.clone(),
            derive_traits: extract_derive_traits(&p.annotations),
            fields,
            derived,
            requires,
            items,
            span: p.span,
        })
    }

    fn lower_frame(&mut self, f: &AstFrameDecl) -> SemaResult<SemanticFrame> {
        let frame_id = format!("frame:{}", f.name);
        let tag_type = self.resolve_named_type(&f.tag_type, f.span)?;

        // Tag field name is visible to branch scope expressions (e.g., `if frame_type & 0x04`)
        let tag_declared = vec![f.tag_field.clone()];

        let mut variants = Vec::new();
        for (i, branch) in f.branches.iter().enumerate() {
            let sem = self.lower_variant_scope(
                branch,
                &frame_id,
                i as u32,
                &tag_declared,
                VariantOwner::Frame {
                    frame_id: frame_id.clone(),
                },
            )?;
            variants.push(sem);
        }

        self.first_error()?;

        // Finding 8: match exhaustiveness — require wildcard branch
        let has_wildcard = variants
            .iter()
            .any(|v| matches!(&v.pattern, SemanticPattern::Wildcard));
        if !has_wildcard {
            return Err(SemaError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "frame '{}': match is not exhaustive, add a wildcard (_) branch",
                    f.name
                ),
            )
            .with_span(f.span));
        }

        Ok(SemanticFrame {
            frame_id,
            name: f.name.clone(),
            derive_traits: extract_derive_traits(&f.annotations),
            tag_name: f.tag_field.clone(),
            tag_type,
            variants,
            span: f.span,
        })
    }

    fn lower_capsule(&mut self, c: &AstCapsuleDecl) -> SemaResult<SemanticCapsule> {
        let capsule_id = format!("capsule:{}", c.name);
        let scope_id = capsule_id.clone();

        // Lower header fields
        let mut header_fields = Vec::new();
        let mut header_derived = Vec::new();
        let mut header_requires = Vec::new();
        let mut header_items = Vec::new();
        let mut declared: Vec<String> = Vec::new();
        let mut field_idx: usize = 0;
        let mut derived_idx: usize = 0;
        let mut require_idx: usize = 0;

        for fi in &c.fields {
            match fi {
                AstFieldItem::Field(f) => {
                    let sem = self.lower_field(f, &scope_id, field_idx, &declared)?;
                    Self::validate_integer_like_size_ref(&f.type_expr, &header_fields)?;
                    declared.push(f.name.clone());
                    header_items.push(SemanticScopeItem::Field {
                        field_id: sem.field_id.clone(),
                    });
                    header_fields.push(sem);
                    field_idx += 1;
                }
                AstFieldItem::Derived(d) => {
                    let sem = self.lower_derived(d, &scope_id, derived_idx, &declared)?;
                    declared.push(d.name.clone());
                    header_items.push(SemanticScopeItem::Derived {
                        derived_id: sem.derived_id.clone(),
                    });
                    header_derived.push(sem);
                    derived_idx += 1;
                }
                AstFieldItem::Require(r) => {
                    let sem = self.lower_require(r, &scope_id, require_idx, &declared);
                    header_items.push(SemanticScopeItem::Require {
                        require_id: sem.require_id.clone(),
                    });
                    header_requires.push(sem);
                    require_idx += 1;
                }
            }
        }

        // Resolve tag selector
        let tag_selector = match &c.payload_tag {
            AstPayloadTagSelector::Field { field_name } => {
                // Find the field_id in header_fields
                let fid = header_fields
                    .iter()
                    .find(|hf| hf.name == *field_name)
                    .map(|hf| hf.field_id.clone())
                    .unwrap_or_else(|| format!("{}.field:unknown", scope_id));
                CapsuleTagSelector::Field {
                    field_id: fid,
                    field_name: field_name.clone(),
                }
            }
            AstPayloadTagSelector::Expr { expr } => CapsuleTagSelector::Expr {
                expr: self.lower_expr(expr, &declared, &[]),
            },
        };

        // Determine tag type from selector
        let tag_type = match &tag_selector {
            CapsuleTagSelector::Field { field_name, .. } => header_fields
                .iter()
                .find(|hf| hf.name == *field_name)
                .map(|hf| hf.ty.clone())
                .unwrap_or(SemanticType::Primitive {
                    wire: PrimitiveWireType::U8,
                    endianness: None,
                }),
            CapsuleTagSelector::Expr { .. } => SemanticType::Primitive {
                wire: PrimitiveWireType::U8,
                endianness: None,
            },
        };

        // Resolve within_field
        let within_field_name = c.payload_within.clone();
        let within_field_id = header_fields
            .iter()
            .find(|hf| hf.name == within_field_name)
            .map(|hf| hf.field_id.clone())
            .unwrap_or_else(|| format!("{}.field:unknown", scope_id));

        // Lower variants
        let mut variants = Vec::new();
        for (i, branch) in c.branches.iter().enumerate() {
            let sem = self.lower_variant_scope(
                branch,
                &capsule_id,
                i as u32,
                &declared,
                VariantOwner::CapsulePayload {
                    capsule_id: capsule_id.clone(),
                },
            )?;
            variants.push(sem);
        }

        self.first_error()?;

        // Finding 8: match exhaustiveness — require wildcard branch
        let has_wildcard = variants
            .iter()
            .any(|v| matches!(&v.pattern, SemanticPattern::Wildcard));
        if !has_wildcard {
            return Err(SemaError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "capsule '{}': match is not exhaustive, add a wildcard (_) branch",
                    c.name
                ),
            )
            .with_span(c.span));
        }

        Ok(SemanticCapsule {
            capsule_id,
            name: c.name.clone(),
            derive_traits: extract_derive_traits(&c.annotations),
            tag_type,
            tag_selector,
            within_field_id,
            within_field_name,
            header_fields,
            header_derived,
            header_requires,
            header_items,
            variants,
            span: c.span,
        })
    }

    fn lower_variant_scope(
        &mut self,
        branch: &AstFrameBranch,
        owner_id: &str,
        ordinal: u32,
        parent_declared: &[String],
        owner: VariantOwner,
    ) -> SemaResult<SemanticVariantScope> {
        let scope_id = format!("{}/variant:{}", owner_id, branch.variant_name);

        let pattern = lower_pattern(&branch.pattern);

        let mut fields = Vec::new();
        let mut derived_list = Vec::new();
        let mut requires = Vec::new();
        let mut items = Vec::new();
        let mut declared: Vec<String> = parent_declared.to_vec();
        let mut optional_fields: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut field_idx: usize = 0;
        let mut derived_idx: usize = 0;
        let mut require_idx: usize = 0;

        for fi in &branch.fields {
            match fi {
                AstFieldItem::Field(f) => {
                    // Task 7: track optional fields
                    if matches!(&f.type_expr, AstTypeExpr::Optional { .. }) {
                        optional_fields.insert(f.name.clone());
                    }
                    // Task 7: validate bytes[length_or_remaining:] references an optional field
                    if let AstTypeExpr::Bytes {
                        kind: AstBytesKind::LengthOrRemaining,
                        size_expr: Some(expr),
                        ..
                    } = &f.type_expr
                        && let AstExpr::NameRef { name, .. } = &**expr
                        && !optional_fields.contains(name.as_str())
                    {
                        return Err(SemaError::new(
                                    ErrorKind::InvalidLengthOrRemaining,
                                    format!(
                                        "bytes[length_or_remaining: {name}]: '{name}' must be an optional field"
                                    ),
                                )
                                .with_span(f.span));
                    }
                    let sem = self.lower_field(f, &scope_id, field_idx, &declared)?;
                    Self::validate_integer_like_size_ref(&f.type_expr, &fields)?;
                    declared.push(f.name.clone());
                    items.push(SemanticScopeItem::Field {
                        field_id: sem.field_id.clone(),
                    });
                    fields.push(sem);
                    field_idx += 1;
                }
                AstFieldItem::Derived(d) => {
                    let sem = self.lower_derived(d, &scope_id, derived_idx, &declared)?;
                    declared.push(d.name.clone());
                    items.push(SemanticScopeItem::Derived {
                        derived_id: sem.derived_id.clone(),
                    });
                    derived_list.push(sem);
                    derived_idx += 1;
                }
                AstFieldItem::Require(r) => {
                    let sem = self.lower_require(r, &scope_id, require_idx, &declared);
                    items.push(SemanticScopeItem::Require {
                        require_id: sem.require_id.clone(),
                    });
                    requires.push(sem);
                    require_idx += 1;
                }
            }
        }

        // Scope-level validations
        let field_descriptors: Vec<FieldDescriptor> = fields
            .iter()
            .map(|f| FieldDescriptor {
                name: f.name.clone(),
                is_remaining: matches!(
                    &f.ty,
                    SemanticType::Bytes {
                        bytes_kind: SemanticBytesKind::Remaining,
                        ..
                    }
                ),
                is_fill: matches!(
                    &f.ty,
                    SemanticType::Array {
                        count_expr: None,
                        ..
                    }
                ),
                is_wire: true,
            })
            .collect();
        validate_remaining_is_last(&field_descriptors)?;

        let checksum_fields: Vec<&str> = fields
            .iter()
            .filter_map(|f| f.checksum_algorithm.as_deref())
            .collect();
        validate_single_checksum(
            &checksum_fields,
            &format!("variant '{}'", branch.variant_name),
        )?;

        Ok(SemanticVariantScope {
            scope_id,
            owner,
            variant_name: branch.variant_name.clone(),
            ordinal,
            pattern,
            fields,
            derived: derived_list,
            requires,
            items,
            span: branch.span,
        })
    }

    fn lower_field(
        &mut self,
        field: &AstFieldDef,
        scope_id: &str,
        index: usize,
        declared: &[String],
    ) -> SemaResult<SemanticField> {
        let field_id = format!("{}.field[{}]", scope_id, index);

        // Clear any pending hint before resolving
        self.pending_asn1_hint = None;

        // Resolve type expression
        let (ty, presence) = self.resolve_type_expr(&field.type_expr)?;

        // Take the pending ASN.1 hint (set by resolve_type_expr for asn1() types)
        let asn1_hint = self.pending_asn1_hint.take();

        // bool is a semantic type, not a wire type — reject in wire field context
        if let SemanticType::Primitive {
            wire: PrimitiveWireType::Bool,
            ..
        } = &ty
        {
            return Err(SemaError::new(
                ErrorKind::TypeMismatch,
                format!("'bool' is a semantic type, not a wire type; use 'u8' or 'bits[1]' for field '{}'", field.name),
            ).with_span(field.span)
             .with_hint("'bool' is valid in derived fields: let flag: bool = ...".to_string()));
        }

        // Check for forward references
        let mut refs = Vec::new();
        collect_type_expr_refs(&field.type_expr, &mut refs);
        // Filter refs to only those that look like field names (not type names)
        let field_refs: Vec<String> = refs
            .into_iter()
            .filter(|r| {
                // If it's a known type/const, it's not a forward ref to a field
                !self.registry.contains(r)
            })
            .collect();
        validate_no_forward_refs(&field_refs, declared, &field.name, field.span)?;

        // Check annotations
        let mut checksum_algorithm = None;
        let mut max_elements = None;
        for ann in &field.annotations {
            if ann.name == "checksum"
                && let Some(AstAnnotationArg::Identifier(algo)) = ann.args.first()
            {
                validate_checksum_profile(algo, self.profile)?;
                checksum_algorithm = Some(algo.clone());
            }
            if ann.name == "max_len"
                && let Some(AstAnnotationArg::Int(n)) = ann.args.first()
            {
                max_elements = Some(*n as u32);
            }
        }

        // Validate checksum field type matches algorithm
        if let Some(ref algo) = checksum_algorithm {
            let field_type_name = match &ty {
                SemanticType::Primitive { wire, .. } => match wire {
                    PrimitiveWireType::U8 => "u8",
                    PrimitiveWireType::U16 => "u16",
                    PrimitiveWireType::U24 => "u24",
                    PrimitiveWireType::U32 => "u32",
                    PrimitiveWireType::U64 => "u64",
                    PrimitiveWireType::I8 => "i8",
                    PrimitiveWireType::I16 => "i16",
                    PrimitiveWireType::I32 => "i32",
                    PrimitiveWireType::I64 => "i64",
                    PrimitiveWireType::Bool => "bool",
                    PrimitiveWireType::Bit => "bit",
                },
                _ => "unknown",
            };
            validate_checksum_field_type(algo, field_type_name, &field.name)?;
        }

        Ok(SemanticField {
            field_id,
            name: field.name.clone(),
            ty,
            presence,
            max_elements,
            checksum_algorithm,
            asn1_hint,
            span: field.span,
        })
    }

    /// Validate that bytes[length:]/bytes[length_or_remaining:] and array count
    /// expressions that reference a field use an integer-like type (spec §4.1).
    fn validate_integer_like_size_ref(
        ast_type_expr: &AstTypeExpr,
        lowered_fields: &[SemanticField],
    ) -> SemaResult<()> {
        match ast_type_expr {
            AstTypeExpr::Bytes {
                kind: AstBytesKind::Length | AstBytesKind::LengthOrRemaining,
                size_expr: Some(expr),
                ..
            } => {
                if let AstExpr::NameRef { name, span } = &**expr
                    && let Some(field) = lowered_fields.iter().find(|f| &f.name == name)
                {
                    let ty = match &field.presence {
                        FieldPresence::Conditional { .. } => {
                            // For optional fields, the underlying type is what matters
                            &field.ty
                        }
                        _ => &field.ty,
                    };
                    if !ty.is_integer_like() {
                        return Err(SemaError::new(
                                ErrorKind::InvalidBytesLength,
                                format!(
                                    "bytes length reference '{name}' must be an integer-like type"
                                ),
                            )
                            .with_span(*span)
                            .with_hint(
                                "integer-like types: u8, u16, u24, u32, u64, i8..i64, VarInt, bits[N], enum"
                                    .to_string(),
                            ));
                    }
                }
                Ok(())
            }
            AstTypeExpr::Array {
                count: AstArrayCount::Expr(expr),
                ..
            } => {
                if let AstExpr::NameRef { name, span } = expr
                    && let Some(field) = lowered_fields.iter().find(|f| &f.name == name)
                    && !field.ty.is_integer_like()
                {
                    return Err(SemaError::new(
                                ErrorKind::InvalidArrayCount,
                                format!(
                                    "array count reference '{name}' must be an integer-like type"
                                ),
                            )
                            .with_span(*span)
                            .with_hint(
                                "integer-like types: u8, u16, u24, u32, u64, i8..i64, VarInt, bits[N], enum"
                                    .to_string(),
                            ));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn lower_derived(
        &mut self,
        d: &AstDerivedField,
        scope_id: &str,
        index: usize,
        declared: &[String],
    ) -> SemaResult<SemanticDerived> {
        let derived_id = format!("{}.derived[{}]", scope_id, index);
        let ty = self.resolve_named_type(&d.type_name, d.span)?;
        let expr = self.lower_expr(&d.expr, declared, &[]);
        Ok(SemanticDerived {
            derived_id,
            name: d.name.clone(),
            ty,
            expr,
            span: d.span,
        })
    }

    fn lower_require(
        &mut self,
        r: &AstRequireClause,
        scope_id: &str,
        index: usize,
        declared: &[String],
    ) -> SemanticRequire {
        let require_id = format!("{}.require[{}]", scope_id, index);
        let expr = self.lower_expr(&r.expr, declared, &[]);
        SemanticRequire {
            require_id,
            expr,
            span: r.span,
        }
    }

    // ── Type resolution ──

    fn resolve_named_type(
        &mut self,
        name: &str,
        span: Option<wirespec_syntax::span::Span>,
    ) -> SemaResult<SemanticType> {
        match self.registry.resolve_type_name(name) {
            Some(ResolvedType::Primitive(wire, endian)) => Ok(SemanticType::Primitive {
                wire,
                endianness: endian,
            }),
            Some(ResolvedType::UserDefined(resolved_name, kind)) => match kind {
                DeclKind::VarInt => Ok(SemanticType::VarIntRef {
                    varint_id: format!("varint:{}", resolved_name),
                    name: resolved_name,
                }),
                DeclKind::Packet => Ok(SemanticType::PacketRef {
                    packet_id: format!("packet:{}", resolved_name),
                    name: resolved_name,
                }),
                DeclKind::Enum => Ok(SemanticType::EnumRef {
                    enum_id: format!("enum:{}", resolved_name),
                    name: resolved_name,
                    is_flags: false,
                }),
                DeclKind::Flags => Ok(SemanticType::EnumRef {
                    enum_id: format!("enum:{}", resolved_name),
                    name: resolved_name,
                    is_flags: true,
                }),
                DeclKind::Frame => Ok(SemanticType::FrameRef {
                    frame_id: format!("frame:{}", resolved_name),
                    name: resolved_name,
                }),
                DeclKind::Capsule => Ok(SemanticType::CapsuleRef {
                    capsule_id: format!("capsule:{}", resolved_name),
                    name: resolved_name,
                }),
                DeclKind::StateMachine => {
                    // State machines can't be used as field types
                    Err(SemaError::new(
                        ErrorKind::TypeMismatch,
                        format!(
                            "state machine '{}' cannot be used as a field type",
                            resolved_name
                        ),
                    )
                    .with_span(span))
                }
                DeclKind::Const => {
                    // Consts can't be used as types
                    Err(SemaError::new(
                        ErrorKind::TypeMismatch,
                        format!("const '{}' cannot be used as a type", resolved_name),
                    )
                    .with_span(span))
                }
            },
            None => {
                let all_type_names = self.registry.all_names();
                let candidate_strs: Vec<&str> = all_type_names.iter().map(|s| s.as_str()).collect();
                let hint = suggest_similar(name, &candidate_strs, 2)
                    .map(|suggestion| format!("did you mean '{suggestion}'?"));

                let mut err = SemaError::new(
                    ErrorKind::UndefinedType,
                    format!("undefined type '{}'", name),
                )
                .with_span(span);
                if let Some(h) = hint {
                    err = err.with_hint(h);
                }
                Err(err)
            }
        }
    }

    fn resolve_type_expr(
        &mut self,
        texpr: &AstTypeExpr,
    ) -> SemaResult<(SemanticType, FieldPresence)> {
        match texpr {
            AstTypeExpr::Named { name, span } => {
                let ty = self.resolve_named_type(name, *span)?;
                Ok((ty, FieldPresence::Always))
            }
            AstTypeExpr::Bits { width, .. } => Ok((
                SemanticType::Bits { width_bits: *width },
                FieldPresence::Always,
            )),
            AstTypeExpr::Bytes {
                kind,
                fixed_size,
                size_expr,
                ..
            } => {
                let bytes_kind = match kind {
                    AstBytesKind::Fixed => SemanticBytesKind::Fixed,
                    AstBytesKind::Length => SemanticBytesKind::Length,
                    AstBytesKind::Remaining => SemanticBytesKind::Remaining,
                    AstBytesKind::LengthOrRemaining => SemanticBytesKind::LengthOrRemaining,
                };
                let sem_size_expr = size_expr
                    .as_ref()
                    .map(|e| Box::new(self.lower_expr(e, &[], &[])));
                Ok((
                    SemanticType::Bytes {
                        bytes_kind,
                        fixed_size: *fixed_size,
                        size_expr: sem_size_expr,
                    },
                    FieldPresence::Always,
                ))
            }
            AstTypeExpr::Array {
                element_type,
                count,
                within_expr,
                ..
            } => {
                let (elem_ty, _) = self.resolve_type_expr(element_type)?;
                let count_expr = match count {
                    AstArrayCount::Expr(e) => Some(Box::new(self.lower_expr(e, &[], &[]))),
                    AstArrayCount::Fill => None,
                };
                let sem_within = within_expr
                    .as_ref()
                    .map(|e| Box::new(self.lower_expr(e, &[], &[])));
                Ok((
                    SemanticType::Array {
                        element_type: Box::new(elem_ty),
                        count_expr,
                        within_expr: sem_within,
                    },
                    FieldPresence::Always,
                ))
            }
            AstTypeExpr::Optional {
                condition,
                inner_type,
                ..
            } => {
                let (ty, _) = self.resolve_type_expr(inner_type)?;
                let cond = self.lower_expr(condition, &[], &[]);
                Ok((ty, FieldPresence::Conditional { condition: cond }))
            }
            AstTypeExpr::Match { branches, .. } => {
                // For match type expressions used outside varint context,
                // just resolve the first branch's type
                if let Some(first) = branches.first() {
                    self.resolve_type_expr(&first.result_type)
                } else {
                    Err(SemaError::new(
                        ErrorKind::TypeMismatch,
                        "empty match type expression",
                    ))
                }
            }
            AstTypeExpr::Asn1 {
                type_name,
                encoding,
                length,
                ..
            } => {
                // Validate type name exists in extern declarations
                let extern_decl = self
                    .asn1_externs
                    .iter()
                    .find(|e| e.type_names.contains(type_name))
                    .ok_or_else(|| {
                        SemaError::new(
                            ErrorKind::UndefinedAsn1Type,
                            format!(
                                "ASN.1 type '{}' not declared in any 'extern asn1' block",
                                type_name
                            ),
                        )
                    })?;
                let extern_path = extern_decl.path.clone();
                let rust_module = extern_decl.rust_module.clone();

                // Validate encoding
                const SUPPORTED_ENCODINGS: &[&str] = &["uper", "ber", "der", "aper", "oer", "coer"];
                if !SUPPORTED_ENCODINGS.contains(&encoding.as_str()) {
                    return Err(SemaError::new(
                        ErrorKind::UnsupportedAsn1Encoding,
                        format!(
                            "unsupported ASN.1 encoding '{}'; supported: {}",
                            encoding,
                            SUPPORTED_ENCODINGS.join(", ")
                        ),
                    ));
                }

                // Lower to bytes type
                use wirespec_syntax::ast::Asn1Length;
                let (bytes_kind, size_expr) = match length {
                    Asn1Length::Remaining => (SemanticBytesKind::Remaining, None),
                    Asn1Length::Expr(expr) => {
                        let sem_expr = self.lower_expr(expr, &[], &[]);
                        (SemanticBytesKind::Length, Some(Box::new(sem_expr)))
                    }
                };

                // Store hint for pending attachment
                self.pending_asn1_hint = Some(Asn1Hint {
                    type_name: type_name.clone(),
                    encoding: encoding.clone(),
                    extern_path,
                    rust_module,
                });

                Ok((
                    SemanticType::Bytes {
                        bytes_kind,
                        fixed_size: None,
                        size_expr,
                    },
                    FieldPresence::Always,
                ))
            }
        }
    }

    // ── Expression lowering ──

    fn lower_expr(
        &self,
        expr: &AstExpr,
        declared: &[String],
        const_names: &[String],
    ) -> SemanticExpr {
        self.lower_expr_scoped(expr, declared, const_names, "")
    }

    fn lower_expr_scoped(
        &self,
        expr: &AstExpr,
        declared: &[String],
        const_names: &[String],
        _scope_id: &str,
    ) -> SemanticExpr {
        match expr {
            AstExpr::Int { value, .. } => SemanticExpr::Literal {
                value: SemanticLiteral::Int(*value),
            },
            AstExpr::Bool { value, .. } => SemanticExpr::Literal {
                value: SemanticLiteral::Bool(*value),
            },
            AstExpr::Null { .. } => SemanticExpr::Literal {
                value: SemanticLiteral::Null,
            },
            AstExpr::NameRef { name, .. } => {
                // Check declared fields first — use bare name as value_id
                // (backends extract field name from value_id for code generation)
                if declared.iter().any(|n| n == name) {
                    SemanticExpr::ValueRef {
                        reference: ValueRef {
                            value_id: name.clone(),
                            kind: ValueRefKind::Field,
                        },
                    }
                } else if self.registry.get_const_value(name).is_some() {
                    SemanticExpr::ValueRef {
                        reference: ValueRef {
                            value_id: name.clone(),
                            kind: ValueRefKind::Const,
                        },
                    }
                } else {
                    // Unknown reference: emit as field ref with bare name
                    // (forward reference checking catches actual errors earlier)
                    SemanticExpr::ValueRef {
                        reference: ValueRef {
                            value_id: name.clone(),
                            kind: ValueRefKind::Field,
                        },
                    }
                }
            }
            AstExpr::Binary {
                op, left, right, ..
            } => SemanticExpr::Binary {
                op: binop_to_string(op),
                left: Box::new(self.lower_expr(left, declared, const_names)),
                right: Box::new(self.lower_expr(right, declared, const_names)),
            },
            AstExpr::Unary { op, operand, .. } => SemanticExpr::Unary {
                op: unaryop_to_string(op),
                operand: Box::new(self.lower_expr(operand, declared, const_names)),
            },
            AstExpr::Coalesce {
                expr: inner,
                default,
                ..
            } => SemanticExpr::Coalesce {
                expr: Box::new(self.lower_expr(inner, declared, const_names)),
                default: Box::new(self.lower_expr(default, declared, const_names)),
            },
            AstExpr::MemberAccess { base, field, .. } => {
                // Check if base is NameRef("src") or NameRef("dst")
                if let AstExpr::NameRef { name, .. } = base.as_ref()
                    && (name == "src" || name == "dst")
                {
                    let peer = if name == "src" {
                        TransitionPeerKind::Src
                    } else {
                        TransitionPeerKind::Dst
                    };
                    return SemanticExpr::TransitionPeerRef {
                        reference: TransitionPeerRef {
                            peer,
                            event_param_id: None,
                            path: vec![field.clone()],
                        },
                    };
                }
                // Otherwise create a ValueRef with dotted path
                let base_name = extract_base_name(base);
                SemanticExpr::ValueRef {
                    reference: ValueRef {
                        value_id: format!("{}.{}", base_name, field),
                        kind: ValueRefKind::Field,
                    },
                }
            }
            AstExpr::Subscript { base, index, .. } => SemanticExpr::Subscript {
                base: Box::new(self.lower_expr(base, declared, const_names)),
                index: Box::new(self.lower_expr(index, declared, const_names)),
            },
            AstExpr::Fill { value, count, .. } => SemanticExpr::Fill {
                value: Box::new(self.lower_expr(value, declared, const_names)),
                count: Box::new(self.lower_expr(count, declared, const_names)),
            },
            AstExpr::Slice {
                base, start, end, ..
            } => SemanticExpr::Slice {
                base: Box::new(self.lower_expr(base, declared, const_names)),
                start: Box::new(self.lower_expr(start, declared, const_names)),
                end: Box::new(self.lower_expr(end, declared, const_names)),
            },
            AstExpr::StateConstructor {
                sm_name,
                state_name,
                args,
                ..
            } => {
                let sm_id = format!("sm:{}", sm_name);
                let state_id = format!("sm:{}/state:{}", sm_name, state_name);
                SemanticExpr::StateConstructor {
                    sm_id,
                    sm_name: sm_name.clone(),
                    state_id,
                    state_name: state_name.clone(),
                    args: args
                        .iter()
                        .map(|a| self.lower_expr(a, declared, const_names))
                        .collect(),
                }
            }
            AstExpr::InState {
                expr: inner,
                state_name,
                ..
            } => {
                // We need to find which SM this refers to.
                // For now, use a placeholder sm_id derived from context.
                let inner_lowered = self.lower_expr(inner, declared, const_names);
                SemanticExpr::InState {
                    expr: Box::new(inner_lowered),
                    sm_id: String::new(),
                    sm_name: String::new(),
                    state_id: String::new(),
                    state_name: state_name.clone(),
                }
            }
            AstExpr::All {
                collection,
                state_name,
                ..
            } => SemanticExpr::All {
                collection: Box::new(self.lower_expr(collection, declared, const_names)),
                sm_id: String::new(),
                sm_name: String::new(),
                state_id: String::new(),
                state_name: state_name.clone(),
            },
        }
    }

    // ── State machine ──

    fn lower_state_machine(
        &mut self,
        sm: &AstStateMachineDecl,
    ) -> SemaResult<SemanticStateMachine> {
        let sm_id = format!("sm:{}", sm.name);

        // Collect states
        let mut states = Vec::new();
        let mut state_names: Vec<String> = Vec::new();
        let mut terminal_names: Vec<String> = Vec::new();

        for s in &sm.states {
            let state_id = format!("{}/state:{}", sm_id, s.name);
            let fields: Vec<SemanticStateField> = s
                .fields
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let ty = self.resolve_state_field_type(&f.type_expr).unwrap_or(
                        SemanticType::Primitive {
                            wire: PrimitiveWireType::U8,
                            endianness: None,
                        },
                    );
                    let default_value = f.default_value.as_ref().map(|dv| match dv {
                        AstLiteralValue::Int(v) => SemanticLiteral::Int(*v),
                        AstLiteralValue::Bool(b) => SemanticLiteral::Bool(*b),
                        AstLiteralValue::String(s) => SemanticLiteral::String(s.clone()),
                        AstLiteralValue::Null => SemanticLiteral::Null,
                    });
                    // Detect child state machine references (direct or array element)
                    let sm_ref_type = match &ty {
                        SemanticType::PacketRef {
                            packet_id,
                            name: ref_name,
                        } => Some((packet_id, ref_name)),
                        SemanticType::Array { element_type, .. } => {
                            if let SemanticType::PacketRef {
                                packet_id,
                                name: ref_name,
                            } = element_type.as_ref()
                            {
                                Some((packet_id, ref_name))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    let (child_sm_id, child_sm_name) =
                        if let Some((packet_id, ref_name)) = sm_ref_type {
                            if packet_id.starts_with("sm:") {
                                (Some(packet_id.clone()), Some(ref_name.clone()))
                            } else {
                                (None, None)
                            }
                        } else {
                            (None, None)
                        };
                    SemanticStateField {
                        field_id: format!("{}.field[{}]", state_id, i),
                        name: f.name.clone(),
                        ty,
                        default_value,
                        child_sm_id,
                        child_sm_name,
                        span: f.span,
                    }
                })
                .collect();

            if s.is_terminal {
                terminal_names.push(s.name.clone());
            }

            states.push(SemanticState {
                state_id,
                name: s.name.clone(),
                fields,
                is_terminal: s.is_terminal,
                span: s.span,
            });
            state_names.push(s.name.clone());
        }

        // Validate initial state exists (Task 4)
        if sm.initial_state.is_empty() {
            return Err(SemaError::new(
                ErrorKind::SmInvalidInitial,
                format!("no initial state declared in state machine '{}'", sm.name),
            )
            .with_span(sm.span));
        }
        if !state_names.contains(&sm.initial_state) {
            return Err(SemaError::new(
                ErrorKind::SmInvalidInitial,
                format!(
                    "initial state '{}' not found in state machine '{}'",
                    sm.initial_state, sm.name
                ),
            )
            .with_span(sm.span));
        }
        let initial_state_id = format!("{}/state:{}", sm_id, sm.initial_state);

        // Collect unique events from transitions
        let mut event_map: std::collections::HashMap<String, SemanticEvent> =
            std::collections::HashMap::new();

        for t in &sm.transitions {
            for ev in &t.events {
                // child_state_changed is a built-in event that users reference
                // in transitions but must not define with custom parameters.
                // Allow it as a bare trigger (no params); reject if user tries
                // to define it with custom params.
                if ev.name == "child_state_changed" {
                    if !ev.params.is_empty() {
                        return Err(SemaError::new(
                            ErrorKind::ReservedIdentifier,
                            "'child_state_changed' is a reserved identifier".to_string(),
                        ));
                    }
                } else {
                    Self::check_reserved(&ev.name)?;
                }
                if !event_map.contains_key(&ev.name) {
                    let event_id = format!("{}/event:{}", sm_id, ev.name);
                    let params: Vec<SemanticEventParam> = ev
                        .params
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let ty = self.resolve_state_field_type(&p.type_expr).unwrap_or(
                                SemanticType::Primitive {
                                    wire: PrimitiveWireType::U8,
                                    endianness: None,
                                },
                            );
                            SemanticEventParam {
                                param_id: format!("{}.param[{}]", event_id, i),
                                name: p.name.clone(),
                                ty,
                                span: p.span,
                            }
                        })
                        .collect();
                    event_map.insert(
                        ev.name.clone(),
                        SemanticEvent {
                            event_id,
                            name: ev.name.clone(),
                            params,
                            span: ev.span,
                        },
                    );
                }
            }
        }
        let mut seen_events: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut events: Vec<SemanticEvent> = Vec::new();
        for t in &sm.transitions {
            for ev in &t.events {
                if seen_events.insert(ev.name.clone())
                    && let Some(event) = event_map.get(&ev.name)
                {
                    events.push(event.clone());
                }
            }
        }

        // Lower transitions
        let mut transitions = Vec::new();
        let mut transition_idx: usize = 0;

        // Finding 7: Collect concrete (state, event) pairs so wildcard expansion
        // skips states that already have a concrete transition for the same event.
        let mut concrete_pairs: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        for t in &sm.transitions {
            if t.src_state != "*" {
                for ev in &t.events {
                    concrete_pairs.insert((t.src_state.clone(), ev.name.clone()));
                }
            }
        }

        for t in &sm.transitions {
            let is_wildcard = t.src_state == "*";

            // S2: Terminal states cannot have explicit outgoing transitions
            if !is_wildcard && terminal_names.contains(&t.src_state) {
                return Err(SemaError::new(
                    ErrorKind::SmTerminalHasOutgoing,
                    format!(
                        "terminal state '{}' cannot have outgoing transitions in state machine '{}'",
                        t.src_state, sm.name
                    ),
                )
                .with_span(t.span)
                .with_hint(
                    "remove this transition or un-mark the state as [terminal]",
                ));
            }

            // Determine source states to expand
            let src_states: Vec<String> = if is_wildcard {
                // Expand to all non-terminal states
                state_names
                    .iter()
                    .filter(|sn| !terminal_names.contains(sn))
                    .cloned()
                    .collect()
            } else {
                if !state_names.contains(&t.src_state) {
                    return Err(SemaError::new(
                        ErrorKind::SmUndefinedState,
                        format!(
                            "undefined source state '{}' in state machine '{}'",
                            t.src_state, sm.name
                        ),
                    )
                    .with_span(t.span));
                }
                vec![t.src_state.clone()]
            };

            // Validate destination state
            if !state_names.contains(&t.dst_state) {
                return Err(SemaError::new(
                    ErrorKind::SmUndefinedState,
                    format!(
                        "undefined destination state '{}' in state machine '{}'",
                        t.dst_state, sm.name
                    ),
                )
                .with_span(t.span));
            }

            let dst_state_id = format!("{}/state:{}", sm_id, t.dst_state);

            // Task 2: delegate is only allowed on self-transitions
            if t.delegate.is_some() {
                for src_name in &src_states {
                    if src_name != &t.dst_state {
                        return Err(SemaError::new(
                            ErrorKind::SmDelegateNotSelfTransition,
                            format!(
                                "delegate is only allowed in self-transitions ({}->{})",
                                src_name, t.dst_state
                            ),
                        )
                        .with_span(t.span));
                    }
                }
            }

            // Task 3: delegate and action are mutually exclusive
            if t.delegate.is_some() && !t.actions.is_empty() {
                return Err(SemaError::new(
                    ErrorKind::SmDelegateWithAction,
                    "delegate and action are mutually exclusive in a transition".to_string(),
                )
                .with_span(t.span));
            }

            // Collect event parameter names for this transition so bare
            // NameRefs can be resolved to EventParam peer references.
            let event_param_names: Vec<String> = t
                .events
                .iter()
                .flat_map(|ev| ev.params.iter().map(|p| p.name.clone()))
                .collect();

            // Guard — resolve event params and child SM names for InState/All
            let guard = t.guard.as_ref().map(|g| {
                let mut lowered = self.lower_expr(g, &[], &[]);
                resolve_event_params(&mut lowered, &event_param_names);
                resolve_guard_sm_names(&mut lowered, &states);
                lowered
            });

            // Actions — resolve event params in action expressions
            let actions: Vec<SemanticAction> = t
                .actions
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let action_id =
                        format!("{}/transition[{}].action[{}]", sm_id, transition_idx, i);
                    let mut target = self.lower_expr(&a.target, &[], &[]);
                    resolve_event_params(&mut target, &event_param_names);
                    let mut value = self.lower_expr(&a.value, &[], &[]);
                    resolve_event_params(&mut value, &event_param_names);
                    SemanticAction {
                        action_id,
                        target,
                        op: a.op.clone(),
                        value,
                        span: a.span,
                    }
                })
                .collect();

            // Delegate
            let delegate = t.delegate.as_ref().map(|d| {
                let event_id = event_map
                    .get(&d.event_name)
                    .map(|e| e.event_id.clone())
                    .unwrap_or_default();
                let mut target = self.lower_expr(&d.target, &[], &[]);
                resolve_event_params(&mut target, &event_param_names);
                SemanticDelegate {
                    target,
                    event_id,
                    event_name: d.event_name.clone(),
                    span: d.span,
                }
            });

            // Normalize: one SemanticTransition per (src_state, event)
            for src_name in &src_states {
                let src_state_id = format!("{}/state:{}", sm_id, src_name);
                for ev in &t.events {
                    // Finding 7: skip wildcard expansion for (state, event) pairs
                    // that already have a concrete transition (concrete overrides wildcard).
                    if is_wildcard && concrete_pairs.contains(&(src_name.clone(), ev.name.clone()))
                    {
                        continue;
                    }
                    let event_id = event_map
                        .get(&ev.name)
                        .map(|e| e.event_id.clone())
                        .unwrap_or_default();
                    let tid = format!("{}/transition[{}]", sm_id, transition_idx);
                    transitions.push(SemanticTransition {
                        transition_id: tid,
                        src_state_id: src_state_id.clone(),
                        src_state_name: src_name.clone(),
                        dst_state_id: dst_state_id.clone(),
                        dst_state_name: t.dst_state.clone(),
                        event_id,
                        event_name: ev.name.clone(),
                        guard: guard.clone(),
                        actions: actions.clone(),
                        delegate: delegate.clone(),
                        span: t.span,
                    });
                    transition_idx += 1;
                }
            }
        }

        // Task 1: detect duplicate (src_state, event) pairs
        {
            use std::collections::HashSet;
            let mut seen_transitions: HashSet<(String, String)> = HashSet::new();
            for t in &transitions {
                let key = (t.src_state_name.clone(), t.event_name.clone());
                if !seen_transitions.insert(key) {
                    return Err(SemaError::new(
                        ErrorKind::SmDuplicateTransition,
                        format!(
                            "duplicate transition: state '{}' + event '{}'",
                            t.src_state_name, t.event_name
                        ),
                    ));
                }
            }
        }

        // SM exhaustiveness: every non-terminal state must have outgoing transitions
        {
            use std::collections::HashSet;
            let mut states_with_transitions: HashSet<String> = HashSet::new();
            let mut has_wildcard = false;
            for t in &sm.transitions {
                if t.src_state == "*" {
                    has_wildcard = true;
                } else {
                    states_with_transitions.insert(t.src_state.clone());
                }
            }

            if !has_wildcard {
                for state in &states {
                    if !state.is_terminal && !states_with_transitions.contains(&state.name) {
                        return Err(SemaError::new(
                            ErrorKind::SmUnhandledEvent,
                            format!(
                                "non-terminal state '{}' has no outgoing transitions",
                                state.name
                            ),
                        ));
                    }
                }
            }
        }

        // SmMissingAssignment: all dst fields without default values must be
        // assigned in the action block (spec §3.9 rule 2a).
        // Delegate transitions auto-copy src to dst (rule 2b), so skip them.
        {
            use std::collections::HashSet;
            for t in &transitions {
                // Delegate transitions auto-initialize dst from src
                if t.delegate.is_some() {
                    continue;
                }

                // Find the destination state
                let dst_state = match states.iter().find(|s| s.name == t.dst_state_name) {
                    Some(s) => s,
                    None => continue, // validated elsewhere
                };

                // Collect field names assigned in action block
                let mut assigned_fields: HashSet<&str> = HashSet::new();
                for action in &t.actions {
                    if let SemanticExpr::TransitionPeerRef { reference } = &action.target
                        && reference.peer == TransitionPeerKind::Dst
                        && let Some(field_name) = reference.path.first()
                    {
                        assigned_fields.insert(field_name.as_str());
                    }
                }

                // Check each dst field without a default value is assigned
                for field in &dst_state.fields {
                    if field.default_value.is_none()
                        && !assigned_fields.contains(field.name.as_str())
                    {
                        return Err(SemaError::new(
                            ErrorKind::SmMissingAssignment,
                            format!(
                                "transition {} -> {} on '{}': destination field '{}' has no default value and is not assigned in action block",
                                t.src_state_name,
                                t.dst_state_name,
                                t.event_name,
                                field.name,
                            ),
                        )
                        .with_span(t.span));
                    }
                }
            }
        }

        self.first_error()?;

        Ok(SemanticStateMachine {
            sm_id,
            name: sm.name.clone(),
            derive_traits: extract_derive_traits(&sm.annotations),
            states,
            events,
            initial_state_id,
            transitions,
            has_child_state_changed: event_map.contains_key("child_state_changed"),
            span: sm.span,
        })
    }

    /// Resolve a type expression used in state fields (simplified: only Named types).
    /// Unlike `resolve_named_type`, this allows `DeclKind::StateMachine` references
    /// (child SM fields in parent SM states).
    fn resolve_state_field_type(&mut self, texpr: &AstTypeExpr) -> SemaResult<SemanticType> {
        match texpr {
            AstTypeExpr::Named { name, span } => {
                // Try resolve_named_type first; if it fails because the type is a
                // state machine, handle that case here by returning a PacketRef
                // (opaque reference).
                match self.resolve_named_type(name, *span) {
                    Ok(ty) => Ok(ty),
                    Err(e) if e.kind == ErrorKind::TypeMismatch => {
                        // Check if this is a state machine type
                        if let Some(ResolvedType::UserDefined(
                            resolved_name,
                            DeclKind::StateMachine,
                        )) = self.registry.resolve_type_name(name)
                        {
                            // State machine types are valid in state field context
                            Ok(SemanticType::PacketRef {
                                packet_id: format!("sm:{}", resolved_name),
                                name: resolved_name,
                            })
                        } else {
                            Err(e)
                        }
                    }
                    Err(e) => Err(e),
                }
            }
            AstTypeExpr::Bits { width, .. } => Ok(SemanticType::Bits { width_bits: *width }),
            AstTypeExpr::Array {
                element_type,
                count,
                within_expr,
                ..
            } => {
                // Resolve element type through resolve_state_field_type
                // so that state machine types (e.g., [ChildSM; 4]) are
                // correctly resolved as PacketRef instead of falling back
                // to U8.
                let elem_ty = self.resolve_state_field_type(element_type)?;
                let count_expr = match count {
                    AstArrayCount::Expr(e) => Some(Box::new(self.lower_expr(e, &[], &[]))),
                    AstArrayCount::Fill => None,
                };
                let sem_within = within_expr
                    .as_ref()
                    .map(|e| Box::new(self.lower_expr(e, &[], &[])));
                Ok(SemanticType::Array {
                    element_type: Box::new(elem_ty),
                    count_expr,
                    within_expr: sem_within,
                })
            }
            _ => {
                let (ty, _) = self.resolve_type_expr(texpr)?;
                Ok(ty)
            }
        }
    }
}

/// Convert bare NameRef/ValueRef nodes that match event parameter names into
/// `TransitionPeerRef { peer: EventParam }` so the C backend emits
/// `event->{event_snake}.{param}` instead of `sm->{state}.{param}`.
fn resolve_event_params(expr: &mut SemanticExpr, param_names: &[String]) {
    match expr {
        SemanticExpr::ValueRef { reference }
            if reference.kind == ValueRefKind::Field
                && param_names.contains(&reference.value_id) =>
        {
            let name = reference.value_id.clone();
            *expr = SemanticExpr::TransitionPeerRef {
                reference: TransitionPeerRef {
                    peer: TransitionPeerKind::EventParam,
                    event_param_id: None,
                    path: vec![name],
                },
            };
        }
        SemanticExpr::Binary { left, right, .. } => {
            resolve_event_params(left, param_names);
            resolve_event_params(right, param_names);
        }
        SemanticExpr::Unary { operand, .. } => {
            resolve_event_params(operand, param_names);
        }
        SemanticExpr::Subscript { base, index } => {
            resolve_event_params(base, param_names);
            resolve_event_params(index, param_names);
        }
        SemanticExpr::StateConstructor { args, .. } => {
            for arg in args {
                resolve_event_params(arg, param_names);
            }
        }
        SemanticExpr::Fill { value, count } => {
            resolve_event_params(value, param_names);
            resolve_event_params(count, param_names);
        }
        SemanticExpr::InState { expr: inner, .. } => {
            resolve_event_params(inner, param_names);
        }
        SemanticExpr::All { collection, .. } => {
            resolve_event_params(collection, param_names);
        }
        SemanticExpr::Slice { base, start, end } => {
            resolve_event_params(base, param_names);
            resolve_event_params(start, param_names);
            resolve_event_params(end, param_names);
        }
        SemanticExpr::Coalesce { expr: e, default } => {
            resolve_event_params(e, param_names);
            resolve_event_params(default, param_names);
        }
        _ => {}
    }
}

/// Resolve empty `sm_name`/`sm_id` in InState/All expressions by looking up
/// the referenced field's child SM type from the source states.
fn resolve_guard_sm_names(expr: &mut SemanticExpr, states: &[SemanticState]) {
    match expr {
        SemanticExpr::InState {
            expr: inner,
            sm_id,
            sm_name,
            ..
        } if sm_name.is_empty() => {
            // Try to resolve from the inner expression's field reference
            if let Some(child_sm) = extract_child_sm_name(inner, states) {
                *sm_name = child_sm.clone();
                *sm_id = format!("sm:{child_sm}");
            }
            resolve_guard_sm_names(inner, states);
        }
        SemanticExpr::All {
            collection,
            sm_id,
            sm_name,
            ..
        } if sm_name.is_empty() => {
            // For All, the collection is typically a Slice whose base is a field ref
            let field_expr = match collection.as_ref() {
                SemanticExpr::Slice { base, .. } => Some(base.as_ref()),
                _ => None,
            };
            if let Some(fe) = field_expr
                && let Some(child_sm) = extract_child_sm_name(fe, states)
            {
                *sm_name = child_sm.clone();
                *sm_id = format!("sm:{child_sm}");
            }
            resolve_guard_sm_names(collection, states);
        }
        SemanticExpr::Binary { left, right, .. } => {
            resolve_guard_sm_names(left, states);
            resolve_guard_sm_names(right, states);
        }
        SemanticExpr::Unary { operand, .. } => {
            resolve_guard_sm_names(operand, states);
        }
        _ => {}
    }
}

/// Extract the child SM name from a field reference expression by looking
/// up the field in the state definitions.
fn extract_child_sm_name(expr: &SemanticExpr, states: &[SemanticState]) -> Option<String> {
    if let SemanticExpr::TransitionPeerRef { reference } = expr
        && let Some(field_name) = reference.path.first()
    {
        // Search all states for this field
        for state in states {
            for field in &state.fields {
                if &field.name == field_name {
                    return field.child_sm_name.clone();
                }
            }
        }
    }
    None
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
