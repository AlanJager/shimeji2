//! Tokeniser for the expression subset.

use crate::ast::Span;
use crate::parser::ParseError;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Number(f64),
    String(String),
    Ident(String),
    True,
    False,
    Null,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Question,
    Colon,
    Dot,
    Comma,
    LParen,
    RParen,
    LBracket,
    RBracket,
    EqEq,
    BangEq,
    EqEqEq,
    BangEqEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    AndAnd,
    OrOr,
    Eof,
}

impl TokenKind {
    pub fn describe(&self) -> String {
        match self {
            Self::Number(n) => format!("number {n}"),
            Self::String(s) => format!("string {s:?}"),
            Self::Ident(name) => format!("identifier {name}"),
            Self::True => "true".into(),
            Self::False => "false".into(),
            Self::Null => "null".into(),
            Self::Plus => "'+'".into(),
            Self::Minus => "'-'".into(),
            Self::Star => "'*'".into(),
            Self::Slash => "'/'".into(),
            Self::Percent => "'%'".into(),
            Self::Bang => "'!'".into(),
            Self::Question => "'?'".into(),
            Self::Colon => "':'".into(),
            Self::Dot => "'.'".into(),
            Self::Comma => "','".into(),
            Self::LParen => "'('".into(),
            Self::RParen => "')'".into(),
            Self::LBracket => "'['".into(),
            Self::RBracket => "']'".into(),
            Self::EqEq => "'=='".into(),
            Self::BangEq => "'!='".into(),
            Self::EqEqEq => "'==='".into(),
            Self::BangEqEq => "'!=='".into(),
            Self::Lt => "'<'".into(),
            Self::LtEq => "'<='".into(),
            Self::Gt => "'>'".into(),
            Self::GtEq => "'>='".into(),
            Self::AndAnd => "'&&'".into(),
            Self::OrOr => "'||'".into(),
            Self::Eof => "end of expression".into(),
        }
    }
}

