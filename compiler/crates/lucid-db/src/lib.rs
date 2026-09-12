//! Incremental compiler database.
//!
//! This crate deliberately starts at the source boundary.  Semantic queries
//! are added only after their inputs have stable IDs; that prevents the old
//! string-keyed checker state from leaking into the new pipeline.

use std::collections::HashMap;
use std::sync::Arc;

use lucid_syntax::{Module, parse, parse_lossless};
use rowan::GreenNode;

#[salsa::db]
pub trait Db: salsa::Database {}

#[salsa::db]
#[derive(Default)]
pub struct CompilerDatabase {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for CompilerDatabase {}

#[salsa::db]
impl Db for CompilerDatabase {}

#[salsa::input]
#[derive(Debug)]
pub struct SourceFile {
    #[returns(ref)]
    pub text: String,
    #[returns(ref)]
    pub path: String,
}

#[salsa::input]
pub struct Project {
    #[returns(ref)]
    pub files: Vec<SourceFile>,
}

/// A stable declaration identity.  Names are only the spelling attached to a
/// symbol; all later queries use this interned identity plus its defining file.
#[salsa::interned]
#[derive(Debug)]
pub struct Symbol {
    pub file: SourceFile,
    pub name: String,
}

/// Canonical type identity shared by typed-HIR consumers. The spelling is
/// retained as the interned key for now; downstream queries use the stable ID
/// rather than comparing independently formatted type strings.
#[salsa::interned]
#[derive(Debug)]
pub struct TypeId {
    pub canonical: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    Class,
    Interface,
    Trait,
    TypeAlias,
    Function,
    Value,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::SalsaValue)]
pub struct ResolvedDecl<'db> {
    pub symbol: Symbol<'db>,
    pub kind: DeclKind,
    pub is_dispatch: bool,
    pub exported: bool,
    pub span: lucid_syntax::Span,
}

/// The resolved, module-level HIR boundary.  All names crossing this
/// boundary have interned identities; consumers do not need to inspect the
/// parser AST to answer declaration or import questions.
#[derive(Debug, Clone, PartialEq, Eq, salsa::SalsaValue)]
pub struct ResolvedModule<'db> {
    pub file: SourceFile,
    pub declarations: Arc<[ResolvedDecl<'db>]>,
    pub imports: Arc<[String]>,
}

/// A name made visible by an import, retaining both its local spelling and
/// the defining symbol identity.
#[derive(Debug, Clone, PartialEq, Eq, salsa::SalsaValue)]
pub struct ResolvedBinding<'db> {
    pub local_name: String,
    pub symbol: Symbol<'db>,
}

/// Validated module-level HIR. Initializers and their expression trees carry
/// interned canonical types; the `checked` flag records the validation
/// boundary consumed by downstream lowering.
#[derive(Debug, Clone, PartialEq, Eq, salsa::SalsaValue)]
pub struct TypedModule<'db> {
    pub resolved: Arc<ResolvedModule<'db>>,
    pub initializers: Arc<[TypedInitializer<'db>]>,
    pub functions: Arc<[TypedFunction<'db>]>,
    /// Every expression reachable from a top-level initializer, in post-order.
    /// Child IDs therefore always refer to an earlier node in this slice.
    pub expressions: Arc<[TypedExpr<'db>]>,
    pub checked: bool,
}

/// The parser-independent shape of a typed expression. `kind` is deliberately
/// semantic rather than an AST enum: downstream consumers can match stable
/// tags without depending on parser implementation details, while `children`
/// provides the complete expression tree.
#[derive(Debug, Clone, PartialEq, Eq, salsa::SalsaValue)]
pub struct TypedExpr<'db> {
    pub id: u32,
    pub type_id: TypeId<'db>,
    pub type_name: String,
    pub kind: String,
    /// Operator, identifier, or attribute spelling when the kind has one.
    /// It is `None` for structural nodes.
    pub detail: Option<String>,
    pub children: Arc<[u32]>,
    /// Primitive literal payload retained so typed-HIR lowering does not
    /// need to recover values by reparsing source text.
    pub literal: Option<lucid_cir::TypedLiteral>,
    pub span: lucid_syntax::Span,
}

/// A typed top-level initializer crossing the checker/HIR boundary. The
/// symbol remains interned while the canonical type text is stable and
/// serializable for snapshots and downstream backends.
#[derive(Debug, Clone, PartialEq, Eq, salsa::SalsaValue)]
pub struct TypedInitializer<'db> {
    pub symbol: Symbol<'db>,
    pub type_id: TypeId<'db>,
    pub type_name: String,
    /// Root node in [`TypedModule::expressions`] for this initializer.
    pub expression: u32,
    pub span: lucid_syntax::Span,
}

/// A checked top-level function signature crossing the typed-HIR boundary.
/// Each component is interned so downstream lowering can compare identities
/// without reparsing or relying on printed type names.
#[derive(Debug, Clone, PartialEq, Eq, salsa::SalsaValue)]
pub struct TypedFunction<'db> {
    pub symbol: Symbol<'db>,
    pub is_dispatch: bool,
    pub is_async: bool,
    pub parameter_names: Arc<[String]>,
    pub required_parameters: Arc<[bool]>,
    pub parameter_types: Arc<[TypeId<'db>]>,
    pub return_type: TypeId<'db>,
    /// All declared overloads, in declaration order. Ordinary functions have
    /// one entry; dispatch functions retain the complete candidate set.
    pub overload_types: Arc<[TypeId<'db>]>,
    /// Post-order typed expression graph for this function's defaults and
    /// body. IDs are local to the function, so separate overloads cannot
    /// accidentally alias one another.
    pub body_expressions: Arc<[TypedExpr<'db>]>,
    pub span: lucid_syntax::Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, salsa::SalsaValue)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::SalsaValue)]
pub struct Diagnostic {
    pub file: SourceFile,
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub span: lucid_syntax::Span,
}

#[salsa::tracked]
pub fn parse_file(db: &dyn Db, file: SourceFile) -> Arc<GreenNode> {
    Arc::new(parse_lossless(file.text(db)).green().clone().into_owned())
}

#[salsa::tracked]
pub fn source_position(db: &dyn Db, file: SourceFile, byte_offset: u32) -> (u32, u32) {
    let source = file.text(db);
    let mut offset = (byte_offset as usize).min(source.len());
    while offset > 0 && !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
    let tail = prefix.rsplit_once('\n').map_or(prefix, |(_, tail)| tail);
    let tail = tail.strip_suffix('\r').unwrap_or(tail);
    let column = tail.chars().count() as u32 + 1;
    (line, column)
}

/// Convert a one-based Unicode line/column position to a UTF-8 byte offset.
/// Positions past the end of the file clamp to its length; columns never
/// split a code point, making this the safe inverse boundary for editors.
#[salsa::tracked]
pub fn source_offset(db: &dyn Db, file: SourceFile, line: u32, column: u32) -> u32 {
    let source = file.text(db);
    let target_line = line.max(1);
    let target_column = column.max(1);
    let mut current_line = 1u32;
    let mut current_column = 1u32;
    for (offset, character) in source.char_indices() {
        if current_line == target_line && current_column == target_column {
            return offset as u32;
        }
        if character == '\n' && current_line == target_line {
            // Clamp a column past the line's content to the start of its
            // terminator, excluding a preceding CR in CRLF input.
            return offset.saturating_sub(usize::from(
                offset > 0 && source.as_bytes()[offset - 1] == b'\r',
            )) as u32;
        }
        if character == '\n' {
            current_line += 1;
            current_column = 1;
        } else {
            current_column += 1;
        }
    }
    source.len() as u32
}

/// Return one-based source-line text without its line terminator. Both LF and
/// CRLF input are accepted, and out-of-range lines return an empty string.
#[salsa::tracked]
pub fn source_line(db: &dyn Db, file: SourceFile, line: u32) -> Arc<str> {
    let requested = line.max(1) as usize;
    let Some(raw) = file.text(db).split('\n').nth(requested - 1) else {
        return Arc::from("");
    };
    Arc::from(raw.strip_suffix('\r').unwrap_or(raw))
}

/// Return the UTF-8 source text covered by a byte span. Spans originate in
/// the lexer, but callers may request a range ending inside a code point while
/// rendering an editor diagnostic; clamp both boundaries to character
/// boundaries instead of panicking on an invalid slice.
#[salsa::tracked]
pub fn span_text(db: &dyn Db, file: SourceFile, span: lucid_syntax::Span) -> Arc<str> {
    let source = file.text(db);
    let mut start = span.start.min(source.len());
    let mut end = span.end.min(source.len());
    while start > 0 && !source.is_char_boundary(start) {
        start -= 1;
    }
    while end > start && !source.is_char_boundary(end) {
        end -= 1;
    }
    if end < start {
        return Arc::from("");
    }
    Arc::from(&source[start..end])
}

fn first_lexical_error_span(db: &dyn Db, file: SourceFile) -> lucid_syntax::Span {
    let cst = parse_file(db, file);
    let Some(range) = lucid_syntax::error_ranges(
        &rowan::SyntaxNode::<lucid_syntax::LucidLanguage>::new_root(cst.as_ref().clone()),
    )
    .into_iter()
    .next() else {
        return lucid_syntax::Span::default();
    };
    let start = u32::from(range.start());
    let end = u32::from(range.end());
    let (line, column) = *source_position(db, file, start);
    lucid_syntax::Span::new(start as usize, end as usize, line as usize, column as usize)
}

/// Return the parser's own span for a grammar error when the source is
/// lexically valid. `parse_ast` intentionally exposes a compact string for
/// callers, but structured diagnostics must retain the precise token range
/// reported by the recovering parser.
fn first_parse_error_span(db: &dyn Db, file: SourceFile) -> lucid_syntax::Span {
    match lucid_syntax::parse_recovering(file.text(db)) {
        Ok((_, errors)) => errors
            .into_iter()
            .next()
            .map(|error| error.span)
            .unwrap_or_default(),
        Err(_) => first_lexical_error_span(db, file),
    }
}

/// Strict semantic parse, kept separate from the lossless CST query so editor
/// recovery never becomes accidental language semantics.
#[salsa::tracked]
pub fn parse_ast(db: &dyn Db, file: SourceFile) -> Result<Arc<Module>, Arc<str>> {
    parse(file.text(db)).map(Arc::new).map_err(Arc::<str>::from)
}

/// Semantic validation query.  Keeping this behind the database means the
/// eventual resolved/typed HIR can replace the query body without changing
/// callers or reintroducing direct AST walks in the driver.
#[salsa::tracked]
pub fn type_check_file(db: &dyn Db, file: SourceFile) -> Result<(), Arc<str>> {
    typed_module(db, file)
        .as_ref()
        .map(|_| ())
        .map_err(Arc::clone)
}

#[salsa::tracked]
pub fn top_level_symbols<'db>(db: &'db dyn Db, file: SourceFile) -> Arc<[Symbol<'db>]> {
    let Ok(module) = parse_ast(db, file) else {
        return Arc::from([]);
    };
    let mut symbols = Vec::new();
    for statement in &module.statements {
        let statement = match statement {
            lucid_syntax::Stmt::Export(inner) => inner.as_ref(),
            other => other,
        };
        let name = match statement {
            lucid_syntax::Stmt::ClassDef { name, .. }
            | lucid_syntax::Stmt::InterfaceDef { name, .. }
            | lucid_syntax::Stmt::TraitDef { name, .. }
            | lucid_syntax::Stmt::TypeAlias { name, .. }
            | lucid_syntax::Stmt::Function(lucid_syntax::FunctionDef { name, .. }) => Some(name),
            lucid_syntax::Stmt::VarDef {
                pattern: lucid_syntax::Pattern::Ident(name, _),
                ..
            } => Some(name),
            _ => None,
        };
        if let Some(name) = name {
            symbols.push(Symbol::new(db, file, name.clone()));
        }
    }
    Arc::from(symbols)
}

/// Resolved declaration index used by later HIR and visibility queries.
#[salsa::tracked]
pub fn resolved_declarations<'db>(db: &'db dyn Db, file: SourceFile) -> Arc<[ResolvedDecl<'db>]> {
    let Ok(module) = parse_ast(db, file) else {
        return Arc::from([]);
    };
    let mut declarations = Vec::new();
    for statement in &module.statements {
        let (statement, _explicitly_exported) = match statement {
            lucid_syntax::Stmt::Export(inner) => (inner.as_ref(), true),
            other => (other, false),
        };
        let (name, kind, is_dispatch) = match statement {
            lucid_syntax::Stmt::ClassDef { name, .. } => (Some(name), DeclKind::Class, false),
            lucid_syntax::Stmt::InterfaceDef { name, .. } => {
                (Some(name), DeclKind::Interface, false)
            }
            lucid_syntax::Stmt::TraitDef { name, .. } => (Some(name), DeclKind::Trait, false),
            lucid_syntax::Stmt::TypeAlias { name, .. } => (Some(name), DeclKind::TypeAlias, false),
            lucid_syntax::Stmt::Function(lucid_syntax::FunctionDef {
                name, is_dispatch, ..
            }) => (Some(name), DeclKind::Function, *is_dispatch),
            lucid_syntax::Stmt::VarDef {
                pattern: lucid_syntax::Pattern::Ident(name, _),
                ..
            } => (Some(name), DeclKind::Value, false),
            lucid_syntax::Stmt::Assignment {
                target: lucid_syntax::Expr::Ident { name, .. },
                ..
            } => (Some(name), DeclKind::Value, false),
            _ => (None, DeclKind::Value, false),
        };
        let span = match statement {
            lucid_syntax::Stmt::ClassDef { span, .. }
            | lucid_syntax::Stmt::InterfaceDef { span, .. }
            | lucid_syntax::Stmt::TraitDef { span, .. }
            | lucid_syntax::Stmt::TypeAlias { span, .. }
            | lucid_syntax::Stmt::VarDef { span, .. }
            | lucid_syntax::Stmt::Assignment { span, .. } => *span,
            lucid_syntax::Stmt::Function(lucid_syntax::FunctionDef { span, .. }) => *span,
            _ => lucid_syntax::Span::default(),
        };
        if let Some(name) = name {
            declarations.push(ResolvedDecl {
                symbol: Symbol::new(db, file, name.clone()),
                kind,
                is_dispatch,
                exported: !name.starts_with('_'),
                span,
            });
        }
    }
    Arc::from(declarations)
}

#[salsa::tracked]
pub fn imports(db: &dyn Db, file: SourceFile) -> Arc<[String]> {
    let Ok(module) = parse_ast(db, file) else {
        return Arc::from([]);
    };
    let mut names = Vec::new();
    for statement in &module.statements {
        match statement {
            lucid_syntax::Stmt::Import { module, .. }
            | lucid_syntax::Stmt::FromImport { module, .. } => names.push(module.clone()),
            _ => {}
        }
    }
    names.sort();
    names.dedup();
    Arc::from(names)
}

#[salsa::tracked]
pub fn resolved_module<'db>(db: &'db dyn Db, file: SourceFile) -> Arc<ResolvedModule<'db>> {
    Arc::new(ResolvedModule {
        file,
        declarations: resolved_declarations(db, file).clone(),
        imports: imports(db, file).clone(),
    })
}

#[salsa::tracked]
pub fn imported_bindings<'db>(
    db: &'db dyn Db,
    project: Project,
    file: SourceFile,
) -> Arc<[ResolvedBinding<'db>]> {
    let Ok(module) = parse_ast(db, file) else {
        return Arc::from([]);
    };
    let mut bindings = Vec::new();
    for statement in &module.statements {
        if let lucid_syntax::Stmt::Import { module, alias, .. } = statement {
            let Some(imported_file) = resolve_import(db, project, file, module.clone()) else {
                continue;
            };
            let local_name = alias.clone().unwrap_or_else(|| {
                module
                    .rsplit('.')
                    .next()
                    .unwrap_or(module)
                    .trim_start_matches('.')
                    .to_string()
            });
            bindings.push(ResolvedBinding {
                local_name: local_name.clone(),
                symbol: Symbol::new(db, *imported_file, local_name),
            });
            continue;
        }
        let lucid_syntax::Stmt::FromImport { module, names, .. } = statement else {
            continue;
        };
        let Some(imported_file) = resolve_import(db, project, file, module.clone()) else {
            continue;
        };
        for (name, alias) in names {
            let Some(declaration) = resolved_declarations(db, *imported_file)
                .iter()
                .find(|decl| decl.exported && decl.symbol.name(db).as_str() == name)
            else {
                continue;
            };
            bindings.push(ResolvedBinding {
                local_name: alias.clone().unwrap_or_else(|| name.clone()),
                symbol: declaration.symbol,
            });
        }
    }
    bindings.sort_by(|left, right| left.local_name.cmp(&right.local_name));
    bindings.dedup_by(|left, right| left.local_name == right.local_name);
    Arc::from(bindings)
}

fn collect_typed_exprs<'db>(
    db: &'db dyn Db,
    checker: &lucid_checker::TypeChecker,
    expr: &lucid_syntax::Expr,
    nodes: &mut Vec<TypedExpr<'db>>,
) -> Result<u32, Arc<str>> {
    use lucid_syntax::Expr;
    let (kind, children): (&str, Vec<&Expr>) = match expr {
        Expr::Literal { .. } => ("literal", Vec::new()),
        Expr::Ident { .. } => ("name", Vec::new()),
        Expr::Binary {
            op, left, right, ..
        } => {
            let _ = op;
            ("binary", vec![left, right])
        }
        Expr::Unary { expr, .. } => ("unary", vec![expr]),
        Expr::Call { func, args, .. } => {
            let mut children = vec![func.as_ref()];
            children.extend(args.iter().map(|arg| &arg.value));
            ("call", children)
        }
        Expr::Construct { args, .. } => ("construct", args.iter().map(|arg| &arg.value).collect()),
        Expr::Propagate { expr, .. } => ("propagate", vec![expr]),
        Expr::Await { expr, .. } => ("await", vec![expr]),
        Expr::Attribute { value, .. } => ("attribute", vec![value]),
        Expr::Index { value, index, .. } => ("index", vec![value, index]),
        Expr::Slice {
            start, stop, step, ..
        } => {
            let mut children = Vec::new();
            children.extend(start.as_deref());
            children.extend(stop.as_deref());
            children.extend(step.as_deref());
            ("slice", children)
        }
        Expr::Record { fields, .. } => ("record", fields.iter().map(|(_, value)| value).collect()),
        Expr::List { elements, .. } => ("list", elements.iter().collect()),
        Expr::Dict { entries, .. } => (
            "dict",
            entries
                .iter()
                .flat_map(|(key, value)| [key, value])
                .collect(),
        ),
        Expr::Set { elements, .. } => ("set", elements.iter().collect()),
        Expr::AnonymousDef { body, .. } => {
            // Statements in an anonymous function are a separate body graph;
            // preserve the node here and let function lowering own that body.
            let _ = body;
            ("anonymous-function", Vec::new())
        }
        Expr::Trust { expr, .. } => ("trust", vec![expr]),
        Expr::Freeze { expr, .. } => ("freeze", vec![expr]),
        Expr::Skip(_) => ("skip", Vec::new()),
        Expr::Type(_) => ("type", Vec::new()),
        Expr::ListComp {
            element,
            iter,
            condition,
            ..
        } => {
            let mut children = vec![element.as_ref(), iter.as_ref()];
            children.extend(condition.as_deref());
            ("list-comprehension", children)
        }
        Expr::SetComp {
            element,
            iter,
            condition,
            ..
        } => {
            let mut children = vec![element.as_ref(), iter.as_ref()];
            children.extend(condition.as_deref());
            ("set-comprehension", children)
        }
        Expr::DictComp {
            key,
            value,
            iter,
            condition,
            ..
        } => {
            let mut children = vec![key.as_ref(), value.as_ref(), iter.as_ref()];
            children.extend(condition.as_deref());
            ("dict-comprehension", children)
        }
        Expr::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => ("if", vec![condition, then_branch, else_branch]),
    };
    let child_ids = children
        .into_iter()
        .map(|child| collect_typed_exprs(db, checker, child, nodes))
        .collect::<Result<Vec<_>, _>>()?;
    let detail = match expr {
        Expr::Ident { name, .. } => Some(name.clone()),
        Expr::Binary { op, .. } => Some(format!("{op:?}")),
        Expr::Unary { op, .. } => Some(format!("{op:?}")),
        Expr::Attribute { attr, .. } => Some(attr.clone()),
        _ => None,
    };
    let literal = match expr {
        Expr::Literal {
            value: lucid_syntax::LiteralValue::Int(value),
            ..
        } => Some(lucid_cir::TypedLiteral::Int(*value)),
        Expr::Literal {
            value: lucid_syntax::LiteralValue::Bool(value),
            ..
        } => Some(lucid_cir::TypedLiteral::Bool(*value)),
        _ => None,
    };
    let ty = checker
        .type_of_expr(expr)
        .map_err(|error| Arc::<str>::from(error.message))?
        .canonical();
    let id = u32::try_from(nodes.len()).map_err(|_| Arc::<str>::from("too many expressions"))?;
    let type_name = ty.canonical_string();
    nodes.push(TypedExpr {
        id,
        type_id: TypeId::new(db, type_name.clone()),
        type_name,
        kind: kind.to_string(),
        detail,
        children: Arc::from(child_ids),
        literal,
        span: expr.span(),
    });
    Ok(id)
}

