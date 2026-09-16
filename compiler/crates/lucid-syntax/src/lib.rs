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
    fn python_spelled_constants_are_identifiers_not_literals() {
        let mut lexer = Lexer::new("True False None");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Ident("True".to_string()));
        assert_eq!(tokens[1].kind, TokenKind::Ident("False".to_string()));
        assert_eq!(tokens[2].kind, TokenKind::Ident("None".to_string()));

        let module = parse("x = None\n").unwrap();
        let Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        assert!(matches!(value, Expr::Ident { name, .. } if name == "None"));

        let module = parse("x = none\n").unwrap();
        let Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        assert!(matches!(
            value,
            Expr::Literal {
                value: LiteralValue::None,
                ..
            }
        ));
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
        let src = "trait class factory construct out *** ? -> ! &";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Trait);
        assert_eq!(tokens[1].kind, TokenKind::Class);
        assert_eq!(tokens[2].kind, TokenKind::Factory);
        assert_eq!(tokens[3].kind, TokenKind::Construct);
        assert_eq!(tokens[4].kind, TokenKind::Out);
        assert_eq!(tokens[5].kind, TokenKind::TripleStar);
        assert_eq!(tokens[6].kind, TokenKind::Question);
        assert_eq!(tokens[7].kind, TokenKind::Arrow);
        assert_eq!(tokens[8].kind, TokenKind::Bang);
        assert_eq!(tokens[9].kind, TokenKind::Amp);
    }

    #[test]
    fn interface_and_export_are_ordinary_identifiers() {
        let tokens = Lexer::new("interface export").tokenize().unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Ident("interface".to_string()));
        assert_eq!(tokens[1].kind, TokenKind::Ident("export".to_string()));

        assert!(parse("interface Printable:\n    def format(self) -> str\n").is_err());
        assert!(parse("export def helper(x: int) -> int:\n    return x\n").is_err());
        assert!(parse("export class Service:\n    pass\n").is_err());
        assert!(parse("from util export helper\n").is_err());
    }

    #[test]
    fn ampersand_is_intersection_not_a_view_marker() {
        assert!(parse("def take(xs: &list[int]) -> int:\n    return 0\n").is_err());
        let module = parse("def take(xs: Sized & Iterable[int]) -> int:\n    return 0\n").unwrap();
        assert_eq!(module.statements.len(), 1);
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
class Point:
    x: float
    y: float

    factory origin(cls) -> Point:
        return construct(0.0, 0.0)

    def dist(self: ~Self) -> float:
        return self.x + self.y
"#;
        let module = parse(src).unwrap();
        assert_eq!(module.statements.len(), 1);
        match &module.statements[0] {
            Stmt::ClassDef { name, body, .. } => {
                assert_eq!(name, "Point");
                assert_eq!(body.len(), 4);
            }
            _ => panic!("expected class def"),
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
    fn test_parse_float_literal_match_pattern() {
        let src = r#"
def process(x: float) -> int:
    match x:
        case 1.5:
            return 1
        case _:
            return 0
"#;
        let module = parse(src).unwrap();
        let Stmt::Function(function) = &module.statements[0] else {
            panic!("expected function");
        };
        let Stmt::Match { arms, .. } = &function.body[0] else {
            panic!("expected match");
        };
        assert!(matches!(
            arms[0].pattern,
            Pattern::Literal(LiteralValue::Float(value), _) if (value - 1.5).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn test_parse_complex_literal_match_pattern() {
        let src = r#"
def process(x: complex) -> int:
    match x:
        case 1j:
            return 1
        case _:
            return 0
"#;
        let module = parse(src).unwrap();
        let Stmt::Function(function) = &module.statements[0] else {
            panic!("expected function");
        };
        let Stmt::Match { arms, .. } = &function.body[0] else {
            panic!("expected match");
        };
        assert!(matches!(
            arms[0].pattern,
            Pattern::Literal(LiteralValue::Complex(value), _) if (value - 1.0).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn test_parse_ellipsis_literal_match_pattern() {
        let src = r#"
def process(x) -> int:
    match x:
        case ...:
            return 1
        case _:
            return 0
"#;
        let module = parse(src).unwrap();
        let Stmt::Function(function) = &module.statements[0] else {
            panic!("expected function");
        };
        let Stmt::Match { arms, .. } = &function.body[0] else {
            panic!("expected match");
        };
        assert!(matches!(
            arms[0].pattern,
            Pattern::Literal(LiteralValue::Ellipsis, _)
        ));
    }

    #[test]
    fn test_parse_readme_example() {
        let src = r#"
trait Scorable[in K]:
    def score(self, item: K) -> float

trait ScoreBands[in K](Scorable[K]):
    def is_confident(self, item: K) -> bool:
        return self.score(item) >= 0.8

class InferenceModel[in ~out K](Scorable[K], ScoreBands[K]):
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
        let Stmt::ClassDef { type_params, .. } = &module.statements[2] else {
            panic!("expected class definition");
        };
        assert_eq!(type_params[0].variance, Variance::ViewCovariant);
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
    fn test_parse_single_and_multi_index_without_internal_unwrap() {
        let module = parse("value = xs[i]\npair = grid[row, col]\n").unwrap();
        let Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        let Expr::Index { index, .. } = value else {
            panic!("expected single index expression");
        };
        assert!(matches!(index.as_ref(), Expr::Ident { name, .. } if name == "i"));

        let Stmt::Assignment { value, .. } = &module.statements[1] else {
            panic!("expected assignment");
        };
        let Expr::Index { index, .. } = value else {
            panic!("expected multi-index expression");
        };
        let Expr::Record { fields, .. } = index.as_ref() else {
            panic!("expected multi-index tuple record");
        };
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn test_parse_rejects_classmethod_decorator_keyword() {
        let src = "class Factory:\n    @classmethod\n    def make(cls) -> int:\n        return 1\n";
        let error = parse(src).expect_err("classmethod decorator must fail in parsing");
        assert!(error.contains("classmethod is not supported as a decorator"));
    }

    #[test]
    fn test_parse_rejects_removed_type_builtin_call() {
        let error = parse("value = type(1)\n").expect_err("type() must fail in parsing");
        assert!(error.contains("type() is not supported"));
    }

    #[test]
    fn test_parse_requires_parenthesized_assert() {
        parse("assert(true)\n").expect("parenthesized assert should parse");
        parse("assert(true, \"message\")\n").expect("assert message should parse");

        let error = parse("assert true\n").expect_err("bare assert must fail in parsing");
        assert!(error.contains("assert requires parentheses"));
    }

    #[test]
    fn test_parse_rejects_adjacent_string_literals() {
        for source in ["value = \"a\" \"b\"\n", "value = b\"a\" b\"b\"\n"] {
            let error = parse(source).expect_err("adjacent string literals must fail in parsing");
            assert!(error.contains("adjacent string literals are not supported"));
        }
    }

    #[test]
    fn test_parse_rejects_field_delete_with_fixed_shape_message() {
        let error = parse("del obj.field\n").expect_err("field delete must fail in parsing");
        assert!(error.contains("fields are fixed, not deletable"));
    }

    #[test]
    fn test_parse_rejects_index_delete_with_removal_message() {
        for source in ["del items[0]\n", "del first, items[0]\n"] {
            let error = parse(source).expect_err("index delete must fail in parsing");
            assert!(error.contains("indexed deletion is not supported"));
        }
    }

    #[test]
    fn test_parse_rejects_keyword_class_bases() {
        let src = "class Meta:\n    pass\nclass Model(metaclass=Meta):\n    pass\n";
        let error = parse(src).expect_err("keyword class bases must fail in parsing");
        assert!(error.contains("unsupported class option 'metaclass='"));
    }

    #[test]
    fn test_parse_class_value_semantics_options() {
        let module = parse("class Point(eq=true, order=false, hash=false):\n    pass\n").unwrap();
        let Stmt::ClassDef { without_traits, .. } = &module.statements[0] else {
            panic!("expected class definition");
        };
        assert_eq!(
            without_traits,
            &vec!["Ord".to_string(), "Hashable".to_string()]
        );
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
    fn test_type_not_binds_to_immediate_type_before_union() {
        let module = parse("type Value = not int | str\n").unwrap();
        let Stmt::TypeAlias {
            value: TypeAliasValue::Direct(TypeExpr::Union { types, .. }),
            ..
        } = &module.statements[0]
        else {
            panic!("expected union type alias");
        };
        assert_eq!(types.len(), 2);
        assert!(matches!(
            &types[0],
            TypeExpr::Named { name, args, .. }
                if name == "__not__"
                    && matches!(
                        args.as_slice(),
                        [TypeExpr::Named { name, .. }] if name == "int"
                    )
        ));
        assert!(matches!(
            &types[1],
            TypeExpr::Named { name, .. } if name == "str"
        ));

        let module = parse("type Value = not (int | str)\n").unwrap();
        let Stmt::TypeAlias {
            value: TypeAliasValue::Direct(TypeExpr::Named { name, args, .. }),
            ..
        } = &module.statements[0]
        else {
            panic!("expected negated type alias");
        };
        assert_eq!(name, "__not__");
        assert!(matches!(args.as_slice(), [TypeExpr::Union { types, .. }] if types.len() == 2));
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
    fn test_not_binds_looser_than_comparison() {
        let module = parse("value = not n > high or n <= low\n").unwrap();
        let Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        let Expr::Binary {
            op: BinaryOp::Or,
            left,
            right,
            ..
        } = value
        else {
            panic!("expected top-level logical or");
        };
        assert!(matches!(
            left.as_ref(),
            Expr::Unary {
                op: UnaryOp::Not,
                expr,
                ..
            } if matches!(
                expr.as_ref(),
                Expr::Binary {
                    op: BinaryOp::Gt,
                    ..
                }
            )
        ));
        assert!(matches!(
            right.as_ref(),
            Expr::Binary {
                op: BinaryOp::LtEq,
                ..
            }
        ));
    }

    #[test]
    fn test_two_token_comparison_spans_cover_right_operand() {
        for source in [
            "value = left is not right\n",
            "value = item not in values\n",
        ] {
            let module = parse(source).unwrap();
            let Stmt::Assignment { value, .. } = &module.statements[0] else {
                panic!("expected assignment");
            };
            let Expr::Binary { right, span, .. } = value else {
                panic!("expected binary comparison");
            };
            assert_eq!(span.start, 8);
            assert_eq!(span.end, right.span().end);
        }
    }

    #[test]
    fn test_parse_dict_shape_type_as_record() {
        let module = parse("type Movie = {\"name\": str, \"year\": int}\n").unwrap();
        let Stmt::TypeAlias {
            value:
                TypeAliasValue::Direct(TypeExpr::Record {
                    fields, is_open, ..
                }),
            ..
        } = &module.statements[0]
        else {
            panic!("expected record type alias");
        };
        assert!(!is_open);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name.as_deref(), Some("name"));
        assert!(matches!(
            fields[0].type_expr,
            TypeExpr::Named { ref name, .. } if name == "str"
        ));
        assert_eq!(fields[1].name.as_deref(), Some("year"));
        assert!(matches!(
            fields[1].type_expr,
            TypeExpr::Named { ref name, .. } if name == "int"
        ));

        let module = parse("type Movie = {\"name\": str, ...}\n").unwrap();
        let Stmt::TypeAlias {
            value: TypeAliasValue::Direct(TypeExpr::Record { is_open, .. }),
            ..
        } = &module.statements[0]
        else {
            panic!("expected open record type alias");
        };
        assert!(is_open);
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

    fn first_type_param_variance(source: &str) -> Variance {
        let module = parse(source).unwrap();
        match &module.statements[0] {
            Stmt::ClassDef { type_params, .. } | Stmt::TraitDef { type_params, .. } => {
                type_params[0].variance.clone()
            }
            other => panic!("expected class or trait definition, got {other:?}"),
        }
    }

    #[test]
    fn variance_keywords_parse_to_their_variants() {
        for (spelling, expected) in [
            ("out K", Variance::Covariant),
            ("in K", Variance::Contravariant),
            ("in out K", Variance::Invariant),
            ("in ~out K", Variance::ViewCovariant),
            ("~in out K", Variance::ViewContravariant),
            ("K", Variance::Unmarked),
        ] {
            assert_eq!(
                first_type_param_variance(&format!("class Box[{spelling}]:\n    pass\n")),
                expected,
                "class Box[{spelling}]"
            );
            assert_eq!(
                first_type_param_variance(&format!("trait Box[{spelling}]:\n    pass\n")),
                expected,
                "trait Box[{spelling}]"
            );
        }
    }

    #[test]
    fn variance_keywords_compose_with_bounds_and_multiple_params() {
        let module = parse("class Cache[in out K: Hashable, in ~out V]:\n    pass\n").unwrap();
        let Stmt::ClassDef { type_params, .. } = &module.statements[0] else {
            panic!("expected class definition");
        };
        assert_eq!(type_params.len(), 2);
        assert_eq!(type_params[0].name, "K");
        assert_eq!(type_params[0].variance, Variance::Invariant);
        assert!(matches!(
            &type_params[0].bound,
            Some(TypeExpr::Named { name, .. }) if name == "Hashable"
        ));
        assert_eq!(type_params[1].name, "V");
        assert_eq!(type_params[1].variance, Variance::ViewCovariant);
    }

    #[test]
    fn type_parameter_defaults_parse_after_the_bound() {
        let module = parse("class Foo[in out T = int, out U: Animal = Dog]:\n    pass\n").unwrap();
        let Stmt::ClassDef { type_params, .. } = &module.statements[0] else {
            panic!("expected class definition");
        };
        assert!(matches!(
            &type_params[0].default,
            Some(TypeExpr::Named { name, .. }) if name == "int"
        ));
        assert!(type_params[0].bound.is_none());
        assert!(matches!(
            &type_params[1].bound,
            Some(TypeExpr::Named { name, .. }) if name == "Animal"
        ));
        assert!(matches!(
            &type_params[1].default,
            Some(TypeExpr::Named { name, .. }) if name == "Dog"
        ));

        let module = parse("trait Managed[in out T = Self]:\n    def get(self) -> T\n").unwrap();
        let Stmt::TraitDef { type_params, .. } = &module.statements[0] else {
            panic!("expected trait definition");
        };
        assert!(matches!(
            &type_params[0].default,
            Some(TypeExpr::Named { name, .. }) if name == "Self"
        ));
    }

    #[test]
    fn fixed_type_sets_parse_after_the_name() {
        let module =
            parse("def concat[T in (str, bytes)](a: T, b: T) -> T:\n    return a + b\n").unwrap();
        let Stmt::Function(FunctionDef { type_params, .. }) = &module.statements[0] else {
            panic!("expected function definition");
        };
        assert!(type_params[0].bound.is_none());
        assert_eq!(type_params[0].alternatives.len(), 2);

        let module = parse("class Container[in out T in (int, str)]:\n    pass\n").unwrap();
        let Stmt::ClassDef { type_params, .. } = &module.statements[0] else {
            panic!("expected class definition");
        };
        assert_eq!(type_params[0].variance, Variance::Invariant);
        assert_eq!(type_params[0].alternatives.len(), 2);

        let error = parse("def one[T in (int)](a: T) -> T:\n    return a\n").unwrap_err();
        assert!(error.contains("at least two members"), "{error}");
        let error = parse("def both[T: Sized in (int, str)](a: T) -> T:\n    return a\n").unwrap_err();
        assert!(error.contains("either a bound or a fixed set"), "{error}");
    }

    #[test]
    fn getters_and_setters_accept_an_annotated_receiver() {
        let module = parse(
            "class Animal:\n    getter offspring(self: ~Self) -> Animal:\n        return self\n    setter name(self: Self, value: str):\n        pass\ntrait Named:\n    getter label(self: ~Self) -> str\n    setter label(self, value: str)\n",
        )
        .unwrap();
        assert_eq!(module.statements.len(), 2);
    }

    #[test]
    fn contextmanager_stacks_on_trait_members() {
        let module = parse(
            "trait Managed[in out T = Self]:\n    contextmanager def __cm__(self) -> T\n    contextmanager classmethod open(cls) -> T:\n        yield cls()\n",
        )
        .unwrap();
        let Stmt::TraitDef { body, .. } = &module.statements[0] else {
            panic!("expected trait definition");
        };
        let TraitMember::Method(method) = &body[0] else {
            panic!("expected method member");
        };
        assert!(method.decorators.iter().any(|decorator| {
            matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager")
        }));
        assert!(matches!(&body[1], TraitMember::ClassMethod(_)));
    }

    #[test]
    fn obsolete_variance_sigils_are_rejected() {
        for spelling in ["+K", "-K", "=K", "+=K", "-=K"] {
            assert!(
                parse(&format!("class Box[{spelling}]:\n    pass\n")).is_err(),
                "class Box[{spelling}] must not parse"
            );
        }
    }

    #[test]
    fn partial_view_variance_spellings_are_rejected() {
        for spelling in ["~out K", "~in K", "out ~in K", "in ~in K", "~in ~out K"] {
            assert!(
                parse(&format!("class Box[{spelling}]:\n    pass\n")).is_err(),
                "class Box[{spelling}] must not parse"
            );
        }
    }

    #[test]
    fn type_record_positional_only_marker_applies_to_prior_fields() {
        let module = parse("type F = (x: int, /, y: int)\n").unwrap();
        let Stmt::TypeAlias {
            value: TypeAliasValue::Direct(TypeExpr::Record { fields, .. }),
            ..
        } = &module.statements[0]
        else {
            panic!("expected record function type alias");
        };
        assert!(fields[0].is_positional_only);
        assert!(!fields[1].is_positional_only);
    }

    #[test]
    fn wildcard_import_reports_explicit_rejection() {
        let error = parse("from module import *\n").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("wildcard imports are not supported")
        );
    }

    #[test]
    fn dunder_all_binding_reports_explicit_rejection() {
        for source in [
            "__all__ = [\"value\"]\n",
            "let __all__ = [\"value\"]\n",
            "def __all__() -> int:\n    return 1\n",
            "class __all__:\n    pass\n",
            "type __all__ = int\n",
        ] {
            let error = parse(source).unwrap_err();
            assert!(error.to_string().contains("__all__ is not supported"));
        }
    }

    #[test]
    fn recovering_parser_drops_dunder_all_but_keeps_later_statements() {
        let (module, errors) = parse_recovering("__all__ = []\nkept = 1\n").unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(module.statements.len(), 1);
        assert!(
            matches!(&module.statements[0], Stmt::Assignment { target: Expr::Ident { name, .. }, .. } if name == "kept")
        );
        assert!(errors[0].message.contains("__all__ is not supported"));
    }

    #[test]
    fn discarded_scope_and_lambda_keywords_report_explicit_rejections() {
        for (source, expected) in [
            ("global counter\n", "global is not supported"),
            ("nonlocal counter\n", "nonlocal is not supported"),
            ("handler = lambda x: x\n", "lambda is not supported"),
        ] {
            let error = parse(source).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }
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
    fn recovering_parser_discards_malformed_block_header_body() {
        let (module, errors) = parse_recovering("if :\n    leaked = 1\nkept = 2\n").unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(module.statements.len(), 1);
        assert!(
            matches!(&module.statements[0], Stmt::Assignment { target: Expr::Ident { name, .. }, .. } if name == "kept")
        );
    }

    #[test]
    fn recovering_parser_discards_rest_of_malformed_multiline_statement() {
        let source = "def broken():\n    ok = 1\n    bad =\n    leaked = 2\nkept = 3\n";
        let (module, errors) = parse_recovering(source).unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(module.statements.len(), 1);
        assert!(
            matches!(&module.statements[0], Stmt::Assignment { target: Expr::Ident { name, .. }, .. } if name == "kept")
        );
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
