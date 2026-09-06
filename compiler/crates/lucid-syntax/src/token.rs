use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

impl Span {
    pub fn new(start: usize, end: usize, line: usize, column: usize) -> Self {
        Self { start, end, line, column }
    }

    pub fn merge(self, other: Span) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: self.line.min(other.line),
            column: if self.start <= other.start { self.column } else { other.column },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // --- Literals ---
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    True,
    False,
    None,

    // --- Lucid Keywords ---
    Export,
    Factory,
    Construct,
    Getter,
    Setter,
    Final,
    Sealed,
    Override,
    Without,
    Interface,
    Trait,
    Class,
    Implement,
    Dispatch,
    Type,
    Any,
    Trust,
    Match,
    Case,
    IfBroken,
    Skip,
    Let,
    Caller,
    FromVarName,
    ClassMethod,
    ClassVar,

    // --- Preserved Python Keywords ---
    And,
    As,
    Assert,
    Async,
    Await,
    Break,
    Continue,
    Def,
    Del,
    Elif,
    Else,
    Except,
    Finally,
    For,
    From,
    If,
    Import,
    In,
    Is,
    Not,
    Or,
    Pass,
    Raise,
    Return,
    Try,
    While,
    With,
    Yield,

    // --- Discarded Keywords (captured for clear error diagnostics) ---
    Global,
    Nonlocal,
    Lambda,

    // --- Operators & Symbols ---
    TripleStar, // ***
    DoubleStar, // **
    Star,       // *
    Arrow,      // ->
    Question,   // ?
    Bang,       // !
    Amp,        // &
    Plus,       // +
    Minus,      // -
    Slash,      // /
    DoubleSlash,// //
    Percent,    // %
    EqEq,       // ==
    NotEq,      // !=
    Lt,         // <
    LtEq,       // <=
    Gt,         // >
    GtEq,       // >=
    Pipe,       // |
    Caret,      // ^
    Tilde,      // ~
    Shl,        // <<
    Shr,        // >>
    Walrus,     // :=
    Eq,         // =
    PlusEq,     // +=
    MinusEq,    // -=
    StarEq,     // *=
    SlashEq,    // /=
    PercentEq,  // %=
    DoubleStarEq, // **=
    DoubleSlashEq, // //=
    At,         // @
    Dot,        // .
    Ellipsis,   // ...
    Comma,      // ,
    Colon,      // :
    Semi,       // ;
    LParen,     // (
    RParen,     // )
    LBracket,   // [
    RBracket,   // ]
    LBrace,     // {
    RBrace,     // }