/// Collect every expression-bearing statement in a function body. Keeping
/// this traversal here makes the HIR independent of parser statement layout
/// while still preserving all expression nodes needed by later lowering.
fn collect_typed_body<'db>(
    db: &'db dyn Db,
    checker: &lucid_checker::TypeChecker,
    statements: &[lucid_syntax::Stmt],
    nodes: &mut Vec<TypedExpr<'db>>,
) -> Result<(), Arc<str>> {
    use lucid_syntax::Stmt;
    for statement in statements {
        match statement {
            Stmt::Export(inner) => {
                collect_typed_body(db, checker, std::slice::from_ref(inner.as_ref()), nodes)?
            }
            Stmt::VarDef {
                value: Some(value), ..
            } => {
                collect_typed_exprs(db, checker, value, nodes)?;
            }
            Stmt::Assignment { target, value, .. } | Stmt::AugAssign { target, value, .. } => {
                collect_typed_exprs(db, checker, target, nodes)?;
                collect_typed_exprs(db, checker, value, nodes)?;
            }
            Stmt::With { items, body, .. } => {
                let mut body_checker = checker.clone();
                for item in items {
                    collect_typed_exprs(db, checker, &item.context_expr, nodes)?;
                    if let Some(pattern) = &item.target {
                        fn bind_with_pattern(
                            checker: &mut lucid_checker::TypeChecker,
                            pattern: &lucid_syntax::Pattern,
                        ) {
                            let bind = |checker: &mut lucid_checker::TypeChecker, name: &String| {
                                checker.env.variables.insert(
                                    name.clone(),
                                    (
                                        lucid_checker::Type::TypeVar("Any".into()),
                                        lucid_syntax::MutabilityView::Mutable,
                                    ),
                                );
                            };
                            match pattern {
                                lucid_syntax::Pattern::Ident(name, _) => bind(checker, name),
                                lucid_syntax::Pattern::ClassDestructure { fields, .. }
                                | lucid_syntax::Pattern::RecordDestructure(fields, _) => {
                                    for (_, nested) in fields {
                                        bind_with_pattern(checker, nested);
                                    }
                                }
                                lucid_syntax::Pattern::Tuple(items, _) => {
                                    for item in items {
                                        bind_with_pattern(checker, item);
                                    }
                                }
                                lucid_syntax::Pattern::Star(nested, _) => {
                                    bind_with_pattern(checker, nested)
                                }
                                lucid_syntax::Pattern::Literal(_, _)
                                | lucid_syntax::Pattern::Wildcard(_)
                                | lucid_syntax::Pattern::Type(_, _) => {}
                            }
                        }
                        bind_with_pattern(&mut body_checker, pattern);
                    }
                }
                for statement in body {
                    body_checker
                        .check_statement(statement)
                        .map_err(|error| Arc::<str>::from(error.message))?;
                    collect_typed_body(db, &body_checker, std::slice::from_ref(statement), nodes)?;
                }
            }
            Stmt::If {
                condition,
                then_branch,
                elif_branches,
                else_branch,
                span,
                ..
            } => {
                let condition_id = collect_typed_exprs(db, checker, condition, nodes)?;
                let collect_branch =
                    |branch: &[lucid_syntax::Stmt], nodes: &mut Vec<TypedExpr<'db>>| {
                        let mut branch_checker = checker.clone();
                        for statement in branch {
                            branch_checker
                                .check_statement(statement)
                                .map_err(|error| Arc::<str>::from(error.message))?;
                            collect_typed_body(
                                db,
                                &branch_checker,
                                std::slice::from_ref(statement),
                                nodes,
                            )?;
                        }
                        Ok::<(), Arc<str>>(())
                    };
                collect_branch(then_branch, nodes)?;
                for (condition, body) in elif_branches {
                    collect_typed_exprs(db, checker, condition, nodes)?;
                    collect_branch(body, nodes)?;
                }
                if let Some(body) = else_branch {
                    collect_branch(body, nodes)?;
                }
                // A conditional whose arms return directly is an expression
                // at the CIR boundary. Preserve that relationship in typed
                // HIR instead of leaving three unrelated expression nodes for
                // the lowering stage to guess about.
                if elif_branches.is_empty()
                    && then_branch.len() == 1
                    && else_branch.as_ref().is_some_and(|body| body.len() == 1)
                    && let Stmt::Return {
                        value: Some(then_value),
                        ..
                    } = &then_branch[0]
                    && let Some(else_body) = else_branch.as_ref()
                    && let Stmt::Return {
                        value: Some(else_value),
                        ..
                    } = &else_body[0]
                {
                    let then_id = collect_typed_exprs(db, checker, then_value, nodes)?;
                    let else_id = collect_typed_exprs(db, checker, else_value, nodes)?;
                    let ty = checker
                        .type_of_expr(then_value)
                        .map_err(|error| Arc::<str>::from(error.message))?
                        .canonical();
                    let id = u32::try_from(nodes.len())
                        .map_err(|_| Arc::<str>::from("too many expressions"))?;
                    nodes.push(TypedExpr {
                        id,
                        type_id: TypeId::new(db, ty.canonical_string()),
                        type_name: ty.canonical_string(),
                        kind: "if".into(),
                        detail: Some("statement-return".into()),
                        children: Arc::from([condition_id, then_id, else_id]),
                        literal: None,
                        span: *span,
                    });
                }
                // A conditional that assigns one value in each arm and is
                // followed by `return name` has the same SSA shape as direct
                // returns. Record that shape in typed HIR as well. The
                // assignment expressions have already been checked and
                // collected above; this node only links them, so it cannot
                // discard an initializer or accidentally execute both arms.
                if elif_branches.is_empty()
                    && then_branch.len() == 1
                    && else_branch.as_ref().is_some_and(|body| body.len() == 1)
                    && let Stmt::Assignment {
                        target:
                            lucid_syntax::Expr::Ident {
                                name: then_name, ..
                            },
                        value: then_value,
                        ..
                    }
                    | Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(then_name, _),
                        value: Some(then_value),
                        ..
                    } = &then_branch[0]
                    && let Some(else_body) = else_branch.as_ref()
                    && let Stmt::Assignment {
                        target:
                            lucid_syntax::Expr::Ident {
                                name: else_name, ..
                            },
                        value: else_value,
                        ..
                    }
                    | Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(else_name, _),
                        value: Some(else_value),
                        ..
                    } = &else_body[0]
                    && then_name == else_name
                {
                    let then_id = collect_typed_exprs(db, checker, then_value, nodes)?;
                    let else_id = collect_typed_exprs(db, checker, else_value, nodes)?;
                    let ty = checker
                        .type_of_expr(then_value)
                        .map_err(|error| Arc::<str>::from(error.message))?
                        .canonical();
                    let id = u32::try_from(nodes.len())
                        .map_err(|_| Arc::<str>::from("too many expressions"))?;
                    nodes.push(TypedExpr {
                        id,
                        type_id: TypeId::new(db, ty.canonical_string()),
                        type_name: ty.canonical_string(),
                        kind: "if".into(),
                        detail: Some("statement-assignment".into()),
                        children: Arc::from([condition_id, then_id, else_id]),
                        literal: None,
                        span: *span,
                    });
                }
            }
            Stmt::For {
                target,
                iterable,
                body,
                if_broken,
                ..
            } => {
                collect_typed_exprs(db, checker, iterable, nodes)?;
                let mut body_checker = checker.clone();
                fn bind_pattern(
                    checker: &mut lucid_checker::TypeChecker,
                    pattern: &lucid_syntax::Pattern,
                ) {
                    let bind = |checker: &mut lucid_checker::TypeChecker, name: &String| {
                        checker.env.variables.insert(
                            name.clone(),
                            (
                                lucid_checker::Type::TypeVar("Any".into()),
                                lucid_syntax::MutabilityView::Mutable,
                            ),
                        );
                    };
                    match pattern {
                        lucid_syntax::Pattern::Ident(name, _) => bind(checker, name),
                        lucid_syntax::Pattern::ClassDestructure { fields, .. }
                        | lucid_syntax::Pattern::RecordDestructure(fields, _) => {
                            for (_, nested) in fields {
                                bind_pattern(checker, nested);
                            }
                        }
                        lucid_syntax::Pattern::Tuple(items, _) => {
                            for item in items {
                                bind_pattern(checker, item);
                            }
                        }
                        lucid_syntax::Pattern::Star(nested, _) => bind_pattern(checker, nested),
                        lucid_syntax::Pattern::Literal(_, _)
                        | lucid_syntax::Pattern::Wildcard(_)
                        | lucid_syntax::Pattern::Type(_, _) => {}
                    }
                }
                bind_pattern(&mut body_checker, target);
                let mut collect_scoped_body =
                    |body: &[lucid_syntax::Stmt], nodes: &mut Vec<TypedExpr<'db>>| {
                        for statement in body {
                            body_checker
                                .check_statement(statement)
                                .map_err(|error| Arc::<str>::from(error.message))?;
                            collect_typed_body(
                                db,
                                &body_checker,
                                std::slice::from_ref(statement),
                                nodes,
                            )?;
                        }
                        Ok::<(), Arc<str>>(())
                    };
                collect_scoped_body(body, nodes)?;
                if let Some(body) = if_broken {
                    collect_scoped_body(body, nodes)?;
                }
            }
            Stmt::While {
                condition,
                body,
                if_broken,
                ..
            } => {
                collect_typed_exprs(db, checker, condition, nodes)?;
                let mut body_checker = checker.clone();
                let mut collect_scoped_body =
                    |body: &[lucid_syntax::Stmt], nodes: &mut Vec<TypedExpr<'db>>| {
                        for statement in body {
                            body_checker
                                .check_statement(statement)
                                .map_err(|error| Arc::<str>::from(error.message))?;
                            collect_typed_body(
                                db,
                                &body_checker,
                                std::slice::from_ref(statement),
                                nodes,
                            )?;
                        }
                        Ok::<(), Arc<str>>(())
                    };
                collect_scoped_body(body, nodes)?;
                if let Some(body) = if_broken {
                    collect_scoped_body(body, nodes)?;
                }
            }
            Stmt::Match { subject, arms, .. } => {
                let subject_id = collect_typed_exprs(db, checker, subject, nodes)?;
                for arm in arms {
                    let mut arm_checker = checker.clone();
                    fn bind_match_pattern(
                        checker: &mut lucid_checker::TypeChecker,
                        pattern: &lucid_syntax::Pattern,
                    ) {
                        let bind = |checker: &mut lucid_checker::TypeChecker, name: &String| {
                            checker.env.variables.insert(
                                name.clone(),
                                (
                                    lucid_checker::Type::TypeVar("Any".into()),
                                    lucid_syntax::MutabilityView::Mutable,
                                ),
                            );
                        };
                        match pattern {
                            lucid_syntax::Pattern::Ident(name, _) => bind(checker, name),
                            lucid_syntax::Pattern::ClassDestructure { fields, .. }
                            | lucid_syntax::Pattern::RecordDestructure(fields, _) => {
                                for (_, nested) in fields {
                                    bind_match_pattern(checker, nested);
                                }
                            }
                            lucid_syntax::Pattern::Tuple(items, _) => {
                                for item in items {
                                    bind_match_pattern(checker, item);
                                }
                            }
                            lucid_syntax::Pattern::Star(nested, _) => {
                                bind_match_pattern(checker, nested)
                            }
                            lucid_syntax::Pattern::Literal(_, _)
                            | lucid_syntax::Pattern::Wildcard(_)
                            | lucid_syntax::Pattern::Type(_, _) => {}
                        }
                    }
                    bind_match_pattern(&mut arm_checker, &arm.pattern);
                    if let Some(guard) = &arm.guard {
                        collect_typed_exprs(db, &arm_checker, guard, nodes)?;
                    }
                    for statement in &arm.body {
                        arm_checker
                            .check_statement(statement)
                            .map_err(|error| Arc::<str>::from(error.message))?;
                        collect_typed_body(
                            db,
                            &arm_checker,
                            std::slice::from_ref(statement),
                            nodes,
                        )?;
                    }
                }
                // Preserve the control-flow relationship for the primitive
                // literal/wildcard form. Without this synthetic node, the
                // typed graph would contain unrelated subject and arm values
                // and the lowering stage would have to recover match order
                // from syntax again.
                if arms.len() == 2
                    && arms.iter().all(|arm| arm.guard.is_none())
                    && let (
                        lucid_syntax::Pattern::Literal(_, _),
                        lucid_syntax::Pattern::Wildcard(_),
                    ) = (&arms[0].pattern, &arms[1].pattern)
                    && let (literal_arm, wildcard_arm) = (&arms[0], &arms[1])
                    && let lucid_syntax::Pattern::Literal(literal, _) = &literal_arm.pattern
                    && matches!(
                        literal,
                        lucid_syntax::LiteralValue::Int(_) | lucid_syntax::LiteralValue::Bool(_)
                    )
                    && let Some(literal_value) = match_arm_result(literal_arm)
                    && let Some(wildcard_value) = match_arm_result(wildcard_arm)
                {
                    let literal_id = nodes
                        .iter()
                        .rev()
                        .find(|node| node.span == literal_value.span())
                        .map(|node| node.id);
                    let wildcard_id = nodes
                        .iter()
                        .rev()
                        .find(|node| node.span == wildcard_value.span())
                        .map(|node| node.id);
                    if let (Some(literal_id), Some(wildcard_id)) = (literal_id, wildcard_id) {
                        let id = u32::try_from(nodes.len())
                            .map_err(|_| Arc::<str>::from("too many expressions"))?;
                        let detail = match literal {
                            lucid_syntax::LiteralValue::Int(value) => {
                                format!("literal-int:{value}")
                            }
                            lucid_syntax::LiteralValue::Bool(value) => {
                                format!("literal-bool:{value}")
                            }
                            _ => return Err(Arc::from("unsupported match literal")),
                        };
                        let ty = checker
                            .type_of_expr(literal_value)
                            .map_err(|error| Arc::<str>::from(error.message))?
                            .canonical();
                        nodes.push(TypedExpr {
                            id,
                            type_id: TypeId::new(db, ty.canonical_string()),
                            type_name: ty.canonical_string(),
                            kind: "match".into(),
                            detail: Some(detail),
                            children: Arc::from([subject_id, literal_id, wildcard_id]),
                            literal: None,
                            span: subject.span(),
                        });
                    }
                }

                fn typed_match_pure_expr(expr: &lucid_syntax::Expr) -> bool {
                    match expr {
                        lucid_syntax::Expr::Literal { .. } | lucid_syntax::Expr::Ident { .. } => {
                            true
                        }
                        lucid_syntax::Expr::Unary { expr, .. } => typed_match_pure_expr(expr),
                        lucid_syntax::Expr::Binary { left, right, .. } => {
                            typed_match_pure_expr(left) && typed_match_pure_expr(right)
                        }
                        lucid_syntax::Expr::IfExpr {
                            condition,
                            then_branch,
                            else_branch,
                            ..
                        } => {
                            typed_match_pure_expr(condition)
                                && typed_match_pure_expr(then_branch)
                                && typed_match_pure_expr(else_branch)
                        }
                        _ => false,
                    }
                }
                fn typed_match_static_truth(expr: &lucid_syntax::Expr) -> Option<bool> {
                    match expr {
                        lucid_syntax::Expr::Literal {
                            value: lucid_syntax::LiteralValue::Bool(value),
                            ..
                        } => Some(*value),
                        lucid_syntax::Expr::Literal {
                            value: lucid_syntax::LiteralValue::Int(value),
                            ..
                        } => Some(*value != 0),
                        lucid_syntax::Expr::Unary {
                            op: lucid_syntax::UnaryOp::Not,
                            expr,
                            ..
                        } => typed_match_static_truth(expr).map(|value| !value),
                        lucid_syntax::Expr::Binary {
                            op: lucid_syntax::BinaryOp::And,
                            left,
                            right,
                            ..
                        } => match typed_match_static_truth(left) {
                            Some(false) => Some(false),
                            Some(true) => typed_match_static_truth(right),
                            None => None,
                        },
                        lucid_syntax::Expr::Binary {
                            op: lucid_syntax::BinaryOp::Or,
                            left,
                            right,
                            ..
                        } => match typed_match_static_truth(left) {
                            Some(true) => Some(true),
                            Some(false) => typed_match_static_truth(right),
                            None => None,
                        },
                        lucid_syntax::Expr::Binary {
                            op, left, right, ..
                        } => {
                            let (
                                lucid_syntax::Expr::Literal {
                                    value: lucid_syntax::LiteralValue::Bool(left),
                                    ..
                                },
                                lucid_syntax::Expr::Literal {
                                    value: lucid_syntax::LiteralValue::Bool(right),
                                    ..
                                },
                            ) = (left.as_ref(), right.as_ref())
                            else {
                                return None;
                            };
                            match op {
                                lucid_syntax::BinaryOp::Eq
                                | lucid_syntax::BinaryOp::Identity
                                | lucid_syntax::BinaryOp::Is => Some(left == right),
                                lucid_syntax::BinaryOp::NotEq
                                | lucid_syntax::BinaryOp::NotIdentity
                                | lucid_syntax::BinaryOp::IsNot => Some(left != right),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                }
                fn typed_match_selected_static_branch<'a>(
                    condition: &lucid_syntax::Expr,
                    then_branch: &'a [lucid_syntax::Stmt],
                    elif_branches: &'a [(lucid_syntax::Expr, Vec<lucid_syntax::Stmt>)],
                    else_branch: Option<&'a Vec<lucid_syntax::Stmt>>,
                ) -> Option<Option<&'a [lucid_syntax::Stmt]>> {
                    match typed_match_static_truth(condition) {
                        Some(true) => Some(Some(then_branch)),
                        Some(false) => {
                            for (condition, branch) in elif_branches {
                                match typed_match_static_truth(condition) {
                                    Some(true) => return Some(Some(branch.as_slice())),
                                    Some(false) => continue,
                                    None => return None,
                                }
                            }
                            Some(else_branch.map(Vec::as_slice))
                        }
                        None => None,
                    }
                }
                fn typed_match_noop_statement(statement: &lucid_syntax::Stmt) -> bool {
                    matches!(statement, lucid_syntax::Stmt::Pass(_))
                        || matches!(
                            statement,
                            lucid_syntax::Stmt::Expr(expr) if typed_match_pure_expr(expr)
                        )
                        || matches!(
                            statement,
                            lucid_syntax::Stmt::Assert { condition, .. }
                                if typed_match_static_truth(condition) == Some(true)
                        )
                        || matches!(
                            statement,
                            lucid_syntax::Stmt::While {
                                condition,
                                if_broken: None,
                                ..
                            } if typed_match_static_truth(condition) == Some(false)
                        )
                        || matches!(
                            statement,
                            lucid_syntax::Stmt::For {
                                iterable,
                                if_broken: None,
                                ..
                            } if lucid_cir::is_const_empty_iterable(iterable)
                        )
                        || match statement {
                            lucid_syntax::Stmt::If {
                                condition,
                                then_branch,
                                elif_branches,
                                else_branch,
                                ..
                            } => match typed_match_selected_static_branch(
                                condition,
                                then_branch,
                                elif_branches,
                                else_branch.as_ref(),
                            ) {
                                Some(Some(branch)) => branch.iter().all(typed_match_noop_statement),
                                Some(None) => true,
                                None => false,
                            },
                            _ => false,
                        }
                }
                fn match_statements_result(
                    statements: &[lucid_syntax::Stmt],
                ) -> Option<&lucid_syntax::Expr> {
                    let mut meaningful = statements
                        .iter()
                        .filter(|statement| !typed_match_noop_statement(statement));
                    let first = meaningful.next()?;
                    let second = meaningful.next();
                    if meaningful.next().is_some() {
                        return None;
                    }
                    match (first, second) {
                        (
                            lucid_syntax::Stmt::Return {
                                value: Some(value), ..
                            },
                            None,
                        ) => Some(value),
                        (
                            lucid_syntax::Stmt::Assignment {
                                target: lucid_syntax::Expr::Ident { name, .. },
                                value,
                                ..
                            },
                            Some(lucid_syntax::Stmt::Return {
                                value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
                                ..
                            }),
                        )
                        | (
                            lucid_syntax::Stmt::VarDef {
                                pattern: lucid_syntax::Pattern::Ident(name, _),
                                value: Some(value),
                                ..
                            },
                            Some(lucid_syntax::Stmt::Return {
                                value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
                                ..
                            }),
                        ) if name == returned => Some(value),
                        (
                            lucid_syntax::Stmt::If {
                                condition,
                                then_branch,
                                elif_branches,
                                else_branch,
                                ..
                            },
                            None,
                        ) => {
                            let selected = typed_match_selected_static_branch(
                                condition,
                                then_branch,
                                elif_branches,
                                else_branch.as_ref(),
                            )??;
                            match_statements_result(selected)
                        }
                        _ => None,
                    }
                }
                fn match_arm_result(arm: &lucid_syntax::MatchArm) -> Option<&lucid_syntax::Expr> {
                    match_statements_result(&arm.body)
                }
                if arms.len() >= 3
                    && arms.iter().all(|arm| arm.guard.is_none())
                    && matches!(
                        arms.last().map(|arm| &arm.pattern),
                        Some(lucid_syntax::Pattern::Wildcard(_))
                    )
                    && arms[..arms.len() - 1]
                        .iter()
                        .all(|arm| matches!(arm.pattern, lucid_syntax::Pattern::Literal(_, _)))
                    && arms.iter().all(|arm| match_arm_result(arm).is_some())
                {
                    let detail = arms[..arms.len() - 1]
                        .iter()
                        .map(|arm| match &arm.pattern {
                            lucid_syntax::Pattern::Literal(
                                lucid_syntax::LiteralValue::Int(value),
                                _,
                            ) => Ok(format!("i{value}")),
                            lucid_syntax::Pattern::Literal(
                                lucid_syntax::LiteralValue::Bool(value),
                                _,
                            ) => Ok(format!("b{value}")),
                            _ => Err(Arc::from("unsupported match literal pattern")),
                        })
                        .collect::<Result<Vec<_>, Arc<str>>>()?
                        .join(",");
                    let mut children = vec![subject_id];
                    let mut result_ids = Vec::with_capacity(arms.len());
                    for arm in arms {
                        let Some(value) = match_arm_result(arm) else {
                            return Err(Arc::from("unsupported match result"));
                        };
                        let Some(id) = nodes
                            .iter()
                            .rev()
                            .find(|node| node.span == value.span())
                            .map(|node| node.id)
                        else {
                            result_ids.clear();
                            break;
                        };
                        result_ids.push(id);
                    }
                    if result_ids.len() == arms.len() {
                        children.extend(result_ids);
                        let id = u32::try_from(nodes.len())
                            .map_err(|_| Arc::<str>::from("too many expressions"))?;
                        let Some(result) = match_arm_result(&arms[0]) else {
                            return Err(Arc::from("unsupported match result"));
                        };
                        let ty = checker
                            .type_of_expr(result)
                            .map_err(|error| Arc::<str>::from(error.message))?
                            .canonical();
                        nodes.push(TypedExpr {
                            id,
                            type_id: TypeId::new(db, ty.canonical_string()),
                            type_name: ty.canonical_string(),
                            kind: "match-chain".into(),
                            detail: Some(format!("literal-chain:{detail}")),
                            children: Arc::from(children),
                            literal: None,
                            span: subject.span(),
                        });
                    }
                }
                let optional_value_arms =
                    if arms.len() >= 2 && arms.iter().all(|arm| arm.guard.is_none()) {
                        if matches!(
                            arms.last().map(|arm| &arm.pattern),
                            Some(lucid_syntax::Pattern::Wildcard(_))
                        ) && arms
                            .last()
                            .is_some_and(|arm| arm.body.iter().all(typed_match_noop_statement))
                        {
                            Some(&arms[..arms.len() - 1])
                        } else if arms
                            .iter()
                            .all(|arm| matches!(arm.pattern, lucid_syntax::Pattern::Literal(_, _)))
                        {
                            Some(arms.as_slice())
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                if let Some(value_arms) = optional_value_arms.filter(|value_arms| {
                    value_arms.len() >= 2
                        && value_arms
                            .iter()
                            .all(|arm| matches!(arm.pattern, lucid_syntax::Pattern::Literal(_, _)))
                        && value_arms.iter().all(|arm| match_arm_result(arm).is_some())
                }) {
                    let detail = value_arms
                        .iter()
                        .map(|arm| match &arm.pattern {
                            lucid_syntax::Pattern::Literal(
                                lucid_syntax::LiteralValue::Int(value),
                                _,
                            ) => Ok(format!("i{value}")),
                            lucid_syntax::Pattern::Literal(
                                lucid_syntax::LiteralValue::Bool(value),
                                _,
                            ) => Ok(format!("b{value}")),
                            _ => Err(Arc::from("unsupported match literal pattern")),
                        })
                        .collect::<Result<Vec<_>, Arc<str>>>()?
                        .join(",");
                    let mut children = vec![subject_id];
                    let mut result_ids = Vec::with_capacity(value_arms.len());
                    for arm in value_arms {
                        let Some(value) = match_arm_result(arm) else {
                            return Err(Arc::from("unsupported match result"));
                        };
                        let Some(id) = nodes
                            .iter()
                            .rev()
                            .find(|node| node.span == value.span())
                            .map(|node| node.id)
                        else {
                            result_ids.clear();
                            break;
                        };
                        result_ids.push(id);
                    }
                    if result_ids.len() == value_arms.len() {
                        children.extend(result_ids);
                        let id = u32::try_from(nodes.len())
                            .map_err(|_| Arc::<str>::from("too many expressions"))?;
                        let Some(result) = match_arm_result(&value_arms[0]) else {
                            return Err(Arc::from("unsupported match result"));
                        };
                        let ty = checker
                            .type_of_expr(result)
                            .map_err(|error| Arc::<str>::from(error.message))?
                            .canonical();
                        nodes.push(TypedExpr {
                            id,
                            type_id: TypeId::new(db, ty.canonical_string()),
                            type_name: ty.canonical_string(),
                            kind: "optional-match-chain".into(),
                            detail: Some(format!("literal-chain:{detail}")),
                            children: Arc::from(children),
                            literal: None,
                            span: subject.span(),
                        });
                    }
                }
                let void_arms = if arms.len() >= 2 && arms.iter().all(|arm| arm.guard.is_none()) {
                    if matches!(
                        arms.last().map(|arm| &arm.pattern),
                        Some(lucid_syntax::Pattern::Wildcard(_))
                    ) && arms
                        .last()
                        .is_some_and(|arm| arm.body.iter().all(typed_match_noop_statement))
                    {
                        Some(&arms[..arms.len() - 1])
                    } else {
                        Some(arms.as_slice())
                    }
                } else {
                    None
                };
                if let Some(void_arms) = void_arms.filter(|void_arms| {
                    !void_arms.is_empty()
                        && void_arms
                            .iter()
                            .all(|arm| matches!(arm.pattern, lucid_syntax::Pattern::Literal(_, _)))
                        && void_arms.iter().all(|arm| {
                            let Some((last, prefix)) = arm.body.split_last() else {
                                return false;
                            };
                            prefix.iter().all(typed_match_noop_statement)
                                && matches!(last, lucid_syntax::Stmt::Return { value: None, .. })
                                || arm.body.iter().all(typed_match_noop_statement)
                        })
                }) {
                    let detail = void_arms
                        .iter()
                        .map(|arm| match &arm.pattern {
                            lucid_syntax::Pattern::Literal(
                                lucid_syntax::LiteralValue::Int(value),
                                _,
                            ) => Ok(format!("i{value}")),
                            lucid_syntax::Pattern::Literal(
                                lucid_syntax::LiteralValue::Bool(value),
                                _,
                            ) => Ok(format!("b{value}")),
                            _ => Err(Arc::from("unsupported match literal pattern")),
                        })
                        .collect::<Result<Vec<_>, Arc<str>>>()?
                        .join(",");
                    let id = u32::try_from(nodes.len())
                        .map_err(|_| Arc::<str>::from("too many expressions"))?;
                    nodes.push(TypedExpr {
                        id,
                        type_id: TypeId::new(db, "none"),
                        type_name: "none".into(),
                        kind: "void-match-chain".into(),
                        detail: Some(format!("literal-chain:{detail}")),
                        children: Arc::from([subject_id]),
                        literal: None,
                        span: subject.span(),
                    });
                }
            }
            Stmt::Try {
                body,
                handlers,
                finally_body,
                ..
            } => {
                let collect_scoped = |base: &lucid_checker::TypeChecker,
                                      body: &[lucid_syntax::Stmt],
                                      nodes: &mut Vec<TypedExpr<'db>>|
                 -> Result<(), Arc<str>> {
                    let mut scoped = base.clone();
                    for statement in body {
                        scoped
                            .check_statement(statement)
                            .map_err(|error| Arc::<str>::from(error.message))?;
                        collect_typed_body(db, &scoped, std::slice::from_ref(statement), nodes)?;
                    }
                    Ok(())
                };
                collect_scoped(checker, body, nodes)?;
                for handler in handlers {
                    let mut handler_checker = checker.clone();
                    if let Some(name) = &handler.name {
                        handler_checker.env.variables.insert(
                            name.clone(),
                            (
                                lucid_checker::Type::TypeVar("Any".into()),
                                lucid_syntax::MutabilityView::Mutable,
                            ),
                        );
                    }
                    collect_scoped(&handler_checker, &handler.body, nodes)?;
                }
                if let Some(body) = finally_body {
                    collect_scoped(checker, body, nodes)?;
                }
            }
            Stmt::Return {
                value: Some(value), ..
            }
            | Stmt::Raise {
                exception: value, ..
            }
            | Stmt::Yield { value, .. }
            | Stmt::Expr(value) => {
                collect_typed_exprs(db, checker, value, nodes)?;
            }
            Stmt::Assert {
                condition, message, ..
            } => {
                collect_typed_exprs(db, checker, condition, nodes)?;
                if let Some(message) = message {
                    collect_typed_exprs(db, checker, message, nodes)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[salsa::tracked]
pub fn typed_module<'db>(
    db: &'db dyn Db,
    file: SourceFile,
) -> Result<Arc<TypedModule<'db>>, Arc<str>> {
    let module = parse_ast(db, file).as_ref().map_err(Arc::clone)?;
    let mut checker = lucid_checker::TypeChecker::new();
    checker
        .check_module(module)
        .map_err(|error| Arc::<str>::from(error.message))?;
    let mut initializers = Vec::new();
    let mut expressions = Vec::new();
    for statement in &module.statements {
        let statement = match statement {
            lucid_syntax::Stmt::Export(inner) => inner.as_ref(),
            other => other,
        };
        let (name, value) = match statement {
            lucid_syntax::Stmt::Assignment {
                target: lucid_syntax::Expr::Ident { name, .. },
                value,
                ..
            }
            | lucid_syntax::Stmt::VarDef {
                pattern: lucid_syntax::Pattern::Ident(name, _),
                value: Some(value),
                ..
            } => (name, value),
            _ => continue,
        };
        let ty = checker
            .type_of_expr(value)
            .map_err(|error| Arc::<str>::from(error.message))?
            .canonical();
        let Some(symbol) = resolve_top_level(db, file, name.clone()) else {
            continue;
        };
        let expression = collect_typed_exprs(db, &checker, value, &mut expressions)?;
        initializers.push(TypedInitializer {
            symbol: *symbol,
            type_id: TypeId::new(db, ty.canonical_string()),
            type_name: ty.canonical_string(),
            expression,
            span: value.span(),
        });
    }
    let mut functions = Vec::new();
    let mut seen_function_symbols = std::collections::HashSet::new();
    for statement in &module.statements {
        let statement = match statement {
            lucid_syntax::Stmt::Export(inner) => inner.as_ref(),
            other => other,
        };
        let lucid_syntax::Stmt::Function(function) = statement else {
            continue;
        };
        let Some(signature) = checker.env.functions.get(&function.name) else {
            continue;
        };
        let lucid_checker::Type::Function {
            params,
            return_type,
        } = signature.canonical()
        else {
            continue;
        };
        let Some(symbol) = resolve_top_level(db, file, function.name.clone()) else {
            continue;
        };
        if !seen_function_symbols.insert(*symbol) {
            continue;
        }
        let parameter_types = params
            .iter()
            .map(|ty| TypeId::new(db, ty.canonical_string()))
            .collect::<Vec<_>>();
        let parameter_names = function
            .params
            .iter()
            .map(|param| param.name.clone())
            .collect::<Vec<_>>();
        let required_parameters = function
            .params
            .iter()
            .map(|param| {
                param.default.is_none()
                    && !param.is_variadic_positional
                    && !param.is_variadic_keyword
                    && !param.is_gather
            })
            .collect::<Vec<_>>();
        let return_type = TypeId::new(db, return_type.canonical_string());
        let overload_types = checker
            .env
            .function_overloads
            .get(&function.name)
            .cloned()
            .unwrap_or_else(|| vec![signature.clone()])
            .into_iter()
            .map(|ty| TypeId::new(db, ty.canonical().canonical_string()))
            .collect::<Vec<_>>();
        let mut body_expressions = Vec::new();
        let mut body_checker = checker.clone();
        let body_return_type = match signature.canonical() {
            lucid_checker::Type::Function { return_type, .. }
                if matches!(return_type.as_ref(), lucid_checker::Type::None) =>
            {
                lucid_checker::Type::TypeVar("Any".into())
            }
            lucid_checker::Type::Function { return_type, .. } => return_type.as_ref().clone(),
            _ => lucid_checker::Type::TypeVar("Any".into()),
        };
        if let lucid_checker::Type::Function { params, .. } = signature.canonical() {
            for (param, ty) in function.params.iter().zip(params.iter()) {
                body_checker.env.variables.insert(
                    param.name.clone(),
                    (ty.clone(), lucid_syntax::MutabilityView::Mutable),
                );
                if let Some(default) = &param.default {
                    collect_typed_exprs(db, &body_checker, default, &mut body_expressions)?;
                }
            }
        }
        body_checker.env.current_return_type = Some(body_return_type);
        for statement in &function.body {
            if let lucid_syntax::Stmt::Function(nested) = statement {
                let params = nested
                    .params
                    .iter()
                    .map(|param| {
                        param
                            .type_annotation
                            .as_ref()
                            .map(|annotation| body_checker.resolve_type_expr(annotation))
                            .transpose()
                            .map(|ty| ty.unwrap_or(lucid_checker::Type::TypeVar("Any".into())))
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| Arc::<str>::from(error.message))?;
                let return_type = nested
                    .return_type
                    .as_ref()
                    .map(|annotation| body_checker.resolve_type_expr(annotation))
                    .transpose()
                    .map_err(|error| Arc::<str>::from(error.message))?
                    .unwrap_or(lucid_checker::Type::TypeVar("Any".into()));
                body_checker.env.variables.insert(
                    nested.name.clone(),
                    (
                        lucid_checker::Type::Function {
                            params,
                            return_type: Box::new(return_type),
                        },
                        lucid_syntax::MutabilityView::Immutable,
                    ),
                );
            }
        }
        // Seed the body checker with local bindings before collecting the
        // expression graph.  The regular module check already validates the
        // body, but this clone must replay statement effects (for example a
        // local assignment) so later name nodes have a typed environment.
        if !function.is_dispatch {
            for statement in &function.body {
                body_checker
                    .check_statement(statement)
                    .map_err(|error| Arc::<str>::from(error.message))?;
            }
        }
        collect_typed_body(db, &body_checker, &function.body, &mut body_expressions)?;
        functions.push(TypedFunction {
            symbol: *symbol,
            is_dispatch: function.is_dispatch,
            is_async: function.is_async,
            parameter_names: Arc::from(parameter_names),
            required_parameters: Arc::from(required_parameters),
            parameter_types: Arc::from(parameter_types),
            return_type,
            overload_types: Arc::from(overload_types),
            body_expressions: Arc::from(body_expressions),
            span: function.span,
        });
    }
    Ok(Arc::new(TypedModule {
        resolved: resolved_module(db, file).clone(),
        initializers: Arc::from(initializers),
        functions: Arc::from(functions),
        expressions: Arc::from(expressions),
        checked: true,
    }))
}

/// Lower the straight-line top-level initializer sequence through the shared
/// CIR boundary. Control-flow and unsupported statements remain explicit
/// lowering errors rather than silently executing a partial module.
#[salsa::tracked]
pub fn lower_module(db: &dyn Db, file: SourceFile) -> Result<Arc<lucid_cir::Function>, Arc<str>> {
    // Keep lowering behind the semantic boundary. A backend must never be
    // able to execute an expression that the checker rejected.
    let typed = typed_module(db, file).as_ref().map_err(Arc::clone)?;
    // Prefer the typed-HIR graph whenever every initializer belongs to the
    // currently supported primitive CIR subset. Unsupported graph nodes fall
    // through to the existing statement lowerer, which gives callers an
    // explicit LowerError while the remaining HIR/CIR coverage is built out.
    if !typed.initializers.is_empty() {
        let nodes = typed
            .expressions
            .iter()
            .map(|node| lucid_cir::TypedExprNode {
                id: node.id,
                kind: node.kind.clone(),
                detail: node.detail.clone(),
                children: node.children.to_vec(),
                literal: node.literal,
            })
            .collect::<Vec<_>>();
        let roots = typed
            .initializers
            .iter()
            .map(|initializer| initializer.expression)
            .collect::<Vec<_>>();
        if let Ok(function) = lucid_cir::Function::from_typed_initializers(&nodes, &roots) {
            return Ok(Arc::new(function));
        }
    }
    let module = parse_ast(db, file).as_ref().map_err(Arc::clone)?;
    lucid_cir::Function::from_module(module)
        .map(Arc::new)
        .map_err(|error| {
            Arc::from(match error {
                lucid_cir::LowerError::NoLowerableAssignment => {
                    "module has no top-level assignment to lower"
                }
                lucid_cir::LowerError::UnsupportedExpression => {
                    "unsupported expression for CIR lowering"
                }
            })
        })
}

/// Lower a checked function body through the typed-HIR/CIR boundary.  The
/// The initial slice accepts primitive expression returns, void bodies,
/// statically foldable return branches, and straight-line local bindings.
/// Dynamic statement control flow remains an explicit lowering error until its
/// full typed-CIR representation is added.
#[salsa::tracked]
pub fn lower_function_body(
    db: &dyn Db,
    file: SourceFile,
    function_name: String,
) -> Result<Arc<lucid_cir::Function>, Arc<str>> {
    let typed = typed_module(db, file).as_ref().map_err(Arc::clone)?;
    let function = typed
        .functions
        .iter()
        .find(|function| function.symbol.name(db).as_str() == function_name)
        .ok_or_else(|| Arc::<str>::from("function not found"))?;
    let module = parse_ast(db, file).as_ref().map_err(Arc::clone)?;
    let Some(source_function) = module.statements.iter().find_map(|statement| {
        let statement = match statement {
            lucid_syntax::Stmt::Export(inner) => inner.as_ref(),
            other => other,
        };
        match statement {
            lucid_syntax::Stmt::Function(candidate) if candidate.name == function_name => {
                Some(candidate)
            }
            _ => None,
        }
    }) else {
        return Err(Arc::from("function not found"));
    };
    if function.is_dispatch && function.overload_types.len() != 1 {
        return Err(Arc::from(
            "dispatch overload set bodies require a selected overload for CIR lowering",
        ));
    }
    fn branch_noop_statement(statement: &lucid_syntax::Stmt) -> bool {
        matches!(statement, lucid_syntax::Stmt::Pass(_))
            || matches!(
                statement,
                lucid_syntax::Stmt::Expr(expr) if is_pure_expression(expr)
            )
            || matches!(
                statement,
                lucid_syntax::Stmt::Assert { condition, .. }
                    if static_truth(condition) == Some(true)
            )
            || matches!(
                statement,
                lucid_syntax::Stmt::While {
                    condition,
                    if_broken: None,
                    ..
                } if static_truth(condition) == Some(false)
            )
            || matches!(
                statement,
                lucid_syntax::Stmt::For {
                    iterable,
                    if_broken: None,
                    ..
                } if lucid_cir::is_const_empty_iterable(iterable)
            )
            || match statement {
                lucid_syntax::Stmt::If {
                    condition,
                    then_branch,
                    elif_branches,
                    else_branch,
                    ..
                } => match static_branch_selection(
                    condition,
                    then_branch,
                    elif_branches,
                    else_branch.as_ref(),
                ) {
                    StaticBranch::Selected(branch) => branch.iter().all(branch_noop_statement),
                    StaticBranch::Empty => true,
                    StaticBranch::Unknown => false,
                },
                _ => false,
            }
    }
    let branch_is_single_void = |branch: &[lucid_syntax::Stmt]| {
        let Some((last, prefix)) = branch.split_last() else {
            return false;
        };
        prefix.iter().all(branch_noop_statement)
            && matches!(last, lucid_syntax::Stmt::Return { value: None, .. })
            || branch.iter().all(branch_noop_statement)
    };
    // Route the canonical parameter-backed induction loop through CIR before
    // considering the older linear/function-body adapters. This emits a real
    // back-edge and header Phi; it never unrolls or executes the AST.
    if !function.is_async
        && source_function.body.iter().any(|statement| {
            matches!(
                statement,
                lucid_syntax::Stmt::While { .. } | lucid_syntax::Stmt::For { .. }
            )
        })
    {
        let loop_module = lucid_syntax::Module {
            statements: source_function.body.clone(),
            span: source_function.span,
        };
        if let Ok(lowered) = lucid_cir::Function::from_module_linear_with_params(
            &loop_module,
            &function.parameter_names,
        ) {
            return Ok(Arc::new(lowered));
        }
    }
    // Straight-line bindings followed by void completion have no value root,
    // but their initializers still belong in CIR. Reuse the verified linear
    // statement lowerer so parameter reads and binding dependencies are
    // preserved while the final terminator remains `Return(None)`.
    if !function.is_async
        && matches!(
            source_function.body.last(),
            Some(lucid_syntax::Stmt::Return { value: None, .. } | lucid_syntax::Stmt::Pass(_))
        )
        && source_function.body[..source_function.body.len().saturating_sub(1)]
            .iter()
            .all(|statement| {
                matches!(
                    statement,
                    lucid_syntax::Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(_, _),
                        value: Some(_),
                        ..
                    } | lucid_syntax::Stmt::Assignment {
                        target: lucid_syntax::Expr::Ident { .. },
                        ..
                    } | lucid_syntax::Stmt::Pass(_)
                )
            })
    {
        let module = lucid_syntax::Module {
            statements: source_function.body.clone(),
            span: source_function.span,
        };
        if let Ok(mut lowered) =
            lucid_cir::Function::from_module_linear_with_params(&module, &function.parameter_names)
        {
            if matches!(
                source_function.body.last(),
                Some(lucid_syntax::Stmt::Pass(_))
            ) {
                if let Some(block) = lowered.blocks.last_mut() {
                    block.terminator = lucid_cir::Terminator::Return(None);
                }
                lowered
                    .verify()
                    .map_err(|_| Arc::<str>::from("invalid void function CIR"))?;
            }
            return Ok(Arc::new(lowered));
        }
    }
    // Straight-line augmented assignment is already supported by the shared
    // linear CIR builder. Route it there before the typed-local adapter, whose
    // local binding map records initializer expressions but not read-modify-
    // write updates yet.
    fn statement_contains_augassign(statement: &lucid_syntax::Stmt) -> bool {
        match statement {
            lucid_syntax::Stmt::AugAssign { .. } => true,
            lucid_syntax::Stmt::Export(inner) => statement_contains_augassign(inner),
            lucid_syntax::Stmt::If {
                then_branch,
                elif_branches,
                else_branch,
                ..
            } => {
                then_branch.iter().any(statement_contains_augassign)
                    || elif_branches
                        .iter()
                        .any(|(_, branch)| branch.iter().any(statement_contains_augassign))
                    || else_branch
                        .as_ref()
                        .is_some_and(|branch| branch.iter().any(statement_contains_augassign))
            }
            _ => false,
        }
    }
    if !function.is_async
        && matches!(
            source_function.body.last(),
            Some(lucid_syntax::Stmt::Return { .. } | lucid_syntax::Stmt::Pass(_))
        )
        && source_function
            .body
            .iter()
            .any(statement_contains_augassign)
    {
        let module = lucid_syntax::Module {
            statements: source_function.body.clone(),
            span: source_function.span,
        };
        if let Ok(mut lowered) =
            lucid_cir::Function::from_module_linear_with_params(&module, &function.parameter_names)
        {
            if matches!(
                source_function.body.last(),
                Some(lucid_syntax::Stmt::Pass(_))
            ) {
                if let Some(block) = lowered.blocks.last_mut() {
                    block.terminator = lucid_cir::Terminator::Return(None);
                }
                lowered
                    .verify()
                    .map_err(|_| Arc::<str>::from("invalid void function CIR"))?;
            }
            return Ok(Arc::new(lowered));
        }
    }
    // Constant literal loops can use the same verified linear CIR statement
    // lowerer as module initializers. This keeps ordered unrolling and local
    // rebinding semantics identical without inventing a second function-loop
    // representation; positional parameters are seeded as CIR `Param`s.
    if !function.is_async
        && source_function.body.iter().any(|statement| {
            matches!(
                statement,
                lucid_syntax::Stmt::For { iterable, .. }
                    if matches!(
                        iterable,
                        lucid_syntax::Expr::List { elements, .. }
                            | lucid_syntax::Expr::Set { elements, .. }
                            if !elements.is_empty()
                    ) || matches!(
                        iterable,
                        lucid_syntax::Expr::Dict { entries, .. }
                            if !entries.is_empty()
                    )
            ) || matches!(
                statement,
                lucid_syntax::Stmt::For { iterable, .. }
                    if lucid_cir::const_range_values(iterable)
                        .is_some_and(|values| !values.is_empty())
            )
        })
    {
        let module = lucid_syntax::Module {
            statements: source_function.body.clone(),
            span: source_function.span,
        };
        if let Ok(lowered) =
            lucid_cir::Function::from_module_linear_with_params(&module, &function.parameter_names)
        {
            return Ok(Arc::new(lowered));
        }
    }
    // Lower the primitive exhaustive match shape through the same checked
    // conditional CIR builder. Restricting the subject to a name avoids
    // evaluating an effectful expression once per arm; richer patterns stay
    // an explicit unsupported lowering until CIR carries pattern coverage.
    if !function.is_async
        && let [lucid_syntax::Stmt::Match { subject, arms, .. }] = source_function.body.as_slice()
        && matches!(
            subject,
            lucid_syntax::Expr::Ident { .. }
                | lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Int(_) | lucid_syntax::LiteralValue::Bool(_),
                    ..
                }
        )
    {
        if let Some(root) = function.body_expressions.iter().rev().find(|node| {
            matches!(
                node.kind.as_str(),
                "match" | "match-chain" | "optional-match-chain" | "void-match-chain"
            ) && node.span == subject.span()
        }) {
            let nodes = function
                .body_expressions
                .iter()
                .map(|node| lucid_cir::TypedExprNode {
                    id: node.id,
                    kind: node.kind.clone(),
                    detail: node.detail.clone(),
                    children: node.children.to_vec(),
                    literal: node.literal,
                })
                .collect::<Vec<_>>();
            if let Ok(lowered) = lucid_cir::Function::from_typed_function_body(
                &nodes,
                root.id,
                &function.parameter_names,
            ) {
                return Ok(Arc::new(lowered));
            }
        }
        let parameter_index = match subject {
            lucid_syntax::Expr::Ident { name, .. } => function
                .parameter_names
                .iter()
                .position(|parameter| parameter == name),
            _ => None,
        };
        let primitive_literal = |expr: &lucid_syntax::Expr| match expr {
            lucid_syntax::Expr::Literal {
                value: lucid_syntax::LiteralValue::Int(value),
                ..
            } => Some(lucid_cir::TypedLiteral::Int(*value)),
            lucid_syntax::Expr::Literal {
                value: lucid_syntax::LiteralValue::Bool(value),
                ..
            } => Some(lucid_cir::TypedLiteral::Bool(*value)),
            _ => None,
        };
        fn match_statement_value(statements: &[lucid_syntax::Stmt]) -> Option<&lucid_syntax::Expr> {
            let mut meaningful = statements
                .iter()
                .filter(|statement| !branch_noop_statement(statement));
            let first = meaningful.next()?;
            let second = meaningful.next();
            if meaningful.next().is_some() {
                return None;
            }
            match (first, second) {
                (
                    lucid_syntax::Stmt::Return {
                        value: Some(value), ..
                    },
                    None,
                ) => Some(value),
                (
                    lucid_syntax::Stmt::Assignment {
                        target: lucid_syntax::Expr::Ident { name, .. },
                        value,
                        ..
                    },
                    Some(lucid_syntax::Stmt::Return {
                        value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
                        ..
                    }),
                )
                | (
                    lucid_syntax::Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(name, _),
                        value: Some(value),
                        ..
                    },
                    Some(lucid_syntax::Stmt::Return {
                        value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
                        ..
                    }),
                ) if name == returned => Some(value),
                (
                    lucid_syntax::Stmt::If {
                        condition,
                        then_branch,
                        elif_branches,
                        else_branch,
                        ..
                    },
                    None,
                ) => match static_branch_selection(
                    condition,
                    then_branch,
                    elif_branches,
                    else_branch.as_ref(),
                ) {
                    StaticBranch::Selected(branch) => match_statement_value(branch),
                    StaticBranch::Empty | StaticBranch::Unknown => None,
                },
                _ => None,
            }
        }
        fn match_arm_value(arm: &lucid_syntax::MatchArm) -> Option<&lucid_syntax::Expr> {
            match_statement_value(&arm.body)
        }
        fn match_statements_are_void(statements: &[lucid_syntax::Stmt]) -> bool {
            let Some((last, prefix)) = statements.split_last() else {
                return true;
            };
            if prefix.iter().all(branch_noop_statement)
                && matches!(last, lucid_syntax::Stmt::Return { value: None, .. })
            {
                return true;
            }
            if statements.iter().all(branch_noop_statement) {
                return true;
            }
            let mut meaningful = statements
                .iter()
                .filter(|statement| !branch_noop_statement(statement));
            let Some(lucid_syntax::Stmt::If {
                condition,
                then_branch,
                elif_branches,
                else_branch,
                ..
            }) = meaningful.next()
            else {
                return false;
            };
            if meaningful.next().is_some() {
                return false;
            }
            match static_branch_selection(
                condition,
                then_branch,
                elif_branches,
                else_branch.as_ref(),
            ) {
                StaticBranch::Selected(branch) => match_statements_are_void(branch),
                StaticBranch::Empty => true,
                StaticBranch::Unknown => false,
            }
        }
        fn match_arm_is_void(arm: &lucid_syntax::MatchArm) -> bool {
            match_statements_are_void(&arm.body)
        }
        fn match_arm_void_bindings(
            arm: &lucid_syntax::MatchArm,
        ) -> Result<Option<Vec<(String, lucid_syntax::Span)>>, Arc<str>> {
            let statements = arm.body.as_slice();
            let setup = match statements.split_last() {
                Some((lucid_syntax::Stmt::Return { value: None, .. }, prefix))
                | Some((lucid_syntax::Stmt::Pass(_), prefix)) => prefix,
                Some((_, _)) => statements,
                None => return Ok(Some(Vec::new())),
            };
            let mut bindings = Vec::new();
            fn collect_match_void_binding(
                statement: &lucid_syntax::Stmt,
                bindings: &mut Vec<(String, lucid_syntax::Span)>,
            ) -> Result<bool, Arc<str>> {
                if branch_noop_statement(statement) {
                    return Ok(true);
                }
                let (name, value) = match statement {
                    lucid_syntax::Stmt::Assignment {
                        target: lucid_syntax::Expr::Ident { name, .. },
                        value,
                        ..
                    }
                    | lucid_syntax::Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(name, _),
                        value: Some(value),
                        ..
                    } => (name, value),
                    lucid_syntax::Stmt::Expr(_) => {
                        return Err(Arc::from(
                            "effectful discarded expression is not supported by typed CIR lowering",
                        ));
                    }
                    lucid_syntax::Stmt::Return { value: None, .. } => return Ok(true),
                    lucid_syntax::Stmt::If {
                        condition,
                        then_branch,
                        elif_branches,
                        else_branch,
                        ..
                    } => {
                        return match static_branch_selection(
                            condition,
                            then_branch,
                            elif_branches,
                            else_branch.as_ref(),
                        ) {
                            StaticBranch::Selected(branch) => {
                                for statement in branch {
                                    if !collect_match_void_binding(statement, bindings)? {
                                        return Ok(false);
                                    }
                                }
                                Ok(true)
                            }
                            StaticBranch::Empty => Ok(true),
                            StaticBranch::Unknown => Ok(false),
                        };
                    }
                    _ => return Ok(false),
                };
                bindings.push((name.clone(), value.span()));
                Ok(true)
            }
            for statement in setup {
                if !collect_match_void_binding(statement, &mut bindings)? {
                    return Ok(None);
                }
            }
            Ok(Some(bindings))
        }
        let lower_match_bindings_to_void =
            |bindings: &[(String, lucid_syntax::Span)]| -> Result<Arc<lucid_cir::Function>, Arc<str>> {
                if bindings.is_empty() {
                    let function = lucid_cir::Function {
                        entry: lucid_cir::BlockId(0),
                        blocks: vec![lucid_cir::Block {
                            id: lucid_cir::BlockId(0),
                            instructions: function
                                .parameter_names
                                .iter()
                                .enumerate()
                                .map(|(index, _)| lucid_cir::Instruction::Param {
                                    result: lucid_cir::ValueId(index as u32),
                                    index: index as u32,
                                })
                                .collect(),
                            terminator: lucid_cir::Terminator::Return(None),
                        }],
                    };
                    function
                        .verify()
                        .map_err(|_| Arc::<str>::from("invalid void function CIR"))?;
                    return Ok(Arc::new(function));
                }
                let nodes = function
                    .body_expressions
                    .iter()
                    .map(|node| lucid_cir::TypedExprNode {
                        id: node.id,
                        kind: node.kind.clone(),
                        detail: node.detail.clone(),
                        children: node.children.to_vec(),
                        literal: node.literal,
                    })
                    .collect::<Vec<_>>();
                let local_bindings = bindings
                    .iter()
                    .map(|(name, span)| {
                        function
                            .body_expressions
                            .iter()
                            .rev()
                            .find(|node| node.span == *span)
                            .map(|node| (name.clone(), node.id))
                            .ok_or_else(|| Arc::<str>::from("local binding has no typed expression"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let root_id = local_bindings
                    .last()
                    .map(|(_, id)| *id)
                    .ok_or_else(|| Arc::<str>::from("function has no lowerable expression"))?;
                let mut lowered = lucid_cir::Function::from_typed_function_body_with_locals(
                    &nodes,
                    root_id,
                    &function.parameter_names,
                    &local_bindings,
                )
                .map_err(|_| Arc::<str>::from("unsupported constant match expression"))?;
                if let Some(block) = lowered.blocks.last_mut() {
                    block.terminator = lucid_cir::Terminator::Return(None);
                }
                lowered
                    .verify()
                    .map_err(|_| Arc::<str>::from("invalid void function CIR"))?;
                Ok(Arc::new(lowered))
            };
        let pattern_literal = |pattern: &lucid_syntax::Pattern| match pattern {
            lucid_syntax::Pattern::Literal(lucid_syntax::LiteralValue::Int(value), _) => {
                Some(lucid_cir::TypedLiteral::Int(*value))
            }
            lucid_syntax::Pattern::Literal(lucid_syntax::LiteralValue::Bool(value), _) => {
                Some(lucid_cir::TypedLiteral::Bool(*value))
            }
            _ => None,
        };
        let constant_selected_arm = primitive_literal(subject).and_then(|subject_literal| {
            let mut selected = None;
            for arm in arms {
                let arm_matches = matches!(arm.pattern, lucid_syntax::Pattern::Wildcard(_))
                    || pattern_literal(&arm.pattern) == Some(subject_literal);
                if arm_matches {
                    if arm.guard.is_some() {
                        return None;
                    }
                    selected = Some(arm);
                    break;
                }
                if pattern_literal(&arm.pattern).is_none() {
                    return None;
                }
            }
            selected
        });
        if let Some(selected_arm) = constant_selected_arm {
            if let Some(value) = match_arm_value(selected_arm) {
                let nodes = function
                    .body_expressions
                    .iter()
                    .map(|node| lucid_cir::TypedExprNode {
                        id: node.id,
                        kind: node.kind.clone(),
                        detail: node.detail.clone(),
                        children: node.children.to_vec(),
                        literal: node.literal,
                    })
                    .collect::<Vec<_>>();
                if let Some(root) = function
                    .body_expressions
                    .iter()
                    .rev()
                    .find(|node| node.span == value.span())
                    && let Ok(lowered) = lucid_cir::Function::from_typed_function_body(
                        &nodes,
                        root.id,
                        &function.parameter_names,
                    )
                {
                    return Ok(Arc::new(lowered));
                }
                return Err(Arc::from("unsupported constant match expression"));
            }
            if match_arm_is_void(selected_arm) {
                return lower_match_bindings_to_void(&[]);
            }
            if let Some(bindings) = match_arm_void_bindings(selected_arm)? {
                return lower_match_bindings_to_void(&bindings);
            }
            return Err(Arc::from("unsupported constant match expression"));
        }
        let arm_condition = |arm: &lucid_syntax::MatchArm| match &arm.pattern {
            lucid_syntax::Pattern::Literal(
                literal
                @ (lucid_syntax::LiteralValue::Int(_) | lucid_syntax::LiteralValue::Bool(_)),
                span,
            ) => {
                let pattern_test = lucid_syntax::Expr::Binary {
                    op: lucid_syntax::BinaryOp::Eq,
                    left: Box::new(subject.clone()),
                    right: Box::new(lucid_syntax::Expr::Literal {
                        value: literal.clone(),
                        span: *span,
                    }),
                    span: *span,
                };
                Some(match &arm.guard {
                    Some(guard) => lucid_syntax::Expr::Binary {
                        op: lucid_syntax::BinaryOp::And,
                        left: Box::new(pattern_test),
                        right: Box::new(guard.clone()),
                        span: guard.span(),
                    },
                    None => pattern_test,
                })
            }
            lucid_syntax::Pattern::Wildcard(_) => arm.guard.clone(),
            _ => None,
        };
        if arms.iter().any(|arm| arm.guard.is_some())
            && arms.iter().all(|arm| {
                matches!(
                    arm.pattern,
                    lucid_syntax::Pattern::Literal(
                        lucid_syntax::LiteralValue::Int(_) | lucid_syntax::LiteralValue::Bool(_),
                        _
                    ) | lucid_syntax::Pattern::Wildcard(_)
                ) && match_arm_value(arm).is_some()
            })
        {
            let mut conditions = Vec::new();
            let mut values = Vec::new();
            let mut else_value = None;
            for arm in arms {
                let Some(value) = match_arm_value(arm) else {
                    return Err(Arc::from("unsupported guarded match arm"));
                };
                if let Some(condition) = arm_condition(arm) {
                    conditions.push(condition);
                    values.push(value);
                } else {
                    else_value = Some(value);
                    break;
                }
            }
            if let Some((first_condition, elif_conditions)) = conditions.split_first()
                && let Some((first_value, elif_values)) = values.split_first()
            {
                let elif_pairs = elif_conditions
                    .iter()
                    .zip(elif_values.iter())
                    .map(|(condition, value)| (condition, *value))
                    .collect::<Vec<_>>();
                if let Some(else_value) = else_value {
                    if elif_pairs.is_empty() {
                        return lucid_cir::Function::from_parameterized_if_direct(
                            first_condition,
                            first_value,
                            else_value,
                            &function.parameter_names,
                        )
                        .map(Arc::new)
                        .map_err(|_| Arc::from("unsupported guarded match expression"));
                    }
                    return lucid_cir::Function::from_parameterized_if_elif_chain_direct(
                        first_condition,
                        first_value,
                        &elif_pairs,
                        else_value,
                        &function.parameter_names,
                    )
                    .map(Arc::new)
                    .map_err(|_| Arc::from("unsupported guarded match chain"));
                }
                if elif_pairs.is_empty() {
                    return lucid_cir::Function::from_parameterized_if_optional(
                        first_condition,
                        first_value,
                        &function.parameter_names,
                    )
                    .map(Arc::new)
                    .map_err(|_| Arc::from("unsupported optional guarded match expression"));
                }
                return lucid_cir::Function::from_parameterized_if_elif_optional_chain(
                    first_condition,
                    first_value,
                    &elif_pairs,
                    &function.parameter_names,
                )
                .map(Arc::new)
                .map_err(|_| Arc::from("unsupported optional guarded match chain"));
            }
        }
        if let Some(wildcard_index) = arms.iter().position(|arm| {
            matches!(arm.pattern, lucid_syntax::Pattern::Wildcard(_)) && arm.guard.is_none()
        }) && wildcard_index > 0
            && arms[..wildcard_index]
                .iter()
                .all(|arm| matches!(arm.pattern, lucid_syntax::Pattern::Literal(_, _)))
        {
            let wildcard_arm = &arms[wildcard_index];
            if let Some(wildcard_value) = match_arm_value(wildcard_arm) {
                let explicit_values = arms[..wildcard_index]
                    .iter()
                    .map(match_arm_value)
                    .collect::<Option<Vec<_>>>();
                if let Some(explicit_values) = explicit_values {
                    let conditions = arms[..wildcard_index]
                        .iter()
                        .map(arm_condition)
                        .collect::<Option<Vec<_>>>();
                    if let Some(conditions) = conditions
                        && let Some((first_condition, elif_conditions)) = conditions.split_first()
                        && let Some((first_value, elif_values)) = explicit_values.split_first()
                    {
                        let elif_pairs = elif_conditions
                            .iter()
                            .zip(elif_values.iter())
                            .map(|(condition, value)| (condition, *value))
                            .collect::<Vec<_>>();
                        if elif_pairs.is_empty() {
                            return lucid_cir::Function::from_parameterized_if_direct(
                                first_condition,
                                first_value,
                                wildcard_value,
                                &function.parameter_names,
                            )
                            .map(Arc::new)
                            .map_err(|_| {
                                Arc::from("unsupported middle wildcard match expression")
                            });
                        }
                        return lucid_cir::Function::from_parameterized_if_elif_chain_direct(
                            first_condition,
                            first_value,
                            &elif_pairs,
                            wildcard_value,
                            &function.parameter_names,
                        )
                        .map(Arc::new)
                        .map_err(|_| Arc::from("unsupported middle wildcard match chain"));
                    }
                }
            } else if match_arm_is_void(wildcard_arm)
                && arms[..wildcard_index].iter().all(match_arm_is_void)
            {
                let conditions = arms[..wildcard_index]
                    .iter()
                    .map(arm_condition)
                    .collect::<Option<Vec<_>>>();
                if let Some(conditions) = conditions
                    && let Some((first_condition, elif_conditions)) = conditions.split_first()
                {
                    if elif_conditions.is_empty() {
                        return lucid_cir::Function::from_parameterized_if_void(
                            first_condition,
                            &function.parameter_names,
                        )
                        .map(Arc::new)
                        .map_err(|_| {
                            Arc::from("unsupported middle wildcard void match expression")
                        });
                    }
                    return lucid_cir::Function::from_parameterized_if_elif_void_chain(
                        first_condition,
                        &elif_conditions.iter().collect::<Vec<_>>(),
                        &function.parameter_names,
                    )
                    .map(Arc::new)
                    .map_err(|_| Arc::from("unsupported middle wildcard void match chain"));
                }
            }
        }
        if arms.len() >= 3
            && let Some(parameter_index) = parameter_index
            && arms[..arms.len() - 1]
                .iter()
                .all(|arm| matches!(arm.pattern, lucid_syntax::Pattern::Literal(_, _)))
            && matches!(
                arms.last().map(|arm| &arm.pattern),
                Some(lucid_syntax::Pattern::Wildcard(_))
            )
        {
            let explicit = arms[..arms.len() - 1]
                .iter()
                .map(|arm| {
                    let lucid_syntax::Pattern::Literal(pattern, _) = &arm.pattern else {
                        return None;
                    };
                    let pattern = match pattern {
                        lucid_syntax::LiteralValue::Int(value) => {
                            lucid_cir::TypedLiteral::Int(*value)
                        }
                        lucid_syntax::LiteralValue::Bool(value) => {
                            lucid_cir::TypedLiteral::Bool(*value)
                        }
                        _ => return None,
                    };
                    Some((pattern, primitive_literal(match_arm_value(arm)?)?))
                })
                .collect::<Option<Vec<_>>>();
            let wildcard = arms
                .last()
                .and_then(|arm| primitive_literal(match_arm_value(arm)?));
            if let (Some(explicit), Some(wildcard)) = (explicit, wildcard) {
                return lucid_cir::Function::from_parameterized_literal_match(
                    function.parameter_names.len(),
                    parameter_index,
                    &explicit,
                    wildcard,
                )
                .map(Arc::new)
                .map_err(|_| Arc::from("unsupported literal match for function CIR lowering"));
            }
        }
        if arms.len() >= 3
            && arms[..arms.len() - 1]
                .iter()
                .all(|arm| matches!(arm.pattern, lucid_syntax::Pattern::Literal(_, _)))
            && matches!(
                arms.last().map(|arm| &arm.pattern),
                Some(lucid_syntax::Pattern::Wildcard(_))
            )
        {
            let explicit_values = arms[..arms.len() - 1]
                .iter()
                .map(match_arm_value)
                .collect::<Option<Vec<_>>>();
            let wildcard_value = arms.last().and_then(match_arm_value);
            if let (Some(explicit_values), Some(wildcard_value)) = (explicit_values, wildcard_value)
            {
                let conditions = arms[..arms.len() - 1]
                    .iter()
                    .map(|arm| {
                        let lucid_syntax::Pattern::Literal(literal, span) = &arm.pattern else {
                            return None;
                        };
                        match literal {
                            lucid_syntax::LiteralValue::Int(_)
                            | lucid_syntax::LiteralValue::Bool(_) => {
                                Some(lucid_syntax::Expr::Binary {
                                    op: lucid_syntax::BinaryOp::Eq,
                                    left: Box::new(subject.clone()),
                                    right: Box::new(lucid_syntax::Expr::Literal {
                                        value: literal.clone(),
                                        span: *span,
                                    }),
                                    span: *span,
                                })
                            }
                            _ => None,
                        }
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(conditions) = conditions
                    && let Some((first_condition, elif_conditions)) = conditions.split_first()
                    && let Some((first_value, elif_values)) = explicit_values.split_first()
                {
                    let elif_pairs = elif_conditions
                        .iter()
                        .zip(elif_values.iter())
                        .map(|(condition, value)| (condition, *value))
                        .collect::<Vec<_>>();
                    return lucid_cir::Function::from_parameterized_if_elif_chain_direct(
                        first_condition,
                        first_value,
                        &elif_pairs,
                        wildcard_value,
                        &function.parameter_names,
                    )
                    .map(Arc::new)
                    .map_err(|_| Arc::from("unsupported multi-arm match expression"));
                }
            }
        }
        if !arms.is_empty()
            && arms
                .iter()
                .all(|arm| matches!(arm.pattern, lucid_syntax::Pattern::Literal(_, _)))
        {
            let values = arms.iter().map(match_arm_value).collect::<Option<Vec<_>>>();
            if let Some(values) = values {
                let conditions = arms
                    .iter()
                    .map(|arm| {
                        let lucid_syntax::Pattern::Literal(literal, span) = &arm.pattern else {
                            return None;
                        };
                        match literal {
                            lucid_syntax::LiteralValue::Int(_)
                            | lucid_syntax::LiteralValue::Bool(_) => {
                                Some(lucid_syntax::Expr::Binary {
                                    op: lucid_syntax::BinaryOp::Eq,
                                    left: Box::new(subject.clone()),
                                    right: Box::new(lucid_syntax::Expr::Literal {
                                        value: literal.clone(),
                                        span: *span,
                                    }),
                                    span: *span,
                                })
                            }
                            _ => None,
                        }
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(conditions) = conditions
                    && let Some((first_condition, elif_conditions)) = conditions.split_first()
                    && let Some((first_value, elif_values)) = values.split_first()
                {
                    if elif_conditions.is_empty() {
                        return lucid_cir::Function::from_parameterized_if_optional(
                            first_condition,
                            first_value,
                            &function.parameter_names,
                        )
                        .map(Arc::new)
                        .map_err(|_| Arc::from("unsupported optional match expression"));
                    }
                    let elif_pairs = elif_conditions
                        .iter()
                        .zip(elif_values.iter())
                        .map(|(condition, value)| (condition, *value))
                        .collect::<Vec<_>>();
                    return lucid_cir::Function::from_parameterized_if_elif_optional_chain(
                        first_condition,
                        first_value,
                        &elif_pairs,
                        &function.parameter_names,
                    )
                    .map(Arc::new)
                    .map_err(|_| Arc::from("unsupported optional match chain"));
                }
            }
        }
        if arms.len() >= 2
            && arms[..arms.len() - 1]
                .iter()
                .all(|arm| matches!(arm.pattern, lucid_syntax::Pattern::Literal(_, _)))
            && matches!(
                arms.last().map(|arm| &arm.pattern),
                Some(lucid_syntax::Pattern::Wildcard(_))
            )
            && arms.last().is_some_and(match_arm_is_void)
        {
            let values = arms[..arms.len() - 1]
                .iter()
                .map(match_arm_value)
                .collect::<Option<Vec<_>>>();
            if let Some(values) = values {
                let conditions = arms[..arms.len() - 1]
                    .iter()
                    .map(|arm| {
                        let lucid_syntax::Pattern::Literal(literal, span) = &arm.pattern else {
                            return None;
                        };
                        match literal {
                            lucid_syntax::LiteralValue::Int(_)
                            | lucid_syntax::LiteralValue::Bool(_) => {
                                Some(lucid_syntax::Expr::Binary {
                                    op: lucid_syntax::BinaryOp::Eq,
                                    left: Box::new(subject.clone()),
                                    right: Box::new(lucid_syntax::Expr::Literal {
                                        value: literal.clone(),
                                        span: *span,
                                    }),
                                    span: *span,
                                })
                            }
                            _ => None,
                        }
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(conditions) = conditions
                    && let Some((first_condition, elif_conditions)) = conditions.split_first()
                    && let Some((first_value, elif_values)) = values.split_first()
                {
                    if elif_conditions.is_empty() {
                        return lucid_cir::Function::from_parameterized_if_optional(
                            first_condition,
                            first_value,
                            &function.parameter_names,
                        )
                        .map(Arc::new)
                        .map_err(|_| Arc::from("unsupported pass fallback match expression"));
                    }
                    let elif_pairs = elif_conditions
                        .iter()
                        .zip(elif_values.iter())
                        .map(|(condition, value)| (condition, *value))
                        .collect::<Vec<_>>();
                    return lucid_cir::Function::from_parameterized_if_elif_optional_chain(
                        first_condition,
                        first_value,
                        &elif_pairs,
                        &function.parameter_names,
                    )
                    .map(Arc::new)
                    .map_err(|_| Arc::from("unsupported pass fallback match chain"));
                }
            }
        }
        if arms.iter().any(|arm| arm.guard.is_some())
            && arms.iter().all(|arm| {
                matches!(
                    arm.pattern,
                    lucid_syntax::Pattern::Literal(
                        lucid_syntax::LiteralValue::Int(_) | lucid_syntax::LiteralValue::Bool(_),
                        _
                    ) | lucid_syntax::Pattern::Wildcard(_)
                ) && match_arm_is_void(arm)
            })
        {
            let mut conditions = Vec::new();
            for arm in arms {
                if let Some(condition) = arm_condition(arm) {
                    conditions.push(condition);
                } else {
                    break;
                }
            }
            if let Some((first_condition, elif_conditions)) = conditions.split_first() {
                if elif_conditions.is_empty() {
                    return lucid_cir::Function::from_parameterized_if_void(
                        first_condition,
                        &function.parameter_names,
                    )
                    .map(Arc::new)
                    .map_err(|_| Arc::from("unsupported guarded void match expression"));
                }
                return lucid_cir::Function::from_parameterized_if_elif_void_chain(
                    first_condition,
                    &elif_conditions.iter().collect::<Vec<_>>(),
                    &function.parameter_names,
                )
                .map(Arc::new)
                .map_err(|_| Arc::from("unsupported guarded void match chain"));
            }
        }
        if !arms.is_empty() {
            let explicit_end = if matches!(
                arms.last().map(|arm| &arm.pattern),
                Some(lucid_syntax::Pattern::Wildcard(_))
            ) {
                arms.len() - 1
            } else {
                arms.len()
            };
            if explicit_end > 0
                && arms[..explicit_end]
                    .iter()
                    .all(|arm| matches!(arm.pattern, lucid_syntax::Pattern::Literal(_, _)))
                && arms.iter().all(match_arm_is_void)
            {
                let conditions = arms[..explicit_end]
                    .iter()
                    .map(|arm| {
                        let lucid_syntax::Pattern::Literal(literal, span) = &arm.pattern else {
                            return None;
                        };
                        match literal {
                            lucid_syntax::LiteralValue::Int(_)
                            | lucid_syntax::LiteralValue::Bool(_) => {
                                Some(lucid_syntax::Expr::Binary {
                                    op: lucid_syntax::BinaryOp::Eq,
                                    left: Box::new(subject.clone()),
                                    right: Box::new(lucid_syntax::Expr::Literal {
                                        value: literal.clone(),
                                        span: *span,
                                    }),
                                    span: *span,
                                })
                            }
                            _ => None,
                        }
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(conditions) = conditions
                    && let Some((first_condition, elif_conditions)) = conditions.split_first()
                {
                    if elif_conditions.is_empty() {
                        return lucid_cir::Function::from_parameterized_if_void(
                            first_condition,
                            &function.parameter_names,
                        )
                        .map(Arc::new)
                        .map_err(|_| Arc::from("unsupported void match expression"));
                    }
                    return lucid_cir::Function::from_parameterized_if_elif_void_chain(
                        first_condition,
                        &elif_conditions.iter().collect::<Vec<_>>(),
                        &function.parameter_names,
                    )
                    .map(Arc::new)
                    .map_err(|_| Arc::from("unsupported void match chain"));
                }
            }
        }
        if let Some(first_arm) = arms.first()
            && matches!(first_arm.pattern, lucid_syntax::Pattern::Wildcard(_))
        {
            if let Some(value) = match_arm_value(first_arm) {
                let nodes = function
                    .body_expressions
                    .iter()
                    .map(|node| lucid_cir::TypedExprNode {
                        id: node.id,
                        kind: node.kind.clone(),
                        detail: node.detail.clone(),
                        children: node.children.to_vec(),
                        literal: node.literal,
                    })
                    .collect::<Vec<_>>();
                if let Some(root) = function
                    .body_expressions
                    .iter()
                    .rev()
                    .find(|node| node.span == value.span())
                    && let Ok(lowered) = lucid_cir::Function::from_typed_function_body(
                        &nodes,
                        root.id,
                        &function.parameter_names,
                    )
                {
                    return Ok(Arc::new(lowered));
                }
                return Err(Arc::from("unsupported leading wildcard match expression"));
            }
            if match_arm_is_void(first_arm) {
                return lower_match_bindings_to_void(&[]);
            }
            if let Some(bindings) = match_arm_void_bindings(first_arm)? {
                return lower_match_bindings_to_void(&bindings);
            }
        }
        if arms.len() != 2 {
            return Err(Arc::from(
                "unsupported match shape for function CIR lowering",
            ));
        }
        let (literal_arm, wildcard_arm) = match (&arms[0].pattern, &arms[1].pattern) {
            (lucid_syntax::Pattern::Literal(_, _), lucid_syntax::Pattern::Wildcard(_)) => {
                (&arms[0], &arms[1])
            }
            (lucid_syntax::Pattern::Wildcard(_), lucid_syntax::Pattern::Literal(_, _)) => {
                (&arms[1], &arms[0])
            }
            _ => {
                return Err(Arc::from(
                    "match function requires a literal and wildcard arm",
                ));
            }
        };
        let Some(then_value) = match_arm_value(literal_arm) else {
            return Err(Arc::from("unsupported match arm for function CIR lowering"));
        };
        let Some(else_value) = match_arm_value(wildcard_arm) else {
            return Err(Arc::from("unsupported match arm for function CIR lowering"));
        };
        let lucid_syntax::Pattern::Literal(literal, span) = &literal_arm.pattern else {
            return Err(Arc::from("unsupported match literal pattern"));
        };
        let condition = lucid_syntax::Expr::Binary {
            op: lucid_syntax::BinaryOp::Eq,
            left: Box::new(subject.clone()),
            right: Box::new(lucid_syntax::Expr::Literal {
                value: literal.clone(),
                span: *span,
            }),
            span: *span,
        };
        return lucid_cir::Function::from_parameterized_if(
            &condition,
            then_value,
            else_value,
            &function.parameter_names,
        )
        .map(Arc::new)
        .map_err(|_| Arc::from("unsupported match expression for function CIR lowering"));
    }
    // Prefer the typed expression graph for a complete conditional expression
    // or a statement-level `if` whose arms return directly. The collector
    // records both forms as one post-order `if` node, so lowering does not
    // reconstruct control flow from syntax.
    // This keeps the common dynamic-return shape on the same CST → HIR → CIR
    // path as constant expressions; the AST below remains only a temporary
    // adapter for statement forms that have not reached typed CIR yet.
    if !function.is_async
        && let Some(root) = function
            .body_expressions
            .iter()
            .rev()
            .find(|node| node.kind == "if" && source_function.body.len() == 1)
    {
        let nodes = function
            .body_expressions
            .iter()
            .map(|node| lucid_cir::TypedExprNode {
                id: node.id,
                kind: node.kind.clone(),
                detail: node.detail.clone(),
                children: node.children.to_vec(),
                literal: node.literal,
            })
            .collect::<Vec<_>>();
        if let Ok(lowered) = lucid_cir::Function::from_typed_function_body(
            &nodes,
            root.id,
            &function.parameter_names,
        ) {
            return Ok(Arc::new(lowered));
        }
    }
    // A dynamic conditional with one direct return per arm is already a
    // complete CIR diamond. Lower it before the constant-branch path so this
    // query does not fall back to the legacy AST evaluator.
    fn has_identifier(expr: &lucid_syntax::Expr) -> bool {
        match expr {
            lucid_syntax::Expr::Ident { .. } => true,
            lucid_syntax::Expr::Unary { expr, .. } => has_identifier(expr),
            lucid_syntax::Expr::Binary { left, right, .. } => {
                has_identifier(left) || has_identifier(right)
            }
            _ => false,
        }
    }
    fn has_division(expr: &lucid_syntax::Expr) -> bool {
        match expr {
            lucid_syntax::Expr::Unary { expr, .. } => has_division(expr),
            lucid_syntax::Expr::Binary {
                left, right, op, ..
            } => {
                matches!(
                    op,
                    lucid_syntax::BinaryOp::Div
                        | lucid_syntax::BinaryOp::FloorDiv
                        | lucid_syntax::BinaryOp::Mod
                ) || has_division(left)
                    || has_division(right)
            }
            _ => false,
        }
    }
    fn single_value_return(branch: &[lucid_syntax::Stmt]) -> Option<&lucid_syntax::Expr> {
        let mut meaningful = branch
            .iter()
            .filter(|statement| !branch_noop_statement(statement));
        let first = meaningful.next()?;
        let second = meaningful.next();
        if meaningful.next().is_some() {
            return None;
        }
        match (first, second) {
            (
                lucid_syntax::Stmt::Return {
                    value: Some(value), ..
                },
                None,
            ) => Some(value),
            (
                lucid_syntax::Stmt::Assignment {
                    target: lucid_syntax::Expr::Ident { name: assigned, .. },
                    value,
                    ..
                },
                Some(lucid_syntax::Stmt::Return {
                    value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
                    ..
                }),
            )
            | (
                lucid_syntax::Stmt::VarDef {
                    pattern: lucid_syntax::Pattern::Ident(assigned, _),
                    value: Some(value),
                    ..
                },
                Some(lucid_syntax::Stmt::Return {
                    value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
                    ..
                }),
            ) if assigned == returned => Some(value),
            _ => None,
        }
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch,
            ..
        },
    ] = source_function.body.as_slice()
    {
        let selected_branch = match static_truth(condition) {
            Some(true) => Some(then_branch.as_slice()),
            Some(false) => {
                let mut selected = None;
                let mut unknown = false;
                for (elif_condition, branch) in elif_branches {
                    match static_truth(elif_condition) {
                        Some(true) => {
                            selected = Some(branch.as_slice());
                            break;
                        }
                        Some(false) => {}
                        None => {
                            unknown = true;
                            break;
                        }
                    }
                }
                if unknown {
                    None
                } else {
                    selected.or_else(|| else_branch.as_deref())
                }
            }
            None => None,
        };
        if let Some(
            [
                lucid_syntax::Stmt::If {
                    condition: inner_condition,
                    then_branch: inner_then,
                    elif_branches: inner_elifs,
                    else_branch: inner_else,
                    ..
                },
            ],
        ) = selected_branch
            && !inner_elifs.is_empty()
            && static_truth(inner_condition).is_none()
            && has_identifier(inner_condition)
        {
            let mut elif_values = Vec::new();
            let mut selected_value_fallback = inner_else.as_deref();
            let mut unsupported_elif = false;
            for (elif_condition, elif_branch) in inner_elifs {
                match static_truth(elif_condition) {
                    Some(false) => continue,
                    Some(true) => {
                        selected_value_fallback = Some(elif_branch.as_slice());
                        break;
                    }
                    None => {
                        if !has_identifier(elif_condition) {
                            unsupported_elif = true;
                            break;
                        }
                        let Some(elif_value) = single_value_return(elif_branch) else {
                            unsupported_elif = true;
                            break;
                        };
                        elif_values.push((elif_condition, elif_value));
                    }
                }
            }
            if !unsupported_elif && let Some(then_value) = single_value_return(inner_then) {
                if function.is_async {
                    return Err(Arc::from(
                        "async function bodies are not yet supported by CIR lowering",
                    ));
                }
                let lowered = match selected_value_fallback {
                    Some(else_branch) => {
                        if let Some(else_value) = single_value_return(else_branch) {
                            if elif_values.is_empty() {
                                lucid_cir::Function::from_parameterized_if_direct(
                                    inner_condition,
                                    then_value,
                                    else_value,
                                    &function.parameter_names,
                                )
                            } else {
                                lucid_cir::Function::from_parameterized_if_elif_chain_direct(
                                    inner_condition,
                                    then_value,
                                    &elif_values,
                                    else_value,
                                    &function.parameter_names,
                                )
                            }
                        } else if branch_is_single_void(else_branch) {
                            if elif_values.is_empty() {
                                lucid_cir::Function::from_parameterized_if_optional(
                                    inner_condition,
                                    then_value,
                                    &function.parameter_names,
                                )
                            } else {
                                lucid_cir::Function::from_parameterized_if_elif_optional_chain(
                                    inner_condition,
                                    then_value,
                                    &elif_values,
                                    &function.parameter_names,
                                )
                            }
                        } else {
                            Err(lucid_cir::LowerError::UnsupportedExpression)
                        }
                    }
                    None => {
                        if elif_values.is_empty() {
                            lucid_cir::Function::from_parameterized_if_optional(
                                inner_condition,
                                then_value,
                                &function.parameter_names,
                            )
                        } else {
                            lucid_cir::Function::from_parameterized_if_elif_optional_chain(
                                inner_condition,
                                then_value,
                                &elif_values,
                                &function.parameter_names,
                            )
                        }
                    }
                };
                return lowered
                    .map(Arc::new)
                    .map_err(|_| Arc::from("unsupported selected nested dynamic elif branch"));
            }
            let mut elif_conditions = Vec::new();
            let mut selected_void_fallback = inner_else.as_deref();
            let mut unsupported_void_elif = false;
            for (elif_condition, elif_branch) in inner_elifs {
                match static_truth(elif_condition) {
                    Some(false) => continue,
                    Some(true) => {
                        selected_void_fallback = Some(elif_branch.as_slice());
                        break;
                    }
                    None => {
                        if !has_identifier(elif_condition) || !branch_is_single_void(elif_branch) {
                            unsupported_void_elif = true;
                            break;
                        }
                        elif_conditions.push(elif_condition);
                    }
                }
            }
            if !unsupported_void_elif
                && branch_is_single_void(inner_then)
                && selected_void_fallback.is_none_or(|branch| branch_is_single_void(branch))
            {
                if function.is_async {
                    return Err(Arc::from(
                        "async function bodies are not yet supported by CIR lowering",
                    ));
                }
                let lowered = if elif_conditions.is_empty() {
                    lucid_cir::Function::from_parameterized_if_void(
                        inner_condition,
                        &function.parameter_names,
                    )
                } else {
                    lucid_cir::Function::from_parameterized_if_elif_void_chain(
                        inner_condition,
                        &elif_conditions,
                        &function.parameter_names,
                    )
                };
                return lowered.map(Arc::new).map_err(|_| {
                    Arc::from("unsupported selected nested dynamic void elif branch")
                });
            }
        }
        if let Some(
            [
                lucid_syntax::Stmt::If {
                    condition: inner_condition,
                    then_branch: inner_then,
                    elif_branches: inner_elifs,
                    else_branch: inner_else,
                    ..
                },
            ],
        ) = selected_branch
            && inner_elifs.is_empty()
            && static_truth(inner_condition).is_none()
            && has_identifier(inner_condition)
        {
            if function.is_async {
                return Err(Arc::from(
                    "async function bodies are not yet supported by CIR lowering",
                ));
            }
            if let Some(then_value) = single_value_return(inner_then) {
                let lowered = match inner_else.as_deref() {
                    Some(else_branch) => {
                        if let Some(else_value) = single_value_return(else_branch) {
                            lucid_cir::Function::from_parameterized_if_direct(
                                inner_condition,
                                then_value,
                                else_value,
                                &function.parameter_names,
                            )
                        } else if branch_is_single_void(else_branch) {
                            lucid_cir::Function::from_parameterized_if_optional(
                                inner_condition,
                                then_value,
                                &function.parameter_names,
                            )
                        } else {
                            Err(lucid_cir::LowerError::UnsupportedExpression)
                        }
                    }
                    None => lucid_cir::Function::from_parameterized_if_optional(
                        inner_condition,
                        then_value,
                        &function.parameter_names,
                    ),
                };
                return lowered
                    .map(Arc::new)
                    .map_err(|_| Arc::from("unsupported selected nested dynamic branch"));
            }
            if branch_is_single_void(inner_then)
                && inner_else
                    .as_ref()
                    .is_none_or(|branch| branch_is_single_void(branch))
            {
                return lucid_cir::Function::from_parameterized_if_void(
                    inner_condition,
                    &function.parameter_names,
                )
                .map(Arc::new)
                .map_err(|_| Arc::from("unsupported selected nested dynamic void branch"));
            }
            if let Some(else_branch) = inner_else.as_deref()
                && branch_is_single_void(inner_then)
                && let Some(else_value) = single_value_return(else_branch)
            {
                let inverted = lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Not,
                    expr: Box::new(inner_condition.clone()),
                    span: inner_condition.span(),
                };
                return lucid_cir::Function::from_parameterized_if_optional(
                    &inverted,
                    else_value,
                    &function.parameter_names,
                )
                .map(Arc::new)
                .map_err(|_| Arc::from("unsupported selected nested mixed branch"));
            }
        }
    }
    if let [
        lucid_syntax::Stmt::Return {
            value:
                Some(lucid_syntax::Expr::IfExpr {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                }),
            ..
        },
    ] = source_function.body.as_slice()
        && static_truth(condition).is_none()
        && has_identifier(condition)
    {
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        let lower = if has_division(then_branch) || has_division(else_branch) {
            lucid_cir::Function::from_parameterized_if_direct(
                condition,
                then_branch,
                else_branch,
                &function.parameter_names,
            )
        } else {
            lucid_cir::Function::from_parameterized_if(
                condition,
                then_branch,
                else_branch,
                &function.parameter_names,
            )
        };
        return lower
            .map(Arc::new)
            .map_err(|_| Arc::from("unsupported conditional return expression"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch,
            ..
        },
    ] = source_function.body.as_slice()
        && static_truth(condition).is_none()
        && has_identifier(condition)
        && let [
            lucid_syntax::Stmt::Return {
                value: Some(then_value),
                ..
            },
        ] = then_branch.as_slice()
        && !elif_branches.is_empty()
        && elif_branches.iter().all(|(elif_condition, branch)| {
            static_truth(elif_condition).is_some()
                && matches!(
                    branch.as_slice(),
                    [lucid_syntax::Stmt::Return { value: Some(_), .. }]
                )
        })
    {
        let selected_elif = elif_branches.iter().find_map(|(elif_condition, branch)| {
            if static_truth(elif_condition) == Some(true) {
                match branch.as_slice() {
                    [
                        lucid_syntax::Stmt::Return {
                            value: Some(value), ..
                        },
                    ] => Some(value),
                    _ => None,
                }
            } else {
                None
            }
        });
        if selected_elif.is_none() && else_branch.is_none() {
            let nodes = function
                .body_expressions
                .iter()
                .map(|node| lucid_cir::TypedExprNode {
                    id: node.id,
                    kind: node.kind.clone(),
                    detail: node.detail.clone(),
                    children: node.children.to_vec(),
                    literal: node.literal,
                })
                .collect::<Vec<_>>();
            let find_id = |span: lucid_syntax::Span| {
                function
                    .body_expressions
                    .iter()
                    .find(|node| node.span == span)
                    .map(|node| node.id)
            };
            if let (Some(condition_id), Some(then_id)) =
                (find_id(condition.span()), find_id(then_value.span()))
                && let Ok(lowered) = lucid_cir::Function::from_typed_statement_if_optional(
                    &nodes,
                    condition_id,
                    then_id,
                    &function.parameter_names,
                    &[],
                )
            {
                return Ok(Arc::new(lowered));
            }
            if function.is_async {
                return Err(Arc::from(
                    "async function bodies are not yet supported by CIR lowering",
                ));
            }
            return lucid_cir::Function::from_parameterized_if_optional(
                condition,
                then_value,
                &function.parameter_names,
            )
            .map(Arc::new)
            .map_err(|_| Arc::from("unsupported dynamic elif fall-through"));
        }
        let else_value = match selected_elif.or(match else_branch.as_deref() {
            Some(
                [
                    lucid_syntax::Stmt::Return {
                        value: Some(value), ..
                    },
                ],
            ) => Some(value),
            _ => None,
        }) {
            Some(value) => value,
            None => return Err(Arc::from("unsupported static elif fall-through")),
        };
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        let lower = if has_division(then_value) || has_division(else_value) {
            lucid_cir::Function::from_parameterized_if_direct(
                condition,
                then_value,
                else_value,
                &function.parameter_names,
            )
        } else {
            lucid_cir::Function::from_parameterized_if(
                condition,
                then_value,
                else_value,
                &function.parameter_names,
            )
        };
        return lower
            .map(Arc::new)
            .map_err(|_| Arc::from("unsupported dynamic elif branch"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            elif_branches,
            else_branch,
            ..
        },
    ] = source_function.body.as_slice()
        && static_truth(condition) == Some(false)
        && !elif_branches.is_empty()
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| static_truth(elif_condition).is_none())
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| has_identifier(elif_condition))
        && let Some(elif_values) = elif_branches
            .iter()
            .map(|(condition, branch)| match branch.as_slice() {
                [
                    lucid_syntax::Stmt::Return {
                        value: Some(value), ..
                    },
                ] => Some((condition, value)),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
        && let Some(((first_condition, first_value), tail_values)) = elif_values.split_first()
    {
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        let lowered = match else_branch.as_deref() {
            Some(
                [
                    lucid_syntax::Stmt::Return {
                        value: Some(else_value),
                        ..
                    },
                ],
            ) => {
                if tail_values.is_empty() {
                    lucid_cir::Function::from_parameterized_if_direct(
                        first_condition,
                        first_value,
                        else_value,
                        &function.parameter_names,
                    )
                } else {
                    lucid_cir::Function::from_parameterized_if_elif_chain_direct(
                        first_condition,
                        first_value,
                        tail_values,
                        else_value,
                        &function.parameter_names,
                    )
                }
            }
            None => {
                if tail_values.is_empty() {
                    lucid_cir::Function::from_parameterized_if_optional(
                        first_condition,
                        first_value,
                        &function.parameter_names,
                    )
                } else {
                    lucid_cir::Function::from_parameterized_if_elif_optional_chain(
                        first_condition,
                        first_value,
                        tail_values,
                        &function.parameter_names,
                    )
                }
            }
            _ => Err(lucid_cir::LowerError::UnsupportedExpression),
        };
        return lowered
            .map(Arc::new)
            .map_err(|_| Arc::from("unsupported false-leading return elif chain"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            elif_branches,
            else_branch: Some(else_branch),
            ..
        },
    ] = source_function.body.as_slice()
        && static_truth(condition) == Some(false)
        && let [(elif_condition, elif_branch)] = elif_branches.as_slice()
        && static_truth(elif_condition).is_none()
        && has_identifier(elif_condition)
        && branch_is_single_void(elif_branch)
        && let [
            lucid_syntax::Stmt::Return {
                value: Some(else_value),
                ..
            },
        ] = else_branch.as_slice()
    {
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        let inverted = lucid_syntax::Expr::Unary {
            op: lucid_syntax::UnaryOp::Not,
            expr: Box::new(elif_condition.clone()),
            span: elif_condition.span(),
        };
        return lucid_cir::Function::from_parameterized_if_optional(
            &inverted,
            else_value,
            &function.parameter_names,
        )
        .map(Arc::new)
        .map_err(|_| Arc::from("unsupported false-leading mixed elif chain"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch: Some(else_branch),
            elif_branches,
            ..
        },
        lucid_syntax::Stmt::Return {
            value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
            ..
        },
    ] = source_function.body.as_slice()
        && elif_branches.is_empty()
        && static_truth(condition).is_none()
        && has_identifier(condition)
    {
        fn assigned_value(
            statements: &[lucid_syntax::Stmt],
        ) -> Option<(&str, &lucid_syntax::Expr)> {
            let mut assignment = None;
            for statement in statements {
                if matches!(statement, lucid_syntax::Stmt::Pass(_)) {
                    continue;
                }
                let value = match statement {
                    lucid_syntax::Stmt::Assignment {
                        target: lucid_syntax::Expr::Ident { name, .. },
                        value,
                        ..
                    }
                    | lucid_syntax::Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(name, _),
                        value: Some(value),
                        ..
                    } => (name.as_str(), value),
                    _ => return None,
                };
                if assignment.replace(value).is_some() {
                    return None;
                }
            }
            assignment
        }
        if let (Some((then_name, then_value)), Some((else_name, else_value))) =
            (assigned_value(then_branch), assigned_value(else_branch))
            && then_name == returned
            && else_name == returned
        {
            let nodes = function
                .body_expressions
                .iter()
                .map(|node| lucid_cir::TypedExprNode {
                    id: node.id,
                    kind: node.kind.clone(),
                    detail: node.detail.clone(),
                    children: node.children.to_vec(),
                    literal: node.literal,
                })
                .collect::<Vec<_>>();
            let find_id = |span: lucid_syntax::Span| {
                function
                    .body_expressions
                    .iter()
                    .find(|node| node.span == span)
                    .map(|node| node.id)
            };
            if let (Some(condition_id), Some(then_id), Some(else_id)) = (
                find_id(condition.span()),
                find_id(then_value.span()),
                find_id(else_value.span()),
            ) && let Ok(lowered) = lucid_cir::Function::from_typed_statement_if(
                &nodes,
                condition_id,
                then_id,
                else_id,
                &function.parameter_names,
                &[],
            ) {
                return Ok(Arc::new(lowered));
            }
            if function.is_async {
                return Err(Arc::from(
                    "async function bodies are not yet supported by CIR lowering",
                ));
            }
            let lower = if has_division(then_value) || has_division(else_value) {
                lucid_cir::Function::from_parameterized_if_direct(
                    condition,
                    then_value,
                    else_value,
                    &function.parameter_names,
                )
            } else {
                lucid_cir::Function::from_parameterized_if(
                    condition,
                    then_value,
                    else_value,
                    &function.parameter_names,
                )
            };
            return lower
                .map(Arc::new)
                .map_err(|_| Arc::from("unsupported dynamic local branch"));
        }
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch: Some(else_branch),
            elif_branches,
            ..
        },
        lucid_syntax::Stmt::Return {
            value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
            ..
        },
    ] = source_function.body.as_slice()
        && !elif_branches.is_empty()
        && static_truth(condition).is_none()
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| static_truth(elif_condition).is_none())
        && has_identifier(condition)
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| has_identifier(elif_condition))
    {
        fn assigned_value(
            statements: &[lucid_syntax::Stmt],
        ) -> Option<(&str, &lucid_syntax::Expr)> {
            let mut assignment = None;
            for statement in statements {
                if matches!(statement, lucid_syntax::Stmt::Pass(_)) {
                    continue;
                }
                let value = match statement {
                    lucid_syntax::Stmt::Assignment {
                        target: lucid_syntax::Expr::Ident { name, .. },
                        value,
                        ..
                    }
                    | lucid_syntax::Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(name, _),
                        value: Some(value),
                        ..
                    } => (name.as_str(), value),
                    _ => return None,
                };
                if assignment.replace(value).is_some() {
                    return None;
                }
            }
            assignment
        }
        if let (Some((then_name, then_value)), Some((else_name, else_value))) =
            (assigned_value(then_branch), assigned_value(else_branch))
            && then_name == returned
            && else_name == returned
            && let Some(elif_values) = elif_branches
                .iter()
                .map(|(condition, branch)| {
                    assigned_value(branch)
                        .and_then(|(name, value)| (name == returned).then_some((condition, value)))
                })
                .collect::<Option<Vec<_>>>()
        {
            if function.is_async {
                return Err(Arc::from(
                    "async function bodies are not yet supported by CIR lowering",
                ));
            }
            return lucid_cir::Function::from_parameterized_if_elif_chain_direct(
                condition,
                then_value,
                &elif_values,
                else_value,
                &function.parameter_names,
            )
            .map(Arc::new)
            .map_err(|_| Arc::from("unsupported dynamic local elif chain"));
        }
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            elif_branches,
            else_branch: Some(else_branch),
            ..
        },
        lucid_syntax::Stmt::Return {
            value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
            ..
        },
    ] = source_function.body.as_slice()
        && static_truth(condition) == Some(false)
        && !elif_branches.is_empty()
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| static_truth(elif_condition).is_none())
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| has_identifier(elif_condition))
    {
        fn assigned_value(
            statements: &[lucid_syntax::Stmt],
        ) -> Option<(&str, &lucid_syntax::Expr)> {
            let mut assignment = None;
            for statement in statements {
                if matches!(statement, lucid_syntax::Stmt::Pass(_)) {
                    continue;
                }
                let value = match statement {
                    lucid_syntax::Stmt::Assignment {
                        target: lucid_syntax::Expr::Ident { name, .. },
                        value,
                        ..
                    }
                    | lucid_syntax::Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(name, _),
                        value: Some(value),
                        ..
                    } => (name.as_str(), value),
                    _ => return None,
                };
                if assignment.replace(value).is_some() {
                    return None;
                }
            }
            assignment
        }
        let Some((else_name, else_value)) = assigned_value(else_branch) else {
            return Err(Arc::from("unsupported false-leading local elif chain"));
        };
        if else_name == returned
            && let Some(elif_values) = elif_branches
                .iter()
                .map(|(condition, branch)| {
                    assigned_value(branch)
                        .and_then(|(name, value)| (name == returned).then_some((condition, value)))
                })
                .collect::<Option<Vec<_>>>()
            && let Some(((first_condition, first_value), tail_values)) = elif_values.split_first()
        {
            if function.is_async {
                return Err(Arc::from(
                    "async function bodies are not yet supported by CIR lowering",
                ));
            }
            let lowered = if tail_values.is_empty() {
                lucid_cir::Function::from_parameterized_if_direct(
                    first_condition,
                    first_value,
                    else_value,
                    &function.parameter_names,
                )
            } else {
                lucid_cir::Function::from_parameterized_if_elif_chain_direct(
                    first_condition,
                    first_value,
                    tail_values,
                    else_value,
                    &function.parameter_names,
                )
            };
            return lowered
                .map(Arc::new)
                .map_err(|_| Arc::from("unsupported false-leading local elif chain"));
        }
    }
    if let [
        initial,
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch: Some(else_branch),
            elif_branches,
            ..
        },
        lucid_syntax::Stmt::Return {
            value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
            ..
        },
    ] = source_function.body.as_slice()
        && elif_branches.is_empty()
        && static_truth(condition).is_none()
        && has_identifier(condition)
    {
        fn assigned_value(
            statements: &[lucid_syntax::Stmt],
        ) -> Option<(&str, &lucid_syntax::Expr)> {
            let mut assignment = None;
            for statement in statements {
                if matches!(statement, lucid_syntax::Stmt::Pass(_)) {
                    continue;
                }
                let value = match statement {
                    lucid_syntax::Stmt::Assignment {
                        target: lucid_syntax::Expr::Ident { name, .. },
                        value,
                        ..
                    }
                    | lucid_syntax::Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(name, _),
                        value: Some(value),
                        ..
                    } => (name.as_str(), value),
                    _ => return None,
                };
                if assignment.replace(value).is_some() {
                    return None;
                }
            }
            assignment
        }
        fn pass_only(statements: &[lucid_syntax::Stmt]) -> bool {
            statements
                .iter()
                .all(|statement| matches!(statement, lucid_syntax::Stmt::Pass(_)))
        }
        fn mentions_name(expr: &lucid_syntax::Expr, target: &str) -> bool {
            match expr {
                lucid_syntax::Expr::Ident { name, .. } => name == target,
                lucid_syntax::Expr::Unary { expr, .. } => mentions_name(expr, target),
                lucid_syntax::Expr::Binary { left, right, .. } => {
                    mentions_name(left, target) || mentions_name(right, target)
                }
                lucid_syntax::Expr::IfExpr {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    mentions_name(condition, target)
                        || mentions_name(then_branch, target)
                        || mentions_name(else_branch, target)
                }
                _ => false,
            }
        }
        let initial_value = match initial {
            lucid_syntax::Stmt::Assignment {
                target: lucid_syntax::Expr::Ident { name, .. },
                value,
                ..
            }
            | lucid_syntax::Stmt::VarDef {
                pattern: lucid_syntax::Pattern::Ident(name, _),
                value: Some(value),
                ..
            } if name == returned && !mentions_name(value, returned) => Some(value),
            _ => None,
        };
        if let Some(initial_value) = initial_value {
            let then_value = match assigned_value(then_branch) {
                Some((name, value)) if name == returned && !mentions_name(value, returned) => {
                    Some(value)
                }
                None if pass_only(then_branch) => Some(initial_value),
                _ => None,
            };
            let else_value = match assigned_value(else_branch) {
                Some((name, value)) if name == returned && !mentions_name(value, returned) => {
                    Some(value)
                }
                None if pass_only(else_branch) => Some(initial_value),
                _ => None,
            };
            if let (Some(then_value), Some(else_value)) = (then_value, else_value) {
                if function.is_async {
                    return Err(Arc::from(
                        "async function bodies are not yet supported by CIR lowering",
                    ));
                }
                let lower = if has_division(then_value)
                    || has_division(else_value)
                    || has_division(initial_value)
                {
                    lucid_cir::Function::from_parameterized_if_direct(
                        condition,
                        then_value,
                        else_value,
                        &function.parameter_names,
                    )
                } else {
                    lucid_cir::Function::from_parameterized_if(
                        condition,
                        then_value,
                        else_value,
                        &function.parameter_names,
                    )
                };
                return lower
                    .map(Arc::new)
                    .map_err(|_| Arc::from("unsupported initialized local else conditional"));
            }
        }
    }
    if let [
        initial,
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch: Some(else_branch),
            elif_branches,
            ..
        },
        lucid_syntax::Stmt::Return {
            value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
            ..
        },
    ] = source_function.body.as_slice()
        && !elif_branches.is_empty()
        && static_truth(condition).is_none()
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| static_truth(elif_condition).is_none())
        && has_identifier(condition)
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| has_identifier(elif_condition))
    {
        fn assigned_value(
            statements: &[lucid_syntax::Stmt],
        ) -> Option<(&str, &lucid_syntax::Expr)> {
            let mut assignment = None;
            for statement in statements {
                if matches!(statement, lucid_syntax::Stmt::Pass(_)) {
                    continue;
                }
                let value = match statement {
                    lucid_syntax::Stmt::Assignment {
                        target: lucid_syntax::Expr::Ident { name, .. },
                        value,
                        ..
                    }
                    | lucid_syntax::Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(name, _),
                        value: Some(value),
                        ..
                    } => (name.as_str(), value),
                    _ => return None,
                };
                if assignment.replace(value).is_some() {
                    return None;
                }
            }
            assignment
        }
        fn pass_only(statements: &[lucid_syntax::Stmt]) -> bool {
            statements
                .iter()
                .all(|statement| matches!(statement, lucid_syntax::Stmt::Pass(_)))
        }
        fn mentions_name(expr: &lucid_syntax::Expr, target: &str) -> bool {
            match expr {
                lucid_syntax::Expr::Ident { name, .. } => name == target,
                lucid_syntax::Expr::Unary { expr, .. } => mentions_name(expr, target),
                lucid_syntax::Expr::Binary { left, right, .. } => {
                    mentions_name(left, target) || mentions_name(right, target)
                }
                lucid_syntax::Expr::IfExpr {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    mentions_name(condition, target)
                        || mentions_name(then_branch, target)
                        || mentions_name(else_branch, target)
                }
                _ => false,
            }
        }
        let initial_value = match initial {
            lucid_syntax::Stmt::Assignment {
                target: lucid_syntax::Expr::Ident { name, .. },
                value,
                ..
            }
            | lucid_syntax::Stmt::VarDef {
                pattern: lucid_syntax::Pattern::Ident(name, _),
                value: Some(value),
                ..
            } if name == returned && !mentions_name(value, returned) => Some(value),
            _ => None,
        };
        if let Some(initial_value) = initial_value {
            let then_value = match assigned_value(then_branch) {
                Some((name, value)) if name == returned && !mentions_name(value, returned) => {
                    Some(value)
                }
                None if pass_only(then_branch) => Some(initial_value),
                _ => None,
            };
            let else_value = match assigned_value(else_branch) {
                Some((name, value)) if name == returned && !mentions_name(value, returned) => {
                    Some(value)
                }
                None if pass_only(else_branch) => Some(initial_value),
                _ => None,
            };
            if let (Some(then_value), Some(else_value)) = (then_value, else_value)
                && let Some(elif_values) = elif_branches
                    .iter()
                    .map(|(condition, branch)| match assigned_value(branch) {
                        Some((name, value))
                            if name == returned && !mentions_name(value, returned) =>
                        {
                            Some((condition, value))
                        }
                        None if pass_only(branch) => Some((condition, initial_value)),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()
            {
                if function.is_async {
                    return Err(Arc::from(
                        "async function bodies are not yet supported by CIR lowering",
                    ));
                }
                return lucid_cir::Function::from_parameterized_if_elif_chain_direct(
                    condition,
                    then_value,
                    &elif_values,
                    else_value,
                    &function.parameter_names,
                )
                .map(Arc::new)
                .map_err(|_| Arc::from("unsupported initialized local else elif chain"));
            }
        }
    }
    if let [
        initial,
        lucid_syntax::Stmt::If {
            condition,
            elif_branches,
            else_branch: Some(else_branch),
            ..
        },
        lucid_syntax::Stmt::Return {
            value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
            ..
        },
    ] = source_function.body.as_slice()
        && static_truth(condition) == Some(false)
        && !elif_branches.is_empty()
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| static_truth(elif_condition).is_none())
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| has_identifier(elif_condition))
    {
        fn assigned_value(
            statements: &[lucid_syntax::Stmt],
        ) -> Option<(&str, &lucid_syntax::Expr)> {
            let mut assignment = None;
            for statement in statements {
                if matches!(statement, lucid_syntax::Stmt::Pass(_)) {
                    continue;
                }
                let value = match statement {
                    lucid_syntax::Stmt::Assignment {
                        target: lucid_syntax::Expr::Ident { name, .. },
                        value,
                        ..
                    }
                    | lucid_syntax::Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(name, _),
                        value: Some(value),
                        ..
                    } => (name.as_str(), value),
                    _ => return None,
                };
                if assignment.replace(value).is_some() {
                    return None;
                }
            }
            assignment
        }
        fn pass_only(statements: &[lucid_syntax::Stmt]) -> bool {
            statements
                .iter()
                .all(|statement| matches!(statement, lucid_syntax::Stmt::Pass(_)))
        }
        fn mentions_name(expr: &lucid_syntax::Expr, target: &str) -> bool {
            match expr {
                lucid_syntax::Expr::Ident { name, .. } => name == target,
                lucid_syntax::Expr::Unary { expr, .. } => mentions_name(expr, target),
                lucid_syntax::Expr::Binary { left, right, .. } => {
                    mentions_name(left, target) || mentions_name(right, target)
                }
                lucid_syntax::Expr::IfExpr {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    mentions_name(condition, target)
                        || mentions_name(then_branch, target)
                        || mentions_name(else_branch, target)
                }
                _ => false,
            }
        }
        let initial_value = match initial {
            lucid_syntax::Stmt::Assignment {
                target: lucid_syntax::Expr::Ident { name, .. },
                value,
                ..
            }
            | lucid_syntax::Stmt::VarDef {
                pattern: lucid_syntax::Pattern::Ident(name, _),
                value: Some(value),
                ..
            } if name == returned && !mentions_name(value, returned) => Some(value),
            _ => None,
        };
        if let Some(initial_value) = initial_value {
            let else_value = match assigned_value(else_branch) {
                Some((name, value)) if name == returned && !mentions_name(value, returned) => {
                    Some(value)
                }
                None if pass_only(else_branch) => Some(initial_value),
                _ => None,
            };
            if let Some(else_value) = else_value
                && let Some(elif_values) = elif_branches
                    .iter()
                    .map(|(condition, branch)| match assigned_value(branch) {
                        Some((name, value))
                            if name == returned && !mentions_name(value, returned) =>
                        {
                            Some((condition, value))
                        }
                        None if pass_only(branch) => Some((condition, initial_value)),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()
                && let Some(((first_condition, first_value), tail_values)) =
                    elif_values.split_first()
            {
                if function.is_async {
                    return Err(Arc::from(
                        "async function bodies are not yet supported by CIR lowering",
                    ));
                }
                let lowered = if tail_values.is_empty() {
                    lucid_cir::Function::from_parameterized_if_direct(
                        first_condition,
                        first_value,
                        else_value,
                        &function.parameter_names,
                    )
                } else {
                    lucid_cir::Function::from_parameterized_if_elif_chain_direct(
                        first_condition,
                        first_value,
                        tail_values,
                        else_value,
                        &function.parameter_names,
                    )
                };
                return lowered.map(Arc::new).map_err(|_| {
                    Arc::from("unsupported false-leading initialized local elif chain")
                });
            }
        }
    }
    if let [
        initial,
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch: None,
            elif_branches,
            ..
        },
        lucid_syntax::Stmt::Return {
            value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
            ..
        },
    ] = source_function.body.as_slice()
        && !elif_branches.is_empty()
        && static_truth(condition).is_none()
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| static_truth(elif_condition).is_none())
        && has_identifier(condition)
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| has_identifier(elif_condition))
    {
        fn assigned_value(
            statements: &[lucid_syntax::Stmt],
        ) -> Option<(&str, &lucid_syntax::Expr)> {
            let mut assignment = None;
            for statement in statements {
                if matches!(statement, lucid_syntax::Stmt::Pass(_)) {
                    continue;
                }
                let value = match statement {
                    lucid_syntax::Stmt::Assignment {
                        target: lucid_syntax::Expr::Ident { name, .. },
                        value,
                        ..
                    }
                    | lucid_syntax::Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(name, _),
                        value: Some(value),
                        ..
                    } => (name.as_str(), value),
                    _ => return None,
                };
                if assignment.replace(value).is_some() {
                    return None;
                }
            }
            assignment
        }
        fn pass_only(statements: &[lucid_syntax::Stmt]) -> bool {
            statements
                .iter()
                .all(|statement| matches!(statement, lucid_syntax::Stmt::Pass(_)))
        }
        fn mentions_name(expr: &lucid_syntax::Expr, target: &str) -> bool {
            match expr {
                lucid_syntax::Expr::Ident { name, .. } => name == target,
                lucid_syntax::Expr::Unary { expr, .. } => mentions_name(expr, target),
                lucid_syntax::Expr::Binary { left, right, .. } => {
                    mentions_name(left, target) || mentions_name(right, target)
                }
                lucid_syntax::Expr::IfExpr {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    mentions_name(condition, target)
                        || mentions_name(then_branch, target)
                        || mentions_name(else_branch, target)
                }
                _ => false,
            }
        }
        let initial_value = match initial {
            lucid_syntax::Stmt::Assignment {
                target: lucid_syntax::Expr::Ident { name, .. },
                value,
                ..
            }
            | lucid_syntax::Stmt::VarDef {
                pattern: lucid_syntax::Pattern::Ident(name, _),
                value: Some(value),
                ..
            } if name == returned && !mentions_name(value, returned) => Some(value),
            _ => None,
        };
        if let Some(initial_value) = initial_value
            && let Some(then_value) = match assigned_value(then_branch) {
                Some((name, value)) if name == returned && !mentions_name(value, returned) => {
                    Some(value)
                }
                None if pass_only(then_branch) => Some(initial_value),
                _ => None,
            }
            && let Some(elif_values) = elif_branches
                .iter()
                .map(|(condition, branch)| match assigned_value(branch) {
                    Some((name, value)) if name == returned && !mentions_name(value, returned) => {
                        Some((condition, value))
                    }
                    None if pass_only(branch) => Some((condition, initial_value)),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
        {
            if function.is_async {
                return Err(Arc::from(
                    "async function bodies are not yet supported by CIR lowering",
                ));
            }
            return lucid_cir::Function::from_parameterized_if_elif_chain_direct(
                condition,
                then_value,
                &elif_values,
                initial_value,
                &function.parameter_names,
            )
            .map(Arc::new)
            .map_err(|_| Arc::from("unsupported initialized local elif chain"));
        }
    }
    if let [
        initial,
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch: None,
            elif_branches,
            ..
        },
        lucid_syntax::Stmt::Return {
            value: Some(lucid_syntax::Expr::Ident { name: returned, .. }),
            ..
        },
    ] = source_function.body.as_slice()
        && elif_branches.is_empty()
        && static_truth(condition).is_none()
        && has_identifier(condition)
    {
        fn assigned_value(
            statements: &[lucid_syntax::Stmt],
        ) -> Option<(&str, &lucid_syntax::Expr)> {
            let mut assignment = None;
            for statement in statements {
                if matches!(statement, lucid_syntax::Stmt::Pass(_)) {
                    continue;
                }
                let value = match statement {
                    lucid_syntax::Stmt::Assignment {
                        target: lucid_syntax::Expr::Ident { name, .. },
                        value,
                        ..
                    }
                    | lucid_syntax::Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(name, _),
                        value: Some(value),
                        ..
                    } => (name.as_str(), value),
                    _ => return None,
                };
                if assignment.replace(value).is_some() {
                    return None;
                }
            }
            assignment
        }
        fn mentions_name(expr: &lucid_syntax::Expr, target: &str) -> bool {
            match expr {
                lucid_syntax::Expr::Ident { name, .. } => name == target,
                lucid_syntax::Expr::Unary { expr, .. } => mentions_name(expr, target),
                lucid_syntax::Expr::Binary { left, right, .. } => {
                    mentions_name(left, target) || mentions_name(right, target)
                }
                lucid_syntax::Expr::IfExpr {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    mentions_name(condition, target)
                        || mentions_name(then_branch, target)
                        || mentions_name(else_branch, target)
                }
                _ => false,
            }
        }
        let initial_value = match initial {
            lucid_syntax::Stmt::Assignment {
                target: lucid_syntax::Expr::Ident { name, .. },
                value,
                ..
            }
            | lucid_syntax::Stmt::VarDef {
                pattern: lucid_syntax::Pattern::Ident(name, _),
                value: Some(value),
                ..
            } if name == returned && !mentions_name(value, returned) => Some(value),
            _ => None,
        };
        if let (Some(initial_value), Some((then_name, then_value))) =
            (initial_value, assigned_value(then_branch))
            && then_name == returned
            && !mentions_name(then_value, returned)
        {
            if function.is_async {
                return Err(Arc::from(
                    "async function bodies are not yet supported by CIR lowering",
                ));
            }
            return lucid_cir::Function::from_parameterized_if(
                condition,
                then_value,
                initial_value,
                &function.parameter_names,
            )
            .map(Arc::new)
            .map_err(|_| Arc::from("unsupported initialized local conditional"));
        }
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch: Some(else_branch),
            elif_branches,
            ..
        },
    ] = source_function.body.as_slice()
        && elif_branches.is_empty()
        && static_truth(condition).is_none()
        && has_identifier(condition)
        && branch_is_single_void(else_branch)
        && let [
            lucid_syntax::Stmt::Return {
                value: Some(then_value),
                ..
            },
        ] = then_branch.as_slice()
    {
        let nodes = function
            .body_expressions
            .iter()
            .map(|node| lucid_cir::TypedExprNode {
                id: node.id,
                kind: node.kind.clone(),
                detail: node.detail.clone(),
                children: node.children.to_vec(),
                literal: node.literal,
            })
            .collect::<Vec<_>>();
        let find_id = |span: lucid_syntax::Span| {
            function
                .body_expressions
                .iter()
                .find(|node| node.span == span)
                .map(|node| node.id)
        };
        if let (Some(condition_id), Some(then_id)) =
            (find_id(condition.span()), find_id(then_value.span()))
            && let Ok(lowered) = lucid_cir::Function::from_typed_statement_if_optional(
                &nodes,
                condition_id,
                then_id,
                &function.parameter_names,
                &[],
            )
        {
            return Ok(Arc::new(lowered));
        }
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        return lucid_cir::Function::from_parameterized_if_optional(
            condition,
            then_value,
            &function.parameter_names,
        )
        .map(Arc::new)
        .map_err(|_| Arc::from("unsupported dynamic pass branch"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch: Some(else_branch),
            elif_branches,
            ..
        },
    ] = source_function.body.as_slice()
        && elif_branches.is_empty()
        && static_truth(condition).is_none()
        && has_identifier(condition)
        && let (
            [
                lucid_syntax::Stmt::Return {
                    value: Some(then_value),
                    ..
                },
            ],
            [
                lucid_syntax::Stmt::Return {
                    value: Some(else_value),
                    ..
                },
            ],
        ) = (then_branch.as_slice(), else_branch.as_slice())
    {
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        let lower = if has_division(then_value) || has_division(else_value) {
            lucid_cir::Function::from_parameterized_if_direct(
                condition,
                then_value,
                else_value,
                &function.parameter_names,
            )
        } else {
            lucid_cir::Function::from_parameterized_if(
                condition,
                then_value,
                else_value,
                &function.parameter_names,
            )
        };
        return lower
            .map(Arc::new)
            .map_err(|_| Arc::from("unsupported dynamic function branch"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch: None,
            elif_branches,
            ..
        },
    ] = source_function.body.as_slice()
        && elif_branches.is_empty()
        && static_truth(condition).is_none()
        && has_identifier(condition)
        && let [
            lucid_syntax::Stmt::Return {
                value: Some(then_value),
                ..
            },
        ] = then_branch.as_slice()
    {
        let nodes = function
            .body_expressions
            .iter()
            .map(|node| lucid_cir::TypedExprNode {
                id: node.id,
                kind: node.kind.clone(),
                detail: node.detail.clone(),
                children: node.children.to_vec(),
                literal: node.literal,
            })
            .collect::<Vec<_>>();
        let find_id = |span: lucid_syntax::Span| {
            function
                .body_expressions
                .iter()
                .find(|node| node.span == span)
                .map(|node| node.id)
        };
        if let (Some(condition_id), Some(then_id)) =
            (find_id(condition.span()), find_id(then_value.span()))
            && let Ok(lowered) = lucid_cir::Function::from_typed_statement_if_optional(
                &nodes,
                condition_id,
                then_id,
                &function.parameter_names,
                &[],
            )
        {
            return Ok(Arc::new(lowered));
        }
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        return lucid_cir::Function::from_parameterized_if_optional(
            condition,
            then_value,
            &function.parameter_names,
        )
        .map(Arc::new)
        .map_err(|_| Arc::from("unsupported optional dynamic branch"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch,
            elif_branches,
            ..
        },
    ] = source_function.body.as_slice()
        && elif_branches.is_empty()
        && static_truth(condition).is_none()
        && has_identifier(condition)
        && branch_is_single_void(then_branch)
        && else_branch
            .as_ref()
            .is_none_or(|branch| branch_is_single_void(branch))
    {
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        return lucid_cir::Function::from_parameterized_if_void(
            condition,
            &function.parameter_names,
        )
        .map(Arc::new)
        .map_err(|_| Arc::from("unsupported optional dynamic void branch"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch,
            ..
        },
    ] = source_function.body.as_slice()
        && !elif_branches.is_empty()
        && static_truth(condition).is_none()
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| static_truth(elif_condition).is_none())
        && has_identifier(condition)
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| has_identifier(elif_condition))
        && branch_is_single_void(then_branch)
        && else_branch
            .as_ref()
            .is_none_or(|branch| branch_is_single_void(branch))
    {
        let Some(elif_conditions) = elif_branches
            .iter()
            .map(|(condition, branch)| branch_is_single_void(branch).then_some(condition))
            .collect::<Option<Vec<_>>>()
        else {
            return Err(Arc::from("unsupported optional dynamic void elif chain"));
        };
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        return lucid_cir::Function::from_parameterized_if_elif_void_chain(
            condition,
            &elif_conditions,
            &function.parameter_names,
        )
        .map(Arc::new)
        .map_err(|_| Arc::from("unsupported optional dynamic void elif chain"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch: Some(else_branch),
            elif_branches,
            ..
        },
    ] = source_function.body.as_slice()
        && elif_branches.is_empty()
        && static_truth(condition).is_none()
        && has_identifier(condition)
        && branch_is_single_void(then_branch)
        && branch_is_single_void(else_branch)
    {
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        return lucid_cir::Function::from_parameterized_if_void(
            condition,
            &function.parameter_names,
        )
        .map(Arc::new)
        .map_err(|_| Arc::from("unsupported dynamic void branch"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch: Some(else_branch),
            ..
        },
    ] = source_function.body.as_slice()
        && !elif_branches.is_empty()
        && static_truth(condition).is_none()
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| static_truth(elif_condition).is_none())
        && has_identifier(condition)
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| has_identifier(elif_condition))
        && branch_is_single_void(then_branch)
        && branch_is_single_void(else_branch)
    {
        let Some(elif_conditions) = elif_branches
            .iter()
            .map(|(condition, branch)| branch_is_single_void(branch).then_some(condition))
            .collect::<Option<Vec<_>>>()
        else {
            return Err(Arc::from("unsupported dynamic void elif chain"));
        };
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        return lucid_cir::Function::from_parameterized_if_elif_void_chain(
            condition,
            &elif_conditions,
            &function.parameter_names,
        )
        .map(Arc::new)
        .map_err(|_| Arc::from("unsupported dynamic void elif chain"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch,
            ..
        },
    ] = source_function.body.as_slice()
        && !elif_branches.is_empty()
        && static_truth(condition).is_none()
        && has_identifier(condition)
        && branch_is_single_void(then_branch)
        && else_branch
            .as_ref()
            .is_none_or(|branch| branch_is_single_void(branch))
    {
        let mut dynamic_conditions = Vec::new();
        let mut saw_static_false = false;
        let mut unsupported = false;
        for (elif_condition, branch) in elif_branches {
            if !branch_is_single_void(branch) {
                unsupported = true;
                break;
            }
            match static_truth(elif_condition) {
                Some(false) => saw_static_false = true,
                Some(true) => break,
                None => {
                    if !has_identifier(elif_condition) {
                        unsupported = true;
                        break;
                    }
                    dynamic_conditions.push(elif_condition);
                }
            }
        }
        if saw_static_false && !unsupported {
            if function.is_async {
                return Err(Arc::from(
                    "async function bodies are not yet supported by CIR lowering",
                ));
            }
            let lowered = if dynamic_conditions.is_empty() {
                lucid_cir::Function::from_parameterized_if_void(
                    condition,
                    &function.parameter_names,
                )
            } else {
                lucid_cir::Function::from_parameterized_if_elif_void_chain(
                    condition,
                    &dynamic_conditions,
                    &function.parameter_names,
                )
            };
            return lowered
                .map(Arc::new)
                .map_err(|_| Arc::from("unsupported mixed dynamic void elif chain"));
        }
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch,
            ..
        },
    ] = source_function.body.as_slice()
        && !elif_branches.is_empty()
        && static_truth(condition).is_none()
        && has_identifier(condition)
        && let [
            lucid_syntax::Stmt::Return {
                value: Some(then_value),
                ..
            },
        ] = then_branch.as_slice()
    {
        let mut dynamic_elifs = Vec::new();
        let mut unsupported = false;
        for (elif_condition, branch) in elif_branches {
            match static_truth(elif_condition) {
                Some(false) => continue,
                Some(true) | None => {
                    let [
                        lucid_syntax::Stmt::Return {
                            value: Some(value), ..
                        },
                    ] = branch.as_slice()
                    else {
                        unsupported = true;
                        break;
                    };
                    if static_truth(elif_condition).is_none() {
                        if !has_identifier(elif_condition) {
                            unsupported = true;
                            break;
                        }
                        dynamic_elifs.push((elif_condition, value));
                    } else {
                        if function.is_async {
                            return Err(Arc::from(
                                "async function bodies are not yet supported by CIR lowering",
                            ));
                        }
                        let lowered = if dynamic_elifs.is_empty() {
                            lucid_cir::Function::from_parameterized_if_direct(
                                condition,
                                then_value,
                                value,
                                &function.parameter_names,
                            )
                        } else {
                            lucid_cir::Function::from_parameterized_if_elif_chain_direct(
                                condition,
                                then_value,
                                &dynamic_elifs,
                                value,
                                &function.parameter_names,
                            )
                        };
                        return lowered
                            .map(Arc::new)
                            .map_err(|_| Arc::from("unsupported mixed static elif branch"));
                    }
                }
            }
        }
        if !unsupported && dynamic_elifs.len() < elif_branches.len() {
            if function.is_async {
                return Err(Arc::from(
                    "async function bodies are not yet supported by CIR lowering",
                ));
            }
            let lowered = match else_branch.as_deref() {
                Some(
                    [
                        lucid_syntax::Stmt::Return {
                            value: Some(else_value),
                            ..
                        },
                    ],
                ) => {
                    if dynamic_elifs.is_empty() {
                        lucid_cir::Function::from_parameterized_if(
                            condition,
                            then_value,
                            else_value,
                            &function.parameter_names,
                        )
                    } else {
                        lucid_cir::Function::from_parameterized_if_elif_chain_direct(
                            condition,
                            then_value,
                            &dynamic_elifs,
                            else_value,
                            &function.parameter_names,
                        )
                    }
                }
                None => {
                    if dynamic_elifs.is_empty() {
                        lucid_cir::Function::from_parameterized_if_optional(
                            condition,
                            then_value,
                            &function.parameter_names,
                        )
                    } else {
                        lucid_cir::Function::from_parameterized_if_elif_optional_chain(
                            condition,
                            then_value,
                            &dynamic_elifs,
                            &function.parameter_names,
                        )
                    }
                }
                _ => Err(lucid_cir::LowerError::UnsupportedExpression),
            };
            return lowered
                .map(Arc::new)
                .map_err(|_| Arc::from("unsupported mixed dynamic elif chain"));
        }
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch: None,
            ..
        },
    ] = source_function.body.as_slice()
        && !elif_branches.is_empty()
        && static_truth(condition).is_none()
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| static_truth(elif_condition).is_none())
        && has_identifier(condition)
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| has_identifier(elif_condition))
        && let [
            lucid_syntax::Stmt::Return {
                value: Some(then_value),
                ..
            },
        ] = then_branch.as_slice()
    {
        let Some(elif_values) = elif_branches
            .iter()
            .map(|(condition, branch)| match branch.as_slice() {
                [
                    lucid_syntax::Stmt::Return {
                        value: Some(value), ..
                    },
                ] => Some((condition, value)),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
        else {
            return Err(Arc::from("unsupported dynamic optional elif chain"));
        };
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        return lucid_cir::Function::from_parameterized_if_elif_optional_chain(
            condition,
            then_value,
            &elif_values,
            &function.parameter_names,
        )
        .map(Arc::new)
        .map_err(|_| Arc::from("unsupported dynamic optional elif chain"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch: Some(else_branch),
            ..
        },
    ] = source_function.body.as_slice()
        && !elif_branches.is_empty()
        && static_truth(condition).is_none()
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| static_truth(elif_condition).is_none())
        && has_identifier(condition)
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| has_identifier(elif_condition))
        && branch_is_single_void(else_branch)
        && let [
            lucid_syntax::Stmt::Return {
                value: Some(then_value),
                ..
            },
        ] = then_branch.as_slice()
    {
        let Some(elif_values) = elif_branches
            .iter()
            .map(|(condition, branch)| match branch.as_slice() {
                [
                    lucid_syntax::Stmt::Return {
                        value: Some(value), ..
                    },
                ] => Some((condition, value)),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
        else {
            return Err(Arc::from("unsupported dynamic pass elif chain"));
        };
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        return lucid_cir::Function::from_parameterized_if_elif_optional_chain(
            condition,
            then_value,
            &elif_values,
            &function.parameter_names,
        )
        .map(Arc::new)
        .map_err(|_| Arc::from("unsupported dynamic pass elif chain"));
    }
    if let [
        lucid_syntax::Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch: Some(else_branch),
            ..
        },
    ] = source_function.body.as_slice()
        && !elif_branches.is_empty()
        && static_truth(condition).is_none()
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| static_truth(elif_condition).is_none())
        && has_identifier(condition)
        && elif_branches
            .iter()
            .all(|(elif_condition, _)| has_identifier(elif_condition))
        && let [
            lucid_syntax::Stmt::Return {
                value: Some(then_value),
                ..
            },
        ] = then_branch.as_slice()
        && let [
            lucid_syntax::Stmt::Return {
                value: Some(else_value),
                ..
            },
        ] = else_branch.as_slice()
    {
        let Some(elif_values) = elif_branches
            .iter()
            .map(|(condition, branch)| match branch.as_slice() {
                [
                    lucid_syntax::Stmt::Return {
                        value: Some(value), ..
                    },
                ] => Some((condition, value)),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
        else {
            return Err(Arc::from("unsupported dynamic elif chain"));
        };
        if function.is_async {
            return Err(Arc::from(
                "async function bodies are not yet supported by CIR lowering",
            ));
        }
        return lucid_cir::Function::from_parameterized_if_elif_chain_direct(
            condition,
            then_value,
            &elif_values,
            else_value,
            &function.parameter_names,
        )
        .map(Arc::new)
        .map_err(|_| Arc::from("unsupported dynamic elif chain"));
    }
    if matches!(
        source_function.body.as_slice(),
        [lucid_syntax::Stmt::Return { value: None, .. } | lucid_syntax::Stmt::Pass(_)]
    ) {
        let function = lucid_cir::Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                // Materialize unused parameters so the CIR still carries the
                // source-level calling convention into native codegen.
                instructions: function
                    .parameter_names
                    .iter()
                    .enumerate()
                    .map(|(index, _)| lucid_cir::Instruction::Param {
                        result: lucid_cir::ValueId(index as u32),
                        index: index as u32,
                    })
                    .collect(),
                terminator: lucid_cir::Terminator::Return(None),
            }],
        };
        function
            .verify()
            .map_err(|_| Arc::<str>::from("invalid void function CIR"))?;
        return Ok(Arc::new(function));
    }
    fn static_int(expr: &lucid_syntax::Expr) -> Option<i64> {
        match expr {
            lucid_syntax::Expr::Literal {
                value: lucid_syntax::LiteralValue::Int(value),
                ..
            } => Some(*value),
            lucid_syntax::Expr::Unary { op, expr, .. } => match op {
                lucid_syntax::UnaryOp::Neg => static_int(expr)?.checked_neg(),
                lucid_syntax::UnaryOp::Pos => static_int(expr),
                lucid_syntax::UnaryOp::Invert => Some(!static_int(expr)?),
                _ => None,
            },
            lucid_syntax::Expr::Binary {
                op, left, right, ..
            } => {
                let (left, right) = (static_int(left)?, static_int(right)?);
                match op {
                    lucid_syntax::BinaryOp::Add => left.checked_add(right),
                    lucid_syntax::BinaryOp::Sub => left.checked_sub(right),
                    lucid_syntax::BinaryOp::Mul => left.checked_mul(right),
                    lucid_syntax::BinaryOp::Pow => u32::try_from(right)
                        .ok()
                        .and_then(|exponent| left.checked_pow(exponent)),
                    lucid_syntax::BinaryOp::Div => left.checked_div(right),
                    lucid_syntax::BinaryOp::FloorDiv => {
                        let quotient = left.checked_div(right)?;
                        let remainder = left.checked_rem(right)?;
                        if remainder != 0 && (left < 0) != (right < 0) {
                            quotient.checked_sub(1)
                        } else {
                            Some(quotient)
                        }
                    }
                    lucid_syntax::BinaryOp::Mod => {
                        let remainder = left.checked_rem(right)?;
                        if remainder != 0 && (left < 0) != (right < 0) {
                            remainder.checked_add(right)
                        } else {
                            Some(remainder)
                        }
                    }
                    lucid_syntax::BinaryOp::BitAnd => Some(left & right),
                    lucid_syntax::BinaryOp::BitOr => Some(left | right),
                    lucid_syntax::BinaryOp::BitXor => Some(left ^ right),
                    lucid_syntax::BinaryOp::Shl => left.checked_shl(u32::try_from(right).ok()?),
                    lucid_syntax::BinaryOp::Shr => left.checked_shr(u32::try_from(right).ok()?),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn static_big_int(expr: &lucid_syntax::Expr) -> Option<num_bigint::BigInt> {
        use num_bigint::BigInt;
        match expr {
            lucid_syntax::Expr::Literal {
                value: lucid_syntax::LiteralValue::BigInt(value),
                ..
            } => {
                let value = value.trim();
                let negative = value.starts_with('-');
                let digits = value.trim_start_matches(['+', '-']).replace('_', "");
                let (digits, radix) = if let Some(digits) = digits
                    .strip_prefix("0x")
                    .or_else(|| digits.strip_prefix("0X"))
                {
                    (digits, 16)
                } else if let Some(digits) = digits
                    .strip_prefix("0o")
                    .or_else(|| digits.strip_prefix("0O"))
                {
                    (digits, 8)
                } else if let Some(digits) = digits
                    .strip_prefix("0b")
                    .or_else(|| digits.strip_prefix("0B"))
                {
                    (digits, 2)
                } else {
                    (digits.as_str(), 10)
                };
                let value = BigInt::parse_bytes(digits.as_bytes(), radix)?;
                Some(if negative { -value } else { value })
            }
            lucid_syntax::Expr::Literal {
                value: lucid_syntax::LiteralValue::Int(value),
                ..
            } => Some(BigInt::from(*value)),
            lucid_syntax::Expr::Unary { op, expr, .. } => {
                let value = static_big_int(expr)?;
                match op {
                    lucid_syntax::UnaryOp::Neg => Some(-value),
                    lucid_syntax::UnaryOp::Pos => Some(value),
                    lucid_syntax::UnaryOp::Invert => Some(!value),
                    _ => None,
                }
            }
            lucid_syntax::Expr::Binary {
                op, left, right, ..
            } => {
                let (left, right) = (static_big_int(left)?, static_big_int(right)?);
                match op {
                    lucid_syntax::BinaryOp::Add => Some(left + right),
                    lucid_syntax::BinaryOp::Sub => Some(left - right),
                    lucid_syntax::BinaryOp::Mul => Some(left * right),
                    lucid_syntax::BinaryOp::BitAnd => Some(left & right),
                    lucid_syntax::BinaryOp::BitOr => Some(left | right),
                    lucid_syntax::BinaryOp::BitXor => Some(left ^ right),
                    lucid_syntax::BinaryOp::Shl => {
                        u32::try_from(&right).ok().map(|shift| left << shift)
                    }
                    lucid_syntax::BinaryOp::Shr => {
                        u32::try_from(&right).ok().map(|shift| left >> shift)
                    }
                    lucid_syntax::BinaryOp::Pow => u32::try_from(&right)
                        .ok()
                        .map(|exponent| left.pow(exponent)),
                    lucid_syntax::BinaryOp::Div => {
                        if right == 0.into() {
                            None
                        } else {
                            Some(left / right)
                        }
                    }
                    lucid_syntax::BinaryOp::FloorDiv => {
                        if right == 0.into() {
                            None
                        } else {
                            let quotient = &left / &right;
                            let remainder = &left % &right;
                            if remainder != 0.into() && (left < 0.into()) != (right < 0.into()) {
                                Some(quotient - 1)
                            } else {
                                Some(quotient)
                            }
                        }
                    }
                    lucid_syntax::BinaryOp::Mod => {
                        if right == 0.into() {
                            None
                        } else {
                            let remainder = &left % &right;
                            if remainder != 0.into() && (left < 0.into()) != (right < 0.into()) {
                                Some(remainder + right)
                            } else {
                                Some(remainder)
                            }
                        }
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn static_truth(expr: &lucid_syntax::Expr) -> Option<bool> {
        match expr {
            lucid_syntax::Expr::Literal {
                value: lucid_syntax::LiteralValue::Bool(value),
                ..
            } => Some(*value),
            lucid_syntax::Expr::Literal {
                value: lucid_syntax::LiteralValue::Int(value),
                ..
            } => Some(*value != 0),
            lucid_syntax::Expr::Unary {
                op: lucid_syntax::UnaryOp::Not,
                expr,
                ..
            } => static_truth(expr).map(|value| !value),
            lucid_syntax::Expr::Binary {
                op: lucid_syntax::BinaryOp::And,
                left,
                right,
                ..
            } => match static_truth(left) {
                Some(false) => Some(false),
                Some(true) => static_truth(right),
                None => None,
            },
            lucid_syntax::Expr::Binary {
                op: lucid_syntax::BinaryOp::Or,
                left,
                right,
                ..
            } => match static_truth(left) {
                Some(true) => Some(true),
                Some(false) => static_truth(right),
                None => None,
            },
            lucid_syntax::Expr::Binary {
                op, left, right, ..
            } => {
                let equality = |op: &lucid_syntax::BinaryOp, equal: bool| match op {
                    lucid_syntax::BinaryOp::Eq
                    | lucid_syntax::BinaryOp::Identity
                    | lucid_syntax::BinaryOp::Is => Some(equal),
                    lucid_syntax::BinaryOp::NotEq
                    | lucid_syntax::BinaryOp::NotIdentity
                    | lucid_syntax::BinaryOp::IsNot => Some(!equal),
                    _ => None,
                };
                if let (
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::Bool(left),
                        ..
                    },
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::Bool(right),
                        ..
                    },
                ) = (left.as_ref(), right.as_ref())
                {
                    return equality(op, left == right);
                }
                if let (Some(left), Some(right)) = (static_big_int(left), static_big_int(right)) {
                    return Some(match op {
                        lucid_syntax::BinaryOp::Eq
                        | lucid_syntax::BinaryOp::Identity
                        | lucid_syntax::BinaryOp::Is => left == right,
                        lucid_syntax::BinaryOp::NotEq
                        | lucid_syntax::BinaryOp::NotIdentity
                        | lucid_syntax::BinaryOp::IsNot => left != right,
                        lucid_syntax::BinaryOp::Lt => left < right,
                        lucid_syntax::BinaryOp::LtEq => left <= right,
                        lucid_syntax::BinaryOp::Gt => left > right,
                        lucid_syntax::BinaryOp::GtEq => left >= right,
                        _ => return None,
                    });
                }
                if let (
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::Str(left),
                        ..
                    },
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::Str(right),
                        ..
                    },
                ) = (left.as_ref(), right.as_ref())
                {
                    return equality(op, left == right);
                }
                if let (
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::Float(left),
                        ..
                    },
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::Float(right),
                        ..
                    },
                ) = (left.as_ref(), right.as_ref())
                {
                    return Some(match op {
                        lucid_syntax::BinaryOp::Eq
                        | lucid_syntax::BinaryOp::Identity
                        | lucid_syntax::BinaryOp::Is => left == right,
                        lucid_syntax::BinaryOp::NotEq
                        | lucid_syntax::BinaryOp::NotIdentity
                        | lucid_syntax::BinaryOp::IsNot => left != right,
                        lucid_syntax::BinaryOp::Lt => left < right,
                        lucid_syntax::BinaryOp::LtEq => left <= right,
                        lucid_syntax::BinaryOp::Gt => left > right,
                        lucid_syntax::BinaryOp::GtEq => left >= right,
                        _ => return None,
                    });
                }
                if matches!(
                    (left.as_ref(), right.as_ref()),
                    (
                        lucid_syntax::Expr::Literal {
                            value: lucid_syntax::LiteralValue::None,
                            ..
                        },
                        lucid_syntax::Expr::Literal {
                            value: lucid_syntax::LiteralValue::None,
                            ..
                        }
                    )
                ) {
                    return equality(op, true);
                }
                let (left, right) = (static_int(left)?, static_int(right)?);
                Some(match op {
                    lucid_syntax::BinaryOp::Eq
                    | lucid_syntax::BinaryOp::Identity
                    | lucid_syntax::BinaryOp::Is => left == right,
                    lucid_syntax::BinaryOp::NotEq
                    | lucid_syntax::BinaryOp::NotIdentity
                    | lucid_syntax::BinaryOp::IsNot => left != right,
                    lucid_syntax::BinaryOp::Lt => left < right,
                    lucid_syntax::BinaryOp::LtEq => left <= right,
                    lucid_syntax::BinaryOp::Gt => left > right,
                    lucid_syntax::BinaryOp::GtEq => left >= right,
                    _ => return None,
                })
            }
            _ => None,
        }
    }
    fn selected_return_span(
        condition: &lucid_syntax::Expr,
        then_branch: &[lucid_syntax::Stmt],
        elif_branches: &[(lucid_syntax::Expr, Vec<lucid_syntax::Stmt>)],
        else_branch: Option<&Vec<lucid_syntax::Stmt>>,
    ) -> Option<lucid_syntax::Span> {
        let mut selected = if static_truth(condition) == Some(true) {
            Some(then_branch)
        } else {
            None
        };
        if selected.is_none() && static_truth(condition) == Some(false) {
            for (condition, body) in elif_branches {
                match static_truth(condition) {
                    Some(true) => {
                        selected = Some(body.as_slice());
                        break;
                    }
                    Some(false) => continue,
                    None => return None,
                }
            }
            if selected.is_none() {
                selected = else_branch.map(Vec::as_slice);
            }
        }
        match selected {
            Some(
                [
                    lucid_syntax::Stmt::Return {
                        value: Some(value), ..
                    },
                ],
            ) => Some(value.span()),
            _ => None,
        }
    }
    enum StaticBranch<'a> {
        Selected(&'a [lucid_syntax::Stmt]),
        Empty,
        Unknown,
    }
    fn static_branch_selection<'a>(
        condition: &lucid_syntax::Expr,
        then_branch: &'a [lucid_syntax::Stmt],
        elif_branches: &'a [(lucid_syntax::Expr, Vec<lucid_syntax::Stmt>)],
        else_branch: Option<&'a Vec<lucid_syntax::Stmt>>,
    ) -> StaticBranch<'a> {
        match static_truth(condition) {
            Some(true) => StaticBranch::Selected(then_branch),
            Some(false) => {
                for (condition, branch) in elif_branches {
                    match static_truth(condition) {
                        Some(true) => return StaticBranch::Selected(branch),
                        Some(false) => continue,
                        None => return StaticBranch::Unknown,
                    }
                }
                else_branch.map_or(StaticBranch::Empty, |branch| {
                    StaticBranch::Selected(branch.as_slice())
                })
            }
            None => StaticBranch::Unknown,
        }
    }
    fn is_pure_expression(expr: &lucid_syntax::Expr) -> bool {
        match expr {
            lucid_syntax::Expr::Literal { .. } | lucid_syntax::Expr::Ident { .. } => true,
            lucid_syntax::Expr::Unary { expr, .. } => is_pure_expression(expr),
            lucid_syntax::Expr::Binary { left, right, .. } => {
                is_pure_expression(left) && is_pure_expression(right)
            }
            lucid_syntax::Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                is_pure_expression(condition)
                    && is_pure_expression(then_branch)
                    && is_pure_expression(else_branch)
            }
            _ => false,
        }
    }
    fn collect_pre_return_binding(
        statement: &lucid_syntax::Stmt,
        bindings: &mut Vec<(String, lucid_syntax::Span)>,
    ) -> Result<(), Arc<str>> {
        if let lucid_syntax::Stmt::Expr(expr) = statement
            && is_pure_expression(expr)
        {
            return Ok(());
        }
        if matches!(statement, lucid_syntax::Stmt::Expr(_)) {
            return Err(Arc::from(
                "effectful discarded expression is not supported by typed CIR lowering",
            ));
        }
        if matches!(statement, lucid_syntax::Stmt::Pass(_)) {
            return Ok(());
        }
        if let lucid_syntax::Stmt::Assert { condition, .. } = statement
            && static_truth(condition) == Some(true)
        {
            return Ok(());
        }
        if let lucid_syntax::Stmt::While {
            condition,
            if_broken,
            ..
        } = statement
            && static_truth(condition) == Some(false)
            && if_broken.is_none()
        {
            return Ok(());
        }
        if let lucid_syntax::Stmt::For {
            iterable,
            if_broken,
            ..
        } = statement
            && if_broken.is_none()
            && lucid_cir::is_const_empty_iterable(iterable)
        {
            return Ok(());
        }
        if let lucid_syntax::Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch,
            ..
        } = statement
        {
            match static_branch_selection(
                condition,
                then_branch,
                elif_branches,
                else_branch.as_ref(),
            ) {
                StaticBranch::Selected(branch) => {
                    for statement in branch {
                        collect_pre_return_binding(statement, bindings)?;
                    }
                    return Ok(());
                }
                StaticBranch::Empty => return Ok(()),
                StaticBranch::Unknown => {}
            }
        }
        let (name, value) = match statement {
            lucid_syntax::Stmt::Assignment {
                target: lucid_syntax::Expr::Ident { name, .. },
                value,
                ..
            }
            | lucid_syntax::Stmt::VarDef {
                pattern: lucid_syntax::Pattern::Ident(name, _),
                value: Some(value),
                ..
            } => (name, value),
            _ => {
                return Err(Arc::from(
                    "multi-statement function bodies are not yet supported by CIR lowering",
                ));
            }
        };
        bindings.push((name.clone(), value.span()));
        Ok(())
    }
    enum StaticReturn {
        Value(lucid_syntax::Span),
        Void,
    }
    fn collect_static_branch_return(
        branch: &[lucid_syntax::Stmt],
        bindings: &mut Vec<(String, lucid_syntax::Span)>,
    ) -> Result<StaticReturn, Arc<str>> {
        let Some((last, prefix)) = branch.split_last() else {
            return Ok(StaticReturn::Void);
        };
        for statement in prefix {
            collect_pre_return_binding(statement, bindings)?;
        }
        match last {
            lucid_syntax::Stmt::Return {
                value: Some(value), ..
            } => Ok(StaticReturn::Value(value.span())),
            lucid_syntax::Stmt::Return { value: None, .. } | lucid_syntax::Stmt::Pass(_) => {
                Ok(StaticReturn::Void)
            }
            lucid_syntax::Stmt::If {
                condition,
                then_branch,
                elif_branches,
                else_branch,
                ..
            } => match static_branch_selection(
                condition,
                then_branch,
                elif_branches,
                else_branch.as_ref(),
            ) {
                StaticBranch::Selected(branch) => collect_static_branch_return(branch, bindings),
                StaticBranch::Empty => Ok(StaticReturn::Void),
                StaticBranch::Unknown => Err(Arc::from(
                    "constant function branch has no lowerable return",
                )),
            },
            _ => {
                collect_pre_return_binding(last, bindings)?;
                Ok(StaticReturn::Void)
            }
        }
    }
    // A single expression return is the common case.  A constant statement
    // branch with one return per selected arm can also be folded here.  A
    // sequence of simple
    // local definitions/assignments followed by `return name` is also SSA-
    // representable: each name resolves to its latest defining expression.
    let void_function = || {
        let function = lucid_cir::Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: function
                    .parameter_names
                    .iter()
                    .enumerate()
                    .map(|(index, _)| lucid_cir::Instruction::Param {
                        result: lucid_cir::ValueId(index as u32),
                        index: index as u32,
                    })
                    .collect(),
                terminator: lucid_cir::Terminator::Return(None),
            }],
        };
        function
            .verify()
            .map_err(|_| Arc::<str>::from("invalid void function CIR"))?;
        Ok(Arc::new(function))
    };
    let lower_bindings_to_void = |local_specs: &[(String, lucid_syntax::Span)]| {
        if local_specs.is_empty() {
            return void_function();
        }
        let nodes = function
            .body_expressions
            .iter()
            .map(|node| lucid_cir::TypedExprNode {
                id: node.id,
                kind: node.kind.clone(),
                detail: node.detail.clone(),
                children: node.children.to_vec(),
                literal: node.literal,
            })
            .collect::<Vec<_>>();
        let local_bindings = local_specs
            .iter()
            .map(|(name, span)| {
                function
                    .body_expressions
                    .iter()
                    .rev()
                    .find(|node| node.span == *span)
                    .map(|node| (name.clone(), node.id))
                    .ok_or_else(|| Arc::<str>::from("local binding has no typed expression"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let root_id = local_bindings
            .last()
            .map(|(_, id)| *id)
            .ok_or_else(|| Arc::<str>::from("function has no lowerable expression"))?;
        let mut lowered = lucid_cir::Function::from_typed_function_body_with_locals(
            &nodes,
            root_id,
            &function.parameter_names,
            &local_bindings,
        )
        .map_err(|_| Arc::<str>::from("unsupported expression before bare return"))?;
        if let Some(block) = lowered.blocks.last_mut() {
            block.terminator = lucid_cir::Terminator::Return(None);
        }
        lowered
            .verify()
            .map_err(|_| Arc::<str>::from("invalid void function CIR"))?;
        Ok(Arc::new(lowered))
    };
    let (root_span, local_specs): (lucid_syntax::Span, Vec<(String, lucid_syntax::Span)>) =
        match source_function.body.as_slice() {
            [
                lucid_syntax::Stmt::Return {
                    value: Some(value), ..
                },
            ] => (value.span(), Vec::new()),
            [
                lucid_syntax::Stmt::If {
                    condition,
                    then_branch,
                    elif_branches,
                    else_branch,
                    ..
                },
            ] => {
                if let Some(span) = selected_return_span(
                    condition,
                    then_branch,
                    elif_branches,
                    else_branch.as_ref(),
                ) {
                    (span, Vec::new())
                } else {
                    match static_branch_selection(
                        condition,
                        then_branch,
                        elif_branches,
                        else_branch.as_ref(),
                    ) {
                        StaticBranch::Selected([lucid_syntax::Stmt::Pass(_)])
                        | StaticBranch::Empty => return void_function(),
                        StaticBranch::Selected(branch) => {
                            let mut bindings = Vec::new();
                            match collect_static_branch_return(branch, &mut bindings)? {
                                StaticReturn::Value(span) => (span, bindings),
                                StaticReturn::Void => return lower_bindings_to_void(&bindings),
                            }
                        }
                        StaticBranch::Unknown => {
                            return Err(Arc::from(
                                "constant function branch has no lowerable return",
                            ));
                        }
                    }
                }
            }
            statements if statements.len() >= 2 => {
                let Some(last) = statements.last() else {
                    return Err(Arc::from(
                        "multi-statement function bodies are not yet supported by CIR lowering",
                    ));
                };
                if !matches!(
                    last,
                    lucid_syntax::Stmt::Return { .. } | lucid_syntax::Stmt::Pass(_)
                ) {
                    return Err(Arc::from(
                        "multi-statement function bodies are not yet supported by CIR lowering",
                    ));
                }
                let mut bindings = Vec::new();
                for statement in &statements[..statements.len() - 1] {
                    collect_pre_return_binding(statement, &mut bindings)?;
                }
                if matches!(
                    last,
                    lucid_syntax::Stmt::Return { value: None, .. } | lucid_syntax::Stmt::Pass(_)
                ) {
                    return lower_bindings_to_void(&bindings);
                }
                (
                    match last {
                        lucid_syntax::Stmt::Return {
                            value: Some(value), ..
                        } => value.span(),
                        _ => unreachable!("last statement was validated above"),
                    },
                    bindings,
                )
            }
            _ => {
                return Err(Arc::from(
                    "multi-statement function bodies are not yet supported by CIR lowering",
                ));
            }
        };
    if function.is_async {
        return Err(Arc::from(
            "async function bodies are not yet supported by CIR lowering",
        ));
    }
    // A bare return may follow pure, primitive bindings.  Keep this on the
    // same checked CIR boundary as value-returning straight-line bodies: the
    // linear lowerer now clears the value channel for `return` without an
    // expression, so an earlier temporary cannot accidentally become the
    // function result.
    if source_function.body.len() >= 2
        && matches!(
            source_function.body.last(),
            Some(lucid_syntax::Stmt::Return { value: None, .. })
        )
    {
        if !local_specs.is_empty() {
            let nodes = function
                .body_expressions
                .iter()
                .map(|node| lucid_cir::TypedExprNode {
                    id: node.id,
                    kind: node.kind.clone(),
                    detail: node.detail.clone(),
                    children: node.children.to_vec(),
                    literal: node.literal,
                })
                .collect::<Vec<_>>();
            let local_bindings = local_specs
                .iter()
                .map(|(name, span)| {
                    function
                        .body_expressions
                        .iter()
                        .rev()
                        .find(|node| node.span == *span)
                        .map(|node| (name.clone(), node.id))
                        .ok_or_else(|| Arc::<str>::from("local binding has no typed expression"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let root_id = local_bindings
                .last()
                .map(|(_, id)| *id)
                .ok_or_else(|| Arc::<str>::from("function has no lowerable expression"))?;
            if let Ok(mut lowered) = lucid_cir::Function::from_typed_function_body_with_locals(
                &nodes,
                root_id,
                &function.parameter_names,
                &local_bindings,
            ) {
                if let Some(block) = lowered.blocks.last_mut() {
                    block.terminator = lucid_cir::Terminator::Return(None);
                }
                lowered
                    .verify()
                    .map_err(|_| Arc::<str>::from("invalid void function CIR"))?;
                return Ok(Arc::new(lowered));
            }
        }
        let module = lucid_syntax::Module {
            statements: source_function.body.clone(),
            span: source_function.span,
        };
        return lucid_cir::Function::from_module_linear_with_params(
            &module,
            &function.parameter_names,
        )
        .map(Arc::new)
        .map_err(|_| Arc::from("unsupported expression before bare return"));
    }
    let root = function
        .body_expressions
        .iter()
        .rev()
        .find(|node| node.span == root_span)
        .ok_or_else(|| Arc::<str>::from("function has no lowerable expression"))?;
    let nodes = function
        .body_expressions
        .iter()
        .map(|node| lucid_cir::TypedExprNode {
            id: node.id,
            kind: node.kind.clone(),
            detail: node.detail.clone(),
            children: node.children.to_vec(),
            literal: node.literal,
        })
        .collect::<Vec<_>>();
    let local_bindings = local_specs
        .into_iter()
        .map(|(name, span)| {
            function
                .body_expressions
                .iter()
                .rev()
                .find(|node| node.span == span)
                .map(|node| (name, node.id))
                .ok_or_else(|| Arc::<str>::from("local binding has no typed expression"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let root_id = if root.kind == "name"
        && function
            .parameter_names
            .iter()
            .any(|name| root.detail.as_deref() == Some(name.as_str()))
    {
        local_bindings
            .iter()
            .rev()
            .find(|(name, _)| root.detail.as_deref() == Some(name.as_str()))
            .map(|(_, id)| *id)
            .unwrap_or(root.id)
    } else {
        root.id
    };
    lucid_cir::Function::from_typed_function_body_with_locals(
        &nodes,
        root_id,
        &function.parameter_names,
        &local_bindings,
    )
    .map(Arc::new)
    .map_err(|error| match error {
        lucid_cir::LowerError::NoLowerableAssignment => {
            Arc::from("function has no lowerable expression")
        }
        lucid_cir::LowerError::UnsupportedExpression => {
            Arc::from("unsupported expression for function CIR lowering")
        }
    })
}

/// Compatibility name for callers that still use the prototype terminology.
/// The tracked implementation is [`lower_module`]; this alias keeps one
/// lowering query and therefore one memoized semantic boundary.
#[salsa::tracked]
pub fn lower_first_assignment(
    db: &dyn Db,
    file: SourceFile,
) -> Result<Arc<lucid_cir::Function>, Arc<str>> {
    lower_module(db, file).clone()
}

fn module_path(path: &str) -> String {
    let path = path.strip_suffix(".lucid").unwrap_or(path);
    let normalized = path.replace(['\\', '/'], ".");
    if normalized == "__init__" {
        return String::new();
    }
    normalized
        .strip_suffix(".__init__")
        .unwrap_or(&normalized)
        .to_string()
}

fn import_path(file_path: &str, import: &str) -> String {
    let leading = import.bytes().take_while(|byte| *byte == b'.').count();
    if leading == 0 {
        return import.to_string();
    }
    let current = module_path(file_path);
    let mut parts = if current.is_empty() {
        Vec::new()
    } else {
        current.split('.').collect::<Vec<_>>()
    };
    if !file_path.replace('\\', "/").ends_with("/__init__.lucid")
        && !file_path.replace('\\', "/").ends_with("__init__.lucid")
    {
        parts.pop();
    }
    for _ in 1..leading {
        parts.pop();
    }
    let suffix = &import[leading..];
    if !suffix.is_empty() {
        parts.extend(suffix.split('.'));
    }
    parts.join(".")
}

fn statement_span(statement: &lucid_syntax::Stmt) -> lucid_syntax::Span {
    match statement {
        lucid_syntax::Stmt::Import { span, .. } | lucid_syntax::Stmt::FromImport { span, .. } => {
            *span
        }
        lucid_syntax::Stmt::Export(inner) => statement_span(inner),
        lucid_syntax::Stmt::Assignment { span, .. } | lucid_syntax::Stmt::VarDef { span, .. } => {
            *span
        }
        _ => lucid_syntax::Span::default(),
    }
}

/// Resolve an import against the project's source-file table.  Matching is
/// deterministic and does not consult the host filesystem during checking.
#[salsa::tracked]
pub fn resolve_module(db: &dyn Db, project: Project, module: String) -> Option<SourceFile> {
    project
        .files(db)
        .iter()
        .copied()
        .find(|file| module_path(file.path(db)) == module)
}

/// Resolve an absolute or relative import in the context of its importing
/// file. Relative dots walk package segments from the file's module path.
#[salsa::tracked]
pub fn resolve_import(
    db: &dyn Db,
    project: Project,
    file: SourceFile,
    module: String,
) -> Option<SourceFile> {
    *resolve_module(db, project, import_path(file.path(db), &module))
}

/// Compute a deterministic dependency-first initialization order.
#[salsa::tracked]
pub fn module_order(db: &dyn Db, project: Project) -> Result<Arc<[SourceFile]>, Arc<str>> {
    let mut files = project.files(db).to_vec();
    files.sort_by(|left, right| left.path(db).cmp(right.path(db)));
    let mut module_paths = std::collections::BTreeMap::<String, SourceFile>::new();
    for &file in &files {
        let path = module_path(file.path(db));
        if let Some(previous) = module_paths.insert(path.clone(), file) {
            return Err(Arc::from(format!(
                "duplicate module path '{path}' (also declared by '{}')",
                previous.path(db)
            )));
        }
    }
    let indices: HashMap<SourceFile, usize> = files
        .iter()
        .copied()
        .enumerate()
        .map(|(index, file)| (file, index))
        .collect();
    let mut state = vec![0u8; files.len()];
    let mut order = Vec::with_capacity(files.len());

    fn declaration_only(db: &dyn Db, file: SourceFile) -> bool {
        let Ok(module) = parse_ast(db, file) else {
            return false;
        };
        fn statement_is_declaration(statement: &lucid_syntax::Stmt) -> bool {
            match statement {
                lucid_syntax::Stmt::Export(inner) => statement_is_declaration(inner),
                lucid_syntax::Stmt::ClassDef { .. }
                | lucid_syntax::Stmt::InterfaceDef { .. }
                | lucid_syntax::Stmt::TraitDef { .. }
                | lucid_syntax::Stmt::ImplementDef { .. }
                | lucid_syntax::Stmt::TypeAlias { .. }
                | lucid_syntax::Stmt::Function(_)
                | lucid_syntax::Stmt::Import { .. }
                | lucid_syntax::Stmt::FromImport { .. }
                | lucid_syntax::Stmt::Pass(_)
                | lucid_syntax::Stmt::Break(_)
                | lucid_syntax::Stmt::Continue(_) => true,
                // An uninitialized annotation reserves a name but performs
                // no value initialization. Every other statement can read
                // or mutate a value while the cycle is being initialized.
                lucid_syntax::Stmt::VarDef { value: None, .. } => true,
                _ => false,
            }
        }
        module.statements.iter().all(statement_is_declaration)
    }

    #[allow(clippy::too_many_arguments)]
    fn visit(
        db: &dyn Db,
        project: Project,
        files: &[SourceFile],
        indices: &HashMap<SourceFile, usize>,
        state: &mut [u8],
        order: &mut Vec<SourceFile>,
        stack: &mut Vec<usize>,
        index: usize,
    ) -> Result<(), Arc<str>> {
        if state[index] == 2 {
            return Ok(());
        }
        if state[index] == 1 {
            // Import cycles are safe when every module in the cycle only
            // contributes declarations. Declarations are collected before
            // initialization, so classes, traits, interfaces, aliases, and
            // functions can refer to one another without observing a value
            // before its module runs. A top-level value/assignment/side
            // effect still makes the cycle an initialization error.
            let Some(start) = stack.iter().position(|entry| *entry == index) else {
                return Err(Arc::from(format!(
                    "cyclic module initialization involving '{}'",
                    files[index].path(db)
                )));
            };
            if stack[start..]
                .iter()
                .copied()
                .all(|cycle_index| declaration_only(db, files[cycle_index]))
            {
                return Ok(());
            }
            return Err(Arc::from(format!(
                "cyclic module initialization involving '{}'",
                files[index].path(db)
            )));
        }
        state[index] = 1;
        stack.push(index);
        for import in imports(db, files[index]).iter() {
            if let Some(target) = resolve_import(db, project, files[index], import.clone())
                && let Some(&target_index) = indices.get(target)
            {
                visit(
                    db,
                    project,
                    files,
                    indices,
                    state,
                    order,
                    stack,
                    target_index,
                )?;
            }
        }
        stack.pop();
        state[index] = 2;
        order.push(files[index]);
        Ok(())
    }

    for index in 0..files.len() {
        visit(
            db,
            project,
            &files,
            &indices,
            &mut state,
            &mut order,
            &mut Vec::new(),
            index,
        )?;
    }
    Ok(Arc::from(order))
}

/// Return names visible from a file: all local declarations and the explicit
/// bindings introduced by imports. A plain `import module` contributes only
/// its module binding; exported declarations remain reachable through that
/// module and are not injected into the unqualified namespace.
#[salsa::tracked]
pub fn visible_symbols<'db>(
    db: &'db dyn Db,
    project: Project,
    file: SourceFile,
) -> Arc<[Symbol<'db>]> {
    let local_module = resolved_module(db, file);
    let mut visible = local_module
        .declarations
        .iter()
        .map(|decl| decl.symbol)
        .collect::<Vec<_>>();
    visible.extend(
        imported_bindings(db, project, file)
            .iter()
            .map(|binding| binding.symbol),
    );
    visible.sort_by(|left, right| left.name(db).cmp(right.name(db)));
    visible.dedup();
    Arc::from(visible)
}

#[salsa::tracked]
pub fn resolve_visible<'db>(
    db: &'db dyn Db,
    project: Project,
    file: SourceFile,
    name: String,
) -> Option<Symbol<'db>> {
    if let Some(local) = resolved_declarations(db, file)
        .iter()
        .find(|declaration| declaration.symbol.name(db).as_str() == name)
    {
        return Some(local.symbol);
    }
    if let Some(binding) = imported_bindings(db, project, file)
        .iter()
        .find(|binding| binding.local_name == name)
    {
        return Some(binding.symbol);
    }
    visible_symbols(db, project, file)
        .iter()
        .copied()
        .find(|symbol| symbol.name(db).as_str() == name)
}

#[salsa::tracked]
pub fn declaration_diagnostics(db: &dyn Db, file: SourceFile) -> Arc<[Arc<str>]> {
    let declarations = resolved_declarations(db, file);
    let mut seen = std::collections::BTreeMap::new();
    let mut errors = Vec::new();
    for declaration in declarations.iter() {
        let name = declaration.symbol.name(db);
        if let Some(previous) = seen.get(name.as_str()) {
            let dispatch_overload =
                *previous && declaration.kind == DeclKind::Function && declaration.is_dispatch;
            if dispatch_overload {
                continue;
            }
            errors.push(Arc::<str>::from(format!(
                "duplicate top-level declaration '{name}'"
            )));
        } else {
            seen.insert(
                name.to_string(),
                declaration.kind == DeclKind::Function && declaration.is_dispatch,
            );
        }
    }
    Arc::from(errors)
}

#[salsa::tracked]
pub fn file_diagnostics(db: &dyn Db, file: SourceFile) -> Arc<[Diagnostic]> {
    let mut diagnostics = Vec::new();
    if let Err(error) = parse_ast(db, file).as_ref() {
        // Preserve every independently recoverable grammar error. When a
        // lexical error exists, suppress parser errors caused by the
        // truncated token suffix and retain the lexical root span.
        if let Ok((_, errors)) = lucid_syntax::parse_recovering(file.text(db)) {
            let has_lexical_error = errors
                .iter()
                .any(|parse_error| parse_error.message.starts_with("lexer error:"));
            let parse_errors = if has_lexical_error {
                errors
                    .into_iter()
                    .filter(|parse_error| parse_error.message.starts_with("lexer error:"))
                    .collect::<Vec<_>>()
            } else {
                errors
            };
            for parse_error in parse_errors {
                diagnostics.push(Diagnostic {
                    file,
                    severity: Severity::Error,
                    code: "E0001".into(),
                    message: format!("Parse error: {}", parse_error.message),
                    span: parse_error.span,
                });
            }
        }
        if diagnostics.is_empty() {
            diagnostics.push(Diagnostic {
                file,
                severity: Severity::Error,
                code: "E0001".into(),
                message: error.to_string(),
                span: first_parse_error_span(db, file),
            });
        }
        return Arc::from(diagnostics);
    }
    let declarations = resolved_declarations(db, file);
    let mut seen = std::collections::BTreeMap::new();
    for declaration in declarations.iter() {
        let name = declaration.symbol.name(db);
        if let Some(previous) = seen.get(name.as_str()) {
            let dispatch_overload =
                *previous && declaration.kind == DeclKind::Function && declaration.is_dispatch;
            if dispatch_overload {
                continue;
            }
            diagnostics.push(Diagnostic {
                file,
                severity: Severity::Error,
                code: "E0100".into(),
                message: format!("duplicate top-level declaration '{name}'"),
                span: declaration.span,
            });
        } else {
            seen.insert(
                name.to_string(),
                declaration.kind == DeclKind::Function && declaration.is_dispatch,
            );
        }
    }
    let module_result = parse_ast(db, file);
    let Ok(module) = module_result else {
        return Arc::from(diagnostics);
    };
    let mut checker = lucid_checker::TypeChecker::new();
    if let Err(error) = checker.check_module(module) {
        diagnostics.push(Diagnostic {
            file,
            severity: Severity::Error,
            code: "E0200".into(),
            message: error.message,
            span: error.span,
        });
    }
    Arc::from(diagnostics)
}

/// Aggregate structured diagnostics for a complete project, including
/// unresolved module edges.  This is the semantic diagnostic boundary used by
/// editors and future build commands; string formatting is deferred to the
/// presentation layer.
#[salsa::tracked]
pub fn project_diagnostics(db: &dyn Db, project: Project) -> Arc<[Diagnostic]> {
    let mut diagnostics = Vec::new();
    let order = match module_order(db, project).as_ref() {
        Ok(order) => order.clone(),
        Err(message) => {
            if let Some(&file) = project
                .files(db)
                .iter()
                .find(|file| message.contains(file.path(db).as_str()))
                .or_else(|| project.files(db).first())
            {
                let span = parse_ast(db, file)
                    .as_ref()
                    .ok()
                    .and_then(|module| module.statements.first().map(statement_span))
                    .unwrap_or_default();
                diagnostics.push(Diagnostic {
                    file,
                    severity: Severity::Error,
                    code: if message.starts_with("duplicate module path") {
                        "E0303"
                    } else {
                        "E0301"
                    }
                    .into(),
                    message: message.to_string(),
                    span,
                });
            }
            return Arc::from(diagnostics);
        }
    };
    for file in order.iter().copied() {
        diagnostics.extend(file_diagnostics(db, file).iter().cloned());
        for import in imports(db, file).iter() {
            if resolve_import(db, project, file, import.clone()).is_none() {
                let span = parse_ast(db, file)
                    .as_ref()
                    .ok()
                    .and_then(|module| {
                        module.statements.iter().find_map(|statement| {
                            let matches = match statement {
                                lucid_syntax::Stmt::Import { module, .. }
                                | lucid_syntax::Stmt::FromImport { module, .. } => module == import,
                                _ => false,
                            };
                            matches.then(|| statement_span(statement))
                        })
                    })
                    .unwrap_or_default();
                diagnostics.push(Diagnostic {
                    file,
                    severity: Severity::Error,
                    code: "E0300".into(),
                    message: format!("unresolved module import '{import}'"),
                    span,
                });
            }
        }
        let Ok(module) = parse_ast(db, file) else {
            continue;
        };
        let mut imported_names = std::collections::BTreeSet::new();
        for statement in &module.statements {
            if let lucid_syntax::Stmt::Export(inner) = statement {
                let private_name = match inner.as_ref() {
                    lucid_syntax::Stmt::ClassDef { name, .. }
                    | lucid_syntax::Stmt::InterfaceDef { name, .. }
                    | lucid_syntax::Stmt::TraitDef { name, .. }
                    | lucid_syntax::Stmt::TypeAlias { name, .. }
                    | lucid_syntax::Stmt::Function(lucid_syntax::FunctionDef { name, .. }) => {
                        Some(name)
                    }
                    lucid_syntax::Stmt::VarDef {
                        pattern: lucid_syntax::Pattern::Ident(name, _),
                        ..
                    } => Some(name),
                    lucid_syntax::Stmt::Assignment {
                        target: lucid_syntax::Expr::Ident { name, .. },
                        ..
                    } => Some(name),
                    _ => None,
                };
                if let Some(name) = private_name.filter(|name| name.starts_with('_')) {
                    diagnostics.push(Diagnostic {
                        file,
                        severity: Severity::Error,
                        code: "E0304".into(),
                        message: format!("cannot export private name '{name}'"),
                        span: statement_span(statement),
                    });
                }
            }
            if let lucid_syntax::Stmt::Import { module, alias, .. } = statement {
                let local_name = alias.clone().unwrap_or_else(|| {
                    module
                        .rsplit('.')
                        .next()
                        .unwrap_or(module)
                        .trim_start_matches('.')
                        .to_string()
                });
                if !imported_names.insert(local_name.clone()) {
                    diagnostics.push(Diagnostic {
                        file,
                        severity: Severity::Error,
                        code: "E0305".into(),
                        message: format!("duplicate imported binding '{local_name}'"),
                        span: statement_span(statement),
                    });
                }
                continue;
            }
            let lucid_syntax::Stmt::FromImport {
                module: module_name,
                names,
                ..
            } = statement
            else {
                continue;
            };
            let Some(imported_file) = resolve_import(db, project, file, module_name.clone()) else {
                continue;
            };
            let declarations = resolved_declarations(db, *imported_file);
            for (name, alias) in names {
                let local_name = alias.as_ref().unwrap_or(name);
                if !imported_names.insert(local_name.clone()) {
                    diagnostics.push(Diagnostic {
                        file,
                        severity: Severity::Error,
                        code: "E0305".into(),
                        message: format!("duplicate imported binding '{local_name}'"),
                        span: statement_span(statement),
                    });
                }
                let exported = declarations
                    .iter()
                    .any(|decl| decl.exported && decl.symbol.name(db).as_str() == name);
                if !exported {
                    diagnostics.push(Diagnostic {
                        file,
                        severity: Severity::Error,
                        code: "E0302".into(),
                        message: format!("cannot import '{name}' from module '{module_name}'"),
                        span: statement_span(statement),
                    });
                }
            }
        }
    }
    Arc::from(diagnostics)
}

#[salsa::tracked]
pub fn type_check_project(db: &dyn Db, project: Project) -> Arc<[Arc<str>]> {
    let errors = project_diagnostics(db, project)
        .iter()
        .map(|diagnostic| Arc::<str>::from(format!("{}: {}", diagnostic.code, diagnostic.message)))
        .collect::<Vec<_>>();
    Arc::from(errors)
}

/// Resolve a module-level name to its interned declaration identity.
#[salsa::tracked]
pub fn resolve_top_level<'db>(
    db: &'db dyn Db,
    file: SourceFile,
    name: String,
) -> Option<Symbol<'db>> {
    resolved_module(db, file)
        .declarations
        .iter()
        .map(|declaration| declaration.symbol)
        .find(|symbol| symbol.name(db).as_str() == name)
}

impl CompilerDatabase {
    pub fn add_file(&mut self, path: impl Into<String>, text: impl Into<String>) -> SourceFile {
        SourceFile::new(self, text.into(), path.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucid_syntax::LucidLanguage;
    use rowan::SyntaxNode;
    use salsa::Setter;

    #[test]
    fn source_files_parse_incrementally_and_round_trip() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "x = 1  # keep\n");
        {
            let first = parse_file(&db, file);
            let first_tree = SyntaxNode::<LucidLanguage>::new_root(first.as_ref().clone());
            assert_eq!(first_tree.to_string(), "x = 1  # keep\n");
        }
        assert_eq!(*source_position(&db, file, 0), (1, 1));
        assert_eq!(*source_position(&db, file, 10), (1, 11));
        let unicode = db.add_file("unicode.lucid", "é = 1\nnext = 2\n");
        assert_eq!(*source_position(&db, unicode, 1), (1, 1));
        assert_eq!(*source_position(&db, unicode, 3), (1, 3));
        assert_eq!(*source_offset(&db, unicode, 1, 1), 0);
        assert_eq!(*source_offset(&db, unicode, 1, 2), 2);
        assert_eq!(*source_offset(&db, unicode, 1, 99), 6);
        assert_eq!(
            *source_offset(&db, unicode, 99, 99),
            unicode.text(&db).len() as u32
        );
        assert_eq!(
            span_text(&db, unicode, lucid_syntax::Span::new(0, 2, 1, 1)).as_ref(),
            "é"
        );
        assert_eq!(
            span_text(&db, unicode, lucid_syntax::Span::new(1, 2, 1, 1)).as_ref(),
            "é"
        );
        assert_eq!(
            span_text(&db, unicode, lucid_syntax::Span::new(5, 1, 1, 1)).as_ref(),
            ""
        );
        let crlf = db.add_file("crlf.lucid", "first\r\nsecond\n");
        assert_eq!(*source_position(&db, crlf, 7), (2, 1));
        assert_eq!(*source_offset(&db, crlf, 1, 99), 5);
        assert_eq!(source_line(&db, crlf, 1).as_ref(), "first");
        assert_eq!(source_line(&db, crlf, 2).as_ref(), "second");
        assert_eq!(source_line(&db, crlf, 9).as_ref(), "");

        file.set_text(&mut db).to("x = 2  # keep\n".into());
        let second = parse_file(&db, file);
        let second_tree = SyntaxNode::<LucidLanguage>::new_root(second.as_ref().clone());
        assert_eq!(second_tree.to_string(), "x = 2  # keep\n");
    }

    #[test]
    fn span_text_returns_empty_for_reversed_ranges() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "value = 1\n");
        assert_eq!(
            span_text(&db, file, lucid_syntax::Span::new(8, 2, 1, 1)).as_ref(),
            ""
        );
    }

    #[test]
    fn source_map_line_and_offset_round_trip_unicode_positions() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("unicode.lucid", "éx = 1\nnext = 2\n");
        let line = source_line(&db, file, 1);
        assert_eq!(line.as_ref(), "éx = 1");
        let offset = *source_offset(&db, file, 1, 2);
        assert_eq!(
            span_text(
                &db,
                file,
                lucid_syntax::Span::new(offset as usize, offset as usize + 1, 1, 2)
            )
            .as_ref(),
            "x"
        );
        assert_eq!(*source_position(&db, file, offset), (1, 2));
    }

    #[test]
    fn strict_ast_query_caches_success_and_diagnostic() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "value = 42\n");
        assert!(parse_ast(&db, file).is_ok());

        file.set_text(&mut db).to("value = `broken`\n".into());
        let result = parse_ast(&db, file);
        let error = result
            .as_ref()
            .expect_err("invalid source should retain a diagnostic");
        assert!(error.contains("Lexer error"));
    }

    #[test]
    fn typed_module_depends_on_resolved_hir_and_rechecks_changes() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "value: int = 1\n");
        let other = db.add_file("other.lucid", "answer: int = 2\n");
        let typed = typed_module(&db, file).as_ref().expect("valid module");
        assert!(typed.checked);
        assert_eq!(typed.resolved.file, file);
        assert_eq!(typed.initializers.len(), 1);
        assert_eq!(typed.initializers[0].symbol.name(&db), "value");
        assert_eq!(*typed.initializers[0].symbol.file(&db), file);
        assert_eq!(typed.initializers[0].type_name, "int");
        assert_eq!(typed.initializers[0].type_id.canonical(&db), "int");
        let type_id = typed.initializers[0].type_id;
        let other_typed = typed_module(&db, other).as_ref().expect("valid module");
        assert_eq!(type_id, other_typed.initializers[0].type_id);
        file.set_text(&mut db).to("value: int = \"bad\"\n".into());
        assert!(typed_module(&db, file).is_err());
    }

    #[test]
    fn typed_module_exposes_postorder_expression_hir() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "value: int = 2 + 3 * 4\n");
        let typed = typed_module(&db, file).as_ref().expect("valid module");
        assert_eq!(typed.expressions.len(), 5);
        let root = typed.expressions.last().expect("root expression");
        assert_eq!(root.kind, "binary");
        assert_eq!(root.detail.as_deref(), Some("Add"));
        assert_eq!(root.type_name, "int");
        assert_eq!(root.children.as_ref(), &[0, 3]);
        let multiplication = &typed.expressions[3];
        assert_eq!(multiplication.detail.as_deref(), Some("Mul"));
        assert_eq!(multiplication.children.as_ref(), &[1, 2]);
        assert_eq!(typed.expressions[0].kind, "literal");
    }

    #[test]
    fn typed_module_exposes_interned_function_signature() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "main.lucid",
            "def add(left: int, right: int = 1) -> int:\n    return left + right\n",
        );
        let typed = typed_module(&db, file).as_ref().expect("valid module");
        assert_eq!(typed.functions.len(), 1);
        let function = &typed.functions[0];
        assert_eq!(function.symbol.name(&db), "add");
        assert_eq!(&*function.parameter_names, &["left", "right"]);
        assert_eq!(&*function.required_parameters, &[true, false]);
        assert_eq!(function.parameter_types.len(), 2);
        assert_eq!(function.parameter_types[0].canonical(&db), "int");
        assert_eq!(function.parameter_types[1].canonical(&db), "int");
        assert_eq!(function.return_type.canonical(&db), "int");
        assert_eq!(function.overload_types.len(), 1);
        assert_eq!(function.body_expressions.len(), 4);
        let body_root = function.body_expressions.last().expect("return expression");
        assert_eq!(body_root.kind, "binary");
        assert_eq!(body_root.type_name, "int");
        assert_eq!(body_root.children.as_ref(), &[1, 2]);
        assert_eq!(function.body_expressions[1].detail.as_deref(), Some("left"));
    }

    #[test]
    fn typed_module_exposes_inferred_function_return_signature() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "inferred-return.lucid",
            "def answer():\n    value = 40 + 2\n    return value\n",
        );
        let typed = typed_module(&db, file).as_ref().expect("valid module");
        let function = &typed.functions[0];
        assert_eq!(function.symbol.name(&db), "answer");
        assert_eq!(function.return_type.canonical(&db), "int");

        let lowered = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("inferred-return function should lower");
        assert_eq!(lowered.execute(), Ok(Some(42)));
    }

    #[test]
    fn typed_hir_retains_constant_loop_body_expressions() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "loop-hir.lucid",
            "def sum():\n    total = 0\n    for i in [1, 2]:\n        total = total + i\n    return total\n",
        );
        let typed = typed_module(&db, file).as_ref().expect("valid module");
        let body = &typed.functions[0].body_expressions;
        assert!(body.iter().any(|node| node.detail.as_deref() == Some("i")));
        assert!(
            body.iter()
                .any(|node| node.detail.as_deref() == Some("Add"))
        );

        let file = db.add_file(
            "loop-local-hir.lucid",
            "def sum():\n    total = 0\n    for i in [1, 2]:\n        next = i + 1\n        total = total + next\n    return total\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid local loop module");
        let body = &typed.functions[0].body_expressions;
        assert!(
            body.iter()
                .any(|node| node.detail.as_deref() == Some("next"))
        );
        assert!(
            body.iter()
                .filter(|node| node.detail.as_deref() == Some("Add"))
                .count()
                >= 2
        );

        let file = db.add_file(
            "while-local-hir.lucid",
            "def sum(value: int):\n    while value > 0:\n        next = value - 1\n        value = next\n    return value\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid while module");
        let body = &typed.functions[0].body_expressions;
        assert!(
            body.iter()
                .any(|node| node.detail.as_deref() == Some("next"))
        );
        assert!(
            body.iter()
                .filter(|node| node.detail.as_deref() == Some("Sub"))
                .count()
                >= 1
        );

        let file = db.add_file(
            "while-pattern-hir.lucid",
            "def probe(value: int):\n    while value > 0:\n        let pair = value - 1\n        value = pair\n    return value\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid while local module");
        assert!(
            typed.functions[0]
                .body_expressions
                .iter()
                .any(|node| node.detail.as_deref() == Some("pair"))
        );

        let file = db.add_file(
            "if-local-hir.lucid",
            "def choose(flag: bool):\n    if flag:\n        selected = 3\n        return selected\n    else:\n        fallback = 4\n        return fallback\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid branch-local module");
        let body = &typed.functions[0].body_expressions;
        assert!(
            body.iter()
                .any(|node| node.detail.as_deref() == Some("selected"))
        );
        assert!(
            body.iter()
                .any(|node| node.detail.as_deref() == Some("fallback"))
        );
        let file = db.add_file(
            "match-local-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            selected = 3\n            return selected\n        case _:\n            fallback = 4\n            return fallback\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid match-local module");
        let body = &typed.functions[0].body_expressions;
        assert!(
            body.iter()
                .any(|node| node.detail.as_deref() == Some("selected"))
        );
        assert!(
            body.iter()
                .any(|node| node.detail.as_deref() == Some("fallback"))
        );
        let file = db.add_file(
            "match-direct-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            return 3\n        case _:\n            return 4\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid direct match module");
        assert!(typed.functions[0].body_expressions.iter().any(|node| {
            node.kind == "match" && node.detail.as_deref() == Some("literal-int:1")
        }));

        let file = db.add_file(
            "match-noop-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            pass\n            return 3\n        case _:\n            value + 1\n            fallback = 4\n            return fallback\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid no-op match module");
        assert!(typed.functions[0].body_expressions.iter().any(|node| {
            node.kind == "match" && node.detail.as_deref() == Some("literal-int:1")
        }));

        let file = db.add_file(
            "match-assert-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            assert(true)\n            return 3\n        case _:\n            assert(true)\n            fallback = 4\n            return fallback\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid assert match module");
        assert!(typed.functions[0].body_expressions.iter().any(|node| {
            node.kind == "match" && node.detail.as_deref() == Some("literal-int:1")
        }));

        let file = db.add_file(
            "match-dead-loop-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            while false:\n                pass\n            return 3\n        case _:\n            while false:\n                pass\n            fallback = 4\n            return fallback\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid dead-loop match module");
        assert!(typed.functions[0].body_expressions.iter().any(|node| {
            node.kind == "match" && node.detail.as_deref() == Some("literal-int:1")
        }));

        let file = db.add_file(
            "match-empty-for-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            for item in []:\n                pass\n            return 3\n        case _:\n            for item in []:\n                pass\n            fallback = 4\n            return fallback\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid empty-for match module");
        assert!(typed.functions[0].body_expressions.iter().any(|node| {
            node.kind == "match" && node.detail.as_deref() == Some("literal-int:1")
        }));

        let file = db.add_file(
            "match-static-noop-if-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            if false:\n                return 0\n            return 3\n        case _:\n            if true:\n                pass\n            fallback = 4\n            return fallback\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid static no-op branch match module");
        assert!(typed.functions[0].body_expressions.iter().any(|node| {
            node.kind == "match" && node.detail.as_deref() == Some("literal-int:1")
        }));

        let file = db.add_file(
            "match-static-bool-noop-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            assert(not false)\n            return 3\n        case _:\n            while not true:\n                return 0\n            fallback = 4\n            return fallback\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid static-bool no-op match module");
        assert!(typed.functions[0].body_expressions.iter().any(|node| {
            node.kind == "match" && node.detail.as_deref() == Some("literal-int:1")
        }));

        let file = db.add_file(
            "match-bool-expression-noop-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            assert(true and not false)\n            return 3\n        case _:\n            while true and false:\n                return 0\n            fallback = 4\n            return fallback\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid bool-expression no-op match module");
        assert!(typed.functions[0].body_expressions.iter().any(|node| {
            node.kind == "match" && node.detail.as_deref() == Some("literal-int:1")
        }));

        let file = db.add_file(
            "match-static-if-result-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            if true:\n                return 3\n            else:\n                return 0\n        case _:\n            if false:\n                return 0\n            else:\n                fallback = 4\n                return fallback\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid static-if result match module");
        assert!(typed.functions[0].body_expressions.iter().any(|node| {
            node.kind == "match" && node.detail.as_deref() == Some("literal-int:1")
        }));

        let file = db.add_file(
            "try-local-hir.lucid",
            "def choose(value: int):\n    try:\n        selected = value + 1\n        return selected\n    except str as error:\n        fallback = 0\n        return fallback\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("valid try-local module");
        let body = &typed.functions[0].body_expressions;
        assert!(
            body.iter()
                .any(|node| node.detail.as_deref() == Some("selected"))
        );
        assert!(
            body.iter()
                .any(|node| node.detail.as_deref() == Some("fallback"))
        );
    }

    #[test]
    fn typed_hir_scopes_with_target_bindings() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "with-target-hir.lucid",
            "contextmanager def managed():\n    yield 10\n\ndef read():\n    with managed() as ctx:\n        return ctx\n",
        );
        let typed = typed_module(&db, file)
            .as_ref()
            .expect("with target should be visible in the managed body");
        let function = typed
            .functions
            .iter()
            .find(|function| function.symbol.name(&db).as_str() == "read")
            .expect("read function");
        assert!(
            function
                .body_expressions
                .iter()
                .any(|node| node.detail.as_deref() == Some("ctx"))
        );
    }

    #[test]
    fn typed_module_preserves_dispatch_overload_set() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "main.lucid",
            "dispatch def show(value: int) -> int:\n    return value\n\ndispatch def show(value: str) -> int:\n    return 0\n",
        );
        let typed = typed_module(&db, file).as_ref().expect("valid module");
        assert_eq!(typed.functions.len(), 1);
        assert!(typed.functions[0].is_dispatch);
        assert!(!typed.functions[0].is_async);
        assert_eq!(typed.functions[0].overload_types.len(), 2);
        assert!(
            typed.functions[0]
                .overload_types
                .iter()
                .any(|id| id.canonical(&db).contains("int"))
        );
        assert!(
            typed.functions[0]
                .overload_types
                .iter()
                .any(|id| id.canonical(&db).contains("str"))
        );
    }

    #[test]
    fn database_lowers_assignment_through_shared_cir() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "value = 2 + 3 * 4\n");
        let function = lower_module(&db, file)
            .as_ref()
            .expect("assignment should lower");
        assert_eq!(function.execute(), Ok(Some(14)));
        let file = db.add_file("branch.lucid", "if 1 < 2:\n    x = 4\nelse:\n    x = 9\n");
        let function = lower_module(&db, file)
            .as_ref()
            .expect("branch should lower");
        assert_eq!(function.execute(), Ok(Some(4)));
        let file = db.add_file("empty.lucid", "pass\n");
        let function = lower_module(&db, file)
            .as_ref()
            .expect("void module should lower");
        assert_eq!(function.execute(), Ok(None));
    }

    #[test]
    fn database_lowers_primitive_initializer_from_typed_hir() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "value = 2 + 3 * 4\n");
        let typed = typed_module(&db, file).as_ref().expect("valid module");
        assert_eq!(typed.initializers.len(), 1);
        assert_eq!(typed.initializers[0].expression, 4);
        let function = lower_module(&db, file)
            .as_ref()
            .expect("typed initializer should lower");
        assert_eq!(function.execute(), Ok(Some(14)));

        let file = db.add_file("conditional.lucid", "value = 11 if true else 22\n");
        let function = lower_module(&db, file)
            .as_ref()
            .expect("constant conditional should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(11)));

        let file = db.add_file("logical.lucid", "value = true or false\n");
        let function = lower_module(&db, file)
            .as_ref()
            .expect("short-circuit initializer should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(1)));
    }

    #[test]
    fn database_lowers_primitive_function_body_from_typed_hir() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "def answer():\n    return 6 * 7\n");
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("function body should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "dynamic.lucid",
            "def choose(flag: int):\n    return 11 if flag else 22\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("dynamic conditional should lower through typed HIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(22)));

        let file = db.add_file(
            "match.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            return 11\n        case _:\n            return 33\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("literal match should lower through conditional CIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(33)));

        let file = db.add_file(
            "leading-wildcard-match.lucid",
            "def choose(value: int):\n    match value:\n        case _:\n            return value + 100\n        case 1:\n            return 11\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("leading wildcard match should preserve arm order");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(101)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(107)));

        let file = db.add_file(
            "guarded-literal-match.lucid",
            "def choose(value: int):\n    match value:\n        case 1 if false:\n            return 11\n        case _:\n            return value + 100\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("guarded literal match should lower with guard in CIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(101)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(107)));

        let file = db.add_file(
            "guarded-wildcard-match.lucid",
            "def choose(value: int):\n    match value:\n        case _ if value > 0:\n            return value + 100\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("guarded wildcard match should lower as optional CIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(101)));
        assert_eq!(function.execute_with_args(&[-7]), Ok(None));

        let file = db.add_file(
            "guarded-void-match.lucid",
            "def answer(value: int):\n    match value:\n        case 1 if false:\n            return\n        case _:\n            pass\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("guarded void match should lower with guard in CIR");
        assert!(
            function.blocks.iter().any(|block| {
                block
                    .instructions
                    .iter()
                    .any(|instruction| matches!(instruction, lucid_cir::Instruction::And { .. }))
            }),
            "guarded void match CIR should retain the guard expression"
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));

        let file = db.add_file(
            "leading-wildcard-local-match.lucid",
            "def choose(value: int):\n    match value:\n        case _:\n            selected = value + 100\n            return selected\n        case 1:\n            return 11\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("leading wildcard local match should preserve arm order");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(101)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(107)));

        let file = db.add_file(
            "match-noop-local-return.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            assert(true)\n            selected = 11\n            return selected\n        case _:\n            if false:\n                return 0\n            fallback = value + 100\n            return fallback\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("match local returns should ignore no-op setup");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(107)));

        let file = db.add_file(
            "match-pure-expression-local-return.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            value + 1\n            selected = 11\n            return selected\n        case _:\n            value * 2\n            return value + 100\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("match local returns should ignore pure expression setup");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(107)));

        let file = db.add_file(
            "match-static-if-local-return.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            if true:\n                selected = 11\n                return selected\n            else:\n                return 0\n        case _:\n            if false:\n                return 0\n            else:\n                return value + 100\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("match local returns should unwrap static conditionals");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(107)));

        let file = db.add_file(
            "middle-wildcard-match.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            return 11\n        case _:\n            return value + 100\n        case 2:\n            return 22\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("middle wildcard match should make later arms unreachable");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(102)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(107)));

        let file = db.add_file(
            "middle-wildcard-void-match.lucid",
            "def answer(value: int):\n    match value:\n        case 1:\n            return\n        case _:\n            pass\n        case 2:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("middle wildcard void match should make later arms unreachable");
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[2]), Ok(None));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));

        let file = db.add_file(
            "match-noop-void.lucid",
            "def answer(value: int):\n    match value:\n        case 1:\n            while false:\n                return value\n            return\n        case _:\n            for item in []:\n                return value\n            pass\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("match void arms should ignore no-op setup");
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));

        let file = db.add_file(
            "match-static-if-void.lucid",
            "def answer(value: int):\n    match value:\n        case 1:\n            if true:\n                return\n            else:\n                return value\n        case _:\n            if false:\n                return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("match void arms should unwrap static conditionals");
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));

        let file = db.add_file(
            "leading-wildcard-void-match.lucid",
            "def answer(value: int):\n    match value:\n        case _:\n            return\n        case 1:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("leading wildcard void match should preserve arm order");
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));

        let file = db.add_file(
            "leading-wildcard-setup-void-match.lucid",
            "def answer(value: int):\n    match value:\n        case _:\n            temporary = value + 1\n            return\n        case 1:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("leading wildcard setup before void match should preserve selected setup");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "leading-wildcard-setup-fallthrough-match.lucid",
            "def answer(value: int):\n    match value:\n        case _:\n            temporary = value + 1\n        case 1:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("leading wildcard setup-only match should preserve selected setup");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "match-local-cir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            selected = 11\n            return selected\n        case _:\n            fallback = 33\n            return fallback\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("match-local values should lower through typed CIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(33)));

        let file = db.add_file(
            "constant-subject-match.lucid",
            "def choose(value: int):\n    match true as flag:\n        case false:\n            return value * 10\n        case true:\n            selected = value + 10\n            return selected\n        case _:\n            return 0\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("constant subject match should lower only the selected arm");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(17)));

        let file = db.add_file(
            "constant-subject-void-match.lucid",
            "def answer(value: int):\n    match true as flag:\n        case true:\n            return\n        case _:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant subject void match should lower only the selected arm");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));

        let file = db.add_file(
            "constant-subject-pass-match.lucid",
            "def answer(value: int):\n    match true as flag:\n        case true:\n            pass\n        case _:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant subject pass match should lower only the selected arm");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));

        let file = db.add_file(
            "constant-subject-setup-void-match.lucid",
            "def answer(value: int):\n    match true as flag:\n        case true:\n            temporary = value + 1\n            return\n        case _:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant subject setup before void match should preserve setup");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-subject-noop-setup-void-match.lucid",
            "def answer(value: int):\n    match true as flag:\n        case true:\n            assert(true)\n            if false:\n                return value\n            temporary = value + 1\n            return\n        case _:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant subject no-op setup before void match should preserve setup");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-subject-static-branch-setup-void-match.lucid",
            "def answer(value: int):\n    match true as flag:\n        case true:\n            if true:\n                temporary = value + 1\n                return\n            else:\n                return value\n        case _:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant subject static branch setup before void match should lower");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-subject-pure-setup-void-match.lucid",
            "def answer(value: int):\n    match true as flag:\n        case true:\n            value + 1\n            temporary = value + 1\n            return\n        case _:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant subject pure setup before void match should preserve bindings");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-subject-setup-fallthrough-match.lucid",
            "def answer(value: int):\n    match true as flag:\n        case true:\n            temporary = value + 1\n        case _:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant subject setup-only match should preserve setup");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-subject-noop-setup-fallthrough-match.lucid",
            "def answer(value: int):\n    match true as flag:\n        case true:\n            while false:\n                return value\n            temporary = value + 1\n        case _:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant subject no-op setup-only match should preserve setup");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-subject-static-branch-setup-fallthrough-match.lucid",
            "def answer(value: int):\n    match true as flag:\n        case true:\n            if true:\n                temporary = value + 1\n            else:\n                return value\n        case _:\n            return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant subject static branch setup-only match should preserve setup");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-subject-effectful-fallthrough-match.lucid",
            "def answer(value: int):\n    match true as flag:\n        case true:\n            str(value)\n        case _:\n            return value\n",
        );
        let error = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect_err("effectful selected match setup must not be erased");
        assert!(error.contains("effectful discarded expression"));

        let file = db.add_file(
            "constant-subject-dead-invalid-match.lucid",
            "def answer():\n    match true as flag:\n        case true:\n            return 42\n        case _:\n            return 1 // 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant subject match must not evaluate dead fallback arm");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "guarded-constant-subject-match.lucid",
            "def choose(value: int):\n    match true as flag:\n        case true if value > 0:\n            return value + 10\n        case _:\n            return -value\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("guarded constant subject match should keep the guard dynamic");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[-7]), Ok(Some(7)));

        let file = db.add_file(
            "multi-match.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            return 11\n        case 2:\n            return 22\n        case _:\n            return 33\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multi-arm literal match should lower through CIR");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(22)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(33)));

        let file = db.add_file(
            "multi-match-noop-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            pass\n            return 11\n        case 2:\n            value + 1\n            return 22\n        case _:\n            fallback = 33\n            return fallback\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multi-arm literal match with no-op setup should lower through CIR");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm no-op match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(22)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(33)));

        let file = db.add_file(
            "multi-match-assert-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            assert(true)\n            return 11\n        case 2:\n            assert(true)\n            return 22\n        case _:\n            assert(true)\n            fallback = 33\n            return fallback\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multi-arm literal match with assert setup should lower through CIR");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm assert match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(22)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(33)));

        let file = db.add_file(
            "multi-match-dead-loop-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            while false:\n                pass\n            return 11\n        case 2:\n            while false:\n                pass\n            return 22\n        case _:\n            while false:\n                pass\n            fallback = 33\n            return fallback\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multi-arm literal match with dead-loop setup should lower through CIR");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm dead-loop match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(22)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(33)));

        let file = db.add_file(
            "multi-match-empty-for-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            for item in []:\n                pass\n            return 11\n        case 2:\n            for item in []:\n                pass\n            return 22\n        case _:\n            for item in []:\n                pass\n            fallback = 33\n            return fallback\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multi-arm literal match with empty-for setup should lower through CIR");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm empty-for match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(22)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(33)));

        let file = db.add_file(
            "multi-match-static-noop-if-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            if false:\n                return 0\n            return 11\n        case 2:\n            if true:\n                pass\n            return 22\n        case _:\n            if false:\n                return 0\n            fallback = 33\n            return fallback\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multi-arm literal match with static no-op branches should lower through CIR");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm static no-op branch match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(22)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(33)));

        let file = db.add_file(
            "multi-match-static-bool-noop-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            assert(not false)\n            return 11\n        case 2:\n            while not true:\n                return 0\n            return 22\n        case _:\n            assert(not false)\n            while not true:\n                return 0\n            fallback = 33\n            return fallback\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect(
                "multi-arm literal match with static-bool no-op setup should lower through CIR",
            );
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm static-bool no-op match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(22)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(33)));

        let file = db.add_file(
            "multi-match-bool-expression-noop-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            assert(true is true)\n            return 11\n        case 2:\n            while true and false:\n                return 0\n            return 22\n        case _:\n            assert(false is not true)\n            fallback = 33\n            return fallback\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multi-arm literal match with bool-expression no-op setup should lower");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm bool-expression no-op match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(22)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(33)));

        let file = db.add_file(
            "multi-match-static-if-result-hir.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            if true:\n                return 11\n            else:\n                return 0\n        case 2:\n            if false:\n                return 0\n            else:\n                return 22\n        case _:\n            if true:\n                fallback = 33\n                return fallback\n            else:\n                return 0\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multi-arm literal match with static-if result should lower");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm static-if result match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(22)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(33)));

        let file = db.add_file(
            "multi-match-expression.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            return value + 10\n        case 2:\n            return value * 10\n        case _:\n            return -value\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multi-arm expression match should lower through a CIR ladder");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm expression match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(20)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(-7)));

        let file = db.add_file(
            "multi-match-local-expression.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            selected = value + 10\n            return selected\n        case 2:\n            doubled = value * 10\n            return doubled\n        case _:\n            fallback = -value\n            return fallback\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multi-arm local expression match should lower through a CIR ladder");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm local expression match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(20)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(-7)));

        let file = db.add_file(
            "optional-match-expression.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            return value + 10\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("single-arm optional match should lower through CIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));

        let file = db.add_file(
            "optional-match-chain-expression.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            selected = value + 10\n            return selected\n        case 2:\n            doubled = value * 10\n            return doubled\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multi-arm optional match should lower through CIR");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm optional match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "optional-match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(20)));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));

        let file = db.add_file(
            "pass-fallback-match-expression.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            return value + 10\n        case _:\n            pass\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("pass fallback match should lower through optional CIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));

        let file = db.add_file(
            "pass-fallback-match-chain-expression.lucid",
            "def choose(value: int):\n    match value:\n        case 1:\n            selected = value + 10\n            return selected\n        case 2:\n            doubled = value * 10\n            return doubled\n        case _:\n            pass\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multi-arm pass fallback match should lower through optional CIR");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm pass fallback match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "optional-match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(20)));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));

        let file = db.add_file(
            "void-match-expression.lucid",
            "def answer(value: int):\n    match value:\n        case 1:\n            return\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("single-arm void match should lower through CIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));

        let file = db.add_file(
            "void-match-chain-expression.lucid",
            "def answer(value: int):\n    match value:\n        case 1:\n            return\n        case 2:\n            return\n        case _:\n            pass\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("multi-arm void match should lower through CIR");
        assert!(
            typed_module(&db, file)
                .as_ref()
                .expect("multi-arm void match should type check")
                .functions[0]
                .body_expressions
                .iter()
                .any(|node| node.kind == "void-match-chain")
        );
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[2]), Ok(None));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));

        let file = db.add_file("void.lucid", "def answer():\n    return\n");
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("bare return should lower to void CIR");
        assert_eq!(function.execute(), Ok(None));

        let file = db.add_file("pass.lucid", "def answer(value: int):\n    pass\n");
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("pass should lower to void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));

        let file = db.add_file(
            "setup-before-pass.lucid",
            "def answer(value: int):\n    temporary = value + 1\n    pass\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("setup before pass should lower to void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "augassign-before-pass.lucid",
            "def answer(value: int):\n    temporary = value\n    temporary += 1\n    pass\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("augmented assignment before pass should lower to void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-branch.lucid",
            "def answer():\n    if true:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant statement branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-elif.lucid",
            "def answer():\n    if false:\n        return 0\n    elif true:\n        return 42\n    else:\n        return 9\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant elif branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-fallthrough.lucid",
            "def answer(value: int):\n    if false:\n        return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant false branch should lower to void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));

        let file = db.add_file(
            "constant-pass-branch.lucid",
            "def answer(value: int):\n    if true:\n        pass\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant pass branch should lower to void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));

        let file = db.add_file(
            "constant-else-pass-branch.lucid",
            "def answer(value: int):\n    if false:\n        return value\n    else:\n        pass\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant else pass branch should lower to void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));

        let file = db.add_file(
            "constant-elif-pass-branch.lucid",
            "def answer(value: int):\n    if false:\n        return value\n    elif true:\n        pass\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant elif pass branch should lower to void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));

        let file = db.add_file(
            "constant-bare-return-branch.lucid",
            "def answer(value: int):\n    if true:\n        return\n    else:\n        return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant bare return branch should lower to void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));

        let file = db.add_file(
            "constant-elif-bare-return-branch.lucid",
            "def answer(value: int):\n    if false:\n        return value\n    elif true:\n        return\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant elif bare return branch should lower to void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));

        let file = db.add_file(
            "constant-else-bare-return-branch.lucid",
            "def answer(value: int):\n    if false:\n        return value\n    else:\n        assert(true)\n        return\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant else bare return branch with no-op prefix should lower to void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));

        let file = db.add_file(
            "constant-setup-before-bare-return-branch.lucid",
            "def answer(value: int):\n    if true:\n        temporary = value + 1\n        return\n    else:\n        return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("setup before selected bare return should lower through void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-nested-setup-before-bare-return-branch.lucid",
            "def answer(value: int):\n    if true:\n        if true:\n            temporary = value + 1\n        return\n    else:\n        return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("nested setup before selected bare return should lower through void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-setup-fallthrough-branch.lucid",
            "def answer(value: int):\n    if true:\n        temporary = value + 1\n    else:\n        return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("setup-only selected branch should lower through void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-nested-setup-fallthrough-branch.lucid",
            "def answer(value: int):\n    if true:\n        if true:\n            temporary = value + 1\n    else:\n        return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("nested setup-only selected branch should lower through void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-elif-setup-fallthrough-branch.lucid",
            "def answer(value: int):\n    if false:\n        return value\n    elif true:\n        temporary = value + 1\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("setup-only selected elif branch should lower through void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-else-setup-fallthrough-branch.lucid",
            "def answer(value: int):\n    if false:\n        return value\n    else:\n        temporary = value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("setup-only selected else branch should lower through void CIR");
        assert_eq!(function.execute_with_args(&[42]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "constant-effectful-fallthrough-branch.lucid",
            "def answer(value: int):\n    if true:\n        str(value)\n    else:\n        return value\n",
        );
        let error = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect_err("effectful fallthrough setup must not be erased");
        assert!(error.contains("effectful discarded expression"));

        let file = db.add_file(
            "dynamic-elif-pass-branch.lucid",
            "def answer(value: int):\n    if false:\n        return value\n    elif value > 0:\n        pass\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("dead leading mixed elif pass branch should lower through optional CIR");
        assert_eq!(function.execute_with_args(&[5]), Ok(None));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(0)));

        let file = db.add_file(
            "constant-unary-branch.lucid",
            "def answer():\n    if not false:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant unary branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-comparison-branch.lucid",
            "def answer():\n    if 1 < 2:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant comparison branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-branch-local-return.lucid",
            "def answer(value: int):\n    if true:\n        selected = value + 1\n        return selected\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant branch locals before return should lower through typed HIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "constant-elif-local-return.lucid",
            "def answer(value: int):\n    if false:\n        return 0\n    elif true:\n        selected = value + 1\n        return selected\n    else:\n        return 9\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant elif locals before return should lower through typed HIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "constant-else-local-return.lucid",
            "def answer(value: int):\n    if false:\n        return 0\n    else:\n        selected = value + 1\n        return selected\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant else locals before return should lower through typed HIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "constant-nested-branch-return.lucid",
            "def answer(value: int):\n    if true:\n        if true:\n            return value + 1\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("nested constant branch return should lower through typed HIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "constant-nested-branch-local-return.lucid",
            "def answer(value: int):\n    if true:\n        selected = value + 1\n        if true:\n            return selected\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("nested constant branch locals before return should lower through typed HIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "dynamic-nested-branch-return.lucid",
            "def answer(value: int):\n    if true:\n        if value > 0:\n            return value + 1\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested branch return should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(None));

        let file = db.add_file(
            "dynamic-nested-branch-else-return.lucid",
            "def answer(value: int):\n    if true:\n        if value > 0:\n            return value + 1\n        else:\n            return -value\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested branch else return should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(41)));

        let file = db.add_file(
                "dynamic-nested-branch-local-return.lucid",
                "def answer(value: int):\n    if true:\n        if value > 0:\n            selected = value + 1\n            return selected\n        else:\n            fallback = -value\n            return fallback\n    else:\n        return 0\n",
            );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested branch local returns should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(41)));

        let file = db.add_file(
            "dynamic-nested-branch-pass-local-return.lucid",
            "def answer(value: int):\n    if true:\n        if value > 0:\n            pass\n            selected = value + 1\n            return selected\n        else:\n            pass\n            return -value\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested pass-padded local returns should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(41)));

        let file = db.add_file(
            "dynamic-nested-branch-assert-local-return.lucid",
            "def answer(value: int):\n    if true:\n        if value > 0:\n            assert(true)\n            selected = value + 1\n            return selected\n        else:\n            assert(true)\n            return -value\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested assert-padded local returns should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(41)));

        let file = db.add_file(
            "dynamic-nested-branch-dead-loop-local-return.lucid",
            "def answer(value: int):\n    if true:\n        if value > 0:\n            while false:\n                return 0\n            selected = value + 1\n            return selected\n        else:\n            for item in []:\n                return 0\n            return -value\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested dead-loop local returns should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(41)));

        let file = db.add_file(
            "dynamic-nested-branch-dead-if-local-return.lucid",
            "def answer(value: int):\n    if true:\n        if value > 0:\n            if false:\n                return 0\n            selected = value + 1\n            return selected\n        else:\n            if true:\n                pass\n            return -value\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested dead-if local returns should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(41)));

        let file = db.add_file(
            "dynamic-nested-elif-branch-else-return.lucid",
            "def answer(value: int):\n    if true:\n        if value > 10:\n            return 100\n        elif value > 0:\n            return 1\n        else:\n            return -1\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested elif branch else return should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-1]), Ok(Some(-1)));

        let file = db.add_file(
            "dynamic-nested-elif-branch-fallthrough.lucid",
            "def answer(value: int):\n    if true:\n        if value > 0:\n            return 1\n        elif value < 0:\n            return -1\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested elif branch fallthrough should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(None));

        let file = db.add_file(
                "dynamic-nested-elif-static-selection.lucid",
                "def answer(value: int):\n    if true:\n        if value > 10:\n            return 100\n        elif false:\n            return 999\n        elif value > 0:\n            return 1\n        elif true:\n            return 2\n        else:\n            return -1\n    else:\n        return 0\n",
            );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected nested elif static arms should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-1]), Ok(Some(2)));

        let file = db.add_file(
                "dynamic-nested-elif-local-return.lucid",
                "def answer(value: int):\n    if true:\n        if value > 10:\n            selected: int = 100\n            return selected\n        elif value > 0:\n            selected = value\n            return selected\n        else:\n            selected = -value\n            return selected\n    else:\n        return 0\n",
            );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested elif local returns should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(41)));

        let file = db.add_file(
                "dynamic-nested-void-branch.lucid",
                "def answer(value: int):\n    if true:\n        if value > 0:\n            return\n        else:\n            pass\n    else:\n        return value\n",
            );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested void branch should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));
        assert_eq!(function.execute_with_args(&[-41]), Ok(None));

        let file = db.add_file(
            "dynamic-nested-pass-void-branch.lucid",
            "def answer(value: int):\n    if true:\n        if value > 0:\n            pass\n            return\n        else:\n            pass\n            pass\n    else:\n        return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested pass-padded void branch should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));
        assert_eq!(function.execute_with_args(&[-41]), Ok(None));

        let file = db.add_file(
            "dynamic-nested-assert-void-branch.lucid",
            "def answer(value: int):\n    if true:\n        if value > 0:\n            assert(true)\n            return\n        else:\n            assert(true)\n            pass\n    else:\n        return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested assert-padded void branch should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));
        assert_eq!(function.execute_with_args(&[-41]), Ok(None));

        let file = db.add_file(
            "dynamic-nested-dead-loop-void-branch.lucid",
            "def answer(value: int):\n    if true:\n        if value > 0:\n            while false:\n                return value\n            return\n        else:\n            for item in []:\n                return value\n    else:\n        return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested dead-loop void branch should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));
        assert_eq!(function.execute_with_args(&[-41]), Ok(None));

        let file = db.add_file(
            "dynamic-nested-dead-if-void-branch.lucid",
            "def answer(value: int):\n    if true:\n        if value > 0:\n            if false:\n                return value\n            return\n        else:\n            if true:\n                pass\n    else:\n        return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested dead-if void branch should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));
        assert_eq!(function.execute_with_args(&[-41]), Ok(None));

        let file = db.add_file(
            "dynamic-nested-void-elif-branch.lucid",
            "def answer(value: int):\n    if true:\n        if value > 10:\n            return\n        elif value > 0:\n            pass\n        else:\n            return\n    else:\n        return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested void elif branch should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[-1]), Ok(None));

        let file = db.add_file(
                "dynamic-nested-void-elif-fallthrough.lucid",
                "def answer(value: int):\n    if true:\n        if value > 10:\n            return\n        elif value > 0:\n            pass\n    else:\n        return value\n",
            );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested void elif fallthrough should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[-1]), Ok(None));

        let file = db.add_file(
                "dynamic-nested-void-elif-static-selection.lucid",
                "def answer(value: int):\n    if true:\n        if value > 10:\n            return\n        elif false:\n            return value\n        elif value > 0:\n            pass\n        elif true:\n            return\n        else:\n            return value\n    else:\n        return value\n",
            );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected nested void elif static arms should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[-1]), Ok(None));

        let file = db.add_file(
                "dynamic-nested-void-then-value-else.lucid",
                "def answer(value: int):\n    if true:\n        if value > 0:\n            pass\n        else:\n            return -value\n    else:\n        return 0\n",
            );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected dynamic nested void/value branch should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(41)));

        let file = db.add_file(
            "constant-string-branch.lucid",
            "def answer():\n    if \"lucid\" == \"lucid\":\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant string branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "short-circuit-branch.lucid",
            "def answer(value: int):\n    if false and value > 0:\n        return 0\n    else:\n        return 42\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("short-circuit branch should fold without lowering its RHS");
        assert_eq!(function.execute_with_args(&[99]), Ok(Some(42)));

        let file = db.add_file(
            "short-circuit-or-branch.lucid",
            "def answer(value: int):\n    if true or value > 0:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("short-circuit or branch should fold without lowering its RHS");
        assert_eq!(function.execute_with_args(&[-99]), Ok(Some(42)));

        let file = db.add_file(
            "constant-arithmetic-branch.lucid",
            "def answer():\n    if 1 + 1 < 3:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant arithmetic branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-division-branch.lucid",
            "def answer():\n    if 8 // 2 == 4:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant division branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-negative-mod-branch.lucid",
            "def answer():\n    if -5 % 2 == 1:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("negative modulo branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-bigint-branch.lucid",
            "def answer():\n    if 123456789012345678901234567890 == 123456789012345678901234567890:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("BigInt comparison branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-bigint-negation-branch.lucid",
            "def answer():\n    if -0x10000000000000000 == -18446744073709551616:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("BigInt negation branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-bigint-power-branch.lucid",
            "def answer():\n    if 2 ** 100 == 1267650600228229401496703205376:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("BigInt power branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-bigint-mod-branch.lucid",
            "def answer():\n    if 123456789012345678901 % 2 == 1:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("BigInt modulo branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-bigint-bitwise-branch.lucid",
            "def answer():\n    if 0x10000000000000000 | 1 == 18446744073709551617:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("BigInt bitwise branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-bigint-floor-branch.lucid",
            "def answer():\n    if -123456789012345678901 // 2 == -61728394506172839451:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("BigInt floor division branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-bigint-invert-branch.lucid",
            "def answer():\n    if ~0x10000000000000000 == -18446744073709551617:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("BigInt invert branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-bigint-plus-branch.lucid",
            "def answer():\n    if +0x10000000000000000 == 18446744073709551616:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("BigInt unary-plus branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-shift-branch.lucid",
            "def answer():\n    if (1 << 2) == 4:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant shift branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-invert-branch.lucid",
            "def answer():\n    if ~0 == -1:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant invert branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-power-branch.lucid",
            "def answer():\n    if 2 ** 3 == 8:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("constant power branch should lower through typed HIR");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-power-overflow-branch.lucid",
            "def answer():\n    if 2 ** 63 == 0:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("BigInt power should fold without i64 overflow");
        assert_eq!(function.execute(), Ok(Some(0)));

        let file = db.add_file(
            "constant-invalid-division-branch.lucid",
            "def answer():\n    if 1 // 0 == 0:\n        return 42\n    else:\n        return 0\n",
        );
        let error = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect_err("division by zero must not be folded or trapped");
        assert!(error.contains("constant function branch"));

        let file = db.add_file(
            "constant-unselected-invalid-else.lucid",
            "def answer():\n    if true:\n        return 42\n    else:\n        return 1 // 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("unselected invalid else branch must not be evaluated");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "constant-unselected-invalid-then.lucid",
            "def answer():\n    if false:\n        return 1 // 0\n    else:\n        return 42\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("unselected invalid then branch must not be evaluated");
        assert_eq!(function.execute(), Ok(Some(42)));

        let file = db.add_file(
            "parameterized.lucid",
            "def answer(value: int):\n    return value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("parameterized body should lower through typed HIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "parameterized-conditional.lucid",
            "def answer(value: int):\n    if value > 0:\n        return value + 1\n    else:\n        return -value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("dynamic parameter conditional should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(41)));

        let file = db.add_file(
            "parameterized-conditional-division.lucid",
            "def choose(flag: int, left: int, right: int):\n    if flag > 0:\n        return left / right\n    else:\n        return 7\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("conditional division should lower through direct CIR returns");
        assert!(matches!(
            function.blocks[1].terminator,
            lucid_cir::Terminator::Return(Some(_))
        ));

        let file = db.add_file(
            "parameterized-conditional-expression.lucid",
            "def choose(value: int):\n    return value + 1 if value > 0 else -value\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("conditional return expression should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(41)));

        let file = db.add_file(
            "parameterized-static-elif.lucid",
            "def choose(value: int):\n    if value > 0:\n        return value + 1\n    elif true:\n        return 7\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("dynamic condition with static elif should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(7)));

        let file = db.add_file(
            "parameterized-multiple-static-elif.lucid",
            "def choose(value: int):\n    if value > 0:\n        return value + 1\n    elif false:\n        return 3\n    elif true:\n        return 7\n    else:\n        return 9\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multiple static elif branches should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(7)));

        let file = db.add_file(
            "parameterized-dynamic-elif.lucid",
            "def choose(value: int):\n    if value > 0:\n        return 1\n    elif value < 0:\n        return -1\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("single dynamic elif chain should lower through direct CIR branches");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(0)));

        let file = db.add_file(
            "parameterized-multiple-dynamic-elif.lucid",
            "def choose(value: int):\n    if value > 10:\n        return 100\n    elif value > 0:\n        return 1\n    elif value < 0:\n        return -1\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("multiple dynamic elif branches should lower through a CIR ladder");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(0)));

        let file = db.add_file(
            "parameterized-mixed-static-false-dynamic-elif.lucid",
            "def choose(value: int):\n    if value > 10:\n        return 100\n    elif false:\n        return 999\n    elif value > 0:\n        return 1\n    else:\n        return -1\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("static false elif inside dynamic return chain should be skipped");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));

        let file = db.add_file(
            "parameterized-mixed-static-false-optional-elif.lucid",
            "def choose(value: int):\n    if value > 0:\n        return 1\n    elif false:\n        return 999\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("static false optional elif inside dynamic return chain should be skipped");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(None));

        let file = db.add_file(
            "parameterized-dead-leading-dynamic-elif.lucid",
            "def choose(value: int):\n    if false:\n        return 0\n    elif value > 0:\n        return 1\n    else:\n        return -1\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("dead leading branch before dynamic elif returns should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));

        let file = db.add_file(
            "parameterized-dead-leading-multiple-dynamic-elif.lucid",
            "def choose(value: int):\n    if false:\n        return 0\n    elif value > 10:\n        return 100\n    elif value > 0:\n        return 1\n    else:\n        return -1\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("dead leading branch before dynamic elif return chain should lower");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));

        let file = db.add_file(
            "parameterized-dead-leading-optional-dynamic-elif.lucid",
            "def choose(value: int):\n    if false:\n        return 0\n    elif value > 0:\n        return 1\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("dead leading branch before optional dynamic elif return should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(None));

        let file = db.add_file(
            "parameterized-false-elif-fallthrough.lucid",
            "def maybe(value: int):\n    if value > 0:\n        return value + 1\n    elif false:\n        return 7\n",
        );
        let function = lower_function_body(&db, file, "maybe".into())
            .as_ref()
            .expect("false elif without else should lower to void fall-through");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(None));

        let file = db.add_file(
            "parameterized-dynamic-elif-fallthrough.lucid",
            "def maybe(value: int):\n    if value > 10:\n        return 100\n    elif value > 0:\n        return 1\n    elif value < 0:\n        return -1\n",
        );
        let function = lower_function_body(&db, file, "maybe".into())
            .as_ref()
            .expect("dynamic elif chain without else should lower to optional CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(None));

        let file = db.add_file(
            "parameterized-initialized-local-pass-then-no-else-elif.lucid",
            "def choose(value: int):\n    result = 0\n    if value > 10:\n        pass\n    elif value > 0:\n        result = 1\n    elif value < 0:\n        result = -1\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized branch-local pass then no-else elif should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(0)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(0)));

        let file = db.add_file(
            "parameterized-initialized-local-pass-middle-no-else-elif.lucid",
            "def choose(value: int):\n    result = 0\n    if value > 10:\n        result = 100\n    elif value > 0:\n        pass\n    elif value < 0:\n        result = -1\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized branch-local pass middle no-else elif should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(0)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(0)));

        let file = db.add_file(
            "parameterized-local-conditional.lucid",
            "def choose(value: int):\n    if value > 0:\n        result = value + 1\n    else:\n        result = -value\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("branch-local assignment should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(41)));

        let file = db.add_file(
            "parameterized-local-pass-conditional.lucid",
            "def choose(value: int):\n    if value > 0:\n        result = value + 1\n        pass\n    else:\n        pass\n        result = -value\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("pass around branch-local assignment should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(Some(41)));

        let file = db.add_file(
            "parameterized-local-elif.lucid",
            "def choose(value: int):\n    if value > 10:\n        result = 100\n    elif value > 0:\n        pass\n        result = 1\n    elif value < 0:\n        result = -1\n        pass\n    else:\n        result = 0\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("branch-local elif assignment should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(0)));

        let file = db.add_file(
            "parameterized-initialized-local-elif.lucid",
            "def choose(value: int):\n    result = 0\n    if value > 10:\n        result = 100\n    elif value > 0:\n        result = 1\n    elif value < 0:\n        result = -1\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized branch-local elif assignment should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(0)));

        let file = db.add_file(
            "parameterized-initialized-local-else-elif.lucid",
            "def choose(value: int):\n    result = 0\n    if value > 10:\n        result = 100\n    elif value > 0:\n        result = 1\n    elif value < 0:\n        result = -1\n    else:\n        result = -100\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized branch-local else elif assignment should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(-100)));

        let file = db.add_file(
            "parameterized-initialized-local-dead-leading-elif.lucid",
            "def choose(value: int):\n    result = 0\n    if false:\n        result = 100\n    elif value > 0:\n        result = 1\n    else:\n        result = -100\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized local dead leading elif should lower through CIR");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-100)));

        let file = db.add_file(
            "parameterized-initialized-local-dead-leading-pass-elif.lucid",
            "def choose(value: int):\n    result = 0\n    if false:\n        result = 100\n    elif value > 10:\n        result = 10\n    elif value > 0:\n        pass\n    else:\n        result = -100\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized local dead leading pass elif should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(10)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(0)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-100)));

        let file = db.add_file(
            "parameterized-initialized-local-pass-then-elif.lucid",
            "def choose(value: int):\n    result = 0\n    if value > 10:\n        pass\n    elif value > 0:\n        result = 1\n    elif value < 0:\n        result = -1\n    else:\n        result = -100\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized branch-local pass then elif should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(0)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(-100)));

        let file = db.add_file(
            "parameterized-initialized-local-pass-middle-elif.lucid",
            "def choose(value: int):\n    result = 0\n    if value > 10:\n        result = 100\n    elif value > 0:\n        pass\n    elif value < 0:\n        result = -1\n    else:\n        result = -100\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized branch-local pass middle elif should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(0)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(-100)));

        let file = db.add_file(
            "parameterized-initialized-local-pass-else-elif.lucid",
            "def choose(value: int):\n    result = 0\n    if value > 10:\n        result = 100\n    elif value > 0:\n        result = 1\n    elif value < 0:\n        result = -1\n    else:\n        pass\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized branch-local pass else elif should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(0)));

        let file = db.add_file(
            "parameterized-initialized-local-conditional.lucid",
            "def choose(value: int):\n    result = 0\n    if value > 0:\n        result = 1\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized branch-local conditional should lower through CIR");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(0)));

        let file = db.add_file(
            "parameterized-initialized-local-else-conditional.lucid",
            "def choose(value: int):\n    result = 0\n    if value > 0:\n        result = 1\n    else:\n        result = -1\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized branch-local else assignment should lower through CIR");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(-1)));

        let file = db.add_file(
            "parameterized-initialized-local-pass-else-conditional.lucid",
            "def choose(value: int):\n    result = 0\n    if value > 0:\n        result = 1\n    else:\n        pass\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized branch-local pass else should lower through CIR");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(0)));

        let file = db.add_file(
            "parameterized-initialized-local-pass-then-conditional.lucid",
            "def choose(value: int):\n    result = 0\n    if value > 0:\n        pass\n    else:\n        result = -1\n    return result\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("initialized branch-local pass then should lower through CIR");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(0)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(-1)));

        let file = db.add_file(
            "parameterized-void-conditional.lucid",
            "def answer(value: int):\n    if value > 0:\n        return\n    else:\n        return\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("dynamic void conditional should lower through CIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[-1]), Ok(None));

        let file = db.add_file(
            "parameterized-optional-void-conditional.lucid",
            "def answer(value: int):\n    if value > 0:\n        return\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("optional dynamic void conditional should lower through CIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[-1]), Ok(None));

        let file = db.add_file(
            "parameterized-optional-pass-void-conditional.lucid",
            "def answer(value: int):\n    if value > 0:\n        pass\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("optional dynamic pass conditional should lower through CIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[-1]), Ok(None));

        let file = db.add_file(
            "parameterized-pass-void-conditional.lucid",
            "def answer(value: int):\n    if value > 0:\n        return\n    else:\n        pass\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("pass dynamic void conditional should lower through CIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[-1]), Ok(None));

        let file = db.add_file(
            "parameterized-then-pass-void-conditional.lucid",
            "def answer(value: int):\n    if value > 0:\n        pass\n    else:\n        return\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("then-pass dynamic void conditional should lower through CIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[-1]), Ok(None));

        let file = db.add_file(
            "parameterized-void-elif.lucid",
            "def answer(value: int):\n    if value > 10:\n        return\n    elif value > 0:\n        return\n    elif value < 0:\n        return\n    else:\n        return\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("dynamic void elif chain should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(None));
        assert_eq!(function.execute_with_args(&[5]), Ok(None));
        assert_eq!(function.execute_with_args(&[-5]), Ok(None));
        assert_eq!(function.execute_with_args(&[0]), Ok(None));

        let file = db.add_file(
            "parameterized-optional-void-elif.lucid",
            "def answer(value: int):\n    if value > 10:\n        return\n    elif value > 0:\n        return\n    elif value < 0:\n        return\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("optional dynamic void elif chain should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(None));
        assert_eq!(function.execute_with_args(&[5]), Ok(None));
        assert_eq!(function.execute_with_args(&[-5]), Ok(None));
        assert_eq!(function.execute_with_args(&[0]), Ok(None));

        let file = db.add_file(
            "parameterized-pass-void-elif.lucid",
            "def answer(value: int):\n    if value > 10:\n        return\n    elif value > 0:\n        return\n    elif value < 0:\n        return\n    else:\n        pass\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("pass dynamic void elif chain should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(None));
        assert_eq!(function.execute_with_args(&[5]), Ok(None));
        assert_eq!(function.execute_with_args(&[-5]), Ok(None));
        assert_eq!(function.execute_with_args(&[0]), Ok(None));

        let file = db.add_file(
            "parameterized-static-false-void-elif.lucid",
            "def answer(value: int):\n    if value > 10:\n        return\n    elif false:\n        return\n    elif value > 0:\n        pass\n    else:\n        return\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("static false void elif inside dynamic chain should be skipped");
        assert_eq!(function.execute_with_args(&[15]), Ok(None));
        assert_eq!(function.execute_with_args(&[5]), Ok(None));
        assert_eq!(function.execute_with_args(&[-5]), Ok(None));

        let file = db.add_file(
            "parameterized-static-false-optional-void-elif.lucid",
            "def answer(value: int):\n    if value > 0:\n        return\n    elif false:\n        return\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("static false optional void elif inside dynamic chain should be skipped");
        assert_eq!(function.execute_with_args(&[5]), Ok(None));
        assert_eq!(function.execute_with_args(&[-5]), Ok(None));

        let file = db.add_file(
            "parameterized-mixed-pass-void-elif.lucid",
            "def answer(value: int):\n    if value > 10:\n        pass\n    elif value > 0:\n        return\n    elif value < 0:\n        pass\n    else:\n        return\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("mixed pass dynamic void elif chain should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(None));
        assert_eq!(function.execute_with_args(&[5]), Ok(None));
        assert_eq!(function.execute_with_args(&[-5]), Ok(None));
        assert_eq!(function.execute_with_args(&[0]), Ok(None));

        let file = db.add_file(
            "parameterized-boolean-conditional.lucid",
            "def answer(value: int):\n    if value > 0 and value < 10:\n        return 42\n    else:\n        return 0\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("boolean parameter conditional should lower through CIR");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(0)));

        let file = db.add_file(
            "parameterized-optional-conditional.lucid",
            "def answer(value: int):\n    if value > 0:\n        return value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("one-sided parameter conditional should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(None));

        let file = db.add_file(
            "parameterized-pass-conditional.lucid",
            "def maybe(value: int):\n    if value > 0:\n        return value + 1\n    else:\n        pass\n",
        );
        let function = lower_function_body(&db, file, "maybe".into())
            .as_ref()
            .expect("pass branch should lower through the mixed result ABI");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(None));

        let file = db.add_file(
            "parameterized-bare-return-conditional.lucid",
            "def maybe(value: int):\n    if value > 0:\n        return value + 1\n    else:\n        return\n",
        );
        let function = lower_function_body(&db, file, "maybe".into())
            .as_ref()
            .expect("bare return branch should lower through the mixed result ABI");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[-41]), Ok(None));

        let file = db.add_file(
            "parameterized-pass-elif.lucid",
            "def maybe(value: int):\n    if value > 10:\n        return 100\n    elif value > 0:\n        return 1\n    elif value < 0:\n        return -1\n    else:\n        pass\n",
        );
        let function = lower_function_body(&db, file, "maybe".into())
            .as_ref()
            .expect("pass fallback after dynamic elif chain should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(None));

        let file = db.add_file(
            "parameterized-bare-return-elif.lucid",
            "def maybe(value: int):\n    if value > 10:\n        return 100\n    elif value > 0:\n        return 1\n    elif value < 0:\n        return -1\n    else:\n        return\n",
        );
        let function = lower_function_body(&db, file, "maybe".into())
            .as_ref()
            .expect("bare return fallback after dynamic elif chain should lower through CIR");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(100)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(-1)));
        assert_eq!(function.execute_with_args(&[0]), Ok(None));

        let file = db.add_file(
            "temporary.lucid",
            "def answer(value: int):\n    doubled = value * 2\n    return doubled\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a temporary followed by return should lower through SSA");
        assert_eq!(function.execute_with_args(&[21]), Ok(Some(42)));

        let file = db.add_file(
            "declared-temporary.lucid",
            "def answer(value: int):\n    doubled: int = value * 2\n    return doubled\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a declared temporary should lower through SSA");
        assert_eq!(function.execute_with_args(&[21]), Ok(Some(42)));

        let file = db.add_file(
            "reassigned-parameter.lucid",
            "def answer(value: int):\n    value = value + 1\n    return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a reassigned parameter should lower through SSA");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "chained-locals.lucid",
            "def answer(value: int):\n    doubled = value * 2\n    result = doubled + 1\n    return result\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("chained locals should lower through SSA");
        assert_eq!(function.execute_with_args(&[20]), Ok(Some(41)));

        let file = db.add_file(
            "local-direct-return.lucid",
            "def answer(value: int):\n    doubled = value * 2\n    return doubled + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a direct expression return should lower through typed SSA");
        assert_eq!(function.execute_with_args(&[20]), Ok(Some(41)));

        let file = db.add_file(
            "pass-before-return.lucid",
            "def answer(value: int):\n    pass\n    return value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("pass before a return should lower through typed SSA");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "dead-if-before-return.lucid",
            "def answer(value: int):\n    if false:\n        value = 0\n    return value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a statically dead if should be skipped during lowering");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "selected-if-before-return.lucid",
            "def answer(value: int):\n    if true:\n        result = value * 2\n    return result + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a statically selected if assignment should lower");
        assert_eq!(function.execute_with_args(&[20]), Ok(Some(41)));

        let file = db.add_file(
            "selected-elif-before-return.lucid",
            "def answer(value: int):\n    if false:\n        result = 0\n    elif true:\n        result = value * 2\n    return result + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a statically selected elif assignment should lower");
        assert_eq!(function.execute_with_args(&[20]), Ok(Some(41)));

        let file = db.add_file(
            "selected-else-before-return.lucid",
            "def answer(value: int):\n    if false:\n        result = 0\n    else:\n        result = value * 2\n    return result + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a statically selected else assignment should lower");
        assert_eq!(function.execute_with_args(&[20]), Ok(Some(41)));

        let file = db.add_file(
            "selected-branch-locals-before-return.lucid",
            "def answer(value: int):\n    if true:\n        doubled = value * 2\n        result = doubled + 1\n    return result\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("multiple selected branch assignments should lower");
        assert_eq!(function.execute_with_args(&[20]), Ok(Some(41)));

        let file = db.add_file(
            "nested-selected-branch-locals-before-return.lucid",
            "def answer(value: int):\n    if true:\n        if true:\n            result = value * 2\n    return result + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("nested statically selected branch assignments should lower");
        assert_eq!(function.execute_with_args(&[20]), Ok(Some(41)));

        let file = db.add_file(
            "dynamic-elif-before-return.lucid",
            "def answer(value: int):\n    if false:\n        result = 0\n    elif value > 0:\n        result = 1\n    else:\n        result = 2\n    return result\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("dead leading branch before dynamic elif assignment should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(2)));

        let file = db.add_file(
            "dynamic-multiple-elif-before-return.lucid",
            "def answer(value: int):\n    if false:\n        result = 0\n    elif value > 10:\n        result = 10\n    elif value > 0:\n        result = 1\n    else:\n        result = 2\n    return result\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("dead leading branch before dynamic elif assignment chain should lower");
        assert_eq!(function.execute_with_args(&[15]), Ok(Some(10)));
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(1)));
        assert_eq!(function.execute_with_args(&[-5]), Ok(Some(2)));

        let file = db.add_file(
            "dead-while-before-return.lucid",
            "def answer(value: int):\n    while false:\n        value = 0\n    return value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a statically dead while should be skipped during lowering");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "empty-for-before-return.lucid",
            "def answer(value: int):\n    for item in []:\n        value = 0\n    return value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("an empty for should be skipped during lowering");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "empty-range-before-return.lucid",
            "def answer(value: int):\n    for item in range(0):\n        value = 0\n    return value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("an empty range should be skipped during lowering");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "constant-function-loop.lucid",
            "def answer():\n    value = 0\n    for item in [1, 2, 3]:\n        value = value + item\n    return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a parameter-free constant loop should lower through CIR");
        assert_eq!(function.execute(), Ok(Some(6)));

        let file = db.add_file(
            "constant-function-set-loop.lucid",
            "def answer():\n    value = 0\n    for item in {1, 2, 3}:\n        value = value + item\n    return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a parameter-free constant set loop should lower through CIR");
        assert_eq!(function.execute(), Ok(Some(6)));

        let file = db.add_file(
            "constant-function-dict-loop.lucid",
            "def answer():\n    value = 0\n    for item in {1: 2, 3: 4}:\n        value = value + item\n    return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a parameter-free constant dictionary loop should iterate keys");
        assert_eq!(function.execute(), Ok(Some(4)));

        let file = db.add_file(
            "constant-function-range-loop.lucid",
            "def answer():\n    value = 0\n    for item in range(1, 4):\n        value = value + item\n    return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a parameter-free constant range should lower through CIR");
        assert_eq!(function.execute(), Ok(Some(6)));

        let file = db.add_file(
            "parameterized-constant-loop.lucid",
            "def answer(value: int):\n    for item in [1, 2]:\n        value = value + item\n    return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("parameterized constant loops should seed CIR parameters");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(44)));

        let file = db.add_file(
            "parameterized-constant-range-loop.lucid",
            "def answer(value: int):\n    for item in range(1, 4):\n        value = value + item\n    return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("parameterized constant ranges should reuse CIR unrolling");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(47)));

        let file = db.add_file(
            "parameterized-arithmetic-range-loop.lucid",
            "def answer(value: int):\n    for item in range(1 + 1, 2 * 3, 1 << 1):\n        value = value + item\n    return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("arithmetic constant ranges should lower through the database");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(47)));

        let file = db.add_file(
            "parameterized-constant-set-loop.lucid",
            "def answer(value: int):\n    for item in {1, 2, 3}:\n        value = value + item\n    return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("parameterized constant sets should reuse CIR unrolling");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(47)));

        let file = db.add_file(
            "parameterized-constant-dict-loop.lucid",
            "def answer(value: int):\n    for item in {1: 2, 3: 4}:\n        value = value + item\n    return value\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("parameterized constant dictionaries should iterate keys");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(45)));

        let file = db.add_file(
            "empty-loop-if-broken.lucid",
            "def answer(value: int):\n    for item in []:\n        value = 1\n    if_broken:\n        value = 2\n    return value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("empty loop if_broken should not run in function lowering");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "false-while-if-broken.lucid",
            "def answer(value: int):\n    while false:\n        value = 1\n    if_broken:\n        value = 2\n    return value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("statically false while if_broken should not run in function lowering");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "true-assert-before-return.lucid",
            "def answer(value: int):\n    assert(true)\n    return value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a statically true assertion should be skipped during lowering");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "pure-discard-before-pass.lucid",
            "def answer(value: int):\n    value + 1\n    pass\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a pure discarded expression before pass should be skipped during lowering");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));

        let file = db.add_file(
            "effectful-discard-before-pass.lucid",
            "def answer(value: int):\n    str(value)\n    pass\n",
        );
        let error = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect_err("effectful discarded expression before pass must not be erased");
        assert!(error.contains("effectful discarded expression"));

        let file = db.add_file(
            "empty-descending-range-before-return.lucid",
            "def answer(value: int):\n    for item in range(0, 1, -1):\n        value = 0\n    return value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("an empty descending range should be skipped during lowering");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "local-rebinding-order.lucid",
            "def answer():\n    x = 1\n    y = x + 1\n    x = 3\n    return y\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("local rebinding should preserve definition order");
        assert_eq!(function.execute(), Ok(Some(2)));

        let file = db.add_file("multi.lucid", "def answer():\n    1\n    return 2\n");
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("pure discarded expressions should not block CIR lowering");
        assert_eq!(function.execute(), Ok(Some(2)));
    }

    #[test]
    fn database_lowers_sequential_function_bindings_from_typed_hir() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "sequential.lucid",
            "def answer(seed: int):\n    first = seed + 2\n    second = first * 3\n    return second - 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("sequential bindings should lower through typed HIR");
        assert_eq!(function.execute_with_args(&[4]), Ok(Some(17)));
    }

    #[test]
    fn database_lowers_pure_discarded_expression_before_return() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "pure-discarded.lucid",
            "def answer(value: int):\n    value + 1\n    return value * 2\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("pure discarded expression should lower through CIR");
        assert_eq!(function.execute_with_args(&[21]), Ok(Some(42)));
    }

    #[test]
    fn database_rejects_effectful_discarded_expression_before_return() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "effectful-discarded.lucid",
            "def answer(value: int):\n    str(value)\n    return value * 2\n",
        );
        let error = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect_err("effectful discarded expression must not be erased");
        assert!(error.contains("effectful discarded expression"));
    }

    #[test]
    fn database_accepts_pure_expression_in_selected_branch() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "pure-branch-expression.lucid",
            "def answer(value: int):\n    if true:\n        value + 1\n    return value * 2\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("pure branch expression should be ignored safely");
        assert_eq!(function.execute_with_args(&[21]), Ok(Some(42)));
    }

    #[test]
    fn database_accepts_proven_noops_in_selected_branch() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "selected-branch-noops.lucid",
            "def answer(value: int):\n    if true:\n        assert(true)\n        while false:\n            value = 0\n        for item in []:\n            value = 0\n        if false:\n            value = 0\n        result = value + 1\n    return result\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("proven no-op statements in a selected branch should not block CIR lowering");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
    }

    #[test]
    fn database_lowers_bindings_before_bare_return_as_void() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "bare-return-after-binding.lucid",
            "def discard(value: int):\n    temporary = value + 1\n    return\n",
        );
        let function = lower_function_body(&db, file, "discard".into())
            .as_ref()
            .expect("pure bindings before a bare return should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );

        let file = db.add_file(
            "bare-return-after-nested-static-binding.lucid",
            "def discard(value: int):\n    if true:\n        if true:\n            temporary = value + 1\n    return\n",
        );
        let function = lower_function_body(&db, file, "discard".into())
            .as_ref()
            .expect("nested static bindings before a bare return should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, lucid_cir::Instruction::Add { .. }))
        );
    }

    #[test]
    fn database_lowers_pure_discarded_expression_before_bare_return() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "pure-discarded-before-bare-return.lucid",
            "def discard(value: int):\n    value + 1\n    return\n",
        );
        let function = lower_function_body(&db, file, "discard".into())
            .as_ref()
            .expect("pure discarded expression before bare return should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(None));
    }

    #[test]
    fn database_lowers_straight_line_augmented_assignment_through_cir() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "straight-line-augassign.lucid",
            "def answer(value: int):\n    total = value\n    total += 1\n    return total\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("straight-line augmented assignment should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "selected-branch-augassign.lucid",
            "def answer(value: int):\n    total = value\n    if true:\n        total += 1\n    return total\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("selected-branch augmented assignment should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));

        let file = db.add_file(
            "nested-selected-branch-augassign.lucid",
            "def answer(value: int):\n    total = value\n    if true:\n        if true:\n            total += 1\n    return total\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("nested selected-branch augmented assignment should lower through CIR");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
    }

    #[test]
    fn database_lowers_statement_conditional_returns_from_typed_hir() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "statement-if.lucid",
            "def choose(flag: bool):\n    if flag:\n        return 11\n    else:\n        return 22\n",
        );
        let function = lower_function_body(&db, file, "choose".into())
            .as_ref()
            .expect("statement conditional returns should lower through typed HIR");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(22)));
    }

    #[test]
    fn database_lowers_unambiguous_dispatch_body_through_cir() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "single-dispatch.lucid",
            "dispatch def answer(value: int) -> int:\n    return value + 1\n",
        );
        let function = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect("a single dispatch overload has an unambiguous body to lower");
        assert_eq!(function.execute_with_args(&[41]), Ok(Some(42)));
    }

    #[test]
    fn database_rejects_dispatch_overload_set_without_selected_candidate() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "dispatch-overloads.lucid",
            "dispatch def answer(value: int) -> int:\n    return 1\n\ndispatch def answer(value: bool) -> int:\n    return 0\n",
        );
        let error = lower_function_body(&db, file, "answer".into())
            .as_ref()
            .expect_err("an overload set needs a selected candidate before CIR lowering");
        assert!(error.contains("selected overload"));
    }

    #[test]
    fn database_lowers_parameter_counted_while_through_cir() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "counted-while.lucid",
            "def countdown(n: int):\n    while n > 0:\n        n -= 1\n    return n\n",
        );
        let function = lower_function_body(&db, file, "countdown".into())
            .as_ref()
            .expect("counted while should lower through CIR");
        assert_eq!(function.execute_with_args(&[4]), Ok(Some(0)));

        let file = db.add_file(
            "counted-while-local.lucid",
            "def countdown(n: int):\n    value = n\n    while value > 0:\n        value -= 1\n    return value\n",
        );
        let function = lower_function_body(&db, file, "countdown".into())
            .as_ref()
            .expect("local-init counted while should lower through CIR");
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(0)));

        let file = db.add_file(
            "range-accumulate.lucid",
            "def sum_to(n: int):\n    total = 0\n    for i in range(n):\n        total += i\n    return total\n",
        );
        let function = lower_function_body(&db, file, "sum_to".into())
            .as_ref()
            .expect("range accumulation should lower through CIR");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(10)));

        let file = db.add_file(
            "void-counted-while.lucid",
            "def drain(n: int):\n    while n > 0:\n        n -= 1\n",
        );
        let function = lower_function_body(&db, file, "drain".into())
            .as_ref()
            .expect("void counted while should lower through CIR");
        assert_eq!(function.execute_with_args(&[2]), Ok(None));
    }

    #[test]
    fn database_lowering_rejects_type_errors_before_cir() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "value: int = \"wrong\"\n");
        let error = lower_first_assignment(&db, file)
            .as_ref()
            .expect_err("invalid source must not lower");
        assert!(!error.is_empty());
    }

    #[test]
    fn top_level_symbols_are_interned_per_source_file() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "main.lucid",
            "class Point:\n    x: int\ndef make():\n    return none\n",
        );
        let symbols = top_level_symbols(&db, file);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name(&db), "Point");
        assert!(*symbols[0].file(&db) == file);
        assert_eq!(symbols[1].name(&db), "make");
    }

    #[test]
    fn type_check_query_tracks_source_changes() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "value: int = 42\n");
        assert!(type_check_file(&db, file).is_ok());

        file.set_text(&mut db).to("value: int = \"wrong\"\n".into());
        let error = type_check_file(&db, file)
            .as_ref()
            .expect_err("changed source should invalidate semantic result");
        assert!(!error.is_empty());
    }

    #[test]
    fn name_resolution_returns_stable_symbol_identity() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "export class Point:\n    x: int\n");
        let symbol = resolve_top_level(&db, file, "Point".into()).expect("Point should resolve");
        assert_eq!(symbol.name(&db), "Point");
        assert!(resolve_top_level(&db, file, "Missing".into()).is_none());

        file.set_text(&mut db)
            .to("class Other:\n    x: int\n".into());
        assert!(resolve_top_level(&db, file, "Point".into()).is_none());
        assert!(resolve_top_level(&db, file, "Other".into()).is_some());
    }

    #[test]
    fn resolved_declarations_record_kind_and_visibility() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "main.lucid",
            "export class Point:\n    x: int\n\ndef helper():\n    return none\n",
        );
        let declarations = resolved_declarations(&db, file);
        assert_eq!(declarations.len(), 2);
        assert_eq!(declarations[0].kind, DeclKind::Class);
        assert!(declarations[0].exported);
        assert_eq!(declarations[1].kind, DeclKind::Function);
        assert!(!declarations[1].is_dispatch);
        assert!(declarations[1].exported);
    }

    #[test]
    fn resolved_module_bundles_stable_declarations_and_imports() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "export value = 1\nimport support\n");
        let module = resolved_module(&db, file);
        assert_eq!(module.file, file);
        assert_eq!(module.imports.as_ref(), &["support".to_string()]);
        assert_eq!(module.declarations.len(), 1);
        assert!(module.declarations[0].exported);
        assert_eq!(module.declarations[0].symbol.name(&db), "value");
    }

    #[test]
    fn module_imports_are_sorted_and_resolve_within_project() {
        let mut db = CompilerDatabase::default();
        let main = db.add_file(
            "main.lucid",
            "import util.z\nimport util.a\nfrom util.z import Thing\n",
        );
        let util_a = db.add_file("util/a.lucid", "value = 1\n");
        let util_z = db.add_file("util/z.lucid", "class Thing:\n    pass\n");
        let project = Project::new(&db, vec![main, util_a, util_z]);
        assert_eq!(imports(&db, main).as_ref(), &["util.a", "util.z"]);
        assert!(resolve_module(&db, project, "util.z".into()).is_some_and(|file| file == util_z));
        assert!(resolve_module(&db, project, "missing".into()).is_none());
    }

    #[test]
    fn plain_import_aliases_resolve_as_module_bindings() {
        let mut db = CompilerDatabase::default();
        let main = db.add_file("main.lucid", "import util.z as zmod\n");
        let util = db.add_file("util/z.lucid", "value = 1\n");
        let project = Project::new(&db, vec![main, util]);
        let binding = resolve_visible(&db, project, main, "zmod".into()).expect("module alias");
        assert_eq!(*binding.file(&db), util);
        assert_eq!(binding.name(&db), "zmod");
    }

    #[test]
    fn relative_imports_resolve_from_importing_file_package() {
        let mut db = CompilerDatabase::default();
        let main = db.add_file("pkg/main.lucid", "from .reports import build\n");
        let reports = db.add_file("pkg/reports.lucid", "build = 1\n");
        let project = Project::new(&db, vec![main, reports]);
        assert_eq!(
            *resolve_import(&db, project, main, ".reports".into()),
            Some(reports)
        );
        assert_eq!(
            module_order(&db, project).as_ref().unwrap().as_ref(),
            &[reports, main]
        );
    }

    #[test]
    fn package_initializers_resolve_as_package_modules() {
        let mut db = CompilerDatabase::default();
        let main = db.add_file("main.lucid", "import pkg\n");
        let package = db.add_file("pkg/__init__.lucid", "value = 1\n");
        let project = Project::new(&db, vec![main, package]);
        assert_eq!(*resolve_module(&db, project, "pkg".into()), Some(package));
        assert_eq!(
            module_order(&db, project).as_ref().unwrap().as_ref(),
            &[package, main]
        );
    }

    #[test]
    fn relative_imports_inside_package_initializer_stay_in_package() {
        let mut db = CompilerDatabase::default();
        let init = db.add_file("pkg/__init__.lucid", "from .reports import build\n");
        let reports = db.add_file("pkg/reports.lucid", "build = 1\n");
        let project = Project::new(&db, vec![init, reports]);
        assert_eq!(
            *resolve_import(&db, project, init, ".reports".into()),
            Some(reports)
        );
    }

    #[test]
    fn root_initializer_relative_imports_resolve_without_spurious_dot() {
        let mut db = CompilerDatabase::default();
        let init = db.add_file("__init__.lucid", "from .util import value\n");
        let util = db.add_file("util.lucid", "value = 1\n");
        let project = Project::new(&db, vec![init, util]);
        assert_eq!(
            *resolve_import(&db, project, init, ".util".into()),
            Some(util)
        );
    }

    #[test]
    fn module_order_is_dependency_first_and_rejects_cycles() {
        let mut db = CompilerDatabase::default();
        let main = db.add_file("main.lucid", "import lib\n");
        let lib = db.add_file("lib.lucid", "value = 1\n");
        let project = Project::new(&db, vec![main, lib]);
        let order = module_order(&db, project)
            .as_ref()
            .expect("acyclic project should order");
        assert_eq!(order.as_ref(), &[lib, main]);

        let a = db.add_file("a.lucid", "import b\nvalue = 1\n");
        let b = db.add_file("b.lucid", "import a\n");
        let cyclic = Project::new(&db, vec![a, b]);
        let error = module_order(&db, cyclic)
            .as_ref()
            .expect_err("cycle should be diagnosed");
        assert!(error.contains("cyclic module initialization"));
    }

    #[test]
    fn module_order_allows_declaration_only_cycles() {
        let mut db = CompilerDatabase::default();
        let a = db.add_file(
            "decl_a.lucid",
            "import decl_b\nclass A:\n    pass\ndef make_a():\n    return none\n",
        );
        let b = db.add_file(
            "decl_b.lucid",
            "import decl_a\ntrait B:\n    pass\ntype Alias = int\n",
        );
        let project = Project::new(&db, vec![a, b]);
        let order = module_order(&db, project)
            .as_ref()
            .expect("declaration-only cycles should be safe");
        assert_eq!(order.len(), 2);
        assert!(order.contains(&a));
        assert!(order.contains(&b));
    }

    #[test]
    fn cycle_diagnostic_points_at_a_participating_module() {
        let mut db = CompilerDatabase::default();
        let a = db.add_file("a.lucid", "import b\nvalue = 1\n");
        let b = db.add_file("b.lucid", "import a\nvalue = 2\n");
        let project = Project::new(&db, vec![a, b]);
        let diagnostics = project_diagnostics(&db, project);
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "E0301")
            .expect("cycle diagnostic");
        assert!(diagnostic.file == a || diagnostic.file == b);
        assert!(diagnostic.span.end > diagnostic.span.start);
    }

    #[test]
    fn module_order_is_stable_for_independent_files() {
        let mut db = CompilerDatabase::default();
        let z = db.add_file("z.lucid", "z = 1\n");
        let a = db.add_file("a.lucid", "a = 1\n");
        let project = Project::new(&db, vec![z, a]);
        let order = module_order(&db, project).as_ref().expect("order");
        assert_eq!(order.as_ref(), &[a, z]);
    }

    #[test]
    fn visibility_exposes_locals_and_imported_exports_only() {
        let mut db = CompilerDatabase::default();
        let main = db.add_file("main.lucid", "import lib\nlocal = 1\n");
        let lib = db.add_file(
            "lib.lucid",
            "export class Public:\n    pass\nclass _Private:\n    pass\n",
        );
        let project = Project::new(&db, vec![main, lib]);
        let names = visible_symbols(&db, project, main)
            .iter()
            .map(|symbol| symbol.name(&db).clone())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["lib", "local"]);
        assert!(resolve_visible(&db, project, main, "lib".into()).is_some());
        assert!(resolve_visible(&db, project, main, "Public".into()).is_none());
        assert!(resolve_visible(&db, project, main, "local".into()).is_some());
        assert!(resolve_visible(&db, project, main, "_Private".into()).is_none());
    }

    #[test]
    fn imported_bindings_resolve_only_exported_names_and_preserve_aliases() {
        let mut db = CompilerDatabase::default();
        let main = db.add_file(
            "main.lucid",
            "from support import value as answer, _private\n",
        );
        let support = db.add_file(
            "support.lucid",
            "export value = 1\nexport other = 3\n_private = 2\n",
        );
        let project = Project::new(&db, vec![main, support]);
        let bindings = imported_bindings(&db, project, main);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].local_name, "answer");
        assert_eq!(bindings[0].symbol.name(&db), "value");
        assert_eq!(
            resolve_visible(&db, project, main, "answer".into())
                .expect("alias should resolve")
                .name(&db),
            "value"
        );
        assert!(resolve_visible(&db, project, main, "_private".into()).is_none());
        assert!(resolve_visible(&db, project, main, "other".into()).is_none());
        let diagnostics = project_diagnostics(&db, project);
        let missing_name = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "E0302")
            .expect("missing imported name diagnostic");
        assert!(missing_name.span.end > missing_name.span.start);
    }

    #[test]
    fn local_declarations_take_precedence_over_import_aliases() {
        let mut db = CompilerDatabase::default();
        let main = db.add_file(
            "main.lucid",
            "from support import value as answer\nanswer = 2\n",
        );
        let support = db.add_file("support.lucid", "value = 1\n");
        let project = Project::new(&db, vec![main, support]);
        let resolved = resolve_visible(&db, project, main, "answer".into()).expect("answer");
        assert_eq!(*resolved.file(&db), main);
    }

    #[test]
    fn project_type_check_aggregates_errors_in_dependency_order() {
        let mut db = CompilerDatabase::default();
        let main = db.add_file("main.lucid", "import lib\nvalue: int = \"bad\"\n");
        let lib = db.add_file("lib.lucid", "other: int = \"also bad\"\n");
        let project = Project::new(&db, vec![main, lib]);
        let errors = type_check_project(&db, project);
        assert_eq!(errors.len(), 2);
        assert!(!errors[0].is_empty());
        assert!(!errors[1].is_empty());
    }

    #[test]
    fn duplicate_top_level_declarations_are_diagnosed() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "value = 1\nvalue = 2\n");
        let diagnostics = declaration_diagnostics(&db, file);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("duplicate"));
    }

    #[test]
    fn dispatch_overloads_are_not_duplicate_declarations() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file(
            "main.lucid",
            "dispatch def choose(value: int) -> int:\n    return value\ndispatch def choose(value: str) -> str:\n    return value\n",
        );
        let declarations = resolved_declarations(&db, file);
        assert_eq!(declarations.len(), 2);
        assert!(
            declarations
                .iter()
                .all(|declaration| declaration.is_dispatch)
        );
        assert!(declaration_diagnostics(&db, file).is_empty());
        assert!(
            file_diagnostics(&db, file)
                .iter()
                .all(|diagnostic| diagnostic.code != "E0100")
        );
    }

    #[test]
    fn structured_file_diagnostics_carry_codes_and_source_identity() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "value: int = \"wrong\"\n");
        let diagnostics = file_diagnostics(&db, file);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].file, file);
        assert_eq!(diagnostics[0].code, "E0200");
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert!(diagnostics[0].span.end > diagnostics[0].span.start);
    }

    #[test]
    fn project_diagnostics_include_parse_failures() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("broken.lucid", "value = `broken`\n");
        let project = Project::new(&db, vec![file]);
        let errors = type_check_project(&db, project);
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|error| error.starts_with("E0001:")));
    }

    #[test]
    fn syntax_diagnostics_point_at_lexical_error() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("broken.lucid", "value = `broken`\n");
        let diagnostics = file_diagnostics(&db, file);
        assert_eq!(diagnostics.len(), 2);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == "E0001"
                    && diagnostic.message.contains("lexer error")
                    && diagnostic.span.end > diagnostic.span.start
                    && diagnostic.span.line == 1)
        );
    }

    #[test]
    fn syntax_diagnostics_retain_independent_lexical_errors() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("broken.lucid", "first = $\nsecond = `\nthird = 3\n");
        let diagnostics = file_diagnostics(&db, file);
        assert_eq!(diagnostics.len(), 2);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == "E0001"
                    && diagnostic.message.contains("lexer error"))
        );
        assert_eq!(diagnostics[0].span.line, 1);
        assert_eq!(diagnostics[1].span.line, 2);
    }

    #[test]
    fn syntax_diagnostics_point_at_grammar_error() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("broken.lucid", "value =\nnext = 2\n");
        let diagnostics = file_diagnostics(&db, file);
        assert_eq!(diagnostics[0].code, "E0001");
        assert!(diagnostics[0].span.end > diagnostics[0].span.start);
        assert_eq!(diagnostics[0].span.line, 1);
    }

    #[test]
    fn syntax_diagnostics_retain_all_recoverable_grammar_errors() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("broken.lucid", "first =\nsecond =\n");
        let diagnostics = file_diagnostics(&db, file);
        assert_eq!(diagnostics.len(), 2);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == "E0001")
        );
        assert_eq!(diagnostics[0].span.line, 1);
        assert_eq!(diagnostics[1].span.line, 2);
    }

    #[test]
    fn project_diagnostics_report_unresolved_imports() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "import missing\nvalue = 1\n");
        let project = Project::new(&db, vec![file]);
        let diagnostics = project_diagnostics(&db, project);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E0300" && diagnostic.message.contains("missing")
        }));
        let unresolved = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "E0300")
            .expect("unresolved import diagnostic");
        assert!(unresolved.span.end > unresolved.span.start);
        assert!(
            type_check_project(&db, project)
                .iter()
                .any(|error| error.starts_with("E0300:"))
        );
    }

    #[test]
    fn project_diagnostics_reject_duplicate_module_paths() {
        let mut db = CompilerDatabase::default();
        let first = db.add_file("dup.lucid", "value = 1\n");
        let second = db.add_file("dup.lucid", "value = 2\n");
        let project = Project::new(&db, vec![first, second]);
        let diagnostics = project_diagnostics(&db, project);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E0303")
        );
        assert!(
            module_order(&db, project)
                .as_ref()
                .expect_err("duplicate paths must invalidate module order")
                .contains("duplicate module path")
        );
    }

    #[test]
    fn project_diagnostics_reject_exporting_private_names() {
        let mut db = CompilerDatabase::default();
        let file = db.add_file("main.lucid", "export _private = 1\n");
        let project = Project::new(&db, vec![file]);
        let diagnostics = project_diagnostics(&db, project);
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "E0304")
            .expect("private export diagnostic");
        assert!(diagnostic.message.contains("_private"));
        assert!(diagnostic.span.end > diagnostic.span.start);
    }

    #[test]
    fn project_diagnostics_reject_duplicate_import_bindings() {
        let mut db = CompilerDatabase::default();
        let main = db.add_file(
            "main.lucid",
            "from support import value as answer\nfrom support import other as answer\n",
        );
        let support = db.add_file("support.lucid", "value = 1\nother = 2\n");
        let project = Project::new(&db, vec![main, support]);
        let diagnostics = project_diagnostics(&db, project);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E0305" && diagnostic.message.contains("answer")
        }));
    }

    #[test]
    fn project_diagnostics_reject_duplicate_plain_import_aliases() {
        let mut db = CompilerDatabase::default();
        let main = db.add_file("main.lucid", "import one as shared\nimport two as shared\n");
        let one = db.add_file("one.lucid", "value = 1\n");
        let two = db.add_file("two.lucid", "value = 2\n");
        let project = Project::new(&db, vec![main, one, two]);
        assert!(
            project_diagnostics(&db, project)
                .iter()
                .any(|diagnostic| diagnostic.code == "E0305")
        );
    }
}
