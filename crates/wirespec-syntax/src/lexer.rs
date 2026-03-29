//! Hand-written lexer/tokenizer for wirespec.
//!
//! Produces a stream of `Token`s from source text. Handles keywords, operators,
//! integer literals (decimal, hex, binary), string literals, and comments
//! (`#` and `//` line comments).

use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Identifiers and literals
    Name(String),
    Integer(i64),
    StringLit(String),

    // Keywords
    Module,
    Import,
    Const,
    Enum,
    Flags,
    Type,
    Packet,
    Frame,
    Capsule,
    State,
    Machine,
    Transition,
    Initial,
    Terminal,
    On,
    Guard,
    Action,
    Delegate,
    Match,
    If,
    Let,
    Require,
    StaticAssert,
    Within,
    Export,
    Varint,
    Bytes,
    Bits,
    Bit,
    Fill,
    Remaining,
    True,
    False,
    Null,
    And,
    Or,
    Not,
    InState,
    All,

    // Punctuation / operators
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Colon,
    ColonColon,
    Semicolon,
    Comma,
    Dot,
    DotDot,
    DotDotEq,
    Arrow,      // ->
    FatArrow,   // =>
    Assign,     // =
    PlusAssign, // +=
    At,         // @
    LArrow,     // <-

    // Arithmetic
    Plus,
    Minus,
    Star,
    Slash,
    Percent,

    // Bitwise
    Amp,
    Pipe,
    Caret,
    Shl,
    Shr,
    Bang,

    // Comparison
    EqEq,
    BangEq,
    Lt,
    Le,
    Gt,
    Ge,

    // Misc
    QuestionQuestion, // ??

    // Special
    Eof,
}

pub struct Lexer<'src> {
    source: &'src [u8],
    pos: usize,
    tokens: Vec<Token>,
}

impl<'src> Lexer<'src> {
    pub fn new(source: &'src str) -> Self {
        Self {
            source: source.as_bytes(),
            pos: 0,
            tokens: Vec::new(),
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, LexError> {
        while self.pos < self.source.len() {
            self.skip_whitespace_and_comments();
            if self.pos >= self.source.len() {
                break;
            }
            self.next_token()?;
        }
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span::new(self.pos as u32, 0),
        });
        Ok(self.tokens)
    }

    fn peek(&self) -> u8 {
        if self.pos < self.source.len() {
            self.source[self.pos]
        } else {
            0
        }
    }

    fn peek_at(&self, offset: usize) -> u8 {
        let idx = self.pos + offset;
        if idx < self.source.len() {
            self.source[idx]
        } else {
            0
        }
    }

    fn advance(&mut self) -> u8 {
        let ch = self.source[self.pos];
        self.pos += 1;
        ch
    }

    fn skip_whitespace_and_comments(&mut self) {
        while self.pos < self.source.len() {
            let ch = self.peek();
            if ch == b' ' || ch == b'\t' || ch == b'\r' || ch == b'\n' {
                self.pos += 1;
            } else if ch == b'#' || (ch == b'/' && self.peek_at(1) == b'/') {
                // Line comment (# or //)
                while self.pos < self.source.len() && self.source[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn next_token(&mut self) -> Result<(), LexError> {
        let start = self.pos;
        let ch = self.advance();

        let kind = match ch {
            b'{' => TokenKind::LBrace,
            b'}' => TokenKind::RBrace,
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,
            b'[' => TokenKind::LBracket,
            b']' => TokenKind::RBracket,
            b';' => TokenKind::Semicolon,
            b',' => TokenKind::Comma,
            b'*' => TokenKind::Star,
            b'%' => TokenKind::Percent,
            b'^' => TokenKind::Caret,
            b'@' => TokenKind::At,

            b':' => {
                if self.peek() == b':' {
                    self.advance();
                    TokenKind::ColonColon
                } else {
                    TokenKind::Colon
                }
            }

            b'.' => {
                if self.peek() == b'.' {
                    self.advance();
                    if self.peek() == b'=' {
                        self.advance();
                        TokenKind::DotDotEq
                    } else {
                        TokenKind::DotDot
                    }
                } else {
                    TokenKind::Dot
                }
            }

            b'-' => {
                if self.peek() == b'>' {
                    self.advance();
                    TokenKind::Arrow
                } else {
                    TokenKind::Minus
                }
            }

            b'=' => {
                if self.peek() == b'>' {
                    self.advance();
                    TokenKind::FatArrow
                } else if self.peek() == b'=' {
                    self.advance();
                    TokenKind::EqEq
                } else {
                    TokenKind::Assign
                }
            }

            b'+' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::PlusAssign
                } else {
                    TokenKind::Plus
                }
            }

            b'!' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::BangEq
                } else {
                    TokenKind::Bang
                }
            }

            b'<' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::Le
                } else if self.peek() == b'<' {
                    self.advance();
                    TokenKind::Shl
                } else if self.peek() == b'-' {
                    self.advance();
                    TokenKind::LArrow
                } else {
                    TokenKind::Lt
                }
            }