    // --- Layout ---
    Newline,
    Indent,
    Dedent,
    Eof,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Ident(s) => write!(f, "identifier '{s}'"),
            TokenKind::Int(n) => write!(f, "integer {n}"),
            TokenKind::Float(n) => write!(f, "float {n}"),
            TokenKind::Str(s) => write!(f, "string \"{s}\""),
            TokenKind::True => write!(f, "'true'"),
            TokenKind::False => write!(f, "'false'"),
            TokenKind::None => write!(f, "'none'"),
            TokenKind::Export => write!(f, "'export'"),
            TokenKind::Factory => write!(f, "'factory'"),
            TokenKind::Construct => write!(f, "'construct'"),
            TokenKind::Getter => write!(f, "'getter'"),
            TokenKind::Setter => write!(f, "'setter'"),
            TokenKind::Final => write!(f, "'final'"),
            TokenKind::Sealed => write!(f, "'sealed'"),
            TokenKind::Override => write!(f, "'override'"),
            TokenKind::Without => write!(f, "'without'"),
            TokenKind::Interface => write!(f, "'interface'"),
            TokenKind::Trait => write!(f, "'trait'"),
            TokenKind::Class => write!(f, "'class'"),
            TokenKind::Implement => write!(f, "'implement'"),
            TokenKind::Dispatch => write!(f, "'dispatch'"),
            TokenKind::Type => write!(f, "'type'"),
            TokenKind::Any => write!(f, "'any'"),
            TokenKind::Trust => write!(f, "'trust'"),
            TokenKind::Match => write!(f, "'match'"),
            TokenKind::Case => write!(f, "'case'"),
            TokenKind::IfBroken => write!(f, "'if_broken'"),
            TokenKind::Skip => write!(f, "'skip'"),
            TokenKind::Let => write!(f, "'let'"),
            TokenKind::Caller => write!(f, "'caller'"),
            TokenKind::FromVarName => write!(f, "'from_var_name'"),
            TokenKind::ClassMethod => write!(f, "'classmethod'"),
            TokenKind::ClassVar => write!(f, "'classvar'"),
            TokenKind::And => write!(f, "'and'"),
            TokenKind::As => write!(f, "'as'"),
            TokenKind::Assert => write!(f, "'assert'"),
            TokenKind::Async => write!(f, "'async'"),
            TokenKind::Await => write!(f, "'await'"),
            TokenKind::Break => write!(f, "'break'"),
            TokenKind::Continue => write!(f, "'continue'"),
            TokenKind::Def => write!(f, "'def'"),
            TokenKind::Del => write!(f, "'del'"),
            TokenKind::Elif => write!(f, "'elif'"),
            TokenKind::Else => write!(f, "'else'"),
            TokenKind::Except => write!(f, "'except'"),
            TokenKind::Finally => write!(f, "'finally'"),
            TokenKind::For => write!(f, "'for'"),
            TokenKind::From => write!(f, "'from'"),
            TokenKind::If => write!(f, "'if'"),
            TokenKind::Import => write!(f, "'import'"),
            TokenKind::In => write!(f, "'in'"),
            TokenKind::Is => write!(f, "'is'"),
            TokenKind::Not => write!(f, "'not'"),
            TokenKind::Or => write!(f, "'or'"),
            TokenKind::Pass => write!(f, "'pass'"),
            TokenKind::Raise => write!(f, "'raise'"),
            TokenKind::Return => write!(f, "'return'"),
            TokenKind::Try => write!(f, "'try'"),
            TokenKind::While => write!(f, "'while'"),
            TokenKind::With => write!(f, "'with'"),
            TokenKind::Yield => write!(f, "'yield'"),
            TokenKind::Global => write!(f, "'global' (discarded in Lucid)"),
            TokenKind::Nonlocal => write!(f, "'nonlocal' (discarded in Lucid)"),
            TokenKind::Lambda => write!(f, "'lambda' (discarded in Lucid; use anonymous 'def')"),
            TokenKind::TripleStar => write!(f, "'***'"),
            TokenKind::DoubleStar => write!(f, "'**'"),
            TokenKind::Star => write!(f, "'*'"),
            TokenKind::Arrow => write!(f, "'->'"),
            TokenKind::Question => write!(f, "'?'"),
            TokenKind::Bang => write!(f, "'!'"),
            TokenKind::Amp => write!(f, "'&'"),
            TokenKind::Plus => write!(f, "'+'"),
            TokenKind::Minus => write!(f, "'-'"),
            TokenKind::Slash => write!(f, "'/'"),
            TokenKind::DoubleSlash => write!(f, "'//'"),
            TokenKind::Percent => write!(f, "'%'"),
            TokenKind::EqEq => write!(f, "'=='"),
            TokenKind::NotEq => write!(f, "'!='"),
            TokenKind::Lt => write!(f, "'<'"),
            TokenKind::LtEq => write!(f, "'<='"),
            TokenKind::Gt => write!(f, "'>'"),
            TokenKind::GtEq => write!(f, "'>='"),
            TokenKind::Pipe => write!(f, "'|'"),
            TokenKind::Caret => write!(f, "'^'"),
            TokenKind::Tilde => write!(f, "'~'"),
            TokenKind::Shl => write!(f, "'<<'"),
            TokenKind::Shr => write!(f, "'>>'"),
            TokenKind::Walrus => write!(f, "':='"),
            TokenKind::Eq => write!(f, "'='"),
            TokenKind::PlusEq => write!(f, "'+='"),
            TokenKind::MinusEq => write!(f, "'-='"),
            TokenKind::StarEq => write!(f, "'*='"),
            TokenKind::SlashEq => write!(f, "'/='"),
            TokenKind::PercentEq => write!(f, "'%='"),
            TokenKind::DoubleStarEq => write!(f, "'**='"),
            TokenKind::DoubleSlashEq => write!(f, "'//='"),
            TokenKind::At => write!(f, "'@'"),
            TokenKind::Dot => write!(f, "'.'"),
            TokenKind::Ellipsis => write!(f, "'...'"),
            TokenKind::Comma => write!(f, "','"),
            TokenKind::Colon => write!(f, "':'"),
            TokenKind::Semi => write!(f, "';'"),
            TokenKind::LParen => write!(f, "'('"),
            TokenKind::RParen => write!(f, "')'"),
            TokenKind::LBracket => write!(f, "'['"),
            TokenKind::RBracket => write!(f, "']'"),
            TokenKind::LBrace => write!(f, "'{{'"),
            TokenKind::RBrace => write!(f, "'}}'"),
            TokenKind::Newline => write!(f, "NEWLINE"),
            TokenKind::Indent => write!(f, "INDENT"),
            TokenKind::Dedent => write!(f, "DEDENT"),
            TokenKind::Eof => write!(f, "end of file"),
        }
    }
}
