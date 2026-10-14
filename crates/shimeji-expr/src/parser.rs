//! Recursive-descent parser for the expression subset.

use crate::ast::{BinaryOp, Expr, ExprKind, Span, UnaryOp};
use crate::lexer::{Lexer, Token, TokenKind};

/// Parse error with a byte span into the source passed to [`parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl ParseError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}..{}",
            self.message, self.span.start, self.span.end
        )
    }
}

impl std::error::Error for ParseError {}

/// Strip a leading UTF-8 BOM, then an optional whole-string `${…}` / `#{…}` wrapper.
pub fn strip_wrapper(src: &str) -> &str {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let trimmed = src.trim();
    if let Some(inner) = unwrap_delims(trimmed, "${") {
        return inner;
    }
    if let Some(inner) = unwrap_delims(trimmed, "#{") {
        return inner;
    }
    trimmed
}

fn unwrap_delims<'a>(trimmed: &'a str, prefix: &str) -> Option<&'a str> {
    if trimmed.starts_with(prefix) && trimmed.ends_with('}') && trimmed.len() > prefix.len() {
        Some(&trimmed[prefix.len()..trimmed.len() - 1])
    } else {
        None
    }
}

/// Parse `src` as a subset expression. Does not strip `${}` / `#{}`.
pub fn parse(src: &str) -> Result<Expr, ParseError> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut parser = Parser::lex(src)?;
    if parser.at(TokenKind::Eof) {
        return Err(ParseError::new("empty expression", parser.peek().span));
    }
    let expr = parser.parse_expr()?;
    parser.expect_eof()?;
    Ok(expr)
}

/// Strip an optional wrapper, then parse.
pub fn parse_script(src: &str) -> Result<Expr, ParseError> {
    parse(strip_wrapper(src))
}

/// Longest source the lexer will accept. Bundled and corpus expressions are a
/// few hundred bytes; anything longer is rejected before tokens are built.
pub const MAX_SOURCE_LEN: usize = 65_536;

/// Cap on parser recursion and on the number of AST nodes along any path.
/// Prefixes, postfix chains, left-associative operators, and nested
/// parentheses, calls, indexes, and ternaries all share it, so those pieces
/// cannot multiply into a tree that is too deep to drop or lower. Real pack
/// expressions nest only a few levels. 28 still has headroom, and one fewer
/// parenthesis than this parses on a 256 KiB stack: 30 parentheses overflow
/// that stack in a debug build, 29 do not.
pub const MAX_EXPR_DEPTH: u32 = 28;

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Live `parse_expr` frames. Parentheses do not add AST nodes, but each
    /// pair still pushes a frame.
    stack: u32,
    /// Nodes on the deepest path of the expression just parsed.
    ast: u32,
}

fn binop(kind: &TokenKind) -> Option<(BinaryOp, u8)> {
    Some(match kind {
        TokenKind::OrOr => (BinaryOp::Or, 1),
        TokenKind::AndAnd => (BinaryOp::And, 2),
        TokenKind::EqEq => (BinaryOp::Eq, 3),
        TokenKind::BangEq => (BinaryOp::NotEq, 3),
        TokenKind::EqEqEq => (BinaryOp::StrictEq, 3),
        TokenKind::BangEqEq => (BinaryOp::StrictNotEq, 3),
        TokenKind::Lt => (BinaryOp::Lt, 4),
        TokenKind::LtEq => (BinaryOp::LtEq, 4),
        TokenKind::Gt => (BinaryOp::Gt, 4),
        TokenKind::GtEq => (BinaryOp::GtEq, 4),
        TokenKind::Plus => (BinaryOp::Add, 5),
        TokenKind::Minus => (BinaryOp::Sub, 5),
        TokenKind::Star => (BinaryOp::Mul, 6),
        TokenKind::Slash => (BinaryOp::Div, 6),
        TokenKind::Percent => (BinaryOp::Rem, 6),
        _ => return None,
    })
}

