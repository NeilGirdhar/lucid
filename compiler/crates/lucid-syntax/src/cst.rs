//! Lossless concrete syntax support.
//!
//! The semantic AST remains available for the existing lowering code, but
//! tooling and future compiler passes should consume this tree instead.  The
//! tree keeps the original source gaps (including comments and whitespace)
//! between lexer tokens, so formatting and diagnostics never need to rebuild
//! source text from semantic nodes.

use rowan::{GreenNodeBuilder, Language, SyntaxNode, SyntaxToken, TextRange};

use crate::{Lexer, TokenKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    Root = 0,
    Token = 1,
    Trivia = 2,
    Error = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LucidLanguage {}

impl Language for LucidLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        match raw.0 {
            0 => SyntaxKind::Root,
            1 => SyntaxKind::Token,
            2 => SyntaxKind::Trivia,
            _ => SyntaxKind::Error,
        }
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type Cst = SyntaxNode<LucidLanguage>;
pub type CstToken = SyntaxToken<LucidLanguage>;

/// Parse source into a lossless token tree.
///
/// Lexical failures are represented by an `Error` token spanning the failed
/// suffix.  This deliberately keeps the API useful for editor tooling while
/// the ordinary `parse` entry point continues to report a hard error.
pub fn parse_lossless(source: &str) -> Cst {
    let syntax_errors = crate::parse_recovering(source)
        .map(|(_, errors)| errors)
        .unwrap_or_default();
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(LucidLanguage::kind_to_raw(SyntaxKind::Root));
    let mut cursor = 0;
    let mut lexer = Lexer::new(source);
    loop {
        match lexer.next_token() {
            Ok(Some(token)) => {
                let start = token.span.start.min(source.len());
                let end = token.span.end.min(source.len()).max(start);
                if start > cursor {
                    builder.token(
                        LucidLanguage::kind_to_raw(SyntaxKind::Trivia),
                        &source[cursor..start],
                    );
                }
                let kind = if syntax_errors.iter().any(|error| {
                    let error_start = error.span.start.min(source.len());
                    let error_end = error.span.end.min(source.len()).max(error_start);
                    error_start < end && start < error_end
                }) {
                    SyntaxKind::Error
                } else {
                    SyntaxKind::Token
                };
                builder.token(LucidLanguage::kind_to_raw(kind), &source[start..end]);
                cursor = end;
                if token.kind == crate::TokenKind::Eof {
                    break;
                }
            }
            Ok(None) => break,
            Err(error) => {
                let start = error.span.start.min(source.len());
                let end = error.span.end.min(source.len()).max(start);
                if start > cursor {
                    builder.token(
                        LucidLanguage::kind_to_raw(SyntaxKind::Trivia),
                        &source[cursor..start],
                    );
                }
                builder.token(
                    LucidLanguage::kind_to_raw(SyntaxKind::Error),
                    &source[start..end],
                );
                cursor = end;
                if !lexer.recover_from_error(&error) {
                    break;
                }
            }
        }
    }
    if cursor < source.len() {
        builder.token(
            LucidLanguage::kind_to_raw(SyntaxKind::Trivia),
            &source[cursor..],
        );
    }
    builder.finish_node();
    SyntaxNode::new_root(builder.finish())
}

/// Return the original source represented by a CST.
pub fn text(cst: &Cst) -> String {
    cst.text().to_string()
}

/// Return byte ranges for lexical-error tokens retained in a lossless CST.
/// Ranges are relative to the original source and can be converted to the
/// language's `Span` type by a caller that owns the source map.
pub fn error_ranges(cst: &Cst) -> Vec<TextRange> {
    cst.descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind() == SyntaxKind::Error)
        .map(|token| token.text_range())
        .collect()
}

/// Keep this import visible to callers documenting the token/CST boundary.
pub fn token_kind_name(kind: &TokenKind) -> &'static str {
    match kind {
        TokenKind::Ident(_) => "identifier",
        TokenKind::Int(_) | TokenKind::BigInt(_) => "integer",
        TokenKind::Float(_) => "float",
        TokenKind::Complex(_) => "complex",
        TokenKind::Str(_) => "string",
        _ => "token",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cst_round_trips_source_and_keeps_trivia() {
        let source = "x = 1  # preserve me\n\n y = x + 2\n";
        let cst = parse_lossless(source);
        assert_eq!(text(&cst), source);
        assert!(cst.descendants_with_tokens().any(|element| {
            element.into_token().is_some_and(|token| {
                token.kind() == SyntaxKind::Trivia && token.text().contains("preserve me")
            })
        }));
    }

    #[test]
    fn cst_retains_lexical_error_text() {
        let source = "x = `bad`";
        let cst = parse_lossless(source);
        assert_eq!(text(&cst), source);
        assert!(
            cst.descendants_with_tokens()
                .filter_map(|element| element.into_token())
                .any(|token| token.kind() == SyntaxKind::Error)
        );
        assert_eq!(error_ranges(&cst).len(), 2);
    }

    #[test]
    fn cst_recovers_independent_lexical_errors() {
        let source = "x = `bad`\ny = 1\nz = `again`\n";
        let cst = parse_lossless(source);
        assert_eq!(text(&cst), source);
        // Both malformed literals contribute their two offending delimiters,
        // while the valid middle statement remains present.
        assert_eq!(error_ranges(&cst).len(), 4);
        assert!(cst.descendants_with_tokens().any(|element| {
            element
                .into_token()
                .is_some_and(|token| token.text() == "1")
        }));
    }

    #[test]
    fn cst_marks_recovered_grammar_errors_without_losing_following_tokens() {
        let source = "x =\ny = 1\n";
        let cst = parse_lossless(source);
        assert_eq!(text(&cst), source);
        assert!(!error_ranges(&cst).is_empty());
        assert!(cst.descendants_with_tokens().any(|element| {
            element
                .into_token()
                .is_some_and(|token| token.text() == "1")
        }));
    }
}
