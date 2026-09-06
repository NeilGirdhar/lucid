pub mod ast;
pub mod lexer;
pub mod parser;
pub mod token;

pub use ast::*;
pub use lexer::Lexer;
pub use parser::Parser;
pub use token::{Span, Token, TokenKind};

pub fn parse(source: &str) -> Result<ast::Module, String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().map_err(|e| format!("Lexer error: {} at line {}, col {}", e.message, e.span.line, e.span.column))?;
    let mut parser = Parser::new(tokens);
    parser.parse_module().map_err(|e| format!("Parse error: {} at line {}, col {}", e.message, e.span.line, e.span.column))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
}
