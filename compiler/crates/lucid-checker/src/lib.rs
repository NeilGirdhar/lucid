use std::collections::{HashMap, HashSet};
use lucid_syntax::ast::*;
use lucid_syntax::token::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    Float,
    Bool,
    Str,
    None,
    Never,
    Class {
        name: String,
        type_args: Vec<Type>,
        parent: Option<String>,
        traits: Vec<String>,
        interfaces: Vec<String>,
        fields: HashMap<String, Type>,
        is_sealed: bool,
    },
    Interface {
        name: String,
        type_args: Vec<Type>,
        methods: HashSet<String>,
    },
    Trait {
        name: String,
        type_args: Vec<Type>,
        methods: HashSet<String>,
    },
    Record(Vec<(Option<String>, Type)>),
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
    Union(Vec<Type>),
    View {
        mutability: MutabilityView,
        inner: Box<Type>,
    },
    TypeVar(String),
}

impl Type {
    pub fn make_union(types: Vec<Type>) -> Self {
        let mut flattened = Vec::new();
        for t in types {
            match t {
                Type::Union(sub_types) => flattened.extend(sub_types),
                Type::Never => {} // Never is union identity (Never | X = X)
                other => flattened.push(other),
            }
        }

        // Deduplicate
        let mut unique: Vec<Type> = Vec::new();
        for t in flattened {
            if !unique.contains(&t) {
                unique.push(t);
            }
        }

        if unique.is_empty() {
            Type::Never
        } else if unique.len() == 1 {
            unique.pop().unwrap()
        } else {
            Type::Union(unique)
        }
    }

    pub fn is_subtype_of(&self, target: &Type, env: &TypeEnvironment) -> bool {
        if self == target {
            return true;
        }

        // Never is bottom: subtype of everything
        if matches!(self, Type::Never) {
            return true;
        }

        // Union subtype: A | B <: Target iff A <: Target and B <: Target
        if let Type::Union(variants) = self {
            return variants.iter().all(|v| v.is_subtype_of(target, env));
        }

        // Target is Union: Self <: A | B if Self <: A or Self <: B
        if let Type::Union(target_variants) = target {
            return target_variants.iter().any(|tv| self.is_subtype_of(tv, env));
        }

        // Mutability View Subtyping:
        // T  <: &T (mutable subtype of read-only view)
        // !T <: &T (immutable subtype of read-only view)
        match (self, target) {
            (Type::View { mutability: MutabilityView::Immutable, inner: i1 }, Type::View { mutability: MutabilityView::ReadOnly, inner: i2 }) => {
                return i1.is_subtype_of(i2, env);
            }
            (Type::Class { name: n1, .. }, Type::View { mutability: MutabilityView::ReadOnly, inner: i2 }) => {
                let bare_class = Type::Class {
                    name: n1.clone(),
                    type_args: Vec::new(),
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                };
                return bare_class.is_subtype_of(i2, env);
            }
            _ => {}
        }

        // Class inheritance and interface implementation subtyping
        if let Type::Class { name: c_name, parent, traits, interfaces, .. } = self {
            if let Type::Class { name: target_name, .. } = target {
                if c_name == target_name {
                    return true;
                }
                if let Some(p) = parent {
                    if p == target_name {
                        return true;
                    }
                    if let Some(parent_type) = env.classes.get(p) {
                        if parent_type.is_subtype_of(target, env) {
                            return true;
                        }
                    }
                }
            }

            if let Type::Interface { name: target_iface, .. } = target {
                if interfaces.contains(target_iface) {
                    return true;
                }
                for t in traits {
                    if let Some(trait_type) = env.traits.get(t) {
                        if trait_type.is_subtype_of(target, env) {
                            return true;
                        }
                    }
                }
            }

            if let Type::Trait { name: target_trait, .. } = target {
                if traits.contains(target_trait) {
                    return true;
                }
            }
        }

        false
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone, Default)]
pub struct TypeEnvironment {
    pub classes: HashMap<String, Type>,
    pub interfaces: HashMap<String, Type>,
    pub traits: HashMap<String, Type>,
    pub type_aliases: HashMap<String, Type>,
    pub functions: HashMap<String, Type>,
    pub variables: HashMap<String, (Type, MutabilityView)>,
    pub sealed_subclasses: HashMap<String, Vec<String>>,
    pub current_return_type: Option<Type>,
}

