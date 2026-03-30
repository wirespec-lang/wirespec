//! Recursive descent parser for wirespec.
//!
//! Consumes a token stream from the lexer and produces an `AstModule`.
//! Grammar follows wirespec_spec_v1.0 §6.1 (core) and §6.2 (state machines).

use crate::ast::*;
use crate::lexer::{Token, TokenKind};
use crate::span::Span;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub msg: String,
    pub span: Option<Span>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(span) = &self.span {
            write!(f, "parse error at offset {}: {}", span.offset, self.msg)
        } else {
            write!(f, "parse error: {}", self.msg)
        }
    }
}

impl std::error::Error for ParseError {}

type Result<T> = std::result::Result<T, ParseError>;

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    // ── Helpers ──

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(kind)
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if !self.at_eof() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<&Token> {
        if self.at(kind) {
            Ok(self.advance())
        } else {
            Err(self.error(format!("expected {kind:?}, found {:?}", self.peek())))
        }
    }

    fn expect_name(&mut self) -> Result<(String, Span)> {
        let span = self.span();
        if let Some(name) = self.token_as_name() {
            self.advance();
            Ok((name, span))
        } else {
            Err(self.error(format!("expected identifier, found {:?}", self.peek())))
        }
    }

    /// Consume a name token, but also allow certain keywords that can appear
    /// as identifiers in specific contexts (e.g. field names).
    fn expect_name_or_keyword(&mut self) -> Result<(String, Span)> {
        // Same as expect_name — all keywords are accepted as identifiers
        self.expect_name()
    }

    /// Try to interpret the current token as a name/identifier.
    /// Keywords are accepted because wirespec allows keywords as field names,
    /// module path components, etc. (e.g., `quic.varint`, `flags: u8`).
    fn token_as_name(&self) -> Option<String> {
        match self.peek() {
            TokenKind::Name(n) => Some(n.clone()),
            // All keywords that can appear as identifiers in various contexts
            TokenKind::Module => Some("module".into()),
            TokenKind::Import => Some("import".into()),
            TokenKind::Const => Some("const".into()),
            TokenKind::Enum => Some("enum".into()),
            TokenKind::Flags => Some("flags".into()),
            TokenKind::Type => Some("type".into()),
            TokenKind::Packet => Some("packet".into()),
            TokenKind::Frame => Some("frame".into()),
            TokenKind::Capsule => Some("capsule".into()),
            TokenKind::State => Some("state".into()),
            TokenKind::Machine => Some("machine".into()),
            TokenKind::Transition => Some("transition".into()),
            TokenKind::Initial => Some("initial".into()),
            TokenKind::Terminal => Some("terminal".into()),
            TokenKind::On => Some("on".into()),
            TokenKind::Guard => Some("guard".into()),
            TokenKind::Action => Some("action".into()),
            TokenKind::Delegate => Some("delegate".into()),
            TokenKind::Match => Some("match".into()),
            TokenKind::If => Some("if".into()),
            TokenKind::Let => Some("let".into()),
            TokenKind::Require => Some("require".into()),
            TokenKind::StaticAssert => Some("static_assert".into()),
            TokenKind::Within => Some("within".into()),
            TokenKind::Export => Some("export".into()),
            TokenKind::Varint => Some("varint".into()),
            TokenKind::Bytes => Some("bytes".into()),
            TokenKind::Bits => Some("bits".into()),
            TokenKind::Bit => Some("bit".into()),
            TokenKind::Fill => Some("fill".into()),
            TokenKind::Remaining => Some("remaining".into()),
            TokenKind::And => Some("and".into()),
            TokenKind::Or => Some("or".into()),
            TokenKind::Not => Some("not".into()),
            TokenKind::InState => Some("in_state".into()),
            TokenKind::All => Some("all".into()),
            _ => None,
        }
    }

    fn error(&self, msg: String) -> ParseError {
        ParseError {
            msg,
            span: Some(self.span()),
        }
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    // ── Top-level parse ──

    pub fn parse_module(mut self) -> Result<AstModule> {
        let start = self.span();
        let mut module_decl = None;
        let mut imports = Vec::new();
        let mut annotations = Vec::new();
        let mut items = Vec::new();

        // Collect annotations that appear before module decl (file-level)
        let mut pre_module_anns = self.collect_annotations()?;

        // module decl (may appear after file-level annotations like @endian)
        if self.at(&TokenKind::Module) {
            module_decl = Some(self.parse_module_decl()?);
            // Annotations before module decl are file-level
            annotations.append(&mut pre_module_anns);
        }

        // Collect annotations between module decl and first item (also file-level)
        let mut post_module_anns = self.collect_annotations()?;
        if module_decl.is_some() {
            annotations.append(&mut post_module_anns);
        }

        // If no module decl, merge all pre-annotations forward
        let mut pending = if module_decl.is_none() {
            pre_module_anns
        } else {
            Vec::new()
        };
        pending.append(&mut post_module_anns);

        loop {
            // Collect annotations
            let mut item_annotations = self.collect_annotations()?;

            // Prepend any leftover pending annotations
            if !pending.is_empty() {
                let mut combined = std::mem::take(&mut pending);
                combined.append(&mut item_annotations);
                item_annotations = combined;
            }

            if self.at_eof() {
                // Trailing annotations with no item — attach to module
                annotations.append(&mut item_annotations);
                break;
            }

            if self.at(&TokenKind::Import) {
                if !item_annotations.is_empty() {
                    // Annotations before import go to module level
                    annotations.append(&mut item_annotations);
                }
                imports.push(self.parse_import()?);
                continue;
            }

            if let Some(item) = self.try_parse_top_item(item_annotations)? {
                items.push(item);
            } else if self.at_eof() {
                break;
            } else {
                return Err(self.error(format!("unexpected token: {:?}", self.peek())));
            }
        }

        Ok(AstModule {
            module_decl,
            imports,
            annotations,
            items,
            span: Some(start),
        })
    }

    // ── Module / Import ──

    fn parse_module_decl(&mut self) -> Result<AstModuleDecl> {
        let start = self.span();
        self.expect(&TokenKind::Module)?;
        let name = self.parse_dotted_name()?;
        Ok(AstModuleDecl {
            name,
            span: Some(start),
        })
    }

    fn parse_import(&mut self) -> Result<AstImport> {
        let start = self.span();
        self.expect(&TokenKind::Import)?;
        let mut parts = vec![];
        let (first, _) = self.expect_name()?;
        parts.push(first);
        while self.eat(&TokenKind::Dot) {
            let (part, _) = self.expect_name()?;
            parts.push(part);
        }
        // If the last part starts with uppercase, it's a specific item import
        let name = if parts.len() >= 2
            && parts
                .last()
                .expect("parts has at least 2 elements")
                .chars()
                .next()
                .is_some_and(|c| c.is_uppercase())
        {
            let item = parts.pop().expect("parts has at least 2 elements");
            Some(item)
        } else {
            None
        };
        let module = parts.join(".");
        Ok(AstImport {
            module,
            name,
            span: Some(start),
        })
    }

    fn parse_dotted_name(&mut self) -> Result<String> {
        let (first, _) = self.expect_name()?;
        let mut name = first;
        while self.eat(&TokenKind::Dot) {
            let (part, _) = self.expect_name()?;
            name.push('.');
            name.push_str(&part);
        }
        Ok(name)
    }

    // ── Annotations ──

    fn collect_annotations(&mut self) -> Result<Vec<AstAnnotation>> {
        let mut anns = Vec::new();
        while self.at(&TokenKind::At) {
            anns.push(self.parse_annotation()?);
        }
        Ok(anns)
    }

    fn parse_annotation(&mut self) -> Result<AstAnnotation> {
        let start = self.span();
        self.expect(&TokenKind::At)?;
        let (name, _) = self.expect_name()?;
        let mut args = Vec::new();

        if self.eat(&TokenKind::LParen) {
            // @name(arg, arg, ...)
            if !self.at(&TokenKind::RParen) {
                args.push(self.parse_annotation_arg()?);
                while self.eat(&TokenKind::Comma) {
                    if self.at(&TokenKind::RParen) {
                        break;
                    }
                    args.push(self.parse_annotation_arg()?);
                }
            }
            self.expect(&TokenKind::RParen)?;
        } else {
            // @name value  (e.g. @endian big, @doc "text")
            match self.peek().clone() {
                TokenKind::Name(val) => {
                    self.advance();
                    args.push(AstAnnotationArg::Identifier(val));
                }
                TokenKind::StringLit(val) => {
                    self.advance();
                    args.push(AstAnnotationArg::String(val));
                }
                TokenKind::Integer(val) => {
                    self.advance();
                    args.push(AstAnnotationArg::Int(val));
                }
                TokenKind::True => {
                    self.advance();
                    args.push(AstAnnotationArg::Bool(true));
                }
                TokenKind::False => {
                    self.advance();
                    args.push(AstAnnotationArg::Bool(false));
                }
                // Some keywords can appear as annotation values (e.g. @endian big)
                // big/little aren't keywords, they're names. So this is fine.
                _ => {
                    // No arg — bare annotation like @strict
                }
            }
        }

        Ok(AstAnnotation {
            name,
            args,
            span: Some(start),
        })
    }

    fn parse_annotation_arg(&mut self) -> Result<AstAnnotationArg> {
        match self.peek().clone() {
            TokenKind::Integer(v) => {
                self.advance();
                Ok(AstAnnotationArg::Int(v))
            }
            TokenKind::True => {
                self.advance();
                Ok(AstAnnotationArg::Bool(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(AstAnnotationArg::Bool(false))
            }
            TokenKind::StringLit(v) => {
                self.advance();
                Ok(AstAnnotationArg::String(v))
            }
            TokenKind::Name(n) => {
                // Lookahead for NAME "=" literal
                if matches!(self.tokens[self.pos + 1].kind, TokenKind::Assign) {
                    let name = n;
                    self.advance(); // skip name
                    self.advance(); // skip =
                    let value = self.parse_literal_value()?;
                    return Ok(AstAnnotationArg::NamedValue { name, value });
                }
                self.advance();
                Ok(AstAnnotationArg::Identifier(n))
            }
            _ => Err(self.error(format!(
                "expected annotation argument, found {:?}",
                self.peek()
            ))),
        }
    }

    // ── Top Items ──

    fn try_parse_top_item(
        &mut self,
        annotations: Vec<AstAnnotation>,
    ) -> Result<Option<AstTopItem>> {
        let exported = self.eat(&TokenKind::Export);
        match self.peek() {
            TokenKind::Const => Ok(Some(AstTopItem::Const(
                self.parse_const(annotations, exported)?,
            ))),
            TokenKind::Enum => Ok(Some(AstTopItem::Enum(
                self.parse_enum(annotations, exported)?,
            ))),
            TokenKind::Flags => Ok(Some(AstTopItem::Flags(
                self.parse_flags(annotations, exported)?,
            ))),
            TokenKind::StaticAssert => {
                if exported {
                    return Err(self.error("'export' is not allowed on static_assert".into()));
                }
                if !annotations.is_empty() {
                    return Err(self.error("annotations are not allowed on static_assert".into()));
                }
                Ok(Some(AstTopItem::StaticAssert(self.parse_static_assert()?)))
            }
            TokenKind::Type => {
                let item = self.parse_type_def(annotations, exported)?;
                Ok(Some(item))
            }
            TokenKind::Packet => Ok(Some(AstTopItem::Packet(
                self.parse_packet(annotations, exported)?,
            ))),
            TokenKind::Frame => Ok(Some(AstTopItem::Frame(
                self.parse_frame(annotations, exported)?,
            ))),
            TokenKind::Capsule => Ok(Some(AstTopItem::Capsule(
                self.parse_capsule(annotations, exported)?,
            ))),
            TokenKind::State => Ok(Some(AstTopItem::StateMachine(
                self.parse_state_machine(annotations, exported)?,
            ))),
            _ => {
                // Check for "extern asn1"
                if let TokenKind::Name(ref n) = self.peek().clone()
                    && n == "extern"
                {
                    let item = self.parse_extern_asn1(annotations)?;
                    return Ok(Some(item));
                }

                if exported {
                    Err(self.error("expected definition after 'export'".into()))
                } else if !annotations.is_empty() {
                    Err(self.error("dangling annotations".into()))
                } else {
                    Ok(None)
                }
            }
        }
    }

    // ── Const ──

    fn parse_const(
        &mut self,
        annotations: Vec<AstAnnotation>,
        exported: bool,
    ) -> Result<AstConstDecl> {
        let start = self.span();
        self.expect(&TokenKind::Const)?;
        let (name, _) = self.expect_name()?;
        self.expect(&TokenKind::Colon)?;
        let type_name = self.parse_type_ref_name()?;
        self.expect(&TokenKind::Assign)?;
        let value = self.parse_literal_value()?;
        Ok(AstConstDecl {
            name,
            type_name,
            value,
            annotations,
            exported,
            span: Some(start),
        })
    }

    // ── Enum / Flags ──

    fn parse_enum(
        &mut self,
        annotations: Vec<AstAnnotation>,
        exported: bool,
    ) -> Result<AstEnumDecl> {
        let start = self.span();
        self.expect(&TokenKind::Enum)?;
        let (name, _) = self.expect_name()?;
        self.expect(&TokenKind::Colon)?;
        let underlying_type = self.parse_type_ref_name()?;
        self.expect(&TokenKind::LBrace)?;
        let mut members = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let mstart = self.span();
            let (mname, _) = self.expect_name()?;
            self.expect(&TokenKind::Assign)?;
            let mval = self.parse_integer()?;
            members.push(AstEnumMember {
                name: mname,
                value: mval,
                span: Some(mstart),
            });
            self.eat(&TokenKind::Comma);
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(AstEnumDecl {
            name,
            underlying_type,
            members,
            annotations,
            exported,
            span: Some(start),
        })
    }

    fn parse_flags(
        &mut self,
        annotations: Vec<AstAnnotation>,
        exported: bool,
    ) -> Result<AstFlagsDecl> {
        let start = self.span();
        self.expect(&TokenKind::Flags)?;
        let (name, _) = self.expect_name()?;
        self.expect(&TokenKind::Colon)?;
        let underlying_type = self.parse_type_ref_name()?;
        self.expect(&TokenKind::LBrace)?;
        let mut members = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let mstart = self.span();
            let (mname, _) = self.expect_name()?;
            self.expect(&TokenKind::Assign)?;
            let mval = self.parse_integer()?;
            members.push(AstEnumMember {
                name: mname,
                value: mval,
                span: Some(mstart),
            });
            self.eat(&TokenKind::Comma);
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(AstFlagsDecl {
            name,
            underlying_type,
            members,
            annotations,
            exported,
            span: Some(start),
        })
    }

    // ── StaticAssert ──

    fn parse_static_assert(&mut self) -> Result<AstStaticAssertDecl> {
        let start = self.span();
        self.expect(&TokenKind::StaticAssert)?;
        let expr = self.parse_expr()?;
        Ok(AstStaticAssertDecl {
            expr,
            span: Some(start),
        })
    }

    // ── Type ──

    fn parse_type_def(
        &mut self,
        annotations: Vec<AstAnnotation>,
        exported: bool,
    ) -> Result<AstTopItem> {
        let start = self.span();
        self.expect(&TokenKind::Type)?;
        let (name, _) = self.expect_name()?;
        self.expect(&TokenKind::Assign)?;

        // Check for varint block
        if self.at(&TokenKind::Varint) {
            return self.parse_continuation_varint(name, annotations, exported, start);
        }

        // Check for struct body (type Name = { fields })
        if self.at(&TokenKind::LBrace) {
            self.expect(&TokenKind::LBrace)?;
            let fields = self.parse_struct_field_list()?;
            self.expect(&TokenKind::RBrace)?;
            return Ok(AstTopItem::Type(AstTypeDecl {
                name,
                annotations,
                body: AstTypeDeclBody::Fields { fields },
                exported,
                span: Some(start),
            }));
        }

        // Type alias
        let target = self.parse_type_expr()?;
        Ok(AstTopItem::Type(AstTypeDecl {
            name,
            annotations,
            body: AstTypeDeclBody::Alias { target },
            exported,
            span: Some(start),
        }))
    }

    fn parse_continuation_varint(
        &mut self,
        name: String,
        annotations: Vec<AstAnnotation>,
        exported: bool,
        start: Span,
    ) -> Result<AstTopItem> {
        self.expect(&TokenKind::Varint)?;
        self.expect(&TokenKind::LBrace)?;

        let mut continuation_bit = "msb".to_string();
        let mut value_bits: u8 = 7;
        let mut max_bytes: u8 = 4;
        let mut byte_order = "little".to_string();

        while !self.at(&TokenKind::RBrace) {
            let (pname, _) = self.expect_name()?;
            self.expect(&TokenKind::Colon)?;
            match pname.as_str() {
                "continuation_bit" => {
                    let (val, _) = self.expect_name()?;
                    continuation_bit = val;
                }
                "value_bits" => {
                    value_bits = self.parse_integer()? as u8;
                }
                "max_bytes" => {
                    max_bytes = self.parse_integer()? as u8;
                }
                "byte_order" => {
                    let (val, _) = self.expect_name()?;
                    byte_order = val;
                }
                _ => return Err(self.error(format!("unknown varint parameter: {pname}"))),
            }
            self.eat(&TokenKind::Comma);
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(AstTopItem::ContinuationVarInt(AstContinuationVarIntDecl {
            name,
            annotations,
            continuation_bit,
            value_bits,
            max_bytes,
            byte_order,
            exported,
            span: Some(start),
        }))
    }

    // ── Packet ──

    fn parse_packet(
        &mut self,
        annotations: Vec<AstAnnotation>,
        exported: bool,
    ) -> Result<AstPacketDecl> {
        let start = self.span();
        self.expect(&TokenKind::Packet)?;
        let (name, _) = self.expect_name()?;
        self.expect(&TokenKind::LBrace)?;
        let fields = self.parse_field_list()?;
        self.expect(&TokenKind::RBrace)?;
        Ok(AstPacketDecl {
            name,
            fields,
            annotations,
            exported,
            span: Some(start),
        })
    }

    // ── Frame ──

    fn parse_frame(
        &mut self,
        annotations: Vec<AstAnnotation>,
        exported: bool,
    ) -> Result<AstFrameDecl> {
        let start = self.span();
        self.expect(&TokenKind::Frame)?;
        let (name, _) = self.expect_name()?;
        self.expect(&TokenKind::Assign)?;
        self.expect(&TokenKind::Match)?;
        let (tag_field, _) = self.expect_name_or_keyword()?;
        self.expect(&TokenKind::Colon)?;
        let tag_type = self.parse_type_ref_name()?;
        self.expect(&TokenKind::LBrace)?;
        let branches = self.parse_frame_branches()?;
        self.expect(&TokenKind::RBrace)?;
        Ok(AstFrameDecl {
            name,
            tag_field,
            tag_type,
            branches,
            annotations,
            exported,
            span: Some(start),
        })
    }

    fn parse_frame_branches(&mut self) -> Result<Vec<AstFrameBranch>> {
        let mut branches = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            branches.push(self.parse_frame_branch()?);
            self.eat(&TokenKind::Comma);
        }
        Ok(branches)
    }

    fn parse_frame_branch(&mut self) -> Result<AstFrameBranch> {
        let start = self.span();
        let pattern = self.parse_pattern()?;
        self.expect(&TokenKind::FatArrow)?;
        let (variant_name, _) = self.expect_name()?;
        self.expect(&TokenKind::LBrace)?;
        let fields = if self.at(&TokenKind::RBrace) {
            Vec::new()
        } else {
            self.parse_field_list()?
        };
        self.expect(&TokenKind::RBrace)?;
        Ok(AstFrameBranch {
            pattern,
            variant_name,
            fields,
            span: Some(start),
        })
    }

    // ── Capsule ──

    fn parse_capsule(
        &mut self,
        annotations: Vec<AstAnnotation>,
        exported: bool,
    ) -> Result<AstCapsuleDecl> {
        let start = self.span();
        self.expect(&TokenKind::Capsule)?;
        let (name, _) = self.expect_name()?;
        self.expect(&TokenKind::LBrace)?;

        // Parse header fields until we hit "payload:"
        let mut fields = Vec::new();
        loop {
            // Check for the payload field pattern: NAME ":" "match" ...
            if let TokenKind::Name(fname) = self.peek().clone()
                && fname == "payload"
            {
                // Peek ahead: payload : match
                let saved = self.pos;
                self.advance(); // skip "payload"
                if self.at(&TokenKind::Colon) {
                    self.advance(); // skip ":"
                    if self.at(&TokenKind::Match) {
                        // This is the payload match — parse it
                        break;
                    }
                }
                // Not a payload match, restore
                self.pos = saved;
            }
            fields.push(self.parse_field_item()?);
            self.eat(&TokenKind::Comma);
        }

        // Already consumed "payload" ":" and positioned at "match"
        self.expect(&TokenKind::Match)?;

        // capsule_tag = NAME | "(" expr ")"
        let payload_tag = if self.eat(&TokenKind::LParen) {
            let expr = self.parse_expr()?;
            self.expect(&TokenKind::RParen)?;
            AstPayloadTagSelector::Expr { expr }
        } else {
            let (tag_name, _) = self.expect_name_or_keyword()?;
            AstPayloadTagSelector::Field {
                field_name: tag_name,
            }
        };

        self.expect(&TokenKind::Within)?;
        let (payload_within, _) = self.expect_name_or_keyword()?;

        self.expect(&TokenKind::LBrace)?;
        let branches = self.parse_frame_branches()?;
        self.expect(&TokenKind::RBrace)?;

        // Close the capsule body
        self.eat(&TokenKind::Comma);
        self.expect(&TokenKind::RBrace)?;

        Ok(AstCapsuleDecl {
            name,
            fields,
            payload_tag,
            payload_within,
            branches,
            annotations,
            exported,
            span: Some(start),
        })
    }

    // ── State Machine ──

    fn parse_state_machine(
        &mut self,
        annotations: Vec<AstAnnotation>,
        exported: bool,
    ) -> Result<AstStateMachineDecl> {
        let start = self.span();
        self.expect(&TokenKind::State)?;
        self.expect(&TokenKind::Machine)?;
        let (name, _) = self.expect_name()?;
        self.expect(&TokenKind::LBrace)?;

        let mut states = Vec::new();
        let mut initial_state = String::new();
        let mut transitions = Vec::new();

        while !self.at(&TokenKind::RBrace) {
            match self.peek() {
                TokenKind::State => {
                    states.push(self.parse_state_decl()?);
                }
                TokenKind::Initial => {
                    self.advance();
                    let (iname, _) = self.expect_name()?;
                    initial_state = iname;
                }
                TokenKind::Transition => {
                    transitions.push(self.parse_transition()?);
                }
                TokenKind::Name(n) if n == "verify" => {
                    // Skip verify declarations for now (Phase 3)
                    self.skip_verify()?;
                }
                _ => {
                    return Err(
                        self.error(format!("unexpected in state machine: {:?}", self.peek()))
                    );
                }
            }
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(AstStateMachineDecl {
            name,
            states,
            initial_state,
            transitions,
            annotations,
            exported,
            span: Some(start),
        })
    }

    fn parse_state_decl(&mut self) -> Result<AstStateDecl> {
        let start = self.span();
        self.expect(&TokenKind::State)?;
        let (name, _) = self.expect_name()?;

        let mut fields = Vec::new();
        let mut is_terminal = false;

        // Optional fields block
        if self.eat(&TokenKind::LBrace) {
            while !self.at(&TokenKind::RBrace) {
                let fstart = self.span();
                let (fname, _) = self.expect_name()?;
                self.expect(&TokenKind::Colon)?;
                let type_expr = self.parse_type_expr()?;
                let default_value = if self.eat(&TokenKind::Assign) {
                    Some(self.parse_literal_value()?)
                } else {
                    None
                };
                fields.push(AstStateFieldDef {
                    name: fname,
                    type_expr,
                    default_value,
                    span: Some(fstart),
                });
                self.eat(&TokenKind::Comma);
            }
            self.expect(&TokenKind::RBrace)?;
        }

        // [terminal]
        if self.eat(&TokenKind::LBracket) {
            self.expect(&TokenKind::Terminal)?;
            self.expect(&TokenKind::RBracket)?;
            is_terminal = true;
        }

        Ok(AstStateDecl {
            name,
            fields,
            is_terminal,
            span: Some(start),
        })
    }

    fn parse_transition(&mut self) -> Result<AstTransitionDecl> {
        let start = self.span();
        self.expect(&TokenKind::Transition)?;

        // src state: NAME | "*"
        let src_state = if self.eat(&TokenKind::Star) {
            "*".to_string()
        } else {
            let (s, _) = self.expect_name()?;
            s
        };

        self.expect(&TokenKind::Arrow)?;
        let (dst_state, _) = self.expect_name()?;

        self.expect(&TokenKind::LBrace)?;

        let mut events = Vec::new();
        let mut guard = None;
        let mut actions = Vec::new();
        let mut delegate = None;
        let mut has_guard = false;
        let mut has_action = false;
        let mut has_delegate = false;

        while !self.at(&TokenKind::RBrace) {
            match self.peek() {
                TokenKind::On => {
                    events.push(self.parse_event_decl()?);
                }
                TokenKind::Guard => {
                    if has_guard {
                        return Err(self.error("duplicate 'guard' clause in transition".into()));
                    }
                    has_guard = true;
                    self.advance();
                    guard = Some(self.parse_expr()?);
                }
                TokenKind::Action => {
                    if has_action {
                        return Err(self.error("duplicate 'action' clause in transition".into()));
                    }
                    has_action = true;
                    self.advance();
                    self.expect(&TokenKind::LBrace)?;
                    while !self.at(&TokenKind::RBrace) {
                        actions.push(self.parse_assignment()?);
                        self.eat(&TokenKind::Semicolon);
                    }
                    self.expect(&TokenKind::RBrace)?;
                }
                TokenKind::Delegate => {
                    if has_delegate {
                        return Err(self.error("duplicate 'delegate' clause in transition".into()));
                    }
                    has_delegate = true;
                    delegate = Some(self.parse_delegate_clause()?);
                }
                _ => {
                    return Err(self.error(format!("unexpected in transition: {:?}", self.peek())));
                }
            }
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(AstTransitionDecl {
            src_state,
            dst_state,
            events,
            guard,
            actions,
            delegate,
            span: Some(start),
        })
    }

    fn parse_event_decl(&mut self) -> Result<AstEventDecl> {
        let start = self.span();
        self.expect(&TokenKind::On)?;
        let (name, _) = self.expect_name_or_keyword()?;
        let mut params = Vec::new();
        if self.eat(&TokenKind::LParen) {
            if !self.at(&TokenKind::RParen) {
                loop {
                    let pstart = self.span();
                    let (pname, _) = self.expect_name()?;
                    self.expect(&TokenKind::Colon)?;
                    let type_expr = self.parse_type_expr()?;
                    params.push(AstEventParam {
                        name: pname,
                        type_expr,
                        span: Some(pstart),
                    });
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen)?;
        }
        Ok(AstEventDecl {
            name,
            params,
            span: Some(start),
        })
    }

    fn parse_assignment(&mut self) -> Result<AstAssignment> {
        let start = self.span();
        let target = self.parse_postfix_expr()?;
        let op = if self.eat(&TokenKind::PlusAssign) {
            "+=".to_string()
        } else {
            self.expect(&TokenKind::Assign)?;
            "=".to_string()
        };
        let value = self.parse_expr()?;
        Ok(AstAssignment {
            target,
            op,
            value,
            span: Some(start),
        })
    }

    fn parse_delegate_clause(&mut self) -> Result<AstDelegateClause> {
        let start = self.span();
        self.expect(&TokenKind::Delegate)?;
        let target = self.parse_postfix_expr()?;
        self.expect(&TokenKind::LArrow)?;
        let (event_name, _) = self.expect_name()?;
        Ok(AstDelegateClause {
            target,
            event_name,
            span: Some(start),
        })
    }

    fn skip_verify(&mut self) -> Result<()> {
        // Skip "verify" ... until we hit a keyword that starts a new SM item
        self.advance(); // skip "verify"

        // Simple heuristic: skip tokens until we see state/initial/transition/verify/}
        // Handle "verify property Name:" with nested expressions
        if let TokenKind::Name(ref n) = self.peek().clone() {
            if n == "property" {
                self.advance(); // skip "property"
                self.expect_name()?; // skip name
                self.expect(&TokenKind::Colon)?;
            }
        } else {
            // verify Name (bare name reference)
            self.expect_name()?;
            return Ok(());
        }

        // Skip the verify expression body — count braces/parens
        let mut depth = 0i32;
        loop {
            match self.peek() {
                TokenKind::Eof => break,
                TokenKind::RBrace if depth == 0 => break,
                TokenKind::State | TokenKind::Initial | TokenKind::Transition if depth == 0 => {
                    break;
                }
                TokenKind::Name(n) if n == "verify" && depth == 0 => break,
                TokenKind::LParen | TokenKind::LBrace | TokenKind::LBracket => {
                    depth += 1;
                    self.advance();
                }
                TokenKind::RParen | TokenKind::RBrace | TokenKind::RBracket => {
                    depth -= 1;
                    self.advance();
                }
                _ => {
                    self.advance();
                }
            }
        }
        Ok(())
    }

    // ── Fields ──

    fn parse_field_list(&mut self) -> Result<Vec<AstFieldItem>> {
        let mut items = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            items.push(self.parse_field_item()?);
            self.eat(&TokenKind::Comma);
        }
        Ok(items)
    }

    fn parse_struct_field_list(&mut self) -> Result<Vec<AstFieldDef>> {
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let anns = self.collect_annotations()?;
            let start = self.span();
            let (name, _) = self.expect_name()?;
            self.expect(&TokenKind::Colon)?;
            let type_expr = self.parse_type_expr()?;
            fields.push(AstFieldDef {
                name,
                type_expr,
                annotations: anns,
                span: Some(start),
            });
            self.eat(&TokenKind::Comma);
        }
        Ok(fields)
    }

    fn parse_field_item(&mut self) -> Result<AstFieldItem> {
        match self.peek() {
            TokenKind::Let => {
                let start = self.span();
                self.advance();
                let (name, _) = self.expect_name()?;
                self.expect(&TokenKind::Colon)?;
                let type_name = self.parse_type_ref_name()?;
                self.expect(&TokenKind::Assign)?;
                let expr = self.parse_expr()?;
                Ok(AstFieldItem::Derived(AstDerivedField {
                    name,
                    type_name,
                    expr,
                    span: Some(start),
                }))
            }
            TokenKind::Require => {
                let start = self.span();
                self.advance();
                let expr = self.parse_expr()?;
                Ok(AstFieldItem::Require(AstRequireClause {
                    expr,
                    span: Some(start),
                }))
            }
            _ => {
                let anns = self.collect_annotations()?;
                let start = self.span();
                let (name, _) = self.expect_name_or_keyword()?;
                self.expect(&TokenKind::Colon)?;
                let type_expr = self.parse_type_expr()?;
                Ok(AstFieldItem::Field(AstFieldDef {
                    name,
                    type_expr,
                    annotations: anns,
                    span: Some(start),
                }))
            }
        }
    }

    // ── Type Expressions ──

    fn parse_type_expr(&mut self) -> Result<AstTypeExpr> {
        match self.peek() {
            TokenKind::Match => self.parse_match_type_expr(),
            TokenKind::If => self.parse_optional_type_expr(),
            TokenKind::LBracket => self.parse_array_type_expr(),
            TokenKind::Bytes => self.parse_bytes_type_expr(),
            TokenKind::Bits => self.parse_bits_type_expr(),
            TokenKind::Bit => {
                let span = self.span();
                self.advance();
                Ok(AstTypeExpr::Bits {
                    width: 1,
                    span: Some(span),
                })
            }
            _ => {
                let span = self.span();
                let name = self.parse_type_ref_name()?;
                // Check for asn1(...) function-call syntax
                if name == "asn1" && self.at(&TokenKind::LParen) {
                    return self.parse_asn1_type_expr(span);
                }
                Ok(AstTypeExpr::Named {
                    name,
                    span: Some(span),
                })
            }
        }
    }

    fn parse_asn1_type_expr(&mut self, _start: Span) -> Result<AstTypeExpr> {
        self.expect(&TokenKind::LParen)?;

        // Type name
        let (type_name, _) = self.expect_name()?;
        self.expect(&TokenKind::Comma)?;

        // encoding: <name>
        let (enc_kw, _) = self.expect_name()?;
        if enc_kw != "encoding" {
            return Err(self.error(format!("expected 'encoding', found '{enc_kw}'")));
        }
        self.expect(&TokenKind::Colon)?;
        let (encoding, _) = self.expect_name()?;
        self.expect(&TokenKind::Comma)?;

        // length: <expr> OR remaining
        let length = if self.eat(&TokenKind::Remaining) {
            Asn1Length::Remaining
        } else {
            let (len_kw, _) = self.expect_name()?;
            if len_kw != "length" {
                return Err(self.error(format!(
                    "expected 'length' or 'remaining', found '{len_kw}'"
                )));
            }
            self.expect(&TokenKind::Colon)?;
            let expr = self.parse_expr()?;
            Asn1Length::Expr(Box::new(expr))
        };

        self.expect(&TokenKind::RParen)?;

        Ok(AstTypeExpr::Asn1 {
            type_name,
            encoding,
            length,
        })
    }

    fn parse_match_type_expr(&mut self) -> Result<AstTypeExpr> {
        let start = self.span();
        self.expect(&TokenKind::Match)?;
        let (field_name, _) = self.expect_name()?;
        self.expect(&TokenKind::LBrace)?;
        let mut branches = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let bstart = self.span();
            let pattern = self.parse_pattern()?;
            self.expect(&TokenKind::FatArrow)?;
            let result_type = self.parse_type_expr()?;
            branches.push(AstMatchBranch {
                pattern,
                result_type,
                span: Some(bstart),
            });
            self.eat(&TokenKind::Comma);
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(AstTypeExpr::Match {
            field_name,
            branches,
            span: Some(start),
        })
    }

    fn parse_optional_type_expr(&mut self) -> Result<AstTypeExpr> {
        let start = self.span();
        self.expect(&TokenKind::If)?;
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::LBrace)?;
        let inner_type = self.parse_type_expr()?;
        self.expect(&TokenKind::RBrace)?;
        Ok(AstTypeExpr::Optional {
            condition,
            inner_type: Box::new(inner_type),
            span: Some(start),
        })
    }

    fn parse_array_type_expr(&mut self) -> Result<AstTypeExpr> {
        let start = self.span();
        self.expect(&TokenKind::LBracket)?;
        let element_type = self.parse_type_expr()?;
        self.expect(&TokenKind::Semicolon)?;

        let count = if self.eat(&TokenKind::Fill) {
            AstArrayCount::Fill
        } else {
            AstArrayCount::Expr(self.parse_expr()?)
        };

        // Optional: "] within EXPR" for fill-within arrays (M15)
        self.expect(&TokenKind::RBracket)?;

        let within_expr = if self.at(&TokenKind::Within) {
            // Only valid with fill arrays: [T; fill] within expr
            self.advance();
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };

        Ok(AstTypeExpr::Array {
            element_type: Box::new(element_type),
            count,
            within_expr,
            span: Some(start),
        })
    }

    fn parse_bytes_type_expr(&mut self) -> Result<AstTypeExpr> {
        let start = self.span();
        self.expect(&TokenKind::Bytes)?;
        self.expect(&TokenKind::LBracket)?;

        // bytes[remaining]
        if self.eat(&TokenKind::Remaining) {
            self.expect(&TokenKind::RBracket)?;
            return Ok(AstTypeExpr::Bytes {
                kind: AstBytesKind::Remaining,
                fixed_size: None,
                size_expr: None,
                span: Some(start),
            });
        }

        // Check for "length:" or "length_or_remaining:" with lookahead
        if let TokenKind::Name(n) = self.peek().clone()
            && (n == "length" || n == "length_or_remaining")
            && matches!(self.tokens[self.pos + 1].kind, TokenKind::Colon)
        {
            self.advance(); // skip "length" / "length_or_remaining"
            self.advance(); // skip ":"
            let expr = self.parse_expr()?;
            let kind = if n == "length" {
                AstBytesKind::Length
            } else {
                AstBytesKind::LengthOrRemaining
            };
            self.expect(&TokenKind::RBracket)?;
            return Ok(AstTypeExpr::Bytes {
                kind,
                fixed_size: None,
                size_expr: Some(Box::new(expr)),
                span: Some(start),
            });
        }

        // bytes[N] — fixed size integer
        if let TokenKind::Integer(n) = *self.peek() {
            // Only treat as fixed if the next token after the integer is ']'
            if matches!(self.tokens[self.pos + 1].kind, TokenKind::RBracket) {
                self.advance();
                self.expect(&TokenKind::RBracket)?;
                return Ok(AstTypeExpr::Bytes {
                    kind: AstBytesKind::Fixed,
                    fixed_size: Some(n as u64),
                    size_expr: None,
                    span: Some(start),
                });
            }
        }

        // bytes[EXPR] — expression for length (shorthand for bytes[length: EXPR])
        let expr = self.parse_expr()?;
        self.expect(&TokenKind::RBracket)?;
        Ok(AstTypeExpr::Bytes {
            kind: AstBytesKind::Length,
            fixed_size: None,
            size_expr: Some(Box::new(expr)),
            span: Some(start),
        })
    }

    fn parse_bits_type_expr(&mut self) -> Result<AstTypeExpr> {
        let start = self.span();
        self.expect(&TokenKind::Bits)?;
        self.expect(&TokenKind::LBracket)?;
        let width = self.parse_integer()? as u16;
        self.expect(&TokenKind::RBracket)?;
        Ok(AstTypeExpr::Bits {
            width,
            span: Some(start),
        })
    }

    // ── Patterns ──

    fn parse_pattern(&mut self) -> Result<AstPattern> {
        let start = self.span();
        // Check for wildcard "_"
        if let TokenKind::Name(n) = self.peek().clone()
            && n == "_"
        {
            self.advance();
            return Ok(AstPattern::Wildcard { span: Some(start) });
        }

        let value = self.parse_integer()?;

        if self.eat(&TokenKind::DotDotEq) {
            let end = self.parse_integer()?;
            Ok(AstPattern::RangeInclusive {
                start: value,
                end,
                span: Some(start),
            })
        } else {
            Ok(AstPattern::Value {
                value,
                span: Some(start),
            })
        }
    }

    // ── Expressions (precedence climbing) ──
    // Precedence (low to high):
    //   coalesce (??)
    //   or
    //   and
    //   comparison (==, !=, <, <=, >, >=)
    //   bitor (|)
    //   bitxor (^)
    //   bitand (&)
    //   shift (<<, >>)
    //   add (+, -)
    //   mul (*, /, %)
    //   unary (!, -)
    //   postfix (., [], [..])

    fn parse_expr(&mut self) -> Result<AstExpr> {
        self.parse_coalesce_expr()
    }

    fn parse_coalesce_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();
        let mut expr = self.parse_or_expr()?;
        if self.eat(&TokenKind::QuestionQuestion) {
            let default = self.parse_or_expr()?;
            expr = AstExpr::Coalesce {
                expr: Box::new(expr),
                default: Box::new(default),
                span: Some(start),
            };
        }
        Ok(expr)
    }

    fn parse_or_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();
        let mut left = self.parse_and_expr()?;
        while self.eat(&TokenKind::Or) {
            let right = self.parse_and_expr()?;
            left = AstExpr::Binary {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
                span: Some(start),
            };
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();
        let mut left = self.parse_compare_expr()?;
        while self.eat(&TokenKind::And) {
            let right = self.parse_compare_expr()?;
            left = AstExpr::Binary {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
                span: Some(start),
            };
        }
        Ok(left)
    }

    fn parse_compare_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();
        let left = self.parse_bitor_expr()?;
        let op = match self.peek() {
            TokenKind::EqEq => Some(BinOp::Eq),
            TokenKind::BangEq => Some(BinOp::Ne),
            TokenKind::Lt => Some(BinOp::Lt),
            TokenKind::Le => Some(BinOp::Le),
            TokenKind::Gt => Some(BinOp::Gt),
            TokenKind::Ge => Some(BinOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let right = self.parse_bitor_expr()?;
            Ok(AstExpr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span: Some(start),
            })
        } else {
            Ok(left)
        }
    }

    fn parse_bitor_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();
        let mut left = self.parse_bitxor_expr()?;
        while self.eat(&TokenKind::Pipe) {
            let right = self.parse_bitxor_expr()?;
            left = AstExpr::Binary {
                op: BinOp::BitOr,
                left: Box::new(left),
                right: Box::new(right),
                span: Some(start),
            };
        }
        Ok(left)
    }

    fn parse_bitxor_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();
        let mut left = self.parse_bitand_expr()?;
        while self.eat(&TokenKind::Caret) {
            let right = self.parse_bitand_expr()?;
            left = AstExpr::Binary {
                op: BinOp::BitXor,
                left: Box::new(left),
                right: Box::new(right),
                span: Some(start),
            };
        }
        Ok(left)
    }

    fn parse_bitand_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();
        let mut left = self.parse_shift_expr()?;
        while self.eat(&TokenKind::Amp) {
            let right = self.parse_shift_expr()?;
            left = AstExpr::Binary {
                op: BinOp::BitAnd,
                left: Box::new(left),
                right: Box::new(right),
                span: Some(start),
            };
        }
        Ok(left)
    }

    fn parse_shift_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();
        let mut left = self.parse_add_expr()?;
        loop {
            let op = match self.peek() {
                TokenKind::Shl => Some(BinOp::Shl),
                TokenKind::Shr => Some(BinOp::Shr),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
                let right = self.parse_add_expr()?;
                left = AstExpr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                    span: Some(start),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_add_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();
        let mut left = self.parse_mul_expr()?;
        loop {
            let op = match self.peek() {
                TokenKind::Plus => Some(BinOp::Add),
                TokenKind::Minus => Some(BinOp::Sub),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
                let right = self.parse_mul_expr()?;
                left = AstExpr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                    span: Some(start),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_mul_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();
        let mut left = self.parse_unary_expr()?;
        loop {
            let op = match self.peek() {
                TokenKind::Star => Some(BinOp::Mul),
                TokenKind::Slash => Some(BinOp::Div),
                TokenKind::Percent => Some(BinOp::Mod),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
                let right = self.parse_unary_expr()?;
                left = AstExpr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                    span: Some(start),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();
        match self.peek() {
            TokenKind::Bang => {
                self.advance();
                let operand = self.parse_unary_expr()?;
                Ok(AstExpr::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                    span: Some(start),
                })
            }
            TokenKind::Minus => {
                self.advance();
                let operand = self.parse_unary_expr()?;
                Ok(AstExpr::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                    span: Some(start),
                })
            }
            TokenKind::Not => {
                self.advance();
                let operand = self.parse_unary_expr()?;
                Ok(AstExpr::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                    span: Some(start),
                })
            }
            _ => self.parse_postfix_expr(),
        }
    }

    fn parse_postfix_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();
        let mut expr = self.parse_primary_expr()?;

        loop {
            match self.peek() {
                TokenKind::Dot => {
                    self.advance();
                    let (field, _) = self.expect_name_or_keyword()?;
                    expr = AstExpr::MemberAccess {
                        base: Box::new(expr),
                        field,
                        span: Some(start),
                    };
                }
                TokenKind::LBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    // Check for slice: expr[start..end]
                    if self.eat(&TokenKind::DotDot) {
                        let end = self.parse_expr()?;
                        self.expect(&TokenKind::RBracket)?;
                        expr = AstExpr::Slice {
                            base: Box::new(expr),
                            start: Box::new(index),
                            end: Box::new(end),
                            span: Some(start),
                        };
                    } else {
                        self.expect(&TokenKind::RBracket)?;
                        expr = AstExpr::Subscript {
                            base: Box::new(expr),
                            index: Box::new(index),
                            span: Some(start),
                        };
                    }
                }
                TokenKind::InState => {
                    self.advance();
                    self.expect(&TokenKind::LParen)?;
                    let (state_name, _) = self.expect_name()?;
                    self.expect(&TokenKind::RParen)?;
                    expr = AstExpr::InState {
                        expr: Box::new(expr),
                        state_name,
                        span: Some(start),
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary_expr(&mut self) -> Result<AstExpr> {
        let start = self.span();

        match self.peek().clone() {
            TokenKind::Integer(value) => {
                self.advance();
                Ok(AstExpr::Int {
                    value,
                    span: Some(start),
                })
            }
            TokenKind::True => {
                self.advance();
                Ok(AstExpr::Bool {
                    value: true,
                    span: Some(start),
                })
            }
            TokenKind::False => {
                self.advance();
                Ok(AstExpr::Bool {
                    value: false,
                    span: Some(start),
                })
            }
            TokenKind::Null => {
                self.advance();
                Ok(AstExpr::Null { span: Some(start) })
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(expr)
            }
            TokenKind::Fill => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let value = self.parse_expr()?;
                self.expect(&TokenKind::Comma)?;
                let count = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(AstExpr::Fill {
                    value: Box::new(value),
                    count: Box::new(count),
                    span: Some(start),
                })
            }
            TokenKind::All => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let collection = self.parse_expr()?;
                self.expect(&TokenKind::Comma)?;
                // Expect in_state(StateName)
                self.expect(&TokenKind::InState)?;
                self.expect(&TokenKind::LParen)?;
                let (state_name, _) = self.expect_name()?;
                self.expect(&TokenKind::RParen)?;
                self.expect(&TokenKind::RParen)?;
                Ok(AstExpr::All {
                    collection: Box::new(collection),
                    state_name,
                    span: Some(start),
                })
            }
            _ => {
                // Try to parse as a name (including keywords used as identifiers)
                if let Some(name) = self.token_as_name() {
                    self.advance();
                    // Check for state constructor: Name::StateName(args)
                    if self.at(&TokenKind::ColonColon) {
                        self.advance();
                        let (state_name, _) = self.expect_name()?;
                        let mut args = Vec::new();
                        if self.eat(&TokenKind::LParen) {
                            if !self.at(&TokenKind::RParen) {
                                args.push(self.parse_expr()?);
                                while self.eat(&TokenKind::Comma) {
                                    args.push(self.parse_expr()?);
                                }
                            }
                            self.expect(&TokenKind::RParen)?;
                        }
                        Ok(AstExpr::StateConstructor {
                            sm_name: name,
                            state_name,
                            args,
                            span: Some(start),
                        })
                    } else {
                        Ok(AstExpr::NameRef {
                            name,
                            span: Some(start),
                        })
                    }
                } else {
                    Err(self.error(format!("expected expression, found {:?}", self.peek())))
                }
            }
        }
    }

    // ── Helpers ──

    fn parse_type_ref_name(&mut self) -> Result<String> {
        // Primitives, user-defined names, and keywords that can be type names
        if let Some(name) = self.token_as_name() {
            self.advance();
            Ok(name)
        } else {
            Err(self.error(format!("expected type name, found {:?}", self.peek())))
        }
    }

    fn parse_integer(&mut self) -> Result<i64> {
        match self.peek().clone() {
            TokenKind::Integer(v) => {
                self.advance();
                Ok(v)
            }
            _ => Err(self.error(format!("expected integer, found {:?}", self.peek()))),
        }
    }

    fn parse_literal_value(&mut self) -> Result<AstLiteralValue> {
        match self.peek().clone() {
            TokenKind::Integer(v) => {
                self.advance();
                Ok(AstLiteralValue::Int(v))
            }
            TokenKind::True => {
                self.advance();
                Ok(AstLiteralValue::Bool(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(AstLiteralValue::Bool(false))
            }
            TokenKind::StringLit(v) => {
                self.advance();
                Ok(AstLiteralValue::String(v))
            }
            TokenKind::Null => {
                self.advance();
                Ok(AstLiteralValue::Null)
            }
            _ => Err(self.error(format!("expected literal, found {:?}", self.peek()))),
        }
    }

    // ── Extern ASN.1 ──

    fn parse_extern_asn1(&mut self, _annotations: Vec<AstAnnotation>) -> Result<AstTopItem> {
        let start = self.span();
        self.advance(); // skip "extern"

        // Expect "asn1" as a name token
        let (kw, _) = self.expect_name()?;
        if kw != "asn1" {
            return Err(self.error(format!("expected 'asn1' after 'extern', found '{kw}'")));
        }

        // Expect string literal for path
        let path = match self.peek().clone() {
            TokenKind::StringLit(s) => {
                self.advance();
                s
            }
            _ => return Err(self.error("expected string literal for ASN.1 file path".into())),
        };

        // Optional: use <module_path>
        let rust_module = if let Some(name) = self.token_as_name() {
            if name == "use" {
                self.advance(); // skip "use"
                let (first, _) = self.expect_name()?;
                let mut mod_path = first;
                while self.eat(&TokenKind::ColonColon) {
                    let (part, _) = self.expect_name()?;
                    mod_path = format!("{mod_path}::{part}");
                }
                Some(mod_path)
            } else {
                None
            }
        } else {
            None
        };

        // Expect { TypeA, TypeB, ... }
        self.expect(&TokenKind::LBrace)?;
        let mut type_names = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let (name, _) = self.expect_name()?;
            type_names.push(name);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(AstTopItem::ExternAsn1(AstExternAsn1 {
            path,
            rust_module,
            type_names,
            span: Some(start),
        }))
    }
}

// ── Public parse function ──

pub fn parse(source: &str) -> std::result::Result<AstModule, ParseError> {
    let tokens = crate::lexer::Lexer::new(source)
        .tokenize()
        .map_err(|e| ParseError {
            msg: e.msg,
            span: Some(Span::new(e.offset as u32, 0)),
        })?;
    Parser::new(tokens).parse_module()
}
