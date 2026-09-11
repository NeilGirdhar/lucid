use crate::token::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum MutabilityView {
    Mutable,   // T
    ReadOnly,  // &T
    Immutable, // !T
}

#[derive(Debug, Clone, PartialEq)]
pub enum Variance {
    Invariant,     // =K
    Covariant,     // +K
    Contravariant, // -K
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    pub name: String,
    pub variance: Variance,
    pub bound: Option<TypeExpr>,
    pub is_higher_kinded: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordFieldType {
    pub name: Option<String>,
    pub type_expr: TypeExpr,
    pub is_positional_only: bool,
    pub is_keyword_only: bool,
    pub is_variadic_positional: bool,
    pub is_variadic_keyword: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named {
        name: String,
        args: Vec<TypeExpr>,
        span: Span,
    },
    Function {
        params: Vec<TypeExpr>,
        return_type: Box<TypeExpr>,
        span: Span,
    },
    Record {
        fields: Vec<RecordFieldType>,
        span: Span,
    },
    View {
        mutability: MutabilityView,
        inner: Box<TypeExpr>,
        span: Span,
    },
    Union {
        types: Vec<TypeExpr>,
        span: Span,
    },
    Literal {
        value: LiteralValue,
        span: Span,
    },
    Existential {
        interface: Box<TypeExpr>,
        span: Span,
    },
    Reification {
        inner: Box<TypeExpr>,
        span: Span,
    },
    Match {
        subject: Vec<TypeExpr>,
        arms: Vec<(TypeExpr, TypeExpr)>,
        span: Span,
    },
    Wildcard(Span),
    Never(Span),
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Named { span, .. } => *span,
            TypeExpr::Function { span, .. } => *span,
            TypeExpr::Record { span, .. } => *span,
            TypeExpr::View { span, .. } => *span,
            TypeExpr::Union { span, .. } => *span,
            TypeExpr::Literal { span, .. } => *span,
            TypeExpr::Existential { span, .. } => *span,
            TypeExpr::Reification { span, .. } => *span,
            TypeExpr::Match { span, .. } => *span,
            TypeExpr::Wildcard(span) => *span,
            TypeExpr::Never(span) => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiteralValue {
    Int(i64),
    BigInt(String),
    Float(f64),
    Complex(f64),
    Str(String),
    Bool(bool),
    None,
    Sentinel(String),
    Ellipsis,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
    Eq,
    NotEq,
    Identity,
    NotIdentity,
    Lt,
    LtEq,
    Gt,
    GtEq,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    And,
    Or,
    In,
    NotIn,
    Is,
    IsNot,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Pos,
    Neg,
    Not,
    Invert,
    Spread,
    GatherSpread,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub name: Option<String>,
    pub value: Expr,
    pub is_spread: bool,
    pub is_dict_spread: bool,   // **kwargs
    pub is_gather_spread: bool, // *** spread
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub pattern: Option<Pattern>,
    pub type_annotation: Option<TypeExpr>,
    pub default: Option<Expr>,
    pub is_positional_only: bool,
    pub is_keyword_only: bool,
    pub is_variadic_positional: bool, // *args
    pub is_variadic_keyword: bool,    // **kwargs
    pub is_gather: bool,              // ***args
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal {
        value: LiteralValue,
        span: Span,
    },
    Ident {
        name: String,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },
    Call {
        func: Box<Expr>,
        args: Vec<Arg>,
        span: Span,
    },
    Construct {
        args: Vec<Arg>,
        span: Span,
    },
    Propagate {
        expr: Box<Expr>,
        span: Span,
    },
    Await {
        expr: Box<Expr>,
        span: Span,
    },
    Attribute {
        value: Box<Expr>,
        attr: String,
        span: Span,
    },
    Index {
        value: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    Slice {
        start: Option<Box<Expr>>,
        stop: Option<Box<Expr>>,
        step: Option<Box<Expr>>,
        span: Span,
    },
    Record {
        fields: Vec<(Option<String>, Expr)>,
        span: Span,
    },
    List {
        elements: Vec<Expr>,
        span: Span,
    },
    Dict {
        entries: Vec<(Expr, Expr)>,
        span: Span,
    },
    Set {
        elements: Vec<Expr>,
        span: Span,
    },
    AnonymousDef {
        params: Vec<Param>,
        return_type: Option<TypeExpr>,
        body: Vec<Stmt>,
        span: Span,
    },
    Trust {
        target_type: TypeExpr,
        expr: Box<Expr>,
        span: Span,
    },
    Freeze {
        expr: Box<Expr>,
        span: Span,
    },
    Skip(Span),
    Type(TypeExpr),
    ListComp {
        element: Box<Expr>,
        target: Pattern,
        iter: Box<Expr>,
        condition: Option<Box<Expr>>,
        span: Span,
    },
    SetComp {
        element: Box<Expr>,
        target: Pattern,
        iter: Box<Expr>,
        condition: Option<Box<Expr>>,
        span: Span,
    },
    DictComp {
        key: Box<Expr>,
        value: Box<Expr>,
        target: Pattern,
        iter: Box<Expr>,
        condition: Option<Box<Expr>>,
        span: Span,
    },
    IfExpr {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal { span, .. } => *span,
            Expr::Ident { span, .. } => *span,
            Expr::Binary { span, .. } => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::Construct { span, .. } => *span,
            Expr::Propagate { span, .. } => *span,
            Expr::Await { span, .. } => *span,
            Expr::Attribute { span, .. } => *span,
            Expr::Index { span, .. } => *span,
            Expr::Slice { span, .. } => *span,
            Expr::Record { span, .. } => *span,
            Expr::List { span, .. } => *span,
            Expr::Dict { span, .. } => *span,
            Expr::Set { span, .. } => *span,
            Expr::AnonymousDef { span, .. } => *span,
            Expr::Trust { span, .. } => *span,
            Expr::Freeze { span, .. } => *span,
            Expr::Skip(span) => *span,
            Expr::Type(t) => t.span(),
            Expr::ListComp { span, .. } => *span,
            Expr::SetComp { span, .. } => *span,
            Expr::DictComp { span, .. } => *span,
            Expr::IfExpr { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Ident(String, Span),
    Literal(LiteralValue, Span),
    ClassDestructure {
        class_name: String,
        fields: Vec<(Option<String>, Pattern)>,
        span: Span,
    },
    RecordDestructure(Vec<(Option<String>, Pattern)>, Span),
    Tuple(Vec<Pattern>, Span),
    Star(Box<Pattern>, Span),
    Wildcard(Span),
    Type(TypeExpr, Span),
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Ident(_, s) => *s,
            Pattern::Literal(_, s) => *s,
            Pattern::ClassDestructure { span, .. } => *span,
            Pattern::RecordDestructure(_, s) => *s,
            Pattern::Tuple(_, s) => *s,
            Pattern::Star(_, s) => *s,
            Pattern::Wildcard(s) => *s,
            Pattern::Type(_, s) => *s,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub type_narrow: Option<TypeExpr>,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExceptHandler {
    pub exception_type: TypeExpr,
    pub name: Option<String>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub body: Vec<Stmt>,
    pub is_dispatch: bool,
    pub is_async: bool,
    pub is_override: bool,
    pub decorators: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FactoryDef {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GetterDef {
    pub name: String,
    pub return_type: Option<TypeExpr>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetterDef {
    pub name: String,
    pub param: Param,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub name: String,
    pub type_annotation: TypeExpr,
    pub default: Option<Expr>,
    pub is_final: bool,
    pub doc: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClassMember {
    Field(FieldDef),
    Method(FunctionDef),
    Factory(FactoryDef),
    Getter(GetterDef),
    Setter(SetterDef),
    ClassMethod(FunctionDef),
    ClassVar(FieldDef),
    TypeAlias {
        name: String,
        type_params: Vec<TypeParam>,
        value: TypeAliasValue,
        span: Span,
    },
    Pass(Span),
    Ellipsis(Span),
}

#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceMember {
    MethodSig {
        name: String,
        type_params: Vec<TypeParam>,
        params: Vec<Param>,
        return_type: Option<TypeExpr>,
        span: Span,
    },
    GetterSig {
        name: String,
        return_type: Option<TypeExpr>,
        span: Span,
    },
    SetterSig {
        name: String,
        param_type: TypeExpr,
        span: Span,
    },
    ClassMethodSig {
        name: String,
        type_params: Vec<TypeParam>,
        params: Vec<Param>,
        return_type: Option<TypeExpr>,
        span: Span,
    },
    FactorySig {
        name: String,
        type_params: Vec<TypeParam>,
        params: Vec<Param>,
        return_type: Option<TypeExpr>,
        span: Span,
    },
    FieldSig {
        name: String,
        type_annotation: TypeExpr,
        is_final: bool,
        span: Span,
    },
    AssociatedTypeSig {
        name: String,
        bound: Option<TypeExpr>,
        span: Span,
    },
    Pass(Span),
    Ellipsis(Span),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TraitMember {
    Method(FunctionDef),
    ClassMethod(FunctionDef),
    Getter(GetterDef),
    Setter(SetterDef),
    Field(FieldDef),
    Pass(Span),
    Ellipsis(Span),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeAliasValue {
    Direct(TypeExpr),
    Match {
        subject: Vec<TypeExpr>,
        arms: Vec<(TypeExpr, TypeExpr)>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct WithItem {
    pub context_expr: Expr,
    pub target: Option<Pattern>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Export(Box<Stmt>),
    ClassDef {
        name: String,
        type_params: Vec<TypeParam>,
        bases: Vec<TypeExpr>,
        without_traits: Vec<String>,
        body: Vec<ClassMember>,
        is_sealed: bool,
        is_final: bool,
        span: Span,
    },
    InterfaceDef {
        name: String,
        type_params: Vec<TypeParam>,
        bases: Vec<TypeExpr>,
        body: Vec<InterfaceMember>,
        span: Span,
    },
    TraitDef {
        name: String,
        type_params: Vec<TypeParam>,
        bases: Vec<TypeExpr>,
        body: Vec<TraitMember>,
        span: Span,
    },
    ImplementDef {
        interface: TypeExpr,
        target: TypeExpr,
        body: Vec<FunctionDef>,
        span: Span,
    },
    TypeAlias {
        name: String,
        type_params: Vec<TypeParam>,
        value: TypeAliasValue,
        span: Span,
    },
    Function(FunctionDef),
    VarDef {
        pattern: Pattern,
        type_annotation: Option<TypeExpr>,
        value: Option<Expr>,
        is_let: bool,
        is_final: bool,
        span: Span,
    },
    With {
        items: Vec<WithItem>,
        body: Vec<Stmt>,
        span: Span,
    },
    Assignment {
        target: Expr,
        value: Expr,
        span: Span,
    },
    AugAssign {
        target: Expr,
        op: BinaryOp,
        value: Expr,
        span: Span,
    },
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        elif_branches: Vec<(Expr, Vec<Stmt>)>,
        else_branch: Option<Vec<Stmt>>,
        span: Span,
    },
    For {
        target: Pattern,
        iterable: Expr,
        body: Vec<Stmt>,
        if_broken: Option<Vec<Stmt>>,
        span: Span,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
        if_broken: Option<Vec<Stmt>>,
        span: Span,
    },
    Match {
        subject: Expr,
        subject_alias: Option<String>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    Try {
        body: Vec<Stmt>,
        handlers: Vec<ExceptHandler>,
        finally_body: Option<Vec<Stmt>>,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Raise {
        exception: Expr,
        span: Span,
    },
    Yield {
        value: Expr,
        span: Span,
    },
    Assert {
        condition: Expr,
        message: Option<Expr>,
        span: Span,
    },
    Delete {
        names: Vec<String>,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    Pass(Span),
    Import {
        module: String,
        alias: Option<String>,
        span: Span,
    },
    FromImport {
        module: String,
        names: Vec<(String, Option<String>)>,
        is_export: bool,
        span: Span,
    },
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub statements: Vec<Stmt>,
    pub span: Span,
}
