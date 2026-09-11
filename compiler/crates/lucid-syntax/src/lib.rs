pub mod ast;
pub mod cst;
pub mod lexer;
pub mod parser;
pub mod token;

pub use ast::*;
pub use cst::{Cst, CstToken, LucidLanguage, SyntaxKind, error_ranges, parse_lossless};
pub use lexer::Lexer;
pub use parser::Parser;
pub use token::{Span, Token, TokenKind};

pub fn parse(source: &str) -> Result<ast::Module, String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().map_err(|e| {
        format!(
            "Lexer error: {} at line {}, col {}",
            e.message, e.span.line, e.span.column
        )
    })?;
    let mut parser = Parser::new(tokens);
    parser.parse_module().map_err(|e| {
        format!(
            "Parse error: {} at line {}, col {}",
            e.message, e.span.line, e.span.column
        )
    })
}

/// Parse a source file while recovering from lexical and independent
/// statement errors. The returned module contains every statement that could
/// be parsed before recovery stopped; errors retain their original source
/// spans for diagnostics.
pub fn parse_recovering(source: &str) -> Result<(ast::Module, Vec<parser::ParseError>), String> {
    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();
    let mut lexical_errors = Vec::new();
    loop {
        match lexer.next_token() {
            Ok(Some(token)) => {
                let is_eof = token.kind == TokenKind::Eof;
                tokens.push(token);
                if is_eof {
                    break;
                }
            }
            Ok(None) => break,
            Err(error) => {
                let can_continue = lexer.recover_from_error(&error);
                lexical_errors.push(parser::ParseError {
                    message: format!(
                        "lexer error: {} at line {}, col {}",
                        error.message, error.span.line, error.span.column
                    ),
                    span: error.span,
                });
                if !can_continue {
                    break;
                }
            }
        }
    }
    let (module, mut parse_errors) = Parser::new(tokens).parse_module_recovering();
    lexical_errors.append(&mut parse_errors);
    Ok((module, lexical_errors))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_lex_basic_tokens() {
        let src = "true false none 42 3.14 \"hello\"";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].kind, TokenKind::True);
        assert_eq!(tokens[1].kind, TokenKind::False);
        assert_eq!(tokens[2].kind, TokenKind::None);
        assert_eq!(tokens[3].kind, TokenKind::Int(42));
        assert_eq!(tokens[4].kind, TokenKind::Float(3.14));
        assert_eq!(tokens[5].kind, TokenKind::Str("hello".to_string()));
    }

    #[test]
    fn parser_adds_eof_to_hand_built_streams() {
        let module = Parser::new(Vec::new()).parse_module().unwrap();
        assert!(module.statements.is_empty());

        let token = Token::new(TokenKind::Ident("dangling".into()), Span::default());
        let module = Parser::new(vec![token]).parse_module().unwrap();
        assert_eq!(module.statements.len(), 1);
    }

    #[test]
    fn test_complex_literals_are_preserved() {
        let tokens = Lexer::new("1j 2.5J").tokenize().unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Complex(v) if (v - 1.0).abs() < f64::EPSILON));
        assert!(matches!(tokens[1].kind, TokenKind::Complex(v) if (v - 2.5).abs() < f64::EPSILON));
    }

    #[test]
    fn test_radix_integer_literals() {
        let tokens = Lexer::new("0x2a 0o52 0b101010").tokenize().unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Int(42));
        assert_eq!(tokens[1].kind, TokenKind::Int(42));
        assert_eq!(tokens[2].kind, TokenKind::Int(42));
        let tokens = Lexer::new("0x8000000000000000").tokenize().unwrap();
        assert_eq!(
            tokens[0].kind,
            TokenKind::BigInt("0x8000000000000000".to_string())
        );
        assert!(Lexer::new("0b102").tokenize().is_err());
        assert!(Lexer::new("0x1.2").tokenize().is_err());
        assert!(Lexer::new("0x").tokenize().is_err());
        assert!(Lexer::new("0o8").tokenize().is_err());
    }

    #[test]
    fn test_lex_lucid_keywords_and_sigils() {
        let src = "interface trait class factory construct *** ? -> ! &";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Interface);
        assert_eq!(tokens[1].kind, TokenKind::Trait);
        assert_eq!(tokens[2].kind, TokenKind::Class);
        assert_eq!(tokens[3].kind, TokenKind::Factory);
        assert_eq!(tokens[4].kind, TokenKind::Construct);
        assert_eq!(tokens[5].kind, TokenKind::TripleStar);
        assert_eq!(tokens[6].kind, TokenKind::Question);
        assert_eq!(tokens[7].kind, TokenKind::Arrow);
        assert_eq!(tokens[8].kind, TokenKind::Bang);
        assert_eq!(tokens[9].kind, TokenKind::Amp);
    }

    #[test]
    fn test_lex_identity_operator_spellings() {
        let tokens = Lexer::new("a === b; c !== d").tokenize().unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::TripleEq));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::TripleNotEq));
    }

    #[test]
    fn test_parse_class_with_factory_and_views() {
        let src = r#"
export class Point:
    x: float
    y: float

    factory origin(cls) -> Point:
        return construct(0.0, 0.0)

    def dist(self: &Self) -> float:
        return self.x + self.y
"#;
        let module = parse(src).unwrap();
        assert_eq!(module.statements.len(), 1);
        match &module.statements[0] {
            Stmt::Export(inner) => match inner.as_ref() {
                Stmt::ClassDef { name, body, .. } => {
                    assert_eq!(name, "Point");
                    assert_eq!(body.len(), 4);
                }
                _ => panic!("expected class def"),
            },
            _ => panic!("expected export"),
        }
    }

    #[test]
    fn test_parse_pattern_matching_and_results() {
        let src = r#"
def process(x: int | none) -> int:
    match x:
        case int:
            return x + 1
        case none:
            return 0
"#;
        let module = parse(src).unwrap();
        assert_eq!(module.statements.len(), 1);
    }

    #[test]
    fn test_parse_readme_example() {
        let src = r#"
export interface Scorable[+K]:
    def score(self, item: K) -> float

export trait ScoreBands[+K](Scorable[K]):
    def is_confident(self, item: K) -> bool:
        return self.score(item) >= 0.8

export class InferenceModel[=K](Scorable[K], ScoreBands[K]):
    weights: Tensor
    labels: list[K]
    scores: dict[K, float]

    factory from_checkpoint(cls, path: Path, labels: list[K]):
        weights = Tensor.load(path)
        return construct(weights, labels, {:})

    def score(self, item: K) -> float:
        return 0.95
"#;
        let module = parse(src).unwrap();
        assert_eq!(module.statements.len(), 3);
    }

    #[test]
    fn test_parse_contextmanager_and_yield() {
        let src = r#"
contextmanager def locked(lock: Lock):
    lock.acquire()
    yield lock
    lock.release()
"#;
        let module = parse(src).unwrap();
        match &module.statements[0] {
            Stmt::Function(function) => {
                assert_eq!(function.name, "locked");
                assert!(function.decorators.iter().any(|decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager")));
                assert!(matches!(function.body[1], Stmt::Yield { .. }));
            }
            other => panic!("expected function, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_async_def_and_await() {
        let src = r#"
async def load() -> int:
    return await fetch()
"#;
        let module = parse(src).unwrap();
        match &module.statements[0] {
            Stmt::Function(function) => {
                assert!(function.is_async);
                assert!(
                    matches!(function.body[0], Stmt::Return { value: Some(ref value), .. } if matches!(value, Expr::Await { .. }))
                );
            }
            other => panic!("expected async function, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_advanced_type_spelling_and_bytes() {
        let src = r#"
def render(shape: Drawable & Serializable) -> bytes:
    data: Bytes = b"hello"
    return data

def reject(value: not int) -> none:
    return none

def register(handler: class[Handler]) -> none:
    return none
"#;
        let module = parse(src).unwrap();
        assert_eq!(module.statements.len(), 3);
    }

    #[test]
    fn test_parse_boolean_literal_type_annotations() {
        let module = parse("flag: true = true\n").unwrap();
        let Stmt::VarDef {
            type_annotation:
                Some(TypeExpr::Literal {
                    value: LiteralValue::Bool(true),
                    ..
                }),
            ..
        } = &module.statements[0]
        else {
            panic!("expected exact boolean literal annotation");
        };
    }

    #[test]
    fn test_parse_shape_slice_bounds() {
        let module = parse("type Tail = S[:-2]\n").unwrap();
        let Stmt::TypeAlias {
            value: TypeAliasValue::Direct(expr),
            ..
        } = &module.statements[0]
        else {
            panic!("expected type alias");
        };
        assert!(
            matches!(expr, TypeExpr::Named { name, args, .. } if name == "__shape_slice__" && args.len() == 4)
        );
    }

    #[test]
    fn test_parse_tilde_read_only_views() {
        let module = parse("def take(xs: ~list[int]) -> ~list[int]:\n    return xs\n").unwrap();
        assert_eq!(module.statements.len(), 1);
    }

    #[test]
    fn recovering_parser_keeps_later_statements() {
        let (module, errors) = parse_recovering("first = 1\nbad =\nlast = 3\n").unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(module.statements.len(), 2);
        assert!(matches!(module.statements[0], Stmt::Assignment { .. }));
        assert!(matches!(module.statements[1], Stmt::Assignment { .. }));
        assert!(errors[0].span.line >= 2);
    }

    #[test]
    fn recovering_parser_reports_lexical_errors_with_spans() {
        let (module, errors) = parse_recovering("ok = 1\nbad = `unterminated\n").unwrap();
        assert!(!module.statements.is_empty());
        assert!(matches!(module.statements[0], Stmt::Assignment { .. }));
        assert!(!errors.is_empty());
        let lexical_error = errors
            .iter()
            .find(|error| error.message.starts_with("lexer error:"))
            .expect("lexical error should be retained");
        assert!(lexical_error.span.start > 0);
    }

    #[test]
    fn recovering_parser_continues_after_finite_lexical_error() {
        let (module, errors) = parse_recovering("a = @\nb = $\nc = 1\n").unwrap();
        let lexical_errors = errors
            .iter()
            .filter(|error| error.message.starts_with("lexer error:"))
            .count();
        assert!(lexical_errors >= 1);
        assert!(module.statements.iter().any(|statement| {
            matches!(statement, Stmt::Assignment { target: Expr::Ident { name, .. }, .. } if name == "c")
        }));
    }
}