pub struct TypeChecker {
    pub env: TypeEnvironment,
}

impl TypeChecker {
    pub fn new() -> Self {
        let mut env = TypeEnvironment::default();
        // Register builtins
        env.classes.insert("int".to_string(), Type::Int);
        env.classes.insert("float".to_string(), Type::Float);
        env.classes.insert("bool".to_string(), Type::Bool);
        env.classes.insert("str".to_string(), Type::Str);
        env.classes.insert("none".to_string(), Type::None);

        Self { env }
    }

    pub fn check_module(&mut self, module: &Module) -> Result<(), TypeError> {
        // Pass 1: Register class, interface, trait, and alias declarations
        for stmt in &module.statements {
            self.collect_declaration(stmt)?;
        }

        // Pass 2: Verify single-inheritance and trait constraints
        for stmt in &module.statements {
            self.verify_structure(stmt)?;
        }

        // Pass 3: Check function bodies and statements
        for stmt in &module.statements {
            self.check_statement(stmt)?;
        }

        Ok(())
    }

    fn collect_declaration(&mut self, stmt: &Stmt) -> Result<(), TypeError> {
        match stmt {
            Stmt::Export(inner) => self.collect_declaration(inner),
            Stmt::ClassDef { name, bases, body, is_sealed, span, .. } => {
                let mut parent_class = None;
                let mut traits = Vec::new();
                let mut interfaces = Vec::new();

                for base_expr in bases {
                    if let TypeExpr::Named { name: base_name, .. } = base_expr {
                        if self.env.classes.contains_key(base_name) {
                            if parent_class.is_some() {
                                return Err(TypeError {
                                    message: format!("class '{name}' has multiple class parents ('{}' and '{base_name}'). Lucid permits at most one class parent.", parent_class.unwrap()),
                                    span: *span,
                                });
                            }
                            parent_class = Some(base_name.clone());
                        } else if self.env.interfaces.contains_key(base_name) {
                            interfaces.push(base_name.clone());
                        } else if self.env.traits.contains_key(base_name) {
                            traits.push(base_name.clone());
                        } else {
                            // Forward-referenced or external
                            interfaces.push(base_name.clone());
                        }
                    }
                }

                if let Some(ref p) = parent_class {
                    self.env.sealed_subclasses.entry(p.clone()).or_default().push(name.clone());
                }

                let mut fields = HashMap::new();
                for member in body {
                    if let ClassMember::Field(f) = member {
                        if let Ok(ft) = self.resolve_type_expr(&f.type_annotation) {
                            fields.insert(f.name.clone(), ft);
                        }
                    }
                }

                let class_type = Type::Class {
                    name: name.clone(),
                    type_args: Vec::new(),
                    parent: parent_class,
                    traits,
                    interfaces,
                    fields,
                    is_sealed: *is_sealed,
                };

                self.env.classes.insert(name.clone(), class_type);
                Ok(())
            }
            Stmt::InterfaceDef { name, .. } => {
                let iface_type = Type::Interface {
                    name: name.clone(),
                    type_args: Vec::new(),
                    methods: HashSet::new(),
                };
                self.env.interfaces.insert(name.clone(), iface_type);
                Ok(())
            }
            Stmt::TraitDef { name, .. } => {
                let trait_type = Type::Trait {
                    name: name.clone(),
                    type_args: Vec::new(),
                    methods: HashSet::new(),
                };
                self.env.traits.insert(name.clone(), trait_type);
                Ok(())
            }
            Stmt::TypeAlias { name, value, .. } => {
                match value {
                    TypeAliasValue::Direct(texpr) => {
                        let t = self.resolve_type_expr(texpr)?;
                        self.env.type_aliases.insert(name.clone(), t);
                    }
                    TypeAliasValue::Match { .. } => {
                        // Match type registered
                    }
                }
                Ok(())
            }
            Stmt::Function(func) => {
                let ret = if let Some(ref r) = func.return_type {
                    self.resolve_type_expr(r)?
                } else {
                    Type::None
                };
                let mut param_types = Vec::new();
                for p in &func.params {
                    let pt = if let Some(ref t) = p.type_annotation {
                        self.resolve_type_expr(t)?
                    } else {
                        Type::TypeVar("Any".to_string())
                    };
                    param_types.push(pt);
                }
                let fn_type = Type::Function {
                    params: param_types,
                    return_type: Box::new(ret),
                };
                self.env.functions.insert(func.name.clone(), fn_type.clone());
                self.env.variables.insert(func.name.clone(), (fn_type, MutabilityView::Immutable));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn verify_structure(&mut self, stmt: &Stmt) -> Result<(), TypeError> {
        match stmt {
            Stmt::Export(inner) => self.verify_structure(inner),
            Stmt::ClassDef { name, bases, span, .. } => {
                let mut class_parents = 0;
                for b in bases {
                    if let TypeExpr::Named { name: b_name, .. } = b {
                        if self.env.classes.contains_key(b_name) {
                            class_parents += 1;
                        }
                    }
                }

                if class_parents > 1 {
                    return Err(TypeError {
                        message: format!("class '{name}' inherits from multiple classes. Lucid requires single inheritance for classes (use traits for reusable behavior)."),
                        span: *span,
                    });
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn check_statement(&mut self, stmt: &Stmt) -> Result<(), TypeError> {
        match stmt {
            Stmt::Export(inner) => self.check_statement(inner),
            Stmt::Function(func) => {
                let ret_type = if let Some(ref r) = func.return_type {
                    self.resolve_type_expr(r)?
                } else {
                    Type::None
                };

                let prev_ret = self.env.current_return_type.take();
                self.env.current_return_type = Some(ret_type.clone());

                let mut local_vars = self.env.variables.clone();
                for param in &func.params {
                    let pt = if let Some(ref t) = param.type_annotation {
                        self.resolve_type_expr(t)?
                    } else {
                        Type::TypeVar("Any".to_string())
                    };
                    local_vars.insert(param.name.clone(), (pt, MutabilityView::Mutable));
                }

                let old_vars = std::mem::replace(&mut self.env.variables, local_vars);

                for s in &func.body {
                    self.check_statement(s)?;
                }

                self.env.variables = old_vars;
                self.env.current_return_type = prev_ret;
                Ok(())
            }
            Stmt::Return { value, span } => {
                let val_type = if let Some(ref e) = value {
                    self.type_of_expr(e)?
                } else {
                    Type::None
                };

                if let Some(ref expected) = self.env.current_return_type {
                    if !val_type.is_subtype_of(expected, &self.env) {
                        return Err(TypeError {
                            message: format!("return type mismatch: expected {:?}, got {:?}", expected, val_type),
                            span: *span,
                        });
                    }
                }
                Ok(())
            }
            Stmt::VarDef { pattern, type_annotation, value, span, .. } => {
                let inferred_val = if let Some(ref e) = value {
                    Some(self.type_of_expr(e)?)
                } else {
                    None
                };

                let target_type = if let Some(ref t) = type_annotation {
                    let resolved = self.resolve_type_expr(t)?;
                    if let Some(ref iv) = inferred_val {
                        if !iv.is_subtype_of(&resolved, &self.env) {
                            return Err(TypeError {
                                message: format!("type mismatch in variable definition: declared {:?}, got {:?}", resolved, iv),
                                span: *span,
                            });
                        }
                    }
                    resolved
                } else if let Some(iv) = inferred_val {
                    iv
                } else {
                    Type::None
                };

                if let Pattern::Ident(name, _) = pattern {
                    self.env.variables.insert(name.clone(), (target_type, MutabilityView::Mutable));
                }
                Ok(())
            }
            Stmt::Assignment { target, value, span } => {
                let val_type = self.type_of_expr(value)?;
                match target {
                    Expr::Ident { name, .. } => {
                        if let Some((existing_type, view)) = self.env.variables.get(name).cloned() {
                            if view == MutabilityView::ReadOnly || view == MutabilityView::Immutable {
                                return Err(TypeError {
                                    message: format!("cannot reassign to read-only or immutable variable '{name}'"),
                                    span: *span,
                                });
                            }
                            if !val_type.is_subtype_of(&existing_type, &self.env) {
                                return Err(TypeError {
                                    message: format!("cannot assign type {:?} to variable '{name}' of type {:?}", val_type, existing_type),
                                    span: *span,
                                });
                            }
                        } else {
                            self.env.variables.insert(name.clone(), (val_type, MutabilityView::Mutable));
                        }
                    }
                    Expr::Attribute { value: obj_expr, attr, .. } => {
                        let obj_type = self.type_of_expr(obj_expr)?;
                        if let Type::View { mutability, .. } = obj_type {
                            if mutability == MutabilityView::ReadOnly || mutability == MutabilityView::Immutable {
                                return Err(TypeError {
                                    message: format!("cannot mutate attribute '{attr}' on read-only/immutable view"),
                                    span: *span,
                                });
                            }
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            Stmt::Match { subject, arms, span, .. } => {
                let subject_type = self.type_of_expr(subject)?;
                self.check_match_exhaustiveness(&subject_type, arms, *span)?;
                for arm in arms {
                    for s in &arm.body {
                        self.check_statement(s)?;
                    }
                }
                Ok(())
            }
            Stmt::If { condition, then_branch, elif_branches, else_branch, .. } => {
                let cond_type = self.type_of_expr(condition)?;
                if !cond_type.is_subtype_of(&Type::Bool, &self.env) && cond_type != Type::Bool {
                    // Lucid requires explicit boolean tests
                }
                for s in then_branch {
                    self.check_statement(s)?;
                }
                for (c, b) in elif_branches {
                    let _ = self.type_of_expr(c)?;
                    for s in b {
                        self.check_statement(s)?;
                    }
                }
                if let Some(ref eb) = else_branch {
                    for s in eb {
                        self.check_statement(s)?;
                    }
                }
                Ok(())
            }
            Stmt::For { target, iterable, body, if_broken, .. } => {
                let iter_type = self.type_of_expr(iterable)?;
                let elem_type = match iter_type {
                    Type::Class { ref name, ref type_args, .. } if name == "list" => {
                        type_args.first().cloned().unwrap_or(Type::TypeVar("Any".to_string()))
                    }
                    _ => Type::TypeVar("Any".to_string()),
                };

                let old_vars = self.env.variables.clone();
                if let Pattern::Ident(name, _) = target {
                    self.env.variables.insert(name.clone(), (elem_type, MutabilityView::Mutable));
                }

                for s in body {
                    self.check_statement(s)?;
                }
                self.env.variables = old_vars;

                if let Some(ref ib) = if_broken {
                    for s in ib {
                        self.check_statement(s)?;
                    }
                }
                Ok(())
            }
            Stmt::While { condition, body, if_broken, .. } => {
                let _ = self.type_of_expr(condition)?;
                for s in body {
                    self.check_statement(s)?;
                }
                if let Some(ref ib) = if_broken {
                    for s in ib {
                        self.check_statement(s)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn check_match_exhaustiveness(&self, subject_type: &Type, arms: &[MatchArm], span: Span) -> Result<(), TypeError> {
        let has_wildcard = arms.iter().any(|arm| matches!(arm.pattern, Pattern::Wildcard(_)));
        if has_wildcard {
            return Ok(());
        }

        match subject_type {
            Type::Union(variants) => {
                let mut uncovered = variants.clone();
                for arm in arms {
                    match &arm.pattern {
                        Pattern::Ident(name, _) => {
                            uncovered.retain(|v| match v {
                                Type::Class { name: c_name, .. } => c_name != name,
                                Type::Int if name == "int" => false,
                                Type::Float if name == "float" => false,
                                Type::Bool if name == "bool" => false,
                                Type::Str if name == "str" => false,
                                Type::None if name == "none" => false,
                                _ => true,
                            });
                        }
                        _ => {}
                    }
                }

                if !uncovered.is_empty() {
                    return Err(TypeError {
                        message: format!("non-exhaustive match: uncovered variants: {:?}", uncovered),
                        span,
                    });
                }
            }
            Type::Class { name, is_sealed: true, .. } => {
                if let Some(subclasses) = self.env.sealed_subclasses.get(name) {
                    let mut uncovered = subclasses.clone();
                    for arm in arms {
                        if let Pattern::Ident(c_name, _) = &arm.pattern {
                            uncovered.retain(|sub| sub != c_name);
                        }
                    }
                    if !uncovered.is_empty() {
                        return Err(TypeError {
                            message: format!("non-exhaustive match on sealed class '{name}': uncovered subclasses: {:?}", uncovered),
                            span,
                        });
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn type_of_expr(&self, expr: &Expr) -> Result<Type, TypeError> {
        match expr {
            Expr::Literal { value, .. } => Ok(match value {
                LiteralValue::Int(_) => Type::Int,
                LiteralValue::Float(_) => Type::Float,
                LiteralValue::Bool(_) => Type::Bool,
                LiteralValue::Str(_) => Type::Str,
                LiteralValue::None => Type::None,
                LiteralValue::Sentinel(s) => Type::TypeVar(s.clone()),
            }),
            Expr::Ident { name, span } => {
                if let Some((t, _)) = self.env.variables.get(name) {
                    Ok(t.clone())
                } else if let Some(t) = self.env.classes.get(name) {
                    Ok(t.clone())
                } else {
                    Err(TypeError {
                        message: format!("undefined variable '{name}'"),
                        span: *span,
                    })
                }
            }
            Expr::Binary { op, left, right, span: _ } => {
                let lt = self.type_of_expr(left)?;
                let rt = self.type_of_expr(right)?;

                match op {
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                        if lt == Type::Int && rt == Type::Int {
                            Ok(Type::Int)
                        } else if lt == Type::Float || rt == Type::Float {
                            Ok(Type::Float)
                        } else {
                            Ok(lt) // dispatched
                        }
                    }
                    BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                        Ok(Type::Bool)
                    }
                    BinaryOp::And | BinaryOp::Or => Ok(Type::Bool),
                    _ => Ok(Type::Int),
                }
            }
            Expr::Unary { op, expr, .. } => {
                let inner = self.type_of_expr(expr)?;
                match op {
                    UnaryOp::Not => Ok(Type::Bool),
                    _ => Ok(inner),
                }
            }
            Expr::Call { func, .. } => {
                let ft = self.type_of_expr(func)?;
                match ft {
                    Type::Function { return_type, .. } => Ok(*return_type),
                    Type::Class { .. } => Ok(ft), // class construction
                    _ => Ok(Type::Never),
                }
            }
            Expr::Propagate { expr, span } => {
                let inner = self.type_of_expr(expr)?;
                // In Lucid: read_file(path)? extracts Ok value or propagates Error
                match inner {
                    Type::Union(variants) => {
                        // Extract non-error variant
                        let mut ok_types = Vec::new();
                        let mut err_types = Vec::new();

                        for v in variants {
                            if matches!(v, Type::Class { ref name, .. } if name.ends_with("Error")) {
                                err_types.push(v);
                            } else {
                                ok_types.push(v);
                            }
                        }

                        if let Some(ref expected_ret) = self.env.current_return_type {
                            for err in err_types {
                                if !err.is_subtype_of(expected_ret, &self.env) {
                                    return Err(TypeError {
                                        message: format!("'?' propagates error type {:?}, but enclosing function return type does not include it", err),
                                        span: *span,
                                    });
                                }
                            }
                        }

                        Ok(Type::make_union(ok_types))
                    }
                    other => Ok(other),
                }
            }
            Expr::Freeze { expr, .. } => {
                let inner = self.type_of_expr(expr)?;
                Ok(Type::View {
                    mutability: MutabilityView::Immutable,
                    inner: Box::new(inner),
                })
            }
            Expr::Trust { target_type, .. } => {
                self.resolve_type_expr(target_type)
            }
            Expr::List { elements, .. } => {
                let elem_types: Vec<Type> = elements.iter().map(|e| self.type_of_expr(e).unwrap_or(Type::Never)).collect();
                Ok(Type::Class {
                    name: "list".to_string(),
                    type_args: vec![Type::make_union(elem_types)],
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                })
            }
            Expr::Dict { .. } => {
                Ok(Type::Class {
                    name: "dict".to_string(),
                    type_args: Vec::new(),
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                })
            }
            Expr::Construct { .. } => {
                // Returns current enclosing class
                Ok(Type::None)
            }
            Expr::Attribute { value, attr, .. } => {
                let obj_type = self.type_of_expr(value)?;
                match obj_type {
                    Type::Class { ref fields, .. } => {
                        if let Some(field_type) = fields.get(attr) {
                            Ok(field_type.clone())
                        } else {
                            Ok(Type::None)
                        }
                    }
                    Type::View { ref inner, .. } => {
                        if let Type::Class { ref fields, .. } = **inner {
                            if let Some(field_type) = fields.get(attr) {
                                return Ok(field_type.clone());
                            }
                        }
                        Ok(Type::None)
                    }
                    _ => Ok(Type::None),
                }
            }
            _ => Ok(Type::None),
        }
    }

    pub fn resolve_type_expr(&self, texpr: &TypeExpr) -> Result<Type, TypeError> {
        match texpr {
            TypeExpr::Named { name, args, span: _ } => {
                let resolved_args = args.iter().map(|a| self.resolve_type_expr(a)).collect::<Result<Vec<_>, _>>()?;
                match name.as_str() {
                    "int" => Ok(Type::Int),
                    "float" => Ok(Type::Float),
                    "bool" => Ok(Type::Bool),
                    "str" => Ok(Type::Str),
                    "none" => Ok(Type::None),
                    "Never" => Ok(Type::Never),
                    other => {
                        if let Some(alias) = self.env.type_aliases.get(other) {
                            return Ok(alias.clone());
                        }
                        if let Some(c) = self.env.classes.get(other) {
                            let mut c_clone = c.clone();
                            if let Type::Class { ref mut type_args, .. } = c_clone {
                                *type_args = resolved_args;
                            }
                            return Ok(c_clone);
                        }
                        if let Some(iface) = self.env.interfaces.get(other) {
                            return Ok(iface.clone());
                        }
                        if let Some(tr) = self.env.traits.get(other) {
                            return Ok(tr.clone());
                        }
                        // Default to TypeVar or forward reference
                        Ok(Type::Class {
                            name: other.to_string(),
                            type_args: resolved_args,
                            parent: None,
                            traits: Vec::new(),
                            interfaces: Vec::new(),
                            fields: HashMap::new(),
                            is_sealed: false,
                        })
                    }
                }
            }
            TypeExpr::Function { params, return_type, .. } => {
                let resolved_params = params.iter().map(|p| self.resolve_type_expr(p)).collect::<Result<Vec<_>, _>>()?;
                let resolved_ret = self.resolve_type_expr(return_type)?;
                Ok(Type::Function {
                    params: resolved_params,
                    return_type: Box::new(resolved_ret),
                })
            }
            TypeExpr::Union { types, .. } => {
                let resolved = types.iter().map(|t| self.resolve_type_expr(t)).collect::<Result<Vec<_>, _>>()?;
                Ok(Type::make_union(resolved))
            }
            TypeExpr::View { mutability, inner, .. } => {
                let resolved_inner = self.resolve_type_expr(inner)?;
                Ok(Type::View {
                    mutability: mutability.clone(),
                    inner: Box::new(resolved_inner),
                })
            }
            _ => Ok(Type::Never),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucid_syntax::parse;

    #[test]
    fn test_single_inheritance_rule_enforced() {
        let src = r#"
class Base1:
    x: int

class Base2:
    y: int

class Child(Base1, Base2):
    z: int
"#;
        let module = parse(src).unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module);
        assert!(err.is_err());
        assert!(err.unwrap_err().message.contains("multiple class parents"));
    }

    #[test]
    fn test_mutability_subtyping() {
        let checker = TypeChecker::new();
        let point_mutable = Type::Class {
            name: "Point".to_string(),
            type_args: Vec::new(),
            parent: None,
            traits: Vec::new(),
            interfaces: Vec::new(),
            fields: HashMap::new(),
            is_sealed: false,
        };

        let point_read_only = Type::View {
            mutability: MutabilityView::ReadOnly,
            inner: Box::new(point_mutable.clone()),
        };

        let point_immutable = Type::View {
            mutability: MutabilityView::Immutable,
            inner: Box::new(point_mutable.clone()),
        };

        // Point <: &Point (mutable can be passed to read-only parameter)
        assert!(point_mutable.is_subtype_of(&point_read_only, &checker.env));
        // !Point <: &Point (immutable can be passed to read-only parameter)
        assert!(point_immutable.is_subtype_of(&point_read_only, &checker.env));
        // &Point is NOT a subtype of Point (read-only cannot be passed where mutation is required)
        assert!(!point_read_only.is_subtype_of(&point_mutable, &checker.env));
    }

    #[test]
    fn test_exhaustive_match_on_union() {
        let src = r#"
type Shape = int | str

def render(s: Shape) -> int:
    match s:
        case int:
            return 1
"#;
        let module = parse(src).unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module);
        assert!(err.is_err());
        assert!(err.unwrap_err().message.contains("non-exhaustive match"));
    }

    #[test]
    fn test_class_method_attribute_type() {
        let src = r#"
class Circle:
    radius: int

dispatch def area(c: Circle) -> int:
    return c.radius * c.radius
"#;
        let module = parse(src).unwrap();
        let mut checker = TypeChecker::new();
        let res = checker.check_module(&module);
        assert!(res.is_ok(), "{:?}", res);
    }
}
