// crates/wirespec-sema/src/analyzer/register.rs
//! Pass 1: type registration.

use super::*;

impl Analyzer {
    pub(super) fn check_reserved(name: &str) -> SemaResult<()> {
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

    pub(super) fn try_register(&mut self, name: &str, kind: DeclKind) -> SemaResult<()> {
        Self::check_reserved(name)?;
        self.registry
            .register(name, kind)
            .map_err(|msg| SemaError::new(ErrorKind::DuplicateDefinition, msg))
    }

    pub(super) fn register_all(&mut self, ast: &AstModule) -> SemaResult<()> {
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
                        self.registry
                            .register_alias(&td.name, &target_name)
                            .map_err(|msg| SemaError::new(ErrorKind::DuplicateDefinition, msg))?;
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
}
