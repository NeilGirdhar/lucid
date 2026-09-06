use crate::token::{Span, Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub struct LexerError {
    pub message: String,
    pub span: Span,
}

pub struct Lexer<'a> {
    source: &'a str,
    chars: Vec<(usize, char)>,
    cursor: usize,
    line: usize,
    column: usize,
    indent_stack: Vec<usize>,
    open_brackets: usize,
    at_line_start: bool,
    pending_dedents: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        let chars: Vec<(usize, char)> = source.char_indices().collect();
        Self {
            source,
            chars,
            cursor: 0,
            line: 1,
            column: 1,
            indent_stack: vec![0],
            open_brackets: 0,
            at_line_start: true,
            pending_dedents: 0,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexerError> {
        let mut tokens = Vec::new();

        while let Some(tok) = self.next_token()? {
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }

        Ok(tokens)
    }

    fn peek_char(&self) -> Option<char> {
        self.chars.get(self.cursor).map(|&(_, c)| c)
    }

    fn peek_next_char(&self) -> Option<char> {
        self.chars.get(self.cursor + 1).map(|&(_, c)| c)
    }

    fn peek_char_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.cursor + offset).map(|&(_, c)| c)
    }

    fn advance_char(&mut self) -> Option<char> {
        if let Some(&(_, c)) = self.chars.get(self.cursor) {
            self.cursor += 1;
            if c == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            Some(c)
        } else {
            None
        }
    }

    fn current_pos(&self) -> (usize, usize, usize) {
        let byte_pos = self.chars.get(self.cursor).map(|&(idx, _)| idx).unwrap_or(self.source.len());
        (byte_pos, self.line, self.column)
    }

    pub fn next_token(&mut self) -> Result<Option<Token>, LexerError> {
        // Emit any queued dedents
        if self.pending_dedents > 0 {
            self.pending_dedents -= 1;
            let (pos, line, col) = self.current_pos();
            return Ok(Some(Token::new(TokenKind::Dedent, Span::new(pos, pos, line, col))));
        }

        // Handle indentation at start of line
        while self.at_line_start {
            let (start_pos, start_line, start_col) = self.current_pos();
            let mut indent_spaces = 0;

            // Count leading spaces/tabs on this line
            let mut temp_cursor = self.cursor;
            while let Some(&(_, c)) = self.chars.get(temp_cursor) {
                if c == ' ' {
                    indent_spaces += 1;
                    temp_cursor += 1;
                } else if c == '\t' {
                    indent_spaces += 4;
                    temp_cursor += 1;
                } else {
                    break;
                }
            }

            // Check if line is blank or just comment
            let next_c = self.chars.get(temp_cursor).map(|&(_, c)| c);
            if next_c == Some('\n') || next_c == Some('\r') || next_c == Some('#') || next_c.is_none() {
                // Ignore indentation on empty/comment lines: consume blank/comment and advance to next line
                self.cursor = temp_cursor;
                if let Some(c) = self.peek_char() {
                    if c == '#' {
                        while let Some(ch) = self.peek_char() {
                            if ch == '\n' { break; }
                            self.advance_char();
                        }
                    }
                }
                if self.peek_char() == Some('\r') { self.advance_char(); }
                if self.peek_char() == Some('\n') { self.advance_char(); }
                if self.cursor >= self.chars.len() {
                    self.at_line_start = false;
                    break;
                }
                continue;
            }

            self.at_line_start = false;

            if self.open_brackets == 0 {
                // Advance past the indent characters
                while let Some(c) = self.peek_char() {
                    if c == ' ' || c == '\t' {
                        self.advance_char();
                    } else {
                        break;
                    }
                }

                let current_indent = *self.indent_stack.last().unwrap_or(&0);
                if indent_spaces > current_indent {
                    self.indent_stack.push(indent_spaces);
                    let (end_pos, _end_line, _end_col) = self.current_pos();
                    return Ok(Some(Token::new(
                        TokenKind::Indent,
                        Span::new(start_pos, end_pos, start_line, start_col),
                    )));
                } else if indent_spaces < current_indent {
                    let mut dedent_count = 0;
                    while let Some(&top) = self.indent_stack.last() {
                        if top > indent_spaces {
                            self.indent_stack.pop();
                            dedent_count += 1;
                        } else {
                            break;
                        }
                    }

                    if *self.indent_stack.last().unwrap_or(&0) != indent_spaces {
                        return Err(LexerError {
                            message: format!("unindent does not match any outer indentation level ({indent_spaces} spaces)"),
                            span: Span::new(start_pos, start_pos + indent_spaces, start_line, start_col),
                        });
                    }

                    if dedent_count > 0 {
                        self.pending_dedents = dedent_count - 1;
                        let (end_pos, _end_line, _end_col) = self.current_pos();
                        return Ok(Some(Token::new(
                            TokenKind::Dedent,
                            Span::new(start_pos, end_pos, start_line, start_col),
                        )));
                    }
                }
            }
            break;
        }

        // Skip horizontal whitespace
        while let Some(c) = self.peek_char() {
            if c == ' ' || c == '\t' || c == '\r' {
                self.advance_char();
            } else {
                break;
            }
        }

        // Check for comment
        if self.peek_char() == Some('#') {
            while let Some(c) = self.peek_char() {
                if c == '\n' {
                    break;
                }
                self.advance_char();
            }
        }

        let (start_pos, start_line, start_col) = self.current_pos();
        let c = match self.peek_char() {
            Some(c) => c,
            None => {
                // End of file: flush any remaining indents
                if self.indent_stack.len() > 1 {
                    self.indent_stack.pop();
                    return Ok(Some(Token::new(
                        TokenKind::Dedent,
                        Span::new(start_pos, start_pos, start_line, start_col),
                    )));
                }
                return Ok(Some(Token::new(
                    TokenKind::Eof,
                    Span::new(start_pos, start_pos, start_line, start_col),
                )));
            }
        };

        // Handle Newlines
        if c == '\n' {
            self.advance_char();
            self.at_line_start = true;
            if self.open_brackets == 0 {
                return Ok(Some(Token::new(
                    TokenKind::Newline,
                    Span::new(start_pos, start_pos + 1, start_line, start_col),
                )));
            } else {
                // Inside brackets, newlines are ignored
                return self.next_token();
            }
        }

        // Multi-character operator check:
        // ***, **, *=, *
        if c == '*' {
            self.advance_char();
            if self.peek_char() == Some('*') {
                self.advance_char();
                if self.peek_char() == Some('*') {
                    self.advance_char();
                    return Ok(Some(Token::new(
                        TokenKind::TripleStar,
                        Span::new(start_pos, start_pos + 3, start_line, start_col),
                    )));
                }
                return Ok(Some(Token::new(
                    TokenKind::DoubleStar,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            if self.peek_char() == Some('=') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::StarEq,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            return Ok(Some(Token::new(
                TokenKind::Star,
                Span::new(start_pos, start_pos + 1, start_line, start_col),
            )));
        }

        // += or +
        if c == '+' {
            self.advance_char();
            if self.peek_char() == Some('=') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::PlusEq,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            return Ok(Some(Token::new(
                TokenKind::Plus,
                Span::new(start_pos, start_pos + 1, start_line, start_col),
            )));
        }

        // ->, -= or -
        if c == '-' {
            self.advance_char();
            if self.peek_char() == Some('>') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::Arrow,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            if self.peek_char() == Some('=') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::MinusEq,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            return Ok(Some(Token::new(
                TokenKind::Minus,
                Span::new(start_pos, start_pos + 1, start_line, start_col),
            )));
        }

        // //, /= or /
        if c == '/' {
            self.advance_char();
            if self.peek_char() == Some('/') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::DoubleSlash,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            if self.peek_char() == Some('=') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::SlashEq,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            return Ok(Some(Token::new(
                TokenKind::Slash,
                Span::new(start_pos, start_pos + 1, start_line, start_col),
            )));
        }

        // ... or .
        if c == '.' {
            if self.peek_next_char() == Some('.') && self.peek_char_at(2) == Some('.') {
                self.advance_char();
                self.advance_char();
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::Ellipsis,
                    Span::new(start_pos, start_pos + 3, start_line, start_col),
                )));
            }
            self.advance_char();
            return Ok(Some(Token::new(
                TokenKind::Dot,
                Span::new(start_pos, start_pos + 1, start_line, start_col),
            )));
        }

        // == or =
        if c == '=' {
            self.advance_char();
            if self.peek_char() == Some('=') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::EqEq,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            return Ok(Some(Token::new(
                TokenKind::Eq,
                Span::new(start_pos, start_pos + 1, start_line, start_col),
            )));
        }

        // != or !
        if c == '!' {
            self.advance_char();
            if self.peek_char() == Some('=') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::NotEq,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            return Ok(Some(Token::new(
                TokenKind::Bang,
                Span::new(start_pos, start_pos + 1, start_line, start_col),
            )));
        }

        // <=, <<, <
        if c == '<' {
            self.advance_char();
            if self.peek_char() == Some('=') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::LtEq,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            if self.peek_char() == Some('<') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::Shl,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            return Ok(Some(Token::new(
                TokenKind::Lt,
                Span::new(start_pos, start_pos + 1, start_line, start_col),
            )));
        }

        // >=, >>, >
        if c == '>' {
            self.advance_char();
            if self.peek_char() == Some('=') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::GtEq,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            if self.peek_char() == Some('>') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::Shr,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            return Ok(Some(Token::new(
                TokenKind::Gt,
                Span::new(start_pos, start_pos + 1, start_line, start_col),
            )));
        }

        // := or :
        if c == ':' {
            self.advance_char();
            if self.peek_char() == Some('=') {
                self.advance_char();
                return Ok(Some(Token::new(
                    TokenKind::Walrus,
                    Span::new(start_pos, start_pos + 2, start_line, start_col),
                )));
            }
            return Ok(Some(Token::new(
                TokenKind::Colon,
                Span::new(start_pos, start_pos + 1, start_line, start_col),
            )));
        }

        // Single-character punctuation
        let single_tok = match c {
            '(' => { self.open_brackets += 1; Some(TokenKind::LParen) }
            ')' => { self.open_brackets = self.open_brackets.saturating_sub(1); Some(TokenKind::RParen) }
            '[' => { self.open_brackets += 1; Some(TokenKind::LBracket) }
            ']' => { self.open_brackets = self.open_brackets.saturating_sub(1); Some(TokenKind::RBracket) }
            '{' => { self.open_brackets += 1; Some(TokenKind::LBrace) }
            '}' => { self.open_brackets = self.open_brackets.saturating_sub(1); Some(TokenKind::RBrace) }
            ',' => Some(TokenKind::Comma),
            ';' => Some(TokenKind::Semi),
            '@' => Some(TokenKind::At),
            '%' => Some(TokenKind::Percent),
            '|' => Some(TokenKind::Pipe),
            '^' => Some(TokenKind::Caret),
            '~' => Some(TokenKind::Tilde),
            '&' => Some(TokenKind::Amp),
            '?' => Some(TokenKind::Question),
            _ => None,
        };

        if let Some(kind) = single_tok {
            self.advance_char();
            return Ok(Some(Token::new(
                kind,
                Span::new(start_pos, start_pos + 1, start_line, start_col),
            )));
        }

        // String literals: "...", '...', """...""", '''...'''
        if c == '"' || c == '\'' {
            return self.lex_string(c, start_pos, start_line, start_col);
        }

        // Number literals
        if c.is_ascii_digit() {
            return self.lex_number(start_pos, start_line, start_col);
        }

        // Identifiers and keywords (including r"...", f"..." prefixes)
        if c.is_alphabetic() || c == '_' {
            // Check for f"..." or r"..."
            if (c == 'r' || c == 'f') && (self.peek_next_char() == Some('"') || self.peek_next_char() == Some('\'')) {
                let quote = self.peek_next_char().unwrap();
                self.advance_char(); // advance prefix
                return self.lex_string(quote, start_pos, start_line, start_col);
            }

            return self.lex_ident_or_keyword(start_pos, start_line, start_col);
        }

        Err(LexerError {
            message: format!("unexpected character '{c}'"),
            span: Span::new(start_pos, start_pos + c.len_utf8(), start_line, start_col),
        })
    }

    fn lex_string(&mut self, quote: char, start_pos: usize, start_line: usize, start_col: usize) -> Result<Option<Token>, LexerError> {
        self.advance_char(); // consume first quote

        // Check for triple quotes
        let is_triple = if self.peek_char() == Some(quote) && self.peek_next_char() == Some(quote) {
            self.advance_char();
            self.advance_char();
            true
        } else {
            false
        };

        let mut content = String::new();
        loop {
            match self.peek_char() {
                None => {
                    return Err(LexerError {
                        message: "unterminated string literal".to_string(),
                        span: Span::new(start_pos, self.source.len(), start_line, start_col),
                    });
                }
                Some('\\') => {
                    self.advance_char();
                    match self.advance_char() {
                        Some('n') => content.push('\n'),
                        Some('r') => content.push('\r'),
                        Some('t') => content.push('\t'),
                        Some('\\') => content.push('\\'),
                        Some('\'') => content.push('\''),
                        Some('"') => content.push('"'),
                        Some(other) => {
                            content.push('\\');
                            content.push(other);
                        }
                        None => {}
                    }
                }
                Some(c) if c == quote => {
                    if is_triple {
                        if self.peek_next_char() == Some(quote) && self.peek_char_at(2) == Some(quote) {
                            self.advance_char();
                            self.advance_char();
                            self.advance_char();
                            break;
                        } else {
                            content.push(c);
                            self.advance_char();
                        }
                    } else {
                        self.advance_char();
                        break;
                    }
                }
                Some(c) => {
                    content.push(c);
                    self.advance_char();
                }
            }
        }

        let (end_pos, _, _) = self.current_pos();
        Ok(Some(Token::new(
            TokenKind::Str(content),
            Span::new(start_pos, end_pos, start_line, start_col),
        )))
    }

    fn lex_number(&mut self, start_pos: usize, start_line: usize, start_col: usize) -> Result<Option<Token>, LexerError> {
        let mut is_float = false;
        let mut num_str = String::new();

        while let Some(c) = self.peek_char() {
            if c.is_ascii_digit() || c == '_' {
                if c != '_' {
                    num_str.push(c);
                }
                self.advance_char();
            } else if c == '.' && !is_float && self.peek_next_char().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                is_float = true;
                num_str.push(c);
                self.advance_char();
            } else if (c == 'e' || c == 'E') && !num_str.is_empty() {
                is_float = true;
                num_str.push(c);
                self.advance_char();
                if let Some(sign) = self.peek_char() {
                    if sign == '+' || sign == '-' {
                        num_str.push(sign);
                        self.advance_char();
                    }
                }
            } else {
                break;
            }
        }

        if let Some(c) = self.peek_char() {
            if c == 'j' || c == 'J' {
                is_float = true;
                self.advance_char();
            }
        }

        let (end_pos, _, _) = self.current_pos();
        let span = Span::new(start_pos, end_pos, start_line, start_col);

        if is_float {
            match num_str.parse::<f64>() {
                Ok(f) => Ok(Some(Token::new(TokenKind::Float(f), span))),
                Err(_) => Err(LexerError {
                    message: format!("invalid float literal '{num_str}'"),
                    span,
                }),
            }
        } else {
            match num_str.parse::<i64>() {
                Ok(n) => Ok(Some(Token::new(TokenKind::Int(n), span))),
                Err(_) => Err(LexerError {
                    message: format!("invalid integer literal '{num_str}'"),
                    span,
                }),
            }
        }
    }

    fn lex_ident_or_keyword(&mut self, start_pos: usize, start_line: usize, start_col: usize) -> Result<Option<Token>, LexerError> {
        let mut text = String::new();
        while let Some(c) = self.peek_char() {
            if c.is_alphanumeric() || c == '_' {
                text.push(c);
                self.advance_char();
            } else {
                break;
            }
        }

        let (end_pos, _, _) = self.current_pos();
        let span = Span::new(start_pos, end_pos, start_line, start_col);

        let kind = match text.as_str() {
            // Lowercase constants
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "none" => TokenKind::None,

            // Lucid keywords
            "export" => TokenKind::Export,
            "factory" => TokenKind::Factory,
            "construct" => TokenKind::Construct,
            "getter" => TokenKind::Getter,
            "setter" => TokenKind::Setter,
            "final" => TokenKind::Final,
            "sealed" => TokenKind::Sealed,
            "override" => TokenKind::Override,
            "without" => TokenKind::Without,
            "interface" => TokenKind::Interface,
            "trait" => TokenKind::Trait,
            "class" => TokenKind::Class,
            "implement" => TokenKind::Implement,
            "dispatch" => TokenKind::Dispatch,
            "type" => TokenKind::Type,
            "any" => TokenKind::Any,
            "trust" => TokenKind::Trust,
            "match" => TokenKind::Match,
            "case" => TokenKind::Case,
            "if_broken" => TokenKind::IfBroken,
            "skip" => TokenKind::Skip,
            "let" => TokenKind::Let,
            "caller" => TokenKind::Caller,
            "from_var_name" => TokenKind::FromVarName,
            "classmethod" => TokenKind::ClassMethod,
            "classvar" => TokenKind::ClassVar,

            // Preserved Python keywords
            "and" => TokenKind::And,
            "as" => TokenKind::As,
            "assert" => TokenKind::Assert,
            "async" => TokenKind::Async,
            "await" => TokenKind::Await,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "def" => TokenKind::Def,
            "del" => TokenKind::Del,
            "elif" => TokenKind::Elif,
            "else" => TokenKind::Else,
            "except" => TokenKind::Except,
            "finally" => TokenKind::Finally,
            "for" => TokenKind::For,
            "from" => TokenKind::From,
            "if" => TokenKind::If,
            "import" => TokenKind::Import,
            "in" => TokenKind::In,
            "is" => TokenKind::Is,
            "not" => TokenKind::Not,
            "or" => TokenKind::Or,
            "pass" => TokenKind::Pass,
            "raise" => TokenKind::Raise,
            "return" => TokenKind::Return,
            "try" => TokenKind::Try,
            "while" => TokenKind::While,
            "with" => TokenKind::With,
            "yield" => TokenKind::Yield,

            // Discarded keywords
            "global" => TokenKind::Global,
            "nonlocal" => TokenKind::Nonlocal,
            "lambda" => TokenKind::Lambda,

            _ => TokenKind::Ident(text),
        };

        Ok(Some(Token::new(kind, span)))
    }
}