pub struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    pub fn next_token(&mut self) -> Result<Token, ParseError> {
        self.skip_ws();
        let start = self.pos;
        let Some(ch) = self.peek_char() else {
            return Ok(Token {
                kind: TokenKind::Eof,
                span: Span::new(start, start),
            });
        };
        match ch {
            '0'..='9' => self.number(start, false),
            '.' => {
                if self.peek_nth(1).is_some_and(|c| c.is_ascii_digit()) {
                    self.number(start, true)
                } else {
                    self.bump();
                    Ok(self.token(TokenKind::Dot, start))
                }
            }
            '"' | '\'' => self.string(ch, start),
            '+' => self.plus(start),
            '-' => self.minus(start),
            '*' => self.single_or_assign(start, TokenKind::Star, '*'),
            '/' => self.slash(start),
            '%' => self.single_or_assign(start, TokenKind::Percent, '%'),
            '!' => self.bang(start),
            '?' => {
                self.bump();
                Ok(self.token(TokenKind::Question, start))
            }
            ':' => {
                self.bump();
                Ok(self.token(TokenKind::Colon, start))
            }
            ',' => {
                self.bump();
                Ok(self.token(TokenKind::Comma, start))
            }
            '(' => {
                self.bump();
                Ok(self.token(TokenKind::LParen, start))
            }
            ')' => {
                self.bump();
                Ok(self.token(TokenKind::RParen, start))
            }
            '[' => {
                self.bump();
                Ok(self.token(TokenKind::LBracket, start))
            }
            ']' => {
                self.bump();
                Ok(self.token(TokenKind::RBracket, start))
            }
            '=' => self.eq(start),
            '<' => self.lt(start),
            '>' => self.gt(start),
            '&' => self.and(start),
            '|' => self.or(start),
            ';' => Err(self.err("statements are not in the subset", start, 1)),
            '{' | '}' => {
                Err(self.err("object literals and blocks are not in the subset", start, 1))
            }
            '`' => Err(self.err("template literals are not in the subset", start, 1)),
            '#' => Err(self.err("unexpected '#'", start, 1)),
            _ if is_ident_start(ch) => self.ident(start),
            _ => {
                let len = ch.len_utf8();
                Err(self.err(&format!("unexpected {ch:?}"), start, len))
            }
        }
    }

    fn plus(&mut self, start: usize) -> Result<Token, ParseError> {
        self.bump();
        match self.peek_char() {
            Some('+') => Err(self.err("increment is not in the subset", start, 2)),
            Some('=') => Err(self.err("assignment is not in the subset", start, 2)),
            _ => Ok(self.token(TokenKind::Plus, start)),
        }
    }

    fn minus(&mut self, start: usize) -> Result<Token, ParseError> {
        self.bump();
        match self.peek_char() {
            Some('-') => Err(self.err("decrement is not in the subset", start, 2)),
            Some('=') => Err(self.err("assignment is not in the subset", start, 2)),
            _ => Ok(self.token(TokenKind::Minus, start)),
        }
    }

    fn single_or_assign(
        &mut self,
        start: usize,
        kind: TokenKind,
        _ch: char,
    ) -> Result<Token, ParseError> {
        self.bump();
        if self.peek_char() == Some('=') {
            return Err(self.err("assignment is not in the subset", start, 2));
        }
        Ok(self.token(kind, start))
    }

    fn slash(&mut self, start: usize) -> Result<Token, ParseError> {
        self.bump();
        match self.peek_char() {
            Some('/') | Some('*') => Err(self.err("comments are not in the subset", start, 2)),
            Some('=') => Err(self.err("assignment is not in the subset", start, 2)),
            _ => Ok(self.token(TokenKind::Slash, start)),
        }
    }

    fn bang(&mut self, start: usize) -> Result<Token, ParseError> {
        self.bump();
        if self.peek_char() == Some('=') {
            self.bump();
            if self.peek_char() == Some('=') {
                self.bump();
                return Ok(self.token(TokenKind::BangEqEq, start));
            }
            return Ok(self.token(TokenKind::BangEq, start));
        }
        Ok(self.token(TokenKind::Bang, start))
    }

    fn eq(&mut self, start: usize) -> Result<Token, ParseError> {
        self.bump();
        match self.peek_char() {
            Some('=') => {
                self.bump();
                if self.peek_char() == Some('=') {
                    self.bump();
                    Ok(self.token(TokenKind::EqEqEq, start))
                } else {
                    Ok(self.token(TokenKind::EqEq, start))
                }
            }
            Some('>') => Err(self.err("arrow functions are not in the subset", start, 2)),
            _ => Err(self.err("assignment is not in the subset", start, 1)),
        }
    }

    fn lt(&mut self, start: usize) -> Result<Token, ParseError> {
        self.bump();
        match self.peek_char() {
            Some('=') => {
                self.bump();
                Ok(self.token(TokenKind::LtEq, start))
            }
            Some('<') => Err(self.err("bitwise operators are not in the subset", start, 2)),
            _ => Ok(self.token(TokenKind::Lt, start)),
        }
    }

    fn gt(&mut self, start: usize) -> Result<Token, ParseError> {
        self.bump();
        match self.peek_char() {
            Some('=') => {
                self.bump();
                Ok(self.token(TokenKind::GtEq, start))
            }
            Some('>') => Err(self.err("bitwise operators are not in the subset", start, 2)),
            _ => Ok(self.token(TokenKind::Gt, start)),
        }
    }

    fn and(&mut self, start: usize) -> Result<Token, ParseError> {
        self.bump();
        match self.peek_char() {
            Some('&') => {
                self.bump();
                Ok(self.token(TokenKind::AndAnd, start))
            }
            Some('=') => Err(self.err("assignment is not in the subset", start, 2)),
            _ => Err(self.err("bitwise operators are not in the subset", start, 1)),
        }
    }

    fn or(&mut self, start: usize) -> Result<Token, ParseError> {
        self.bump();
        match self.peek_char() {
            Some('|') => {
                self.bump();
                Ok(self.token(TokenKind::OrOr, start))
            }
            Some('=') => Err(self.err("assignment is not in the subset", start, 2)),
            _ => Err(self.err("bitwise operators are not in the subset", start, 1)),
        }
    }

    fn number(&mut self, start: usize, leading_dot: bool) -> Result<Token, ParseError> {
        if leading_dot {
            self.bump();
            self.digits();
        } else {
            let first = self.bump().expect("digit");
            if first == '0' {
                if let Some(next) = self.peek_char() {
                    if matches!(next, 'x' | 'X' | 'b' | 'B' | 'o' | 'O') {
                        return Err(self.err("base prefixes are not in the subset", start, 2));
                    }
                }
            }
            self.digits();
            if self.peek_char() == Some('.') {
                let after_dot = self.peek_nth(1);
                if after_dot.is_some_and(|c| c.is_ascii_digit())
                    || after_dot.is_none()
                    || after_dot.is_some_and(|c| matches!(c, 'e' | 'E') || !is_ident_start(c))
                {
                    self.bump();
                    self.digits();
                }
            }
        }
        if matches!(self.peek_char(), Some('e') | Some('E')) {
            let exp_start = self.pos;
            self.bump();
            if matches!(self.peek_char(), Some('+') | Some('-')) {
                self.bump();
            }
            if !self.peek_char().is_some_and(|c| c.is_ascii_digit()) {
                return Err(self.err("invalid exponent", exp_start, self.pos - exp_start));
            }
            self.digits();
        }
        let lexeme = &self.src[start..self.pos];
        let value = lexeme
            .parse::<f64>()
            .map_err(|_| ParseError::new("invalid number", Span::new(start, self.pos)))?;
        Ok(self.token(TokenKind::Number(value), start))
    }

    fn digits(&mut self) {
        while self.peek_char().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
    }

    fn string(&mut self, quote: char, start: usize) -> Result<Token, ParseError> {
        self.bump();
        let mut out = String::new();
        loop {
            let Some(ch) = self.bump() else {
                return Err(self.err("unterminated string", start, self.pos - start));
            };
            if ch == quote {
                return Ok(self.token(TokenKind::String(out), start));
            }
            if ch == '\\' {
                let Some(esc) = self.bump() else {
                    return Err(self.err("unterminated string escape", start, self.pos - start));
                };
                match esc {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '0' => out.push('\0'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    '\\' | '\'' | '"' => out.push(esc),
                    'u' => {
                        let hex_start = self.pos;
                        for _ in 0..4 {
                            let Some(h) = self.bump() else {
                                return Err(self.err(
                                    "unterminated unicode escape",
                                    start,
                                    self.pos - start,
                                ));
                            };
                            if !h.is_ascii_hexdigit() {
                                return Err(self.err(
                                    "invalid unicode escape",
                                    hex_start,
                                    self.pos - hex_start,
                                ));
                            }
                        }
                        let hex = &self.src[hex_start..self.pos];
                        let code = u32::from_str_radix(hex, 16).expect("hex digits");
                        match char::from_u32(code) {
                            Some(decoded) => out.push(decoded),
                            None => {
                                return Err(self.err("invalid unicode scalar", hex_start, 4));
                            }
                        }
                    }
                    other => {
                        return Err(self.err(
                            &format!("unknown string escape '\\{other}'"),
                            self.pos - other.len_utf8() - 1,
                            other.len_utf8() + 1,
                        ));
                    }
                }
            } else if ch == '\n' || ch == '\r' {
                return Err(self.err("unterminated string", start, self.pos - start));
            } else {
                out.push(ch);
            }
        }
    }

    fn ident(&mut self, start: usize) -> Result<Token, ParseError> {
        while self.peek_char().is_some_and(is_ident_continue) {
            self.bump();
        }
        let name = &self.src[start..self.pos];
        if is_reserved(name) {
            return Err(ParseError::new(
                format!("{name} is not in the subset"),
                Span::new(start, self.pos),
            ));
        }
        let kind = match name {
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "null" => TokenKind::Null,
            _ => TokenKind::Ident(name.to_string()),
        };
        Ok(self.token(kind, start))
    }

    fn skip_ws(&mut self) {
        while self.peek_char().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_nth(&self, n: usize) -> Option<char> {
        self.src[self.pos..].chars().nth(n)
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn token(&self, kind: TokenKind, start: usize) -> Token {
        Token {
            kind,
            span: Span::new(start, self.pos),
        }
    }

    fn err(&self, message: &str, start: usize, len: usize) -> ParseError {
        ParseError::new(message, Span::new(start, start + len))
    }
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    is_ident_start(ch) || ch.is_numeric() || ch == '\u{200c}' || ch == '\u{200d}'
}

fn is_reserved(name: &str) -> bool {
    matches!(
        name,
        "new"
            | "function"
            | "var"
            | "let"
            | "const"
            | "return"
            | "if"
            | "else"
            | "while"
            | "for"
            | "class"
            | "import"
            | "export"
            | "typeof"
            | "void"
            | "delete"
            | "instanceof"
            | "in"
            | "this"
            | "super"
            | "switch"
            | "case"
            | "break"
            | "continue"
            | "throw"
            | "try"
            | "catch"
            | "finally"
            | "with"
            | "debugger"
            | "yield"
            | "await"
            | "do"
    )
}