            b'>' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::Ge
                } else if self.peek() == b'>' {
                    self.advance();
                    TokenKind::Shr
                } else {
                    TokenKind::Gt
                }
            }

            b'&' => TokenKind::Amp,
            b'|' => TokenKind::Pipe,
            b'/' => TokenKind::Slash,

            b'?' => {
                if self.peek() == b'?' {
                    self.advance();
                    TokenKind::QuestionQuestion
                } else {
                    return Err(LexError {
                        msg: "unexpected '?'".into(),
                        offset: start,
                    });
                }
            }

            b'"' => return self.lex_string(start),

            b'0' if self.peek() == b'x' || self.peek() == b'X' => {
                self.advance(); // skip 'x'
                return self.lex_hex(start);
            }

            b'0' if self.peek() == b'b' || self.peek() == b'B' => {
                self.advance(); // skip 'b'
                return self.lex_binary(start);
            }

            ch if ch.is_ascii_digit() => {
                return self.lex_decimal(start);
            }

            ch if ch.is_ascii_alphabetic() || ch == b'_' => {
                return self.lex_name(start);
            }

            _ => {
                return Err(LexError {
                    msg: format!("unexpected character: {:?}", ch as char),
                    offset: start,
                });
            }
        };

        self.tokens.push(Token {
            kind,
            span: Span::new(start as u32, (self.pos - start) as u32),
        });
        Ok(())
    }

    fn lex_decimal(&mut self, start: usize) -> Result<(), LexError> {
        while self.pos < self.source.len() && (self.peek().is_ascii_digit() || self.peek() == b'_')
        {
            self.advance();
        }
        let text: String = self.source[start..self.pos]
            .iter()
            .filter(|&&b| b != b'_')
            .map(|&b| b as char)
            .collect();
        let value = text.parse::<i64>().map_err(|_| LexError {
            msg: format!("invalid integer literal: {text}"),
            offset: start,
        })?;
        self.tokens.push(Token {
            kind: TokenKind::Integer(value),
            span: Span::new(start as u32, (self.pos - start) as u32),
        });
        Ok(())
    }

    fn lex_hex(&mut self, start: usize) -> Result<(), LexError> {
        if self.pos >= self.source.len() || !self.peek().is_ascii_hexdigit() {
            return Err(LexError {
                msg: "expected hex digit after 0x".into(),
                offset: start,
            });
        }
        while self.pos < self.source.len()
            && (self.peek().is_ascii_hexdigit() || self.peek() == b'_')
        {
            self.advance();
        }
        // start points at '0', we skip "0x" prefix for parsing
        let text: String = self.source[start + 2..self.pos]
            .iter()
            .filter(|&&b| b != b'_')
            .map(|&b| b as char)
            .collect();
        let value = i64::from_str_radix(&text, 16).map_err(|_| LexError {
            msg: format!("invalid hex literal: 0x{text}"),
            offset: start,
        })?;
        self.tokens.push(Token {
            kind: TokenKind::Integer(value),
            span: Span::new(start as u32, (self.pos - start) as u32),
        });
        Ok(())
    }

    fn lex_binary(&mut self, start: usize) -> Result<(), LexError> {
        if self.pos >= self.source.len() || (self.peek() != b'0' && self.peek() != b'1') {
            return Err(LexError {
                msg: "expected binary digit after 0b".into(),
                offset: start,
            });
        }
        while self.pos < self.source.len()
            && (self.peek() == b'0' || self.peek() == b'1' || self.peek() == b'_')
        {
            self.advance();
        }
        let text: String = self.source[start + 2..self.pos]
            .iter()
            .filter(|&&b| b != b'_')
            .map(|&b| b as char)
            .collect();
        let value = i64::from_str_radix(&text, 2).map_err(|_| LexError {
            msg: format!("invalid binary literal: 0b{text}"),
            offset: start,
        })?;
        self.tokens.push(Token {
            kind: TokenKind::Integer(value),
            span: Span::new(start as u32, (self.pos - start) as u32),
        });
        Ok(())
    }

    fn lex_string(&mut self, start: usize) -> Result<(), LexError> {
        let mut value = String::new();
        loop {
            if self.pos >= self.source.len() {
                return Err(LexError {
                    msg: "unterminated string literal".into(),
                    offset: start,
                });
            }
            let ch = self.advance();
            match ch {
                b'"' => break,
                b'\\' => {
                    if self.pos >= self.source.len() {
                        return Err(LexError {
                            msg: "unterminated escape in string".into(),
                            offset: start,
                        });
                    }
                    let esc = self.advance();
                    match esc {
                        b'n' => value.push('\n'),
                        b't' => value.push('\t'),
                        b'\\' => value.push('\\'),
                        b'"' => value.push('"'),
                        _ => {
                            return Err(LexError {
                                msg: format!("unknown escape: \\{}", esc as char),
                                offset: self.pos - 1,
                            });
                        }
                    }
                }
                _ => value.push(ch as char),
            }
        }
        self.tokens.push(Token {
            kind: TokenKind::StringLit(value),
            span: Span::new(start as u32, (self.pos - start) as u32),
        });
        Ok(())
    }

    fn lex_name(&mut self, start: usize) -> Result<(), LexError> {
        while self.pos < self.source.len()
            && (self.peek().is_ascii_alphanumeric() || self.peek() == b'_')
        {
            self.advance();
        }
        let text = std::str::from_utf8(&self.source[start..self.pos])
            .expect("identifier bytes must be valid UTF-8");
        let kind = match text {
            "module" => TokenKind::Module,
            "import" => TokenKind::Import,
            "const" => TokenKind::Const,
            "enum" => TokenKind::Enum,
            "flags" => TokenKind::Flags,
            "type" => TokenKind::Type,
            "packet" => TokenKind::Packet,
            "frame" => TokenKind::Frame,
            "capsule" => TokenKind::Capsule,
            "state" => TokenKind::State,
            "machine" => TokenKind::Machine,
            "transition" => TokenKind::Transition,
            "initial" => TokenKind::Initial,
            "terminal" => TokenKind::Terminal,
            "on" => TokenKind::On,
            "guard" => TokenKind::Guard,
            "action" => TokenKind::Action,
            "delegate" => TokenKind::Delegate,
            "match" => TokenKind::Match,
            "if" => TokenKind::If,
            "let" => TokenKind::Let,
            "require" => TokenKind::Require,
            "static_assert" => TokenKind::StaticAssert,
            "within" => TokenKind::Within,
            "export" => TokenKind::Export,
            "varint" => TokenKind::Varint,
            "bytes" => TokenKind::Bytes,
            "bits" => TokenKind::Bits,
            "bit" => TokenKind::Bit,
            "fill" => TokenKind::Fill,
            "remaining" => TokenKind::Remaining,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "null" => TokenKind::Null,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "not" => TokenKind::Not,
            "in_state" => TokenKind::InState,
            "all" => TokenKind::All,
            _ => TokenKind::Name(text.to_string()),
        };
        self.tokens.push(Token {
            kind,
            span: Span::new(start as u32, (self.pos - start) as u32),
        });
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct LexError {
    pub msg: String,
    pub offset: usize,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lex error at offset {}: {}", self.offset, self.msg)
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok_kinds(src: &str) -> Vec<TokenKind> {
        let tokens = Lexer::new(src).tokenize().unwrap();
        tokens.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn keywords() {
        let kinds = tok_kinds("packet frame capsule type match");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Packet,
                TokenKind::Frame,
                TokenKind::Capsule,
                TokenKind::Type,
                TokenKind::Match,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn integers() {
        let kinds = tok_kinds("42 0xFF 0b1010");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Integer(42),
                TokenKind::Integer(0xFF),
                TokenKind::Integer(0b1010),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn operators() {
        let kinds = tok_kinds("+ - * / & | ^ << >> == != <= >= ?? => -> <- ..=");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Plus,
                TokenKind::Minus,
                TokenKind::Star,
                TokenKind::Slash,
                TokenKind::Amp,
                TokenKind::Pipe,
                TokenKind::Caret,
                TokenKind::Shl,
                TokenKind::Shr,
                TokenKind::EqEq,
                TokenKind::BangEq,
                TokenKind::Le,
                TokenKind::Ge,
                TokenKind::QuestionQuestion,
                TokenKind::FatArrow,
                TokenKind::Arrow,
                TokenKind::LArrow,
                TokenKind::DotDotEq,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn string_literal() {
        let kinds = tok_kinds(r#""hello world""#);
        assert_eq!(
            kinds,
            vec![
                TokenKind::StringLit("hello world".to_string()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn comments() {
        let kinds = tok_kinds("packet # comment\nframe // also comment\ncapsule");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Packet,
                TokenKind::Frame,
                TokenKind::Capsule,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn name_and_reserved() {
        let kinds = tok_kinds("src dst fill remaining in_state all true false null");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Name("src".into()),
                TokenKind::Name("dst".into()),
                TokenKind::Fill,
                TokenKind::Remaining,
                TokenKind::InState,
                TokenKind::All,
                TokenKind::True,
                TokenKind::False,
                TokenKind::Null,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn hex_underscore() {
        let kinds = tok_kinds("0xFF_FF");
        assert_eq!(kinds, vec![TokenKind::Integer(0xFFFF), TokenKind::Eof]);
    }

    #[test]
    fn binary_underscore() {
        let kinds = tok_kinds("0b1010_0101");
        assert_eq!(kinds, vec![TokenKind::Integer(0b10100101), TokenKind::Eof]);
    }

    #[test]
    fn decimal_underscore() {
        let kinds = tok_kinds("1_000_000");
        assert_eq!(kinds, vec![TokenKind::Integer(1000000), TokenKind::Eof]);
    }

    #[test]
    fn empty_string() {
        let kinds = tok_kinds(r#""""#);
        assert_eq!(
            kinds,
            vec![TokenKind::StringLit("".to_string()), TokenKind::Eof]
        );
    }

    #[test]
    fn string_escapes() {
        let kinds = tok_kinds(r#""\n\t\\\"" "#);
        assert_eq!(
            kinds,
            vec![TokenKind::StringLit("\n\t\\\"".to_string()), TokenKind::Eof,]
        );
    }

    #[test]
    fn consecutive_operators() {
        let kinds = tok_kinds(">>>=");
        // >> > =
        assert_eq!(kinds, vec![TokenKind::Shr, TokenKind::Ge, TokenKind::Eof]);
    }
}
