#![allow(clippy::needless_return)]

use crate::ast::*;
use crate::token::{Span, Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

pub struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    pub fn new(mut tokens: Vec<Token>) -> Self {
        // Keep the parser total for callers that construct token streams
        // directly (for example editor integrations and fuzzers).  The
        // lexer normally supplies EOF, but a parser must not panic merely
        // because a hand-built stream omitted it or was empty.
        let needs_eof = !matches!(tokens.last().map(|token| &token.kind), Some(TokenKind::Eof));
        if needs_eof {
            let span = tokens.last().map(|token| token.span).unwrap_or_default();
            tokens.push(Token::new(TokenKind::Eof, span));
        }
        Self { tokens, cursor: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.cursor)
            .unwrap_or_else(|| self.tokens.last().expect("token stream must end with EOF"))
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn peek_next(&self) -> Option<&Token> {
        self.tokens.get(self.cursor + 1)
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        if self.cursor < self.tokens.len() - 1 {
            self.cursor += 1;
        }
        tok
    }

    fn check(&self, kind: &TokenKind) -> bool {
        self.peek_kind() == kind
    }

    fn match_tok(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<Token, ParseError> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            Err(ParseError {
                message: format!("expected {kind}, found {}", self.peek_kind()),
                span: self.peek().span,
            })
        }
    }

    fn skip_newlines(&mut self) {
        while self.match_tok(&TokenKind::Newline) {}
    }

    pub fn parse_module(&mut self) -> Result<Module, ParseError> {
        self.skip_newlines();
        let start_span = self.peek().span;
        let mut statements = Vec::new();

        while !self.check(&TokenKind::Eof) {
            let stmt = self.parse_statement()?;
            statements.push(stmt);
            self.skip_newlines();
        }

        let end_span = self.peek().span;
        Ok(Module {
            statements,
            span: start_span.merge(end_span),
        })
    }

    /// Parse as much of a module as possible, collecting statement errors.
    ///
    /// Recovery is deliberately line-oriented for now: after a failed
    /// statement we consume through the next newline and resume at the next
    /// statement boundary.  The strict `parse_module` API remains unchanged
    /// for compiler phases that require a complete AST.
    pub fn parse_module_recovering(&mut self) -> (Module, Vec<ParseError>) {
        self.skip_newlines();
        let start_span = self.peek().span;
        let mut statements = Vec::new();
        let mut errors = Vec::new();

        while !self.check(&TokenKind::Eof) {
            match self.parse_statement() {
                Ok(statement) => statements.push(statement),
                Err(error) => {
                    errors.push(error);
                    while !self.check(&TokenKind::Newline) && !self.check(&TokenKind::Eof) {
                        self.advance();
                    }
                }
            }
            self.skip_newlines();
        }

        let end_span = self.peek().span;
        (
            Module {
                statements,
                span: start_span.merge(end_span),
            },
            errors,
        )
    }

    pub fn parse_statement(&mut self) -> Result<Stmt, ParseError> {
        self.skip_newlines();
        let start = self.peek().span;

        // Check for export prefix
        if self.match_tok(&TokenKind::Export) {
            let inner = self.parse_statement()?;
            return Ok(Stmt::Export(Box::new(inner)));
        }

        // Decorators for functions/classes
        let mut decorators = Vec::new();
        while self.match_tok(&TokenKind::At) {
            let dec = self.parse_decorator_expr()?;
            self.consume_stmt_end()?;
            decorators.push(dec);
            self.skip_newlines();
        }

        match self.peek_kind() {
            TokenKind::Final
                if self
                    .peek_next()
                    .map(|t| t.kind != TokenKind::Class)
                    .unwrap_or(false) =>
            {
                self.advance();
                let pattern = self.parse_pattern()?;
                let type_annotation = if self.match_tok(&TokenKind::Colon) {
                    Some(self.parse_type_expr()?)
                } else {
                    None
                };
                self.expect(&TokenKind::Eq)?;
                let value = Some(self.parse_expr()?);
                self.consume_stmt_end()?;
                Ok(Stmt::VarDef {
                    pattern,
                    type_annotation,
                    value,
                    is_let: true,
                    is_final: true,
                    span: start,
                })
            }
            TokenKind::Class | TokenKind::Sealed | TokenKind::Final => self.parse_class_def(),
            TokenKind::Interface => self.parse_interface_def(),
            TokenKind::Trait => self.parse_trait_def(),
            TokenKind::Implement => self.parse_implement_def(),
            TokenKind::Dispatch => {
                self.advance();
                self.parse_function_def(true, decorators)
            }
            TokenKind::Def => self.parse_function_def(false, decorators),
            TokenKind::Async => {
                self.advance();
                self.expect(&TokenKind::Def)?;
                let mut function = self.parse_raw_function(false, decorators)?;
                function.is_async = true;
                Ok(Stmt::Function(function))
            }
            TokenKind::ContextManager => {
                let modifier_span = self.advance().span;
                let mut context_decorators = decorators;
                context_decorators.push(Expr::Ident {
                    name: "contextmanager".to_string(),
                    span: modifier_span,
                });
                self.parse_function_def(false, context_decorators)
            }
            TokenKind::Factory => {
                self.advance();
                self.match_tok(&TokenKind::Def);
                let func = self.parse_raw_function(false, decorators)?;
                Ok(Stmt::Function(func))
            }
            TokenKind::Type => self.parse_type_alias(),
            TokenKind::Let => self.parse_let_stmt(),
            TokenKind::With => self.parse_with_stmt(),
            TokenKind::If => self.parse_if_stmt(),
            TokenKind::For => self.parse_for_stmt(),
            TokenKind::While => self.parse_while_stmt(),
            TokenKind::Match => self.parse_match_stmt(),
            TokenKind::Try => self.parse_try_stmt(),
            TokenKind::Return => {
                self.advance();
                let value = if self.check(&TokenKind::Newline)
                    || self.check(&TokenKind::Eof)
                    || self.check(&TokenKind::Semi)
                {
                    None
                } else {
                    let first = self.parse_expr()?;
                    if self.match_tok(&TokenKind::Comma) {
                        let mut fields = vec![(None, first)];
                        while !self.check(&TokenKind::Newline)
                            && !self.check(&TokenKind::Semi)
                            && !self.check(&TokenKind::Eof)
                        {
                            fields.push((None, self.parse_expr()?));
                            if !self.match_tok(&TokenKind::Comma) {
                                break;
                            }
                        }
                        Some(Expr::Record {
                            fields,
                            span: start,
                        })
                    } else {
                        Some(first)
                    }
                };
                self.consume_stmt_end()?;
                Ok(Stmt::Return { value, span: start })
            }
            TokenKind::Raise => {
                self.advance();
                let exception = if self.check(&TokenKind::Newline)
                    || self.check(&TokenKind::Dedent)
                    || self.check(&TokenKind::Eof)
                {
                    Expr::Ident {
                        name: "__rethrow__".into(),
                        span: start,
                    }
                } else {
                    self.parse_expr()?
                };
                self.consume_stmt_end()?;
                Ok(Stmt::Raise {
                    exception,
                    span: start,
                })
            }
            TokenKind::Yield => {
                self.advance();
                let value = self.parse_expr()?;
                self.consume_stmt_end()?;
                Ok(Stmt::Yield { value, span: start })
            }
            TokenKind::Assert => {
                self.advance();
                if !self.match_tok(&TokenKind::LParen) {
                    return Err(ParseError {
                        message: "assert requires parentheses: use assert(condition) or assert(condition, message)"
                            .into(),
                        span: self.peek().span,
                    });
                }
                let condition = self.parse_expr()?;
                let message = if self.match_tok(&TokenKind::Comma) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(&TokenKind::RParen)?;
                self.consume_stmt_end()?;
                Ok(Stmt::Assert {
                    condition,
                    message,
                    span: start,
                })
            }
            TokenKind::Del => {
                self.advance();
                let mut names = vec![self.expect_ident()?];
                if self.check(&TokenKind::Dot) {
                    return Err(ParseError {
                        message: "fields are fixed, not deletable; use an explicit removal method instead"
                            .into(),
                        span: self.peek().span,
                    });
                }
                while self.match_tok(&TokenKind::Comma) {
                    names.push(self.expect_ident()?);
                    if self.check(&TokenKind::Dot) {
                        return Err(ParseError {
                            message: "fields are fixed, not deletable; use an explicit removal method instead"
                                .into(),
                            span: self.peek().span,
                        });
                    }
                }
                self.consume_stmt_end()?;
                Ok(Stmt::Delete { names, span: start })
            }
            TokenKind::Break => {
                self.advance();
                self.consume_stmt_end()?;
                Ok(Stmt::Break(start))
            }
            TokenKind::Continue => {
                self.advance();
                self.consume_stmt_end()?;
                Ok(Stmt::Continue(start))
            }
            TokenKind::Pass => {
                self.advance();
                self.consume_stmt_end()?;
                Ok(Stmt::Pass(start))
            }
            TokenKind::Import => self.parse_import_stmt(),
            TokenKind::From => self.parse_from_import_stmt(),
            _ => self.parse_expr_or_assign_stmt(),
        }
    }

    fn parse_with_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::With)?;
        let mut items = Vec::new();
        loop {
            let context_expr = self.parse_expr()?;
            let target = if self.match_tok(&TokenKind::As) {
                Some(self.parse_pattern()?)
            } else {
                None
            };
            items.push(WithItem {
                context_expr,
                target,
            });
            if !self.match_tok(&TokenKind::Comma) {
                break;
            }
        }
        let body = self.parse_block()?;
        Ok(Stmt::With {
            items,
            body,
            span: start,
        })
    }

    fn consume_stmt_end(&mut self) -> Result<(), ParseError> {
        if self.match_tok(&TokenKind::Newline)
            || self.match_tok(&TokenKind::Semi)
            || self.check(&TokenKind::Eof)
            || self.check(&TokenKind::Dedent)
        {
            Ok(())
        } else {
            Err(ParseError {
                message: format!(
                    "expected newline or semicolon at statement end, found {}",
                    self.peek_kind()
                ),
                span: self.peek().span,
            })
        }
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.expect(&TokenKind::Colon)?;
        if self.match_tok(&TokenKind::Newline) {
            self.expect(&TokenKind::Indent)?;

            let mut stmts = Vec::new();
            self.skip_newlines();

            while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                let stmt = self.parse_statement()?;
                stmts.push(stmt);
                self.skip_newlines();
            }

            self.expect(&TokenKind::Dedent)?;
            Ok(stmts)
        } else {
            let stmt = self.parse_statement()?;
            Ok(vec![stmt])
        }
    }

    fn parse_class_def(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        let mut is_sealed = false;
        let mut is_final = false;

        if self.match_tok(&TokenKind::Sealed) {
            is_sealed = true;
        } else if self.match_tok(&TokenKind::Final) {
            is_final = true;
        }

        self.expect(&TokenKind::Class)?;
        let name = self.expect_ident()?;
        let type_params = self.parse_optional_type_params()?;

        let mut bases = Vec::new();
        if self.match_tok(&TokenKind::LParen) {
            if !self.check(&TokenKind::RParen) {
                loop {
                    if matches!(self.peek_kind(), TokenKind::Ident(_))
                        && self
                            .peek_next()
                            .map(|t| t.kind == TokenKind::Eq)
                            .unwrap_or(false)
                    {
                        let keyword = self.expect_ident()?;
                        return Err(ParseError {
                            message: format!(
                                "keyword class base '{keyword}=' is not supported; Lucid has no metaclasses or programmable class headers"
                            ),
                            span: self.peek().span,
                        });
                    } else {
                        bases.push(self.parse_type_expr()?);
                    }
                    if !self.match_tok(&TokenKind::Comma) {
                        break;
                    }
                    if self.check(&TokenKind::RParen) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen)?;
        }

        let mut without_traits = Vec::new();
        if self.match_tok(&TokenKind::Without) {
            loop {
                without_traits.push(self.expect_ident()?);
                if !self.match_tok(&TokenKind::Comma) {
                    break;
                }
            }
        }

        self.expect(&TokenKind::Colon)?;
        let (body, end) = if self.match_tok(&TokenKind::Newline) {
            self.expect(&TokenKind::Indent)?;
            let mut body = Vec::new();
            self.skip_newlines();

            while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                body.push(self.parse_class_member()?);
                self.skip_newlines();
            }

            let end = self.expect(&TokenKind::Dedent)?.span;
            (body, end)
        } else {
            let member = self.parse_class_member()?;
            let end = self.peek().span;
            (vec![member], end)
        };

        Ok(Stmt::ClassDef {
            name,
            type_params,
            bases,
            without_traits,
            body,
            is_sealed,
            is_final,
            span: start.merge(end),
        })
    }

    fn parse_class_member(&mut self) -> Result<ClassMember, ParseError> {
        let start = self.peek().span;

        let mut decorators = Vec::new();
        while self.match_tok(&TokenKind::At) {
            let dec = self.parse_decorator_expr()?;
            self.consume_stmt_end()?;
            decorators.push(dec);
            self.skip_newlines();
        }

        let is_override = self.match_tok(&TokenKind::Override);
        let is_final = self.match_tok(&TokenKind::Final);
        let is_dispatch = self.match_tok(&TokenKind::Dispatch);
        let is_async = self.match_tok(&TokenKind::Async);

        if self.match_tok(&TokenKind::Pass) {
            let span = self.peek().span;
            self.consume_stmt_end()?;
            return Ok(ClassMember::Pass(span));
        }

        if self.match_tok(&TokenKind::Ellipsis) {
            let span = self.peek().span;
            self.consume_stmt_end()?;
            return Ok(ClassMember::Ellipsis(span));
        }

        if self.match_tok(&TokenKind::ClassVar) {
            let field_name = self.expect_ident()?;
            let type_annotation = if self.match_tok(&TokenKind::Colon) {
                self.parse_type_expr()?
            } else {
                TypeExpr::Named {
                    name: "object".into(),
                    args: Vec::new(),
                    span: start,
                }
            };
            let default = if self.match_tok(&TokenKind::Eq) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.consume_stmt_end()?;
            return Ok(ClassMember::ClassVar(FieldDef {
                name: field_name,
                type_annotation,
                default,
                is_final,
                doc: None,
                span: start,
            }));
        }

        if self.match_tok(&TokenKind::Type) {
            let name = self.expect_ident()?;
            let type_params = self.parse_optional_type_params()?;
            self.expect(&TokenKind::Eq)?;
            let val = self.parse_type_expr()?;
            self.consume_stmt_end()?;
            return Ok(ClassMember::TypeAlias {
                name,
                type_params,
                value: TypeAliasValue::Direct(val),
                span: start,
            });
        }

        if self.match_tok(&TokenKind::Factory) {
            let name = self.expect_ident()?;
            let type_params = self.parse_optional_type_params()?;
            let params = self.parse_param_list()?;
            let return_type = if self.match_tok(&TokenKind::Arrow) {
                Some(self.parse_type_expr()?)
            } else {
                None
            };
            let body = self.parse_block()?;
            return Ok(ClassMember::Factory(FactoryDef {
                name,
                type_params,
                params,
                return_type,
                body,
                span: start,
            }));
        }

        if self.match_tok(&TokenKind::Getter) {
            let name = self.expect_ident()?;
            self.expect(&TokenKind::LParen)?;
            self.expect_ident()?; // self
            self.expect(&TokenKind::RParen)?;
            let return_type = if self.match_tok(&TokenKind::Arrow) {
                Some(self.parse_type_expr()?)
            } else {
                None
            };
            let body = self.parse_block()?;
            return Ok(ClassMember::Getter(GetterDef {
                name,
                return_type,
                body,
                span: start,
            }));
        }

        if self.match_tok(&TokenKind::Setter) {
            let name = self.expect_ident()?;
            self.expect(&TokenKind::LParen)?;
            self.expect_ident()?; // self
            self.expect(&TokenKind::Comma)?;
            let param_name = self.expect_ident()?;
            let type_annotation = if self.match_tok(&TokenKind::Colon) {
                Some(self.parse_type_expr()?)
            } else {
                None
            };
            self.expect(&TokenKind::RParen)?;
            let body = self.parse_block()?;
            return Ok(ClassMember::Setter(SetterDef {
                name,
                param: Param {
                    name: param_name,
                    pattern: None,
                    type_annotation,
                    default: None,
                    is_positional_only: false,
                    is_keyword_only: false,
                    is_variadic_positional: false,
                    is_variadic_keyword: false,
                    is_gather: false,
                    span: start,
                },
                body,
                span: start,
            }));
        }

        if self.match_tok(&TokenKind::ClassMethod) {
            let mut func = self.parse_raw_function(false, decorators)?;
            func.is_async = is_async;
            func.is_override = is_override;
            return Ok(ClassMember::ClassMethod(func));
        }

        if self.match_tok(&TokenKind::ContextManager) {
            let modifier_span = start;
            let mut context_decorators = decorators.clone();
            context_decorators.push(Expr::Ident {
                name: "contextmanager".to_string(),
                span: modifier_span,
            });
            if self.match_tok(&TokenKind::ClassMethod) {
                let mut func = self.parse_raw_function(false, context_decorators)?;
                func.is_async = is_async;
                func.is_override = is_override;
                return Ok(ClassMember::ClassMethod(func));
            }
            self.expect(&TokenKind::Def)?;
            let mut func = self.parse_raw_function(is_dispatch, context_decorators)?;
            func.is_async = is_async;
            return Ok(ClassMember::Method(func));
        }

        if self.match_tok(&TokenKind::Def) {
            let member_is_dispatch = is_dispatch || self.match_tok(&TokenKind::Dispatch);
            let mut func = self.parse_raw_function(member_is_dispatch, decorators)?;
            func.is_async = is_async;
            func.is_override = is_override;
            return Ok(ClassMember::Method(func));
        }

        // Field definition: name: Type = default
        let field_name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        let type_annotation = self.parse_type_expr()?;
        let default = if self.match_tok(&TokenKind::Eq) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let doc = if self.match_tok(&TokenKind::Colon) {
            self.expect(&TokenKind::Newline)?;
            self.expect(&TokenKind::Indent)?;
            let mut doc_str = None;
            while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                let doc_expr = self.parse_expr()?;
                if let Expr::Literal {
                    value: LiteralValue::Str(s),
                    ..
                } = doc_expr
                    && doc_str.is_none()
                {
                    doc_str = Some(s);
                }
                self.consume_stmt_end()?;
                self.skip_newlines();
            }
            self.expect(&TokenKind::Dedent)?;
            doc_str
        } else {
            None
        };
        if doc.is_none() {
            self.consume_stmt_end()?;
        }

        Ok(ClassMember::Field(FieldDef {
            name: field_name,
            type_annotation,
            default,
            is_final,
            doc,
            span: start,
        }))
    }

    fn parse_interface_def(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::Interface)?;
        let name = self.expect_ident()?;
        let type_params = self.parse_optional_type_params()?;

        let mut bases = Vec::new();
        if self.match_tok(&TokenKind::LParen) {
            if !self.check(&TokenKind::RParen) {
                loop {
                    bases.push(self.parse_type_expr()?);
                    if !self.match_tok(&TokenKind::Comma) {
                        break;
                    }
                    if self.check(&TokenKind::RParen) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen)?;
        }

        self.expect(&TokenKind::Colon)?;
        self.expect(&TokenKind::Newline)?;
        self.expect(&TokenKind::Indent)?;

        let mut body = Vec::new();
        self.skip_newlines();

        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
            body.push(self.parse_interface_member()?);
            self.skip_newlines();
        }

        let end = self.expect(&TokenKind::Dedent)?.span;
        Ok(Stmt::InterfaceDef {
            name,
            type_params,
            bases,
            body,
            span: start.merge(end),
        })
    }

    fn parse_interface_member(&mut self) -> Result<InterfaceMember, ParseError> {
        let start = self.peek().span;

        if self.match_tok(&TokenKind::Pass) {
            let span = self.peek().span;
            self.consume_stmt_end()?;
            return Ok(InterfaceMember::Pass(span));
        }

        if self.match_tok(&TokenKind::Ellipsis) {
            let span = self.peek().span;
            self.consume_stmt_end()?;
            return Ok(InterfaceMember::Ellipsis(span));
        }

        if self.match_tok(&TokenKind::Getter) {
            let name = self.expect_ident()?;
            self.expect(&TokenKind::LParen)?;
            self.expect_ident()?; // self
            self.expect(&TokenKind::RParen)?;
            let return_type = if self.match_tok(&TokenKind::Arrow) {
                Some(self.parse_type_expr()?)
            } else {
                None
            };
            self.consume_stmt_end()?;
            return Ok(InterfaceMember::GetterSig {
                name,
                return_type,
                span: start,
            });
        }

        if self.match_tok(&TokenKind::Setter) {
            let name = self.expect_ident()?;
            self.expect(&TokenKind::LParen)?;
            self.expect_ident()?; // self
            self.expect(&TokenKind::Comma)?;
            self.expect_ident()?; // val
            self.expect(&TokenKind::Colon)?;
            let param_type = self.parse_type_expr()?;
            self.expect(&TokenKind::RParen)?;
            self.consume_stmt_end()?;
            return Ok(InterfaceMember::SetterSig {
                name,
                param_type,
                span: start,
            });
        }

        if self.match_tok(&TokenKind::ClassMethod) {
            self.match_tok(&TokenKind::Def);
            let name = self.expect_ident()?;
            let type_params = self.parse_optional_type_params()?;
            let params = self.parse_param_list()?;
            let return_type = if self.match_tok(&TokenKind::Arrow) {
                Some(self.parse_type_expr()?)
            } else {
                None
            };
            self.consume_stmt_end()?;
            return Ok(InterfaceMember::ClassMethodSig {
                name,
                type_params,
                params,
                return_type,
                span: start,
            });
        }

        if self.match_tok(&TokenKind::Factory) {
            let name = self.expect_ident()?;
            let type_params = self.parse_optional_type_params()?;
            let params = self.parse_param_list()?;
            let return_type = if self.match_tok(&TokenKind::Arrow) {
                Some(self.parse_type_expr()?)
            } else {
                None
            };
            self.consume_stmt_end()?;
            return Ok(InterfaceMember::FactorySig {
                name,
                type_params,
                params,
                return_type,
                span: start,
            });
        }

        if self.match_tok(&TokenKind::Type) {
            let name = self.expect_ident()?;
            let bound = if self.match_tok(&TokenKind::Colon) {
                Some(self.parse_type_expr()?)
            } else {
                None
            };
            self.consume_stmt_end()?;
            return Ok(InterfaceMember::AssociatedTypeSig {
                name,
                bound,
                span: start,
            });
        }

        if self.match_tok(&TokenKind::Final) {
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let type_annotation = self.parse_type_expr()?;
            self.consume_stmt_end()?;
            return Ok(InterfaceMember::FieldSig {
                name,
                type_annotation,
                is_final: true,
                span: start,
            });
        }

        if matches!(self.peek_kind(), TokenKind::Ident(_))
            && self
                .peek_next()
                .map(|t| t.kind == TokenKind::Colon)
                .unwrap_or(false)
        {
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let type_annotation = self.parse_type_expr()?;
            self.consume_stmt_end()?;
            return Ok(InterfaceMember::FieldSig {
                name,
                type_annotation,
                is_final: false,
                span: start,
            });
        }

        if self.match_tok(&TokenKind::Dispatch) {
            self.match_tok(&TokenKind::Def);
        } else {
            self.expect(&TokenKind::Def)?;
            self.match_tok(&TokenKind::Dispatch);
        }

        let name = self.expect_ident()?;
        let type_params = self.parse_optional_type_params()?;
        let params = self.parse_param_list()?;
        let return_type = if self.match_tok(&TokenKind::Arrow) {
            Some(self.parse_type_expr()?)
        } else {
            None
        };
        if self.match_tok(&TokenKind::Colon) && !self.match_tok(&TokenKind::Ellipsis) {
            self.match_tok(&TokenKind::Pass);
        }
        self.consume_stmt_end()?;
        Ok(InterfaceMember::MethodSig {
            name,
            type_params,
            params,
            return_type,
            span: start,
        })
    }

    fn parse_trait_def(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::Trait)?;
        let name = self.expect_ident()?;
        let type_params = self.parse_optional_type_params()?;

        let mut bases = Vec::new();
        if self.match_tok(&TokenKind::LParen) {
            if !self.check(&TokenKind::RParen) {
                loop {
                    bases.push(self.parse_type_expr()?);
                    if !self.match_tok(&TokenKind::Comma) {
                        break;
                    }
                    if self.check(&TokenKind::RParen) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen)?;
        }

        self.expect(&TokenKind::Colon)?;
        self.expect(&TokenKind::Newline)?;
        self.expect(&TokenKind::Indent)?;

        let mut body = Vec::new();
        self.skip_newlines();

        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
            if self.match_tok(&TokenKind::Pass) {
                let span = self.peek().span;
                self.consume_stmt_end()?;
                body.push(TraitMember::Pass(span));
            } else if self.match_tok(&TokenKind::Ellipsis) {
                let span = self.peek().span;
                self.consume_stmt_end()?;
                body.push(TraitMember::Ellipsis(span));
            } else if self.match_tok(&TokenKind::Getter) {
                let g_start = self.peek().span;
                let name = self.expect_ident()?;
                self.expect(&TokenKind::LParen)?;
                self.expect_ident()?; // self
                self.expect(&TokenKind::RParen)?;
                let return_type = if self.match_tok(&TokenKind::Arrow) {
                    Some(self.parse_type_expr()?)
                } else {
                    None
                };
                let b = if self.check(&TokenKind::Colon) {
                    self.parse_block()?
                } else {
                    self.consume_stmt_end()?;
                    Vec::new()
                };
                body.push(TraitMember::Getter(GetterDef {
                    name,
                    return_type,
                    body: b,
                    span: g_start,
                }));
            } else if self.match_tok(&TokenKind::Setter) {
                let s_start = self.peek().span;
                let setter_name = self.expect_ident()?;
                self.expect(&TokenKind::LParen)?;
                self.expect_ident()?;
                self.expect(&TokenKind::Comma)?;
                let param_name = self.expect_ident()?;
                self.expect(&TokenKind::Colon)?;
                let param_type = self.parse_type_expr()?;
                self.expect(&TokenKind::RParen)?;
                let setter_body = if self.check(&TokenKind::Colon) {
                    self.parse_block()?
                } else {
                    self.consume_stmt_end()?;
                    Vec::new()
                };
                body.push(TraitMember::Setter(SetterDef {
                    name: setter_name,
                    param: Param {
                        name: param_name,
                        pattern: None,
                        type_annotation: Some(param_type),
                        default: None,
                        is_positional_only: false,
                        is_keyword_only: false,
                        is_variadic_positional: false,
                        is_variadic_keyword: false,
                        is_gather: false,
                        span: s_start,
                    },
                    body: setter_body,
                    span: s_start,
                }));
            } else if self.match_tok(&TokenKind::ClassMethod) {
                self.match_tok(&TokenKind::Def);
                let func = self.parse_raw_function(false, Vec::new())?;
                body.push(TraitMember::ClassMethod(func));
            } else if self.check(&TokenKind::Final) || self.check(&TokenKind::ClassVar) {
                let is_final = self.match_tok(&TokenKind::Final);
                self.match_tok(&TokenKind::ClassVar);
                let f_start = self.peek().span;
                let field_name = self.expect_ident()?;
                self.expect(&TokenKind::Colon)?;
                let type_annotation = self.parse_type_expr()?;
                let default = if self.match_tok(&TokenKind::Eq) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.consume_stmt_end()?;
                body.push(TraitMember::Field(FieldDef {
                    name: field_name,
                    type_annotation,
                    default,
                    is_final,
                    doc: None,
                    span: f_start,
                }));
            } else if matches!(self.peek_kind(), TokenKind::Ident(_))
                && self
                    .peek_next()
                    .map(|token| token.kind == TokenKind::Colon)
                    .unwrap_or(false)
            {
                let f_start = self.peek().span;
                let field_name = self.expect_ident()?;
                self.expect(&TokenKind::Colon)?;
                let type_annotation = self.parse_type_expr()?;
                self.consume_stmt_end()?;
                body.push(TraitMember::Field(FieldDef {
                    name: field_name,
                    type_annotation,
                    default: None,
                    is_final: false,
                    doc: None,
                    span: f_start,
                }));
            } else {
                self.expect(&TokenKind::Def)?;
                let is_dispatch = self.match_tok(&TokenKind::Dispatch);
                let func = self.parse_raw_function(is_dispatch, Vec::new())?;
                body.push(TraitMember::Method(func));
            }
            self.skip_newlines();
        }

        let end = self.expect(&TokenKind::Dedent)?.span;
        Ok(Stmt::TraitDef {
            name,
            type_params,
            bases,
            body,
            span: start.merge(end),
        })
    }

    fn parse_implement_def(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::Implement)?;
        let interface = self.parse_type_expr()?;
        self.expect(&TokenKind::For)?;
        let target = self.parse_type_expr()?;

        self.expect(&TokenKind::Colon)?;
        self.expect(&TokenKind::Newline)?;
        self.expect(&TokenKind::Indent)?;

        let mut body = Vec::new();
        self.skip_newlines();

        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
            if self.match_tok(&TokenKind::ClassMethod) {
                self.match_tok(&TokenKind::Def);
                let func = self.parse_raw_function(false, Vec::new())?;
                body.push(func);
            } else {
                self.expect(&TokenKind::Def)?;
                let func = self.parse_raw_function(false, Vec::new())?;
                body.push(func);
            }
            self.skip_newlines();
        }

        let end = self.expect(&TokenKind::Dedent)?.span;
        Ok(Stmt::ImplementDef {
            interface,
            target,
            body,
            span: start.merge(end),
        })
    }

    fn parse_type_alias(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::Type)?;
        let name = self.expect_ident()?;
        let type_params = self.parse_optional_type_params()?;
        self.expect(&TokenKind::Eq)?;
        self.skip_newlines();
        let multiline = self.match_tok(&TokenKind::Indent);
        if multiline {
            let has_conditional = self.tokens[self.cursor..]
                .iter()
                .any(|token| matches!(token.kind, TokenKind::If));
            if has_conditional {
                let start_span = self.peek().span;
                while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                    self.advance();
                }
                self.match_tok(&TokenKind::Dedent);
                return Ok(Stmt::TypeAlias {
                    name,
                    type_params,
                    value: TypeAliasValue::Direct(TypeExpr::Wildcard(start_span)),
                    span: start,
                });
            }
        }

        // Check for match type: type promote[A, B] = match A, B:
        if self.match_tok(&TokenKind::Match) {
            let mut subjects = Vec::new();
            loop {
                subjects.push(self.parse_type_expr()?);
                if !self.match_tok(&TokenKind::Comma) {
                    break;
                }
            }

            self.expect(&TokenKind::Colon)?;
            self.expect(&TokenKind::Newline)?;
            self.expect(&TokenKind::Indent)?;

            let mut arms = Vec::new();
            self.skip_newlines();

            while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                self.expect(&TokenKind::Case)?;
                let pattern_type = self.parse_type_expr()?;
                self.expect(&TokenKind::Colon)?;
                let result_type = self.parse_type_expr()?;
                let is_nested_match = matches!(result_type, TypeExpr::Match { .. });
                arms.push((pattern_type, result_type));
                if !is_nested_match {
                    self.consume_stmt_end()?;
                }
                self.skip_newlines();
            }

            self.expect(&TokenKind::Dedent)?;
            return Ok(Stmt::TypeAlias {
                name,
                type_params,
                value: TypeAliasValue::Match {
                    subject: subjects,
                    arms,
                },
                span: start,
            });
        }

        let value_type = self.parse_type_expr()?;
        self.consume_stmt_end()?;

        Ok(Stmt::TypeAlias {
            name,
            type_params,
            value: TypeAliasValue::Direct(value_type),
            span: start,
        })
    }

    fn parse_function_def(
        &mut self,
        mut is_dispatch: bool,
        decorators: Vec<Expr>,
    ) -> Result<Stmt, ParseError> {
        self.expect(&TokenKind::Def)?;
        if self.match_tok(&TokenKind::Dispatch) {
            is_dispatch = true;
        }
        let func = self.parse_raw_function(is_dispatch, decorators)?;
        Ok(Stmt::Function(func))
    }

    fn parse_decorator_expr(&mut self) -> Result<Expr, ParseError> {
        if self.check(&TokenKind::ClassMethod) {
            return Err(ParseError {
                message: "classmethod is not supported as a decorator; use the classmethod member modifier instead".into(),
                span: self.peek().span,
            });
        }

        self.parse_expr()
    }

    fn parse_raw_function(
        &mut self,
        is_dispatch: bool,
        decorators: Vec<Expr>,
    ) -> Result<FunctionDef, ParseError> {
        let start = self.peek().span;
        let name = if is_dispatch {
            match self.peek_kind() {
                TokenKind::Plus => {
                    self.advance();
                    "+".to_string()
                }
                TokenKind::Minus => {
                    self.advance();
                    "-".to_string()
                }
                TokenKind::Star => {
                    self.advance();
                    "*".to_string()
                }
                TokenKind::Slash => {
                    self.advance();
                    "/".to_string()
                }
                TokenKind::Percent => {
                    self.advance();
                    "%".to_string()
                }
                TokenKind::EqEq => {
                    self.advance();
                    "==".to_string()
                }
                _ => self.expect_ident()?,
            }
        } else {
            self.expect_ident()?
        };
        let type_params = self.parse_optional_type_params()?;
        let params = self.parse_param_list()?;

        let return_type = if self.match_tok(&TokenKind::Arrow) {
            Some(self.parse_type_expr()?)
        } else {
            None
        };

        let body = if self.check(&TokenKind::Colon) {
            self.parse_block()?
        } else {
            self.consume_stmt_end()?;
            Vec::new()
        };

        Ok(FunctionDef {
            name,
            type_params,
            params,
            return_type,
            body,
            is_dispatch,
            is_async: false,
            is_override: false,
            decorators,
            span: start,
        })
    }

    fn parse_let_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::Let)?;
        let pattern = self.parse_pattern()?;

        let type_annotation = if self.match_tok(&TokenKind::Colon) {
            Some(self.parse_type_expr()?)
        } else {
            None
        };

        self.expect(&TokenKind::Eq)?;
        let value = Some(self.parse_expr()?);
        self.consume_stmt_end()?;

        Ok(Stmt::VarDef {
            pattern,
            type_annotation,
            value,
            is_let: true,
            is_final: false,
            span: start,
        })
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::If)?;
        let condition = self.parse_expr()?;
        let then_branch = self.parse_block()?;

        let mut elif_branches = Vec::new();
        while self.match_tok(&TokenKind::Elif) {
            let elif_cond = self.parse_expr()?;
            let elif_block = self.parse_block()?;
            elif_branches.push((elif_cond, elif_block));
        }

        let else_branch = if self.match_tok(&TokenKind::Else) {
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch,
            span: start,
        })
    }

    fn parse_comp_target(&mut self) -> Result<Pattern, ParseError> {
        let start = self.peek().span;
        let mut target = self.parse_pattern()?;
        if self.match_tok(&TokenKind::Comma) {
            let mut elements = vec![target];
            loop {
                elements.push(self.parse_pattern()?);
                if !self.match_tok(&TokenKind::Comma) {
                    break;
                }
                if self.check(&TokenKind::In) {
                    break;
                }
            }
            target = Pattern::Tuple(elements, start);
        }
        Ok(target)
    }

    fn parse_for_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::For)?;
        let target = self.parse_comp_target()?;
        self.expect(&TokenKind::In)?;
        let iterable = self.parse_expr()?;
        let body = self.parse_block()?;

        let if_broken = if self.match_tok(&TokenKind::IfBroken) {
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Stmt::For {
            target,
            iterable,
            body,
            if_broken,
            span: start,
        })
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::While)?;
        let condition = self.parse_expr()?;
        let body = self.parse_block()?;

        let if_broken = if self.match_tok(&TokenKind::IfBroken) {
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Stmt::While {
            condition,
            body,
            if_broken,
            span: start,
        })
    }

    fn parse_match_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::Match)?;
        let subject = self.parse_expr()?;

        let subject_alias = if self.match_tok(&TokenKind::As) {
            Some(self.expect_ident()?)
        } else {
            None
        };

        self.expect(&TokenKind::Colon)?;
        self.expect(&TokenKind::Newline)?;
        self.expect(&TokenKind::Indent)?;

        let mut arms = Vec::new();
        self.skip_newlines();

        while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
            let arm_start = self.peek().span;
            self.expect(&TokenKind::Case)?;

            // In Lucid, match arm pattern can be a Type pattern or Destructure pattern
            let pattern = self.parse_pattern()?;
            let guard = if self.match_tok(&TokenKind::If) {
                Some(self.parse_expr()?)
            } else {
                None
            };

            let body = self.parse_block()?;
            arms.push(MatchArm {
                pattern,
                type_narrow: None,
                guard,
                body,
                span: arm_start,
            });
            self.skip_newlines();
        }

        self.expect(&TokenKind::Dedent)?;
        Ok(Stmt::Match {
            subject,
            subject_alias,
            arms,
            span: start,
        })
    }

    fn parse_try_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::Try)?;
        let body = self.parse_block()?;

        let mut handlers = Vec::new();
        while self.match_tok(&TokenKind::Except) {
            let h_start = self.peek().span;
            let exception_type = if self.check(&TokenKind::Colon) {
                TypeExpr::Named {
                    name: "Any".into(),
                    args: Vec::new(),
                    span: h_start,
                }
            } else {
                self.parse_type_expr()?
            };
            let name = if self.match_tok(&TokenKind::As) {
                Some(self.expect_ident()?)
            } else {
                None
            };
            let h_body = self.parse_block()?;
            handlers.push(ExceptHandler {
                exception_type,
                name,
                body: h_body,
                span: h_start,
            });
        }

        let finally_body = if self.match_tok(&TokenKind::Finally) {
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Stmt::Try {
            body,
            handlers,
            finally_body,
            span: start,
        })
    }

    fn parse_import_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::Import)?;
        let mut module = String::new();
        while self.match_tok(&TokenKind::Dot) {
            module.push('.');
        }
        module.push_str(&self.parse_dotted_name()?);
        let alias = if self.match_tok(&TokenKind::As) {
            Some(self.expect_ident()?)
        } else {
            None
        };
        self.consume_stmt_end()?;
        Ok(Stmt::Import {
            module,
            alias,
            span: start,
        })
    }

    fn parse_from_import_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(&TokenKind::From)?;

        let mut module = String::new();
        while self.match_tok(&TokenKind::Dot) {
            module.push('.');
        }
        module.push_str(&self.parse_dotted_name()?);

        let is_export = self.match_tok(&TokenKind::Export);
        if !is_export {
            self.expect(&TokenKind::Import)?;
        }

        let mut names = Vec::new();
        loop {
            let name = self.expect_name()?;
            let alias = if self.match_tok(&TokenKind::As) {
                Some(self.expect_ident()?)
            } else {
                None
            };
            names.push((name, alias));
            if !self.match_tok(&TokenKind::Comma) {
                break;
            }
        }

        self.consume_stmt_end()?;
        Ok(Stmt::FromImport {
            module,
            names,
            is_export,
            span: start,
        })
    }

    fn parse_expr_or_assign_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        let expr = self.parse_expr()?;

        if self.match_tok(&TokenKind::Colon) {
            let type_annotation = self.parse_type_expr()?;
            let value = if self.match_tok(&TokenKind::Eq) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.consume_stmt_end()?;

            // Convert expression to pattern if valid
            let pattern = match expr {
                Expr::Ident { name, span } => Pattern::Ident(name, span),
                _ => {
                    return Err(ParseError {
                        message: "invalid variable definition target".to_string(),
                        span: expr.span(),
                    });
                }
            };

            return Ok(Stmt::VarDef {
                pattern,
                type_annotation: Some(type_annotation),
                value,
                is_let: false,
                is_final: false,
                span: start,
            });
        }

        let mut target_expr = expr;
        if self.match_tok(&TokenKind::Comma) {
            let mut fields = vec![(None, target_expr)];
            loop {
                let e = self.parse_expr()?;
                fields.push((None, e));
                if !self.match_tok(&TokenKind::Comma) {
                    break;
                }
                if self.check(&TokenKind::Eq)
                    || self.check(&TokenKind::Newline)
                    || self.check(&TokenKind::Semi)
                    || self.check(&TokenKind::Eof)
                {
                    break;
                }
            }
            target_expr = Expr::Record {
                fields,
                span: start,
            };
        }

        if self.match_tok(&TokenKind::Eq) {
            let value = self.parse_expr()?;
            self.consume_stmt_end()?;
            return Ok(Stmt::Assignment {
                target: target_expr,
                value,
                span: start,
            });
        }

        // Augmented assignment
        let aug_op = match self.peek_kind() {
            TokenKind::PlusEq => {
                self.advance();
                Some(BinaryOp::Add)
            }
            TokenKind::MinusEq => {
                self.advance();
                Some(BinaryOp::Sub)
            }
            TokenKind::StarEq => {
                self.advance();
                Some(BinaryOp::Mul)
            }
            TokenKind::SlashEq => {
                self.advance();
                Some(BinaryOp::Div)
            }
            TokenKind::DoubleSlashEq => {
                self.advance();
                Some(BinaryOp::FloorDiv)
            }
            TokenKind::PercentEq => {
                self.advance();
                Some(BinaryOp::Mod)
            }
            TokenKind::DoubleStarEq => {
                self.advance();
                Some(BinaryOp::Pow)
            }
            _ => None,
        };

        if let Some(op) = aug_op {
            let value = self.parse_expr()?;
            self.consume_stmt_end()?;
            return Ok(Stmt::AugAssign {
                target: target_expr,
                op,
                value,
                span: start,
            });
        }

        self.consume_stmt_end()?;
        Ok(Stmt::Expr(target_expr))
    }

    // --- Expression Parsing ---
    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_logical_or()?;
        if self.match_tok(&TokenKind::If) {
            let condition = self.parse_logical_or()?;
            self.expect(&TokenKind::Else)?;
            let otherwise = self.parse_expr()?;
            let span = expr.span().merge(otherwise.span());
            return Ok(Expr::IfExpr {
                condition: Box::new(condition),
                then_branch: Box::new(expr),
                else_branch: Box::new(otherwise),
                span,
            });
        }
        Ok(expr)
    }

    fn parse_logical_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_logical_and()?;
        while self.match_tok(&TokenKind::Or) {
            let right = self.parse_logical_and()?;
            let span = left.span().merge(right.span());
            left = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_logical_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_comparison()?;
        while self.match_tok(&TokenKind::And) {
            let right = self.parse_comparison()?;
            let span = left.span().merge(right.span());
            left = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_or()?;

        let op = match self.peek_kind() {
            TokenKind::EqEq => Some(BinaryOp::Eq),
            TokenKind::TripleEq => Some(BinaryOp::Identity),
            TokenKind::NotEq => Some(BinaryOp::NotEq),
            TokenKind::TripleNotEq => Some(BinaryOp::NotIdentity),
            TokenKind::Lt => Some(BinaryOp::Lt),
            TokenKind::LtEq => Some(BinaryOp::LtEq),
            TokenKind::Gt => Some(BinaryOp::Gt),
            TokenKind::GtEq => Some(BinaryOp::GtEq),
            TokenKind::In => Some(BinaryOp::In),
            TokenKind::Is => {
                if self
                    .peek_next()
                    .map(|t| t.kind == TokenKind::Not)
                    .unwrap_or(false)
                {
                    self.advance();
                    self.advance();
                    return Ok(Expr::Binary {
                        op: BinaryOp::IsNot,
                        left: Box::new(left.clone()),
                        right: Box::new(self.parse_bitwise_or()?),
                        span: left.span(),
                    });
                }
                Some(BinaryOp::Is)
            }
            TokenKind::Not
                if self
                    .peek_next()
                    .map(|t| t.kind == TokenKind::In)
                    .unwrap_or(false) =>
            {
                self.advance();
                self.advance();
                return Ok(Expr::Binary {
                    op: BinaryOp::NotIn,
                    left: Box::new(left.clone()),
                    right: Box::new(self.parse_bitwise_or()?),
                    span: left.span(),
                });
            }
            _ => None,
        };

        if let Some(op) = op {
            self.advance();
            let right = self.parse_bitwise_or()?;
            let span = left.span().merge(right.span());
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_bitwise_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_xor()?;
        while self.match_tok(&TokenKind::Pipe) {
            let right = self.parse_bitwise_xor()?;
            let span = left.span().merge(right.span());
            left = Expr::Binary {
                op: BinaryOp::BitOr,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_bitwise_xor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_and()?;
        while self.match_tok(&TokenKind::Caret) {
            let right = self.parse_bitwise_and()?;
            let span = left.span().merge(right.span());
            left = Expr::Binary {
                op: BinaryOp::BitXor,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_bitwise_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_shift()?;
        while self.match_tok(&TokenKind::Amp) {
            let right = self.parse_shift()?;
            let span = left.span().merge(right.span());
            left = Expr::Binary {
                op: BinaryOp::BitAnd,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_term()?;
        while self.check(&TokenKind::Shl) || self.check(&TokenKind::Shr) {
            let op = if self.match_tok(&TokenKind::Shl) {
                BinaryOp::Shl
            } else {
                self.advance();
                BinaryOp::Shr
            };
            let right = self.parse_term()?;
            let span = left.span().merge(right.span());
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_factor()?;
        while self.check(&TokenKind::Plus) || self.check(&TokenKind::Minus) {
            let op = if self.match_tok(&TokenKind::Plus) {
                BinaryOp::Add
            } else {
                self.advance();
                BinaryOp::Sub
            };
            let right = self.parse_factor()?;
            let span = left.span().merge(right.span());
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_power()?;
        while self.check(&TokenKind::Star)
            || self.check(&TokenKind::Slash)
            || self.check(&TokenKind::DoubleSlash)
            || self.check(&TokenKind::Percent)
        {
            let op = if self.match_tok(&TokenKind::Star) {
                BinaryOp::Mul
            } else if self.match_tok(&TokenKind::Slash) {
                BinaryOp::Div
            } else if self.match_tok(&TokenKind::DoubleSlash) {
                BinaryOp::FloorDiv
            } else {
                self.advance();
                BinaryOp::Mod
            };
            let right = self.parse_power()?;
            let span = left.span().merge(right.span());
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_power(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_unary()?;
        if self.match_tok(&TokenKind::DoubleStar) {
            let right = self.parse_power()?;
            let span = left.span().merge(right.span());
            return Ok(Expr::Binary {
                op: BinaryOp::Pow,
                left: Box::new(left),
                right: Box::new(right),
                span,
            });
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek().span;
        if self.match_tok(&TokenKind::Await) {
            let inner = self.parse_unary()?;
            let span = start.merge(inner.span());
            return Ok(Expr::Await {
                expr: Box::new(inner),
                span,
            });
        }
        if self.match_tok(&TokenKind::Not) {
            let inner = self.parse_unary()?;
            let span = start.merge(inner.span());
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(inner),
                span,
            });
        }
        if self.match_tok(&TokenKind::Minus) {
            let inner = self.parse_unary()?;
            let span = start.merge(inner.span());
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(inner),
                span,
            });
        }
        if self.match_tok(&TokenKind::Plus) {
            let inner = self.parse_unary()?;
            let span = start.merge(inner.span());
            return Ok(Expr::Unary {
                op: UnaryOp::Pos,
                expr: Box::new(inner),
                span,
            });
        }
        if self.match_tok(&TokenKind::Tilde) {
            let inner = self.parse_unary()?;
            let span = start.merge(inner.span());
            return Ok(Expr::Unary {
                op: UnaryOp::Invert,
                expr: Box::new(inner),
                span,
            });
        }
        if self.match_tok(&TokenKind::Bang) {
            let inner = self.parse_unary()?;
            let span = start.merge(inner.span());
            return Ok(Expr::Freeze {
                expr: Box::new(inner),
                span,
            });
        }
        if self.match_tok(&TokenKind::TripleStar) {
            let inner = self.parse_unary()?;
            let span = start.merge(inner.span());
            return Ok(Expr::Unary {
                op: UnaryOp::GatherSpread,
                expr: Box::new(inner),
                span,
            });
        }
        if self.match_tok(&TokenKind::Star) {
            let inner = self.parse_unary()?;
            let span = start.merge(inner.span());
            return Ok(Expr::Unary {
                op: UnaryOp::Spread,
                expr: Box::new(inner),
                span,
            });
        }

        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            // Function call: expr(args)
            if self.match_tok(&TokenKind::LParen) {
                let mut args = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        let is_gather_spread = self.match_tok(&TokenKind::TripleStar);
                        let is_dict_spread =
                            !is_gather_spread && self.match_tok(&TokenKind::DoubleStar);
                        let mut is_spread = !is_gather_spread
                            && !is_dict_spread
                            && self.match_tok(&TokenKind::Star);

                        let arg_start = self.peek().span;

                        // Check for named argument: name = val
                        let (name, mut val) = if !is_gather_spread
                            && !is_dict_spread
                            && !is_spread
                            && self
                                .peek_next()
                                .map(|t| t.kind == TokenKind::Eq)
                                .unwrap_or(false)
                        {
                            let n = self.expect_ident()?;
                            self.advance(); // consume '='
                            let v = self.parse_expr()?;
                            (Some(n), v)
                        } else {
                            (None, self.parse_expr()?)
                        };

                        if self.match_tok(&TokenKind::For) {
                            // A bare comprehension in a call is Lucid's
                            // generator-call shorthand: its produced values
                            // expand into positional arguments.  Bracketed
                            // comprehensions remain ordinary list values.
                            is_spread = true;
                            let target = self.parse_comp_target()?;
                            self.expect(&TokenKind::In)?;
                            let iter = self.parse_expr()?;
                            let condition = if self.match_tok(&TokenKind::If) {
                                Some(Box::new(self.parse_expr()?))
                            } else {
                                None
                            };
                            val = Expr::ListComp {
                                element: Box::new(val),
                                target,
                                iter: Box::new(iter),
                                condition,
                                span: arg_start,
                            };
                        }

                        args.push(Arg {
                            name,
                            value: val,
                            is_spread,
                            is_dict_spread,
                            is_gather_spread,
                            span: arg_start,
                        });

                        if !self.match_tok(&TokenKind::Comma) {
                            break;
                        }
                        if self.check(&TokenKind::RParen) {
                            break;
                        }
                    }
                }
                let end = self.expect(&TokenKind::RParen)?.span;
                let span = expr.span().merge(end);
                expr = Expr::Call {
                    func: Box::new(expr),
                    args,
                    span,
                };
                continue;
            }

            // Indexing, Multi-indexing, Slicing, or Generic specialization: expr[idx, ...]
            if self.match_tok(&TokenKind::LBracket) {
                let first = self.parse_slice_or_expr()?;
                let mut indices = vec![first];
                while self.match_tok(&TokenKind::Comma) {
                    if self.check(&TokenKind::RBracket) {
                        break;
                    }
                    indices.push(self.parse_slice_or_expr()?);
                }
                let end = self.expect(&TokenKind::RBracket)?.span;
                let span = expr.span().merge(end);
                let index = if indices.len() == 1 {
                    indices.pop().unwrap()
                } else {
                    Expr::Record {
                        fields: indices.into_iter().map(|e| (None, e)).collect(),
                        span,
                    }
                };
                expr = Expr::Index {
                    value: Box::new(expr),
                    index: Box::new(index),
                    span,
                };
                continue;
            }

            // Attribute access: expr.attr
            if self.match_tok(&TokenKind::Dot) {
                let attr = self.expect_ident()?;
                let end = self.peek().span;
                let span = expr.span().merge(end);
                expr = Expr::Attribute {
                    value: Box::new(expr),
                    attr,
                    span,
                };
                continue;
            }

            // Error propagation: expr?
            if self.match_tok(&TokenKind::Question) {
                let span = expr.span();
                expr = Expr::Propagate {
                    expr: Box::new(expr),
                    span,
                };
                continue;
            }

            break;
        }

        Ok(expr)
    }

    fn parse_slice_or_expr(&mut self) -> Result<Expr, ParseError> {
        let start_span = self.peek().span;
        // Case: [:stop] or [:]
        if self.match_tok(&TokenKind::Colon) {
            let stop = if !self.check(&TokenKind::Colon)
                && !self.check(&TokenKind::RBracket)
                && !self.check(&TokenKind::Comma)
            {
                Some(Box::new(self.parse_expr()?))
            } else {
                None
            };
            let step = if self.match_tok(&TokenKind::Colon) {
                if !self.check(&TokenKind::RBracket) && !self.check(&TokenKind::Comma) {
                    Some(Box::new(self.parse_expr()?))
                } else {
                    None
                }
            } else {
                None
            };
            let end_span = self.peek().span;
            return Ok(Expr::Slice {
                start: None,
                stop,
                step,
                span: start_span.merge(end_span),
            });
        }

        let expr = self.parse_expr()?;
        // Case: [start:stop:step] or [start:]
        if self.match_tok(&TokenKind::Colon) {
            let stop = if !self.check(&TokenKind::Colon)
                && !self.check(&TokenKind::RBracket)
                && !self.check(&TokenKind::Comma)
            {
                Some(Box::new(self.parse_expr()?))
            } else {
                None
            };
            let step = if self.match_tok(&TokenKind::Colon) {
                if !self.check(&TokenKind::RBracket) && !self.check(&TokenKind::Comma) {
                    Some(Box::new(self.parse_expr()?))
                } else {
                    None
                }
            } else {
                None
            };
            let span = expr.span().merge(self.peek().span);
            return Ok(Expr::Slice {
                start: Some(Box::new(expr)),
                stop,
                step,
                span,
            });
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let tok = self.peek().clone();

        // Byte literals use Python's lexical b"..." spelling. The current
        // runtime represents byte payloads as immutable strings, so preserve
        // the payload while accepting the dedicated syntax.
        if let TokenKind::Ident(prefix) = &tok.kind
            && prefix == "b"
            && let Some(next) = self.peek_next().cloned()
            && let TokenKind::Str(value) = next.kind
        {
            let value = value.clone();
            self.advance();
            self.advance();
            return Ok(Expr::Literal {
                value: LiteralValue::Str(value),
                span: tok.span.merge(next.span),
            });
        }

        match &tok.kind {
            TokenKind::Ellipsis => {
                self.advance();
                Ok(Expr::Literal {
                    value: LiteralValue::Ellipsis,
                    span: tok.span,
                })
            }
            TokenKind::Int(n) => {
                self.advance();
                Ok(Expr::Literal {
                    value: LiteralValue::Int(*n),
                    span: tok.span,
                })
            }
            TokenKind::BigInt(n) => {
                self.advance();
                Ok(Expr::Literal {
                    value: LiteralValue::BigInt(n.clone()),
                    span: tok.span,
                })
            }
            TokenKind::Float(f) => {
                self.advance();
                Ok(Expr::Literal {
                    value: LiteralValue::Float(*f),
                    span: tok.span,
                })
            }
            TokenKind::Complex(f) => {
                self.advance();
                Ok(Expr::Literal {
                    value: LiteralValue::Complex(*f),
                    span: tok.span,
                })
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Expr::Literal {
                    value: LiteralValue::Str(s.clone()),
                    span: tok.span,
                })
            }
            TokenKind::Bytes(s) => {
                self.advance();
                Ok(Expr::Literal {
                    value: LiteralValue::Bytes(s.as_bytes().to_vec()),
                    span: tok.span,
                })
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::Literal {
                    value: LiteralValue::Bool(true),
                    span: tok.span,
                })
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::Literal {
                    value: LiteralValue::Bool(false),
                    span: tok.span,
                })
            }
            TokenKind::None => {
                self.advance();
                Ok(Expr::Literal {
                    value: LiteralValue::None,
                    span: tok.span,
                })
            }
            TokenKind::Skip => {
                self.advance();
                Ok(Expr::Skip(tok.span))
            }
            TokenKind::Construct => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let mut args = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        let val = self.parse_expr()?;
                        args.push(Arg {
                            name: None,
                            value: val,
                            is_spread: false,
                            is_dict_spread: false,
                            is_gather_spread: false,
                            span: tok.span,
                        });
                        if !self.match_tok(&TokenKind::Comma) {
                            break;
                        }
                        if self.check(&TokenKind::RParen) {
                            break;
                        }
                    }
                }
                let end = self.expect(&TokenKind::RParen)?.span;
                Ok(Expr::Construct {
                    args,
                    span: tok.span.merge(end),
                })
            }
            TokenKind::Trust => {
                self.advance();
                self.expect(&TokenKind::LBracket)?;
                let target_type = self.parse_type_expr()?;
                self.expect(&TokenKind::RBracket)?;
                self.expect(&TokenKind::LParen)?;
                let inner = self.parse_expr()?;
                let end = self.expect(&TokenKind::RParen)?.span;
                Ok(Expr::Trust {
                    target_type,
                    expr: Box::new(inner),
                    span: tok.span.merge(end),
                })
            }
            TokenKind::Def => {
                // Anonymous def: def(a: int) -> int: ... or def(a, b): expr or def: expr
                self.advance();
                let (params, return_type) = if self.check(&TokenKind::Colon) {
                    (Vec::new(), None)
                } else {
                    let params = self.parse_param_list()?;
                    let return_type = if self.match_tok(&TokenKind::Arrow) {
                        Some(self.parse_type_expr()?)
                    } else {
                        None
                    };
                    (params, return_type)
                };

                self.expect(&TokenKind::Colon)?;
                // Can be single expression on same line or indented block
                let body = if self.match_tok(&TokenKind::Newline) {
                    self.expect(&TokenKind::Indent)?;
                    let mut stmts = Vec::new();
                    self.skip_newlines();
                    while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                        stmts.push(self.parse_statement()?);
                        self.skip_newlines();
                    }
                    self.expect(&TokenKind::Dedent)?;
                    stmts
                } else {
                    let expr = self.parse_expr()?;
                    vec![Stmt::Return {
                        value: Some(expr),
                        span: tok.span,
                    }]
                };

                Ok(Expr::AnonymousDef {
                    params,
                    return_type,
                    body,
                    span: tok.span,
                })
            }
            TokenKind::FromVarName => {
                let span = tok.span;
                self.advance();
                Ok(Expr::Ident {
                    name: "from_var_name".to_string(),
                    span,
                })
            }
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();
                Ok(Expr::Ident {
                    name,
                    span: tok.span,
                })
            }
            TokenKind::Any => {
                self.advance();
                Ok(Expr::Ident {
                    name: "any".into(),
                    span: tok.span,
                })
            }
            TokenKind::ContextManager => {
                self.advance();
                Ok(Expr::Ident {
                    name: "contextmanager".into(),
                    span: tok.span,
                })
            }
            TokenKind::Type => {
                if self
                    .peek_next()
                    .map(|token| token.kind == TokenKind::LParen)
                    .unwrap_or(false)
                {
                    return Err(ParseError {
                        message: "type() is not supported; use class[X] for type annotations and `is` for instance checks".into(),
                        span: tok.span,
                    });
                }
                let next_is_type_expr = matches!(
                    self.peek_next().map(|t| &t.kind),
                    Some(TokenKind::Ident(_))
                        | Some(TokenKind::Bang)
                        | Some(TokenKind::Amp)
                        | Some(TokenKind::Tilde)
                        | Some(TokenKind::LBracket)
                        | Some(TokenKind::LParen)
                        | Some(TokenKind::LBrace)
                        | Some(TokenKind::Type)
                        | Some(TokenKind::Any)
                );
                if next_is_type_expr {
                    self.advance();
                    let t = self.parse_type_expr()?;
                    return Ok(Expr::Type(t));
                }
                self.advance();
                Ok(Expr::Ident {
                    name: "type".to_string(),
                    span: tok.span,
                })
            }
            TokenKind::Caller => {
                self.advance();
                Ok(Expr::Ident {
                    name: "caller".to_string(),
                    span: tok.span,
                })
            }
            TokenKind::Class | TokenKind::Trait | TokenKind::Interface => {
                let name = match tok.kind {
                    TokenKind::Class => "class",
                    TokenKind::Trait => "trait",
                    _ => "interface",
                };
                self.advance();
                Ok(Expr::Type(TypeExpr::Named {
                    name: name.into(),
                    args: Vec::new(),
                    span: tok.span,
                }))
            }
            TokenKind::Factory => {
                self.advance();
                Ok(Expr::Ident {
                    name: "factory".to_string(),
                    span: tok.span,
                })
            }
            TokenKind::LParen => {
                let mut is_type_shape = false;
                let mut depth = 0;
                let mut i = self.cursor;
                while i < self.tokens.len() {
                    let k = &self.tokens[i].kind;
                    if *k == TokenKind::LParen {
                        depth += 1;
                    } else if *k == TokenKind::RParen {
                        depth -= 1;
                        if depth == 0 {
                            if self
                                .tokens
                                .get(i + 1)
                                .map(|t| t.kind == TokenKind::Arrow)
                                .unwrap_or(false)
                            {
                                is_type_shape = true;
                            }
                            break;
                        }
                    } else if depth == 1 {
                        if *k == TokenKind::Colon {
                            is_type_shape = true;
                        }
                        if *k == TokenKind::Slash
                            && self
                                .tokens
                                .get(i + 1)
                                .map(|t| t.kind == TokenKind::Comma || t.kind == TokenKind::RParen)
                                .unwrap_or(false)
                        {
                            is_type_shape = true;
                        }
                        if *k == TokenKind::Star
                            && self
                                .tokens
                                .get(i + 1)
                                .map(|t| t.kind == TokenKind::Comma)
                                .unwrap_or(false)
                        {
                            is_type_shape = true;
                        }
                    }
                    i += 1;
                }

                if is_type_shape {
                    let t = self.parse_type_expr()?;
                    return Ok(Expr::Type(t));
                }

                self.advance();
                if self.match_tok(&TokenKind::RParen) {
                    // Empty tuple / record ()
                    return Ok(Expr::Record {
                        fields: Vec::new(),
                        span: tok.span,
                    });
                }

                // Check if it's an anonymous record: (x=1, y=2)
                let mut is_record = false;
                if self
                    .peek_next()
                    .map(|t| t.kind == TokenKind::Eq)
                    .unwrap_or(false)
                {
                    is_record = true;
                }

                if is_record {
                    let mut fields = Vec::new();
                    loop {
                        let field_name = self.expect_ident()?;
                        self.expect(&TokenKind::Eq)?;
                        let val = self.parse_expr()?;
                        fields.push((Some(field_name), val));
                        if !self.match_tok(&TokenKind::Comma) {
                            break;
                        }
                        if self.check(&TokenKind::RParen) {
                            break;
                        }
                    }
                    let end = self.expect(&TokenKind::RParen)?.span;
                    return Ok(Expr::Record {
                        fields,
                        span: tok.span.merge(end),
                    });
                }

                let first_expr = self.parse_expr()?;
                if self.match_tok(&TokenKind::For) {
                    let target = self.parse_comp_target()?;
                    self.expect(&TokenKind::In)?;
                    let iter = self.parse_logical_or()?;
                    let condition = if self.match_tok(&TokenKind::If) {
                        Some(Box::new(self.parse_logical_or()?))
                    } else {
                        None
                    };
                    let end = self.expect(&TokenKind::RParen)?.span;
                    return Ok(Expr::ListComp {
                        element: Box::new(first_expr),
                        target,
                        iter: Box::new(iter),
                        condition,
                        span: tok.span.merge(end),
                    });
                }

                if self.match_tok(&TokenKind::Comma) {
                    // Positional record / tuple: (a, b)
                    let mut fields = vec![(None, first_expr)];
                    while !self.check(&TokenKind::RParen) {
                        let expr = self.parse_expr()?;
                        fields.push((None, expr));
                        if !self.match_tok(&TokenKind::Comma) {
                            break;
                        }
                    }
                    let end = self.expect(&TokenKind::RParen)?.span;
                    if self.match_tok(&TokenKind::Arrow) {
                        let ret = self.parse_type_expr()?;
                        let span = tok.span.merge(ret.span());
                        let params = fields
                            .into_iter()
                            .map(|(_, e)| match e {
                                Expr::Ident { name, span } => TypeExpr::Named {
                                    name,
                                    args: Vec::new(),
                                    span,
                                },
                                Expr::Type(t) => t,
                                other => TypeExpr::Named {
                                    name: format!("{other:?}"),
                                    args: Vec::new(),
                                    span: other.span(),
                                },
                            })
                            .collect();
                        return Ok(Expr::Type(TypeExpr::Function {
                            params,
                            return_type: Box::new(ret),
                            span,
                        }));
                    }
                    return Ok(Expr::Record {
                        fields,
                        span: tok.span.merge(end),
                    });
                }

                self.expect(&TokenKind::RParen)?;
                if self.match_tok(&TokenKind::Arrow) {
                    let ret = self.parse_type_expr()?;
                    let span = tok.span.merge(ret.span());
                    let param = match first_expr {
                        Expr::Ident { name, span } => TypeExpr::Named {
                            name,
                            args: Vec::new(),
                            span,
                        },
                        Expr::Type(t) => t,
                        other => TypeExpr::Named {
                            name: format!("{other:?}"),
                            args: Vec::new(),
                            span: other.span(),
                        },
                    };
                    return Ok(Expr::Type(TypeExpr::Function {
                        params: vec![param],
                        return_type: Box::new(ret),
                        span,
                    }));
                }
                Ok(first_expr)
            }
            TokenKind::LBracket => {
                self.advance();
                if self.match_tok(&TokenKind::RBracket) {
                    return Ok(Expr::List {
                        elements: Vec::new(),
                        span: tok.span,
                    });
                }
                let first = self.parse_expr()?;
                if self.match_tok(&TokenKind::For) {
                    let target = self.parse_comp_target()?;
                    self.expect(&TokenKind::In)?;
                    let iter = self.parse_logical_or()?;
                    let condition = if self.match_tok(&TokenKind::If) {
                        Some(Box::new(self.parse_logical_or()?))
                    } else {
                        None
                    };
                    let end = self.expect(&TokenKind::RBracket)?.span;
                    return Ok(Expr::ListComp {
                        element: Box::new(first),
                        target,
                        iter: Box::new(iter),
                        condition,
                        span: tok.span.merge(end),
                    });
                }
                let mut elements = vec![first];
                while self.match_tok(&TokenKind::Comma) {
                    if self.check(&TokenKind::RBracket) {
                        break;
                    }
                    elements.push(self.parse_expr()?);
                }
                let end = self.expect(&TokenKind::RBracket)?.span;
                Ok(Expr::List {
                    elements,
                    span: tok.span.merge(end),
                })
            }
            TokenKind::LBrace => {
                self.advance();
                if self.match_tok(&TokenKind::Colon) {
                    // Empty dictionary: {:}
                    let end = self.expect(&TokenKind::RBrace)?.span;
                    return Ok(Expr::Dict {
                        entries: Vec::new(),
                        span: tok.span.merge(end),
                    });
                }
                if self.match_tok(&TokenKind::RBrace) {
                    // Empty set {}
                    return Ok(Expr::Set {
                        elements: Vec::new(),
                        span: tok.span,
                    });
                }

                if self.match_tok(&TokenKind::Ellipsis) {
                    if self.match_tok(&TokenKind::RBrace) {
                        return Ok(Expr::Dict {
                            entries: Vec::new(),
                            span: tok.span,
                        });
                    }
                    self.match_tok(&TokenKind::Comma);
                }

                let first = self.parse_expr()?;
                if self.match_tok(&TokenKind::Colon) {
                    let first_val = self.parse_expr()?;
                    if self.match_tok(&TokenKind::For) {
                        let target = self.parse_comp_target()?;
                        self.expect(&TokenKind::In)?;
                        let iter = self.parse_logical_or()?;
                        let condition = if self.match_tok(&TokenKind::If) {
                            Some(Box::new(self.parse_logical_or()?))
                        } else {
                            None
                        };
                        let end = self.expect(&TokenKind::RBrace)?.span;
                        return Ok(Expr::DictComp {
                            key: Box::new(first),
                            value: Box::new(first_val),
                            target,
                            iter: Box::new(iter),
                            condition,
                            span: tok.span.merge(end),
                        });
                    }
                    let mut entries = vec![(first, first_val)];
                    while self.match_tok(&TokenKind::Comma) {
                        if self.check(&TokenKind::RBrace) {
                            break;
                        }
                        if self.match_tok(&TokenKind::Ellipsis) {
                            continue;
                        }
                        let k = self.parse_expr()?;
                        self.expect(&TokenKind::Colon)?;
                        let v = self.parse_expr()?;
                        entries.push((k, v));
                    }
                    let end = self.expect(&TokenKind::RBrace)?.span;
                    return Ok(Expr::Dict {
                        entries,
                        span: tok.span.merge(end),
                    });
                } else if self.match_tok(&TokenKind::For) {
                    let target = self.parse_comp_target()?;
                    self.expect(&TokenKind::In)?;
                    let iter = self.parse_logical_or()?;
                    let condition = if self.match_tok(&TokenKind::If) {
                        Some(Box::new(self.parse_logical_or()?))
                    } else {
                        None
                    };
                    let end = self.expect(&TokenKind::RBrace)?.span;
                    return Ok(Expr::SetComp {
                        element: Box::new(first),
                        target,
                        iter: Box::new(iter),
                        condition,
                        span: tok.span.merge(end),
                    });
                } else {
                    let mut elements = vec![first];
                    while self.match_tok(&TokenKind::Comma) {
                        if self.check(&TokenKind::RBrace) {
                            break;
                        }
                        elements.push(self.parse_expr()?);
                    }
                    let end = self.expect(&TokenKind::RBrace)?.span;
                    return Ok(Expr::Set {
                        elements,
                        span: tok.span.merge(end),
                    });
                }
            }
            _ => Err(ParseError {
                message: format!("unexpected token in expression: {}", tok.kind),
                span: tok.span,
            }),
        }
    }

    // --- Type Expression Parsing ---
    pub fn parse_type_expr(&mut self) -> Result<TypeExpr, ParseError> {
        self.skip_newlines();
        self.match_tok(&TokenKind::Indent);
        let mut left = self.parse_type_primary()?;

        // Union types: A | B
        while self.match_tok(&TokenKind::Pipe) {
            let right = self.parse_type_primary()?;
            let span = left.span().merge(right.span());
            left = match left {
                TypeExpr::Union { mut types, span: _ } => {
                    types.push(right);
                    TypeExpr::Union { types, span }
                }
                _ => TypeExpr::Union {
                    types: vec![left, right],
                    span,
                },
            };
        }

        while self.match_tok(&TokenKind::Amp) {
            let right = self.parse_type_primary()?;
            let span = left.span().merge(right.span());
            left = match left {
                TypeExpr::Named {
                    name,
                    mut args,
                    span: _,
                } if name == "__intersection__" => {
                    args.push(right);
                    TypeExpr::Named { name, args, span }
                }
                _ => TypeExpr::Named {
                    name: "__intersection__".to_string(),
                    args: vec![left, right],
                    span,
                },
            };
        }

        while self.check(&TokenKind::Plus)
            || self.check(&TokenKind::Minus)
            || self.check(&TokenKind::Star)
        {
            let op = self.peek().kind.clone();
            self.advance();
            let right = self.parse_type_primary()?;
            let span = left.span().merge(right.span());
            let opname = match op {
                TokenKind::Plus => "__shape_add__",
                TokenKind::Minus => "__shape_sub__",
                _ => "__shape_mul__",
            };
            left = TypeExpr::Named {
                name: opname.to_string(),
                args: vec![left, right],
                span,
            };
        }

        // Type-level conditionals use the same spelling as expression
        // conditionals. Keep the condition opaque for the eventual shape
        // evaluator, but retain both result branches in the type tree.
        if self.match_tok(&TokenKind::If) {
            let mut depth = 0usize;
            while !self.check(&TokenKind::Eof) {
                if depth == 0 && self.check(&TokenKind::Else) {
                    break;
                }
                match self.peek_kind() {
                    TokenKind::LBracket | TokenKind::LParen => depth += 1,
                    TokenKind::RBracket | TokenKind::RParen if depth > 0 => depth -= 1,
                    _ => {}
                }
                self.advance();
            }
            self.expect(&TokenKind::Else)?;
            let else_branch = self.parse_type_expr()?;
            let span = left.span().merge(else_branch.span());
            left = TypeExpr::Named {
                name: "__shape_cond__".into(),
                args: vec![left, else_branch],
                span,
            };
        }

        // Function types: (A, B) -> R or P -> R
        if self.match_tok(&TokenKind::Arrow) {
            let return_type = self.parse_type_expr()?;
            let span = left.span().merge(return_type.span());
            let params = match left {
                TypeExpr::Record { fields, .. } => {
                    fields.into_iter().map(|f| f.type_expr).collect()
                }
                other => vec![other],
            };
            return Ok(TypeExpr::Function {
                params,
                return_type: Box::new(return_type),
                span,
            });
        }

        Ok(left)
    }

    fn parse_type_primary(&mut self) -> Result<TypeExpr, ParseError> {
        let tok = self.peek().clone();

        if self.match_tok(&TokenKind::Final) {
            return self.parse_type_primary();
        }

        if self.match_tok(&TokenKind::Minus) {
            let inner = self.parse_type_primary()?;
            let span = tok.span.merge(inner.span());
            return Ok(TypeExpr::Named {
                name: "__shape_neg__".to_string(),
                args: vec![inner],
                span,
            });
        }

        if self.match_tok(&TokenKind::Not) {
            let inner = self.parse_type_primary()?;
            let span = tok.span.merge(inner.span());
            return Ok(TypeExpr::Named {
                name: "__not__".to_string(),
                args: vec![inner],
                span,
            });
        }

        // Mutability views: !T (immutable), ~T (read-only view). `&T` remains
        // accepted as a compatibility spelling for older documents.
        if self.match_tok(&TokenKind::Bang) {
            let inner = self.parse_type_primary()?;
            let span = tok.span.merge(inner.span());
            return Ok(TypeExpr::View {
                mutability: MutabilityView::Immutable,
                inner: Box::new(inner),
                span,
            });
        }
        if self.match_tok(&TokenKind::Amp) {
            let inner = self.parse_type_primary()?;
            let span = tok.span.merge(inner.span());
            return Ok(TypeExpr::View {
                mutability: MutabilityView::ReadOnly,
                inner: Box::new(inner),
                span,
            });
        }
        if self.match_tok(&TokenKind::Tilde) {
            let inner = self.parse_type_primary()?;
            let span = tok.span.merge(inner.span());
            return Ok(TypeExpr::View {
                mutability: MutabilityView::ReadOnly,
                inner: Box::new(inner),
                span,
            });
        }

        // any Interface
        if self.match_tok(&TokenKind::Any) {
            let interface = self.parse_type_primary()?;
            let span = tok.span.merge(interface.span());
            return Ok(TypeExpr::Existential {
                interface: Box::new(interface),
                span,
            });
        }

        // Match type expression: match Subject: case Pat: Type ...
        if self.match_tok(&TokenKind::Match) {
            let mut subjects = Vec::new();
            loop {
                subjects.push(self.parse_type_expr()?);
                if !self.match_tok(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::Colon)?;
            self.expect(&TokenKind::Newline)?;
            self.expect(&TokenKind::Indent)?;

            let mut arms = Vec::new();
            self.skip_newlines();

            while !self.check(&TokenKind::Dedent) && !self.check(&TokenKind::Eof) {
                self.expect(&TokenKind::Case)?;
                let pattern_type = self.parse_type_expr()?;
                self.expect(&TokenKind::Colon)?;
                let result_type = self.parse_type_expr()?;
                let is_nested_match = matches!(result_type, TypeExpr::Match { .. });
                arms.push((pattern_type, result_type));
                if !is_nested_match {
                    self.consume_stmt_end()?;
                }
                self.skip_newlines();
            }

            let end = self.expect(&TokenKind::Dedent)?.span;
            return Ok(TypeExpr::Match {
                subject: subjects,
                arms,
                span: tok.span.merge(end),
            });
        }

        // Wildcard: _
        if self.match_tok(&TokenKind::Ident("_".to_string())) {
            return Ok(TypeExpr::Wildcard(tok.span));
        }

        // Parentheses: (A, B) -> R or (x: int, y: int)
        if self.match_tok(&TokenKind::LParen) {
            if self.match_tok(&TokenKind::RParen) {
                // Empty tuple type ()
                return Ok(TypeExpr::Record {
                    fields: Vec::new(),
                    span: tok.span,
                });
            }

            let mut fields: Vec<RecordFieldType> = Vec::new();
            let mut is_positional_only = false;
            let mut is_keyword_only = false;

            while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                if self.match_tok(&TokenKind::Slash) {
                    for field in &mut fields {
                        field.is_positional_only = true;
                    }
                    is_positional_only = false;
                    self.match_tok(&TokenKind::Comma);
                    continue;
                }
                if self.match_tok(&TokenKind::Star) {
                    is_keyword_only = true;
                    self.match_tok(&TokenKind::Comma);
                    continue;
                }
                if self.match_tok(&TokenKind::Ellipsis) {
                    if let Some(last) = fields.last_mut() {
                        if is_keyword_only || last.name.as_deref() == Some("_") {
                            last.is_variadic_keyword = true;
                        } else {
                            last.is_variadic_positional = true;
                        }
                    }
                    self.match_tok(&TokenKind::Comma);
                    continue;
                }

                // Check for field name: name: Type
                let name = if self
                    .peek_next()
                    .map(|t| t.kind == TokenKind::Colon)
                    .unwrap_or(false)
                {
                    let n = self.expect_ident()?;
                    self.advance(); // consume ':'
                    Some(n)
                } else {
                    None
                };

                let t = self.parse_type_expr()?;
                let is_variadic_positional = self.match_tok(&TokenKind::Ellipsis);

                fields.push(RecordFieldType {
                    name,
                    type_expr: t,
                    is_positional_only,
                    is_keyword_only,
                    is_variadic_positional,
                    is_variadic_keyword: false,
                });

                if !self.match_tok(&TokenKind::Comma) {
                    break;
                }
            }

            let end = self.expect(&TokenKind::RParen)?.span;
            return Ok(TypeExpr::Record {
                fields,
                span: tok.span.merge(end),
            });
        }

        if let TokenKind::Str(s) = &tok.kind {
            let s = s.clone();
            self.advance();
            return Ok(TypeExpr::Literal {
                value: LiteralValue::Str(s),
                span: tok.span,
            });
        }
        if let TokenKind::Bytes(s) = &tok.kind {
            let s = s.clone();
            self.advance();
            return Ok(TypeExpr::Literal {
                value: LiteralValue::Bytes(s.as_bytes().to_vec()),
                span: tok.span,
            });
        }
        if let TokenKind::Int(n) = &tok.kind {
            let n = *n;
            self.advance();
            return Ok(TypeExpr::Literal {
                value: LiteralValue::Int(n),
                span: tok.span,
            });
        }
        if let TokenKind::BigInt(n) = &tok.kind {
            let n = n.clone();
            self.advance();
            return Ok(TypeExpr::Literal {
                value: LiteralValue::BigInt(n),
                span: tok.span,
            });
        }
        if let TokenKind::Float(value) = &tok.kind {
            let value = *value;
            self.advance();
            return Ok(TypeExpr::Literal {
                value: LiteralValue::Float(value),
                span: tok.span,
            });
        }
        if matches!(tok.kind, TokenKind::True | TokenKind::False) {
            let value = matches!(tok.kind, TokenKind::True);
            self.advance();
            return Ok(TypeExpr::Literal {
                value: LiteralValue::Bool(value),
                span: tok.span,
            });
        }

        // Dict / TypedDict shape: {"a": int, "b": str}
        if self.match_tok(&TokenKind::LBrace) {
            if self.match_tok(&TokenKind::Colon) {
                let end = self.expect(&TokenKind::RBrace)?.span;
                return Ok(TypeExpr::Named {
                    name: "dict".to_string(),
                    args: Vec::new(),
                    span: tok.span.merge(end),
                });
            }
            if self.match_tok(&TokenKind::RBrace) {
                return Ok(TypeExpr::Named {
                    name: "dict".to_string(),
                    args: Vec::new(),
                    span: tok.span,
                });
            }
            let mut entries = Vec::new();
            while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
                if self.match_tok(&TokenKind::Ellipsis) {
                    self.match_tok(&TokenKind::Comma);
                    continue;
                }
                let k = self.parse_type_expr()?;
                self.expect(&TokenKind::Colon)?;
                let v = self.parse_type_expr()?;
                entries.push((k, v));
                if !self.match_tok(&TokenKind::Comma) {
                    break;
                }
            }
            let end = self.expect(&TokenKind::RBrace)?.span;
            let (k_types, v_types): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
            let k_union = if k_types.is_empty() {
                TypeExpr::Wildcard(tok.span)
            } else {
                TypeExpr::Union {
                    types: k_types,
                    span: tok.span,
                }
            };
            let v_union = if v_types.is_empty() {
                TypeExpr::Wildcard(tok.span)
            } else {
                TypeExpr::Union {
                    types: v_types,
                    span: tok.span,
                }
            };
            return Ok(TypeExpr::Named {
                name: "dict".to_string(),
                args: vec![k_union, v_union],
                span: tok.span.merge(end),
            });
        }

        // Nominal type: Name[T1, T2] or type[T] or dotted path like iteration.done
        if self.check(&TokenKind::Type)
            && matches!(
                self.peek_next().map(|token| &token.kind),
                Some(TokenKind::Ident(_))
            )
        {
            self.advance();
            let inner = self.parse_type_primary()?;
            let span = tok.span.merge(inner.span());
            return Ok(TypeExpr::Named {
                name: "type".into(),
                args: vec![inner],
                span,
            });
        }
        let name_opt = match &tok.kind {
            TokenKind::Ident(name) => Some(name.clone()),
            TokenKind::Type => Some("type".to_string()),
            TokenKind::Class => Some("class".to_string()),
            TokenKind::Trait => Some("trait".to_string()),
            TokenKind::Interface => Some("interface".to_string()),
            _ => None,
        };
        if let Some(mut name) = name_opt {
            self.advance();

            while self.match_tok(&TokenKind::Dot) {
                let member = self.expect_ident()?;
                name.push('.');
                name.push_str(&member);
            }

            let mut args = Vec::new();
            let mut slice_spec: Option<(TypeExpr, TypeExpr, TypeExpr)> = None;
            if self.match_tok(&TokenKind::LBracket) {
                if !self.check(&TokenKind::RBracket) {
                    loop {
                        if self.match_tok(&TokenKind::Star) {
                            args.push(self.parse_type_expr()?);
                            if !self.match_tok(&TokenKind::Comma) {
                                break;
                            }
                            continue;
                        }
                        // Shape slicing/indexing (for example S[:-2]) is a
                        // type-level operation. Preserve all three bounds so
                        // the checker can evaluate it instead of discarding it.
                        if self.match_tok(&TokenKind::Colon) {
                            let stop = if self.check(&TokenKind::RBracket)
                                || self.check(&TokenKind::Colon)
                            {
                                TypeExpr::Wildcard(tok.span)
                            } else {
                                self.parse_type_expr()?
                            };
                            let step = if self.match_tok(&TokenKind::Colon) {
                                if self.check(&TokenKind::RBracket) {
                                    TypeExpr::Wildcard(tok.span)
                                } else {
                                    self.parse_type_expr()?
                                }
                            } else {
                                TypeExpr::Wildcard(tok.span)
                            };
                            slice_spec = Some((TypeExpr::Wildcard(tok.span), stop, step));
                            break;
                        }
                        args.push(self.parse_type_expr()?);
                        if self.match_tok(&TokenKind::Colon) {
                            let start = args.pop().unwrap_or(TypeExpr::Wildcard(tok.span));
                            let stop = if self.check(&TokenKind::RBracket)
                                || self.check(&TokenKind::Colon)
                            {
                                TypeExpr::Wildcard(tok.span)
                            } else {
                                self.parse_type_expr()?
                            };
                            let step = if self.match_tok(&TokenKind::Colon) {
                                if self.check(&TokenKind::RBracket) {
                                    TypeExpr::Wildcard(tok.span)
                                } else {
                                    self.parse_type_expr()?
                                }
                            } else {
                                TypeExpr::Wildcard(tok.span)
                            };
                            slice_spec = Some((start, stop, step));
                            break;
                        }
                        if !self.match_tok(&TokenKind::Comma) {
                            break;
                        }
                        if self.check(&TokenKind::RBracket) {
                            break;
                        }
                    }
                }
                let end = self.expect(&TokenKind::RBracket)?.span;
                if let Some((start, stop, step)) = slice_spec {
                    let base = TypeExpr::Named {
                        name,
                        args: Vec::new(),
                        span: tok.span,
                    };
                    return Ok(TypeExpr::Named {
                        name: "__shape_slice__".into(),
                        args: vec![base, start, stop, step],
                        span: tok.span.merge(end),
                    });
                }
                let is_shape_index = args.len() == 1
                    && (matches!(
                        args.first(),
                        Some(TypeExpr::Literal {
                            value: LiteralValue::Int(_),
                            ..
                        })
                    ) || matches!(
                        args.first(),
                        Some(TypeExpr::Named { name, .. }) if name == "__shape_neg__"
                    ));
                if name != "shape" && name != "typing.shape" && is_shape_index {
                    let base = TypeExpr::Named {
                        name,
                        args: Vec::new(),
                        span: tok.span,
                    };
                    return Ok(TypeExpr::Named {
                        name: "__shape_index__".into(),
                        args: vec![base, args.remove(0)],
                        span: tok.span.merge(end),
                    });
                }
                return Ok(TypeExpr::Named {
                    name,
                    args,
                    span: tok.span.merge(end),
                });
            }

            return Ok(TypeExpr::Named {
                name,
                args: Vec::new(),
                span: tok.span,
            });
        }

        if tok.kind == TokenKind::None {
            self.advance();
            return Ok(TypeExpr::Named {
                name: "none".to_string(),
                args: Vec::new(),
                span: tok.span,
            });
        }

        Err(ParseError {
            message: format!("unexpected token in type expression: {}", tok.kind),
            span: tok.span,
        })
    }

    fn parse_optional_type_params(&mut self) -> Result<Vec<TypeParam>, ParseError> {
        if !self.match_tok(&TokenKind::LBracket) {
            return Ok(Vec::new());
        }

        let mut params = Vec::new();
        while !self.check(&TokenKind::RBracket) && !self.check(&TokenKind::Eof) {
            let start = self.peek().span;
            let variance = if self.match_tok(&TokenKind::Plus) || self.match_tok(&TokenKind::PlusEq)
            {
                Variance::Covariant
            } else if self.match_tok(&TokenKind::Minus) {
                Variance::Contravariant
            } else {
                // `=K` is the explicit invariant spelling. It has the same
                // semantic value as an omitted marker, but still must consume
                // the token so the following name parses correctly.
                self.match_tok(&TokenKind::Eq);
                Variance::Invariant
            };

            let name = self.expect_ident()?;
            let mut is_higher_kinded = false;
            if self.match_tok(&TokenKind::LBracket) {
                is_higher_kinded = true;
                while !self.check(&TokenKind::RBracket) && !self.check(&TokenKind::Eof) {
                    self.advance();
                    if !self.match_tok(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RBracket)?;
            }

            let bound = if self.match_tok(&TokenKind::Colon) {
                Some(self.parse_type_expr()?)
            } else {
                None
            };

            params.push(TypeParam {
                name,
                variance,
                bound,
                is_higher_kinded,
                span: start,
            });

            if !self.match_tok(&TokenKind::Comma) {
                break;
            }
        }

        self.expect(&TokenKind::RBracket)?;
        Ok(params)
    }

    fn parse_param_list(&mut self) -> Result<Vec<Param>, ParseError> {
        self.expect(&TokenKind::LParen)?;
        let mut params: Vec<Param> = Vec::new();
        let mut is_positional_only = false;
        let mut is_keyword_only = false;

        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
            if self.match_tok(&TokenKind::Slash) {
                // `/` terminates the positional-only zone: parameters
                // already parsed belong to that zone, while parameters that
                // follow it remain keyword-capable.
                for param in &mut params {
                    param.is_positional_only = true;
                }
                is_positional_only = false;
                self.match_tok(&TokenKind::Comma);
                continue;
            }
            // A bare `*` starts the keyword-only zone. `*name` is instead
            // the variadic positional parameter itself.
            if self.check(&TokenKind::Star)
                && !matches!(
                    self.peek_next().map(|token| &token.kind),
                    Some(TokenKind::Ident(_))
                )
            {
                self.advance();
                is_keyword_only = true;
                self.match_tok(&TokenKind::Comma);
                continue;
            }

            let start = self.peek().span;
            let is_gather = self.match_tok(&TokenKind::TripleStar);
            let is_var_pos = !is_gather && self.match_tok(&TokenKind::Star);
            let is_var_kw = !is_gather && !is_var_pos && self.match_tok(&TokenKind::DoubleStar);

            let (name, pattern) = if !is_gather
                && !is_var_pos
                && !is_var_kw
                && matches!(self.peek_kind(), TokenKind::Ident(_))
                && self
                    .peek_next()
                    .map(|t| t.kind == TokenKind::LParen)
                    .unwrap_or(false)
            {
                let pat = self.parse_pattern()?;
                let n = match &pat {
                    Pattern::ClassDestructure { class_name, .. } => class_name.clone(),
                    _ => "_pat".to_string(),
                };
                (n, Some(pat))
            } else {
                let n = self.expect_ident()?;
                (n, None)
            };

            let type_annotation = if self.match_tok(&TokenKind::Colon) {
                Some(self.parse_type_expr()?)
            } else {
                None
            };

            let default = if self.match_tok(&TokenKind::Eq) {
                Some(self.parse_expr()?)
            } else {
                None
            };

            params.push(Param {
                name,
                pattern,
                type_annotation,
                default,
                is_positional_only,
                is_keyword_only,
                is_variadic_positional: is_var_pos,
                is_variadic_keyword: is_var_kw,
                is_gather,
                span: start,
            });

            if !self.match_tok(&TokenKind::Comma) {
                break;
            }
        }

        self.expect(&TokenKind::RParen)?;
        Ok(params)
    }

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Star => {
                self.advance();
                let nested = self.parse_pattern()?;
                Ok(Pattern::Star(Box::new(nested), tok.span))
            }
            TokenKind::Ident(name) => {
                let mut name = name.clone();
                self.advance();
                if name == "_" {
                    return Ok(Pattern::Wildcard(tok.span));
                }
                while self.match_tok(&TokenKind::Dot) {
                    name.push('.');
                    name.push_str(&self.expect_ident()?);
                }
                // Check for class destructure pattern: Name(a, b)
                if self.match_tok(&TokenKind::LParen) {
                    let mut fields = Vec::new();
                    while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                        let pat = self.parse_pattern()?;
                        fields.push((None, pat));
                        if !self.match_tok(&TokenKind::Comma) {
                            break;
                        }
                    }
                    let end = self.expect(&TokenKind::RParen)?.span;
                    return Ok(Pattern::ClassDestructure {
                        class_name: name,
                        fields,
                        span: tok.span.merge(end),
                    });
                }
                // Check for generic type pattern: Name[...]
                if self.match_tok(&TokenKind::LBracket) {
                    let mut args = Vec::new();
                    while !self.check(&TokenKind::RBracket) && !self.check(&TokenKind::Eof) {
                        args.push(self.parse_type_expr()?);
                        if !self.match_tok(&TokenKind::Comma) {
                            break;
                        }
                    }
                    let end = self.expect(&TokenKind::RBracket)?.span;
                    let type_expr = TypeExpr::Named {
                        name,
                        args,
                        span: tok.span.merge(end),
                    };
                    return Ok(Pattern::Type(type_expr, tok.span.merge(end)));
                }
                Ok(Pattern::Ident(name, tok.span))
            }
            TokenKind::Int(n) => {
                self.advance();
                Ok(Pattern::Literal(LiteralValue::Int(*n), tok.span))
            }
            TokenKind::BigInt(n) => {
                self.advance();
                Ok(Pattern::Literal(LiteralValue::BigInt(n.clone()), tok.span))
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Pattern::Literal(LiteralValue::Str(s.clone()), tok.span))
            }
            TokenKind::Bytes(s) => {
                self.advance();
                Ok(Pattern::Literal(
                    LiteralValue::Bytes(s.as_bytes().to_vec()),
                    tok.span,
                ))
            }
            TokenKind::True => {
                self.advance();
                Ok(Pattern::Literal(LiteralValue::Bool(true), tok.span))
            }
            TokenKind::False => {
                self.advance();
                Ok(Pattern::Literal(LiteralValue::Bool(false), tok.span))
            }
            TokenKind::None => {
                self.advance();
                Ok(Pattern::Literal(LiteralValue::None, tok.span))
            }
            TokenKind::LParen => {
                self.advance();
                let mut elements = Vec::new();
                while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                    elements.push(self.parse_pattern()?);
                    if !self.match_tok(&TokenKind::Comma) {
                        break;
                    }
                }
                let end = self.expect(&TokenKind::RParen)?.span;
                Ok(Pattern::Tuple(elements, tok.span.merge(end)))
            }
            _ => Err(ParseError {
                message: format!("unexpected token in pattern: {}", tok.kind),
                span: tok.span,
            }),
        }
    }

    fn parse_dotted_name(&mut self) -> Result<String, ParseError> {
        let mut name = self.expect_ident()?;
        while self.match_tok(&TokenKind::Dot) {
            name.push('.');
            name.push_str(&self.expect_ident()?);
        }
        Ok(name)
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.peek_kind() {
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }
            // Reserved words that are valid member names in qualified
            // expressions (for example SourceLocation.caller()).
            TokenKind::Caller => {
                self.advance();
                Ok("caller".into())
            }
            TokenKind::Trait => {
                self.advance();
                Ok("trait".into())
            }
            _ => Err(ParseError {
                message: format!("expected identifier, found {}", self.peek_kind()),
                span: self.peek().span,
            }),
        }
    }

    fn expect_name(&mut self) -> Result<String, ParseError> {
        match self.peek_kind() {
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }
            TokenKind::ContextManager => {
                self.advance();
                Ok("contextmanager".into())
            }
            _ => Err(ParseError {
                message: format!("expected identifier, found {}", self.peek_kind()),
                span: self.peek().span,
            }),
        }
    }
}