impl Parser {
    fn lex(src: &str) -> Result<Self, ParseError> {
        if src.len() > MAX_SOURCE_LEN {
            return Err(ParseError::new(
                "expression is too long",
                Span::new(0, src.len()),
            ));
        }
        let mut lexer = Lexer::new(src);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token()?;
            let is_eof = matches!(token.kind, TokenKind::Eof);
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        Ok(Self {
            tokens,
            pos: 0,
            stack: 0,
            ast: 0,
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        if self.stack >= MAX_EXPR_DEPTH {
            return Err(self.too_deep());
        }
        self.stack += 1;
        let result = self.parse_bp(0);
        self.stack -= 1;
        result
    }

    fn too_deep(&self) -> ParseError {
        ParseError::new("expression is too deep", self.peek().span)
    }

    /// One more node above `child`. `child` is 0 when a call has no arguments.
    fn nested(&self, child: u32) -> Result<u32, ParseError> {
        if child >= MAX_EXPR_DEPTH {
            return Err(self.too_deep());
        }
        Ok(child + 1)
    }

    /// Prefixes, postfix, and left-associative operators are loops, and the
    /// binary loop is a separate frame so it is not live during a nested
    /// parenthesis. Recursion is `parse_expr` for parentheses, calls, indexes,
    /// and ternary branches.
    fn parse_bp(&mut self, min_prec: u8) -> Result<Expr, ParseError> {
        let mut expr = self.parse_atom()?;
        expr = self.parse_binaries(expr, min_prec)?;
        if min_prec == 0 && self.eat(&TokenKind::Question) {
            let cond_ast = self.ast;
            let then_branch = self.parse_expr()?;
            let then_ast = self.ast;
            self.expect(&TokenKind::Colon, "expected ':' in ternary")?;
            let else_branch = self.parse_expr()?;
            let ast = self.nested(cond_ast.max(then_ast).max(self.ast))?;
            let span = expr.span.merge(else_branch.span);
            self.ast = ast;
            expr = Expr::new(
                ExprKind::Ternary {
                    cond: Box::new(expr),
                    then_branch: Box::new(then_branch),
                    else_branch: Box::new(else_branch),
                },
                span,
            );
        }
        Ok(expr)
    }

    fn parse_atom(&mut self) -> Result<Expr, ParseError> {
        let mut prefixes = Vec::new();
        loop {
            let op = match &self.peek().kind {
                TokenKind::Bang => Some(UnaryOp::Not),
                TokenKind::Minus => Some(UnaryOp::Neg),
                TokenKind::Plus => Some(UnaryOp::Plus),
                _ => None,
            };
            let Some(op) = op else {
                break;
            };
            // A leaf under this many prefixes is already over the path cap.
            if prefixes.len() as u32 >= MAX_EXPR_DEPTH {
                return Err(self.too_deep());
            }
            let span = self.peek().span;
            self.bump();
            prefixes.push((op, span));
        }

        let mut expr = self.parse_primary()?;
        loop {
            if self.at_kind(&TokenKind::Dot) {
                let ast = self.nested(self.ast)?;
                self.bump();
                let name = self.expect_ident("expected property name after '.'")?;
                let span = expr.span.merge(name.span);
                let TokenKind::Ident(ident) = name.kind else {
                    unreachable!("expect_ident");
                };
                self.ast = ast;
                expr = Expr::new(
                    ExprKind::Member {
                        object: Box::new(expr),
                        name: ident,
                    },
                    span,
                );
            } else if self.at_kind(&TokenKind::LParen) {
                let callee_ast = self.ast;
                self.bump();
                let (args, close) = self.parse_args()?;
                let ast = self.nested(callee_ast.max(self.ast))?;
                let span = expr.span.merge(close);
                self.ast = ast;
                expr = Expr::new(
                    ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                );
            } else if self.at_kind(&TokenKind::LBracket) {
                let object_ast = self.ast;
                self.bump();
                let index = self.parse_expr()?;
                let close = self.expect(&TokenKind::RBracket, "expected ']' after index")?;
                let ast = self.nested(object_ast.max(self.ast))?;
                let span = expr.span.merge(close.span);
                self.ast = ast;
                expr = Expr::new(
                    ExprKind::Index {
                        object: Box::new(expr),
                        index: Box::new(index),
                    },
                    span,
                );
            } else {
                break;
            }
        }

        for (op, span) in prefixes.into_iter().rev() {
            let ast = self.nested(self.ast)?;
            let span = span.merge(expr.span);
            self.ast = ast;
            expr = Expr::new(
                ExprKind::Unary {
                    op,
                    expr: Box::new(expr),
                },
                span,
            );
        }
        Ok(expr)
    }

    fn parse_binaries(&mut self, mut expr: Expr, min_prec: u8) -> Result<Expr, ParseError> {
        let mut left_ast = self.ast;
        loop {
            let Some((op, prec)) = binop(&self.peek().kind) else {
                break;
            };
            if prec < min_prec {
                break;
            }
            self.bump();
            let right = self.parse_bp(prec + 1)?;
            let ast = self.nested(left_ast.max(self.ast))?;
            let span = expr.span.merge(right.span);
            self.ast = ast;
            left_ast = ast;
            expr = Expr::new(
                ExprKind::Binary {
                    op,
                    left: Box::new(expr),
                    right: Box::new(right),
                },
                span,
            );
        }
        self.ast = left_ast;
        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<(Vec<Expr>, Span), ParseError> {
        if let Some(close) = self.eat_span(&TokenKind::RParen) {
            self.ast = 0;
            return Ok((Vec::new(), close));
        }
        let mut args = vec![self.parse_expr()?];
        let mut max_ast = self.ast;
        while self.eat(&TokenKind::Comma) {
            if self.at(TokenKind::RParen) {
                return Err(ParseError::new(
                    "trailing commas are not in the subset",
                    self.peek().span,
                ));
            }
            args.push(self.parse_expr()?);
            max_ast = max_ast.max(self.ast);
        }
        let close = self.expect(&TokenKind::RParen, "expected ')' after arguments")?;
        self.ast = max_ast;
        Ok((args, close.span))
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Number(value) => {
                self.bump();
                self.ast = 1;
                Ok(Expr::new(ExprKind::Number(value), token.span))
            }
            TokenKind::String(value) => {
                self.bump();
                self.ast = 1;
                Ok(Expr::new(ExprKind::String(value), token.span))
            }
            TokenKind::True => {
                self.bump();
                self.ast = 1;
                Ok(Expr::new(ExprKind::Bool(true), token.span))
            }
            TokenKind::False => {
                self.bump();
                self.ast = 1;
                Ok(Expr::new(ExprKind::Bool(false), token.span))
            }
            TokenKind::Null => {
                self.bump();
                self.ast = 1;
                Ok(Expr::new(ExprKind::Null, token.span))
            }
            TokenKind::Ident(name) => {
                self.bump();
                self.ast = 1;
                Ok(Expr::new(ExprKind::Ident(name), token.span))
            }
            TokenKind::LParen => {
                self.bump();
                let inner = self.parse_expr()?;
                let close = self.expect(&TokenKind::RParen, "expected ')'")?;
                Ok(Expr::new(inner.kind, token.span.merge(close.span)))
            }
            TokenKind::Eof => Err(ParseError::new("unexpected end of expression", token.span)),
            other => Err(ParseError::new(
                format!("unexpected {}", other.describe()),
                token.span,
            )),
        }
    }

    fn expect_ident(&mut self, message: &str) -> Result<Token, ParseError> {
        let token = self.peek().clone();
        if matches!(token.kind, TokenKind::Ident(_)) {
            self.bump();
            Ok(token)
        } else {
            Err(ParseError::new(message, token.span))
        }
    }

    fn expect(&mut self, kind: &TokenKind, message: &str) -> Result<Token, ParseError> {
        let token = self.peek().clone();
        if self.at_kind(kind) {
            self.bump();
            Ok(token)
        } else {
            Err(ParseError::new(message, token.span))
        }
    }

    fn expect_eof(&mut self) -> Result<(), ParseError> {
        let token = self.peek();
        if matches!(token.kind, TokenKind::Eof) {
            Ok(())
        } else {
            Err(ParseError::new(
                format!("unexpected {} after expression", token.kind.describe()),
                token.span,
            ))
        }
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at_kind(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_span(&mut self, kind: &TokenKind) -> Option<Span> {
        if self.at_kind(kind) {
            let span = self.peek().span;
            self.bump();
            Some(span)
        } else {
            None
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.at_kind(&kind)
    }

    fn at_kind(&self, kind: &TokenKind) -> bool {
        &self.peek().kind == kind
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn bump(&mut self) {
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
    }
}
