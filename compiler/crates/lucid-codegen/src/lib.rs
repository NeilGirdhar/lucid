//! Lucid Native Code Generator
//! Compiles Lucid AST to highly-optimized native machine code via C99/GCC.

#![allow(
    clippy::collapsible_if,
    clippy::if_same_then_else,
    clippy::iter_cloned_collect,
    clippy::needless_range_loop,
    clippy::unnecessary_map_or,
    clippy::useless_format
)]

use lucid_syntax::ast::*;
use num_bigint::BigInt;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

pub mod cranelift_backend;
pub mod native_abi;
pub use lucid_abi;

fn bigint_literal_decimal(text: &str) -> String {
    let text = text.trim().replace('_', "");
    let negative = text.starts_with('-');
    let unsigned = text.trim_start_matches(['+', '-']);
    let (digits, radix) = if let Some(value) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        (value, 16)
    } else if let Some(value) = unsigned
        .strip_prefix("0o")
        .or_else(|| unsigned.strip_prefix("0O"))
    {
        (value, 8)
    } else if let Some(value) = unsigned
        .strip_prefix("0b")
        .or_else(|| unsigned.strip_prefix("0B"))
    {
        (value, 2)
    } else {
        return text;
    };
    let Some(mut value) = BigInt::parse_bytes(digits.as_bytes(), radix) else {
        return text;
    };
    if negative {
        value = -value;
    }
    value.to_string()
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CodegenError {}

fn is_none_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Literal {
            value: LiteralValue::None,
            ..
        } => true,
        Expr::Ident { name, .. } => name == "none" || name == "None",
        _ => false,
    }
}

fn c_escape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            other => out.push(other),
        }
    }
    out
}

fn type_form_name(type_expr: &TypeExpr) -> String {
    match type_expr {
        TypeExpr::Named { name, args, .. } => {
            if args.is_empty() {
                name.clone()
            } else {
                format!(
                    "{}[{}]",
                    name,
                    args.iter()
                        .map(type_form_name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        TypeExpr::Function {
            params,
            return_type,
            ..
        } => format!(
            "({}) -> {}",
            params
                .iter()
                .map(type_form_name)
                .collect::<Vec<_>>()
                .join(", "),
            type_form_name(return_type)
        ),
        TypeExpr::Record { fields, .. } => format!(
            "({})",
            fields
                .iter()
                .map(|field| format!(
                    "{}: {}",
                    field.name.as_deref().unwrap_or("_"),
                    type_form_name(&field.type_expr)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeExpr::View {
            mutability, inner, ..
        } => format!(
            "{}{}",
            match mutability {
                MutabilityView::Mutable => "",
                MutabilityView::ReadOnly => "~",
                MutabilityView::Immutable => "!",
            },
            type_form_name(inner)
        ),
        TypeExpr::Union { types, .. } => types
            .iter()
            .map(type_form_name)
            .collect::<Vec<_>>()
            .join(" | "),
        TypeExpr::Literal { value, .. } => format!("{value:?}"),
        TypeExpr::Existential { interface, .. } => format!("any {}", type_form_name(interface)),
        TypeExpr::Reification { inner, .. } => format!("type {}", type_form_name(inner)),
        TypeExpr::Match { .. } => "match".into(),
        TypeExpr::Wildcard(_) => "_".into(),
        TypeExpr::Never(_) => "Never".into(),
    }
}

pub struct CCodeGenerator {
    buffer: String,
    indent: usize,
    temp_var_id: usize,
    in_function: bool,
    current_fn_ret_type: Option<String>,
    current_fn_async: bool,
    known_classes: HashMap<String, Vec<String>>,
    known_without_traits: HashMap<String, HashSet<String>>,
    known_parents: HashMap<String, String>,
    known_class_members: HashMap<String, Vec<String>>,
    known_field_types: HashMap<(String, String), String>,
    known_field_defaults: HashMap<(String, String), Option<Expr>>,
    known_methods: HashMap<String, String>,
    known_method_param_names: HashMap<(String, String), Vec<String>>,
    known_method_param_types: HashMap<(String, String), Vec<String>>,
    known_method_return_types: HashMap<(String, String), String>,
    known_method_defaults: HashMap<(String, String), Vec<Option<Expr>>>,
    known_method_gather: HashMap<(String, String), (usize, String)>,
    known_getters: HashMap<(String, String), String>,
    known_class_methods: HashMap<(String, String), String>,
    known_class_vars: HashMap<(String, String), String>,
    known_factories: HashMap<(String, String), String>,
    known_list_element_classes: HashMap<String, String>,
    known_setters: HashMap<(String, String), String>,
    known_setter_types: HashMap<(String, String), String>,
    contextmanager_methods: HashSet<(String, String)>,
    context_error_bodies: HashMap<String, Vec<Stmt>>,
    known_fns: HashMap<String, String>,
    known_fn_params: HashMap<String, Vec<String>>,
    known_fn_param_names: HashMap<String, Vec<String>>,
    known_fn_defaults: HashMap<String, Vec<Option<Expr>>>,
    known_fn_variadic: HashMap<String, usize>,
    known_fn_keyword_variadic: HashSet<String>,
    known_fn_gather: HashMap<String, (usize, String)>,
    async_fns: HashSet<String>,
    dispatch_fns: HashMap<String, Vec<(String, String)>>,
    dispatch_signatures: HashMap<String, Vec<(Vec<String>, String)>>,
    var_types: HashMap<String, String>,
    dynamic_vars: HashSet<String>,
    global_vars: HashMap<String, String>,
    in_setter: bool,
    current_class: Option<String>,
    capture_assignment: Option<String>,
    loop_break_flags: Vec<String>,
    complex_names: HashSet<String>,
    anonymous_bindings: HashMap<String, (Vec<(String, String)>, Expr)>,
    /// Source-level names bound to named functions.  Native functions have
    /// concrete C signatures, so preserving this alias lets `g = f; g(x)`
    /// use the same checked entry point without inventing an untyped C value.
    function_aliases: HashMap<String, String>,
    partial_bindings: HashMap<String, (String, Vec<Arg>)>,
    module_aliases: HashMap<String, String>,
    from_imports: HashMap<String, String>,
    bigint_names: HashSet<String>,
    /// Source-level names removed by `del` in the current emission scope.
    deleted_bindings: HashSet<String>,
    alive_declarations: HashSet<String>,
}

impl Default for CCodeGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl CCodeGenerator {
    fn dispatch_type_distance(&self, actual: &str, expected: &str) -> Option<usize> {
        if expected == "object" || expected == "LucidVal" {
            return Some(usize::MAX / 4);
        }
        if actual == expected {
            return Some(0);
        }
        let mut current = actual;
        let mut distance = 0usize;
        let mut seen = HashSet::new();
        while let Some(parent) = self.known_parents.get(current) {
            if !seen.insert(current.to_owned()) {
                return None;
            }
            distance += 1;
            if parent == expected {
                return Some(distance);
            }
            current = parent;
        }
        None
    }

    fn ensure_alive_binding(&mut self, name: &str) {
        if self.current_fn_ret_type.as_deref() == Some("void") {
            return;
        }
        if !self.alive_declarations.contains(name) {
            self.emit_line(&format!("bool lucid_alive_{name} = false;"));
            self.alive_declarations.insert(name.to_string());
        }
    }

    fn unwrap_export(stmt: &Stmt) -> &Stmt {
        match stmt {
            Stmt::Export(inner) => Self::unwrap_export(inner),
            other => other,
        }
    }

    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            indent: 0,
            temp_var_id: 0,
            in_function: false,
            current_fn_ret_type: None,
            current_fn_async: false,
            known_classes: HashMap::new(),
            known_without_traits: HashMap::new(),
            known_parents: HashMap::new(),
            known_class_members: HashMap::new(),
            known_field_types: HashMap::new(),
            known_field_defaults: HashMap::new(),
            known_methods: HashMap::new(),
            known_method_param_names: HashMap::new(),
            known_method_param_types: HashMap::new(),
            known_method_return_types: HashMap::new(),
            known_method_defaults: HashMap::new(),
            known_method_gather: HashMap::new(),
            known_getters: HashMap::new(),
            known_class_methods: HashMap::new(),
            known_class_vars: HashMap::new(),
            known_factories: HashMap::new(),
            known_list_element_classes: HashMap::new(),
            known_setters: HashMap::new(),
            known_setter_types: HashMap::new(),
            contextmanager_methods: HashSet::new(),
            context_error_bodies: HashMap::new(),
            known_fns: HashMap::new(),
            known_fn_params: HashMap::new(),
            known_fn_param_names: HashMap::new(),
            known_fn_defaults: HashMap::new(),
            known_fn_variadic: HashMap::new(),
            known_fn_keyword_variadic: HashSet::new(),
            known_fn_gather: HashMap::new(),
            async_fns: HashSet::new(),
            dispatch_fns: HashMap::new(),
            dispatch_signatures: HashMap::new(),
            var_types: HashMap::new(),
            dynamic_vars: HashSet::new(),
            global_vars: HashMap::new(),
            in_setter: false,
            current_class: None,
            capture_assignment: None,
            loop_break_flags: Vec::new(),
            complex_names: HashSet::new(),
            anonymous_bindings: HashMap::new(),
            function_aliases: HashMap::new(),
            partial_bindings: HashMap::new(),
            module_aliases: HashMap::new(),
            from_imports: HashMap::new(),
            bigint_names: HashSet::new(),
            deleted_bindings: HashSet::new(),
            alive_declarations: HashSet::new(),
        }
    }

    fn new_temp(&mut self) -> String {
        self.temp_var_id += 1;
        format!("_lucid_tmp_{}", self.temp_var_id)
    }

    fn indent_str(&self) -> String {
        "    ".repeat(self.indent)
    }

    fn emit_line(&mut self, s: &str) {
        self.buffer.push_str(&self.indent_str());
        self.buffer.push_str(s);
        self.buffer.push('\n');
    }

    fn truthy_native(&self, expr: &Expr, code: &str) -> String {
        let mut types = self.var_types.clone();
        types.extend(self.global_vars.clone());
        let ty = self.infer_expr_type(expr, &types);
        let class = ty.trim_end_matches('*');
        if let Some(owner) = self.method_owner(class, "__bool__") {
            format!("{owner}___bool__(({owner}*)({code}))")
        } else if let Some(owner) = self.method_owner(class, "__len__") {
            format!("({owner}___len__(({owner}*)({code})) != 0)")
        } else {
            match ty.as_str() {
                "const char*" | "char*" => format!("lucid_str_truthy({code})"),
                "LucidList*" => format!("lucid_list_truthy({code})"),
                "LucidDict*" => format!("lucid_dict_truthy({code})"),
                "LucidSet*" => format!("lucid_set_truthy({code})"),
                _ => format!("lucid_bool_val({code})"),
            }
        }
    }

    /// Emit an expression used as a condition.  Logical operators return one
    /// of their operands in Lucid, so testing the resulting `LucidVal` with
    /// `lucid_bool_val` would bypass a class's custom `__bool__`/`__len__`.
    /// Lower them directly to short-circuiting truth tests instead.
    fn emit_condition(&mut self, expr: &Expr) -> Result<String, CodegenError> {
        if let Expr::Binary {
            op, left, right, ..
        } = expr
        {
            if matches!(op, BinaryOp::And | BinaryOp::Or) {
                let left_truth = self.emit_condition(left)?;
                let right_truth = self.emit_condition(right)?;
                let join = if matches!(op, BinaryOp::And) {
                    "&&"
                } else {
                    "||"
                };
                return Ok(format!("(({left_truth}) {join} ({right_truth}))"));
            }
        }
        let code = self.emit_expr(expr)?;
        Ok(self.truthy_native(expr, &code))
    }

    fn method_owner(&self, class: &str, method: &str) -> Option<String> {
        let mut current = Some(class.to_string());
        while let Some(name) = current {
            if self
                .known_method_param_names
                .contains_key(&(name.clone(), method.to_string()))
            {
                return Some(name);
            }
            current = self.known_parents.get(&name).cloned();
        }
        None
    }

    fn class_has_capability(&self, class: &str, capability: &str) -> bool {
        let mut current = Some(class.to_string());
        while let Some(name) = current {
            if self
                .known_without_traits
                .get(&name)
                .is_some_and(|traits| traits.contains(capability))
            {
                return false;
            }
            current = self.known_parents.get(&name).cloned();
        }
        true
    }

    /// Materialize a user iterator in native code.  Iterator termination is a
    /// Lucid sentinel, so this must drain through the class's `next` method
    /// rather than treating the object as one of the built-in containers.
    fn native_drain_iterator(&mut self, class: &str, value: &str) -> String {
        let out = self.new_temp();
        let iterator = self.new_temp();
        let item = self.new_temp();
        let iterator_value = if let Some(owner) = self.method_owner(class, "__iter__") {
            format!("({class}*)lucid_as_ptr(lucid_wrap({owner}___iter__(({owner}*)({value}))))")
        } else {
            value.to_string()
        };
        format!(
            "({{ LucidList* {out} = lucid_list_new(8); {class}* {iterator} = {iterator_value}; for (;;) {{ LucidVal {item} = lucid_wrap({class}_next({iterator})); if ({item}.type == LUCID_TYPE_STR && {item}.s && strcmp({item}.s, \"iteration.done\") == 0) break; lucid_list_append({out}, {item}); }} {out}; }})"
        )
    }

    fn native_sorted_custom(&mut self, value: &str, class: &str) -> String {
        let source = self.new_temp();
        let out = self.new_temp();
        let item = self.new_temp();
        let pos = self.new_temp();
        format!(
            "({{ LucidList* {source} = {value}; LucidList* {out} = lucid_list_new({source} ? {source}->len : 0); for (int64_t _i = 0; {source} && _i < {source}->len; ++_i) {{ LucidVal {item} = {source}->items[_i]; int64_t {pos} = {out}->len; lucid_list_append({out}, {item}); while ({pos} > 0 && {class}___lt__(({class}*)lucid_as_ptr({item}), ({class}*)lucid_as_ptr({out}->items[{pos} - 1]))) {{ {out}->items[{pos}] = {out}->items[{pos} - 1]; --{pos}; {out}->items[{pos}] = {item}; }} }} {out}; }})"
        )
    }

    fn native_any_all_custom(&mut self, value: &str, class: &str, any: bool) -> String {
        let items = self.new_temp();
        let result = self.new_temp();
        let item = self.new_temp();
        let truth = if let Some(owner) = self.method_owner(class, "__bool__") {
            format!("{owner}___bool__(({owner}*)lucid_as_ptr({item}))")
        } else if let Some(owner) = self.method_owner(class, "__len__") {
            format!("({owner}___len__(({owner}*)lucid_as_ptr({item})) != 0)")
        } else {
            format!("lucid_bool_val({item})")
        };
        let condition = if any {
            truth.clone()
        } else {
            format!("!({truth})")
        };
        let initial = if any { "false" } else { "true" };
        let terminal = if any { "break" } else { "break" };
        format!(
            "({{ LucidList* {items} = {value}; bool {result} = {initial}; for (int64_t _i = 0; {items} && _i < {items}->len; ++_i) {{ LucidVal {item} = {items}->items[_i]; if ({condition}) {{ {result} = {any}; {terminal}; }} }} {result}; }})"
        )
    }

    fn native_min_max_custom(&mut self, value: &str, class: &str, min: bool) -> String {
        let items = self.new_temp();
        let result = self.new_temp();
        let item = self.new_temp();
        let comparison = if min {
            format!(
                "{class}___lt__(({class}*)lucid_as_ptr({item}), ({class}*)lucid_as_ptr({result}))"
            )
        } else {
            format!(
                "{class}___lt__(({class}*)lucid_as_ptr({result}), ({class}*)lucid_as_ptr({item}))"
            )
        };
        format!(
            "({{ LucidList* {items} = {value}; if (!{items} || {items}->len == 0) {{ fprintf(stderr, \"min/max arg is an empty sequence\\n\"); exit(1); }} LucidVal {result} = {items}->items[0]; for (int64_t _i = 1; _i < {items}->len; ++_i) {{ LucidVal {item} = {items}->items[_i]; if ({comparison}) {result} = {item}; }} {result}; }})"
        )
    }

    fn indexed_element_class(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Ident { name, .. } => self.known_list_element_classes.get(name).cloned(),
            Expr::List { elements, .. } => elements.iter().find_map(|element| {
                let ty = self.infer_expr_type(element, &HashMap::new());
                let class = ty.strip_suffix('*')?;
                self.known_classes
                    .contains_key(class)
                    .then(|| class.to_string())
            }),
            Expr::Call { func, args, .. } => {
                if let Expr::Ident { name, .. } = &**func {
                    if name == "sorted" || name == "list" {
                        return args
                            .first()
                            .and_then(|arg| self.indexed_element_class(&arg.value));
                    }
                }
                None
            }
            Expr::Index { value, .. } => self.indexed_element_class(value),
            _ => None,
        }
    }

    fn record_list_element_class(
        &mut self,
        name: &str,
        value: Option<&Expr>,
        annotation: Option<&TypeExpr>,
    ) {
        let from_annotation = annotation.and_then(|ty| match ty {
            TypeExpr::Named {
                name: container,
                args,
                ..
            } if container == "list" && args.len() == 1 => {
                if let TypeExpr::Named { name: element, .. } = &args[0] {
                    Some(element.clone())
                } else {
                    None
                }
            }
            _ => None,
        });
        let from_value = value.and_then(|expr| match expr {
            Expr::List { elements, .. } => elements.iter().find_map(|element| {
                let ty = self.infer_expr_type(element, &HashMap::new());
                let class = ty.strip_suffix('*')?;
                self.known_classes
                    .contains_key(class)
                    .then(|| class.to_string())
            }),
            Expr::Ident { name, .. } => self.known_list_element_classes.get(name).cloned(),
            _ => None,
        });
        if let Some(class) = from_annotation.or(from_value) {
            self.known_list_element_classes
                .insert(name.to_string(), class);
        }
    }

    pub fn generate(&mut self, module: &Module) -> Result<String, CodegenError> {
        self.module_aliases.clear();
        self.from_imports.clear();
        self.known_list_element_classes.clear();
        for stmt in &module.statements {
            match stmt {
                Stmt::Import { module, alias, .. } => {
                    let bound = alias.clone().unwrap_or_else(|| {
                        module
                            .rsplit('.')
                            .next()
                            .unwrap_or(module)
                            .trim_start_matches('.')
                            .to_string()
                    });
                    self.module_aliases.insert(bound, module.clone());
                }
                Stmt::FromImport { module, names, .. } => {
                    for (name, alias) in names {
                        self.from_imports.insert(
                            alias.clone().unwrap_or_else(|| name.clone()),
                            module.clone(),
                        );
                    }
                }
                _ => {}
            }
        }
        // First pass: collect class metadata and function signatures
        for stmt in &module.statements {
            let stmt = Self::unwrap_export(stmt);
            if let Stmt::ClassDef {
                name,
                bases: _bases,
                without_traits,
                body,
                ..
            } = stmt
            {
                self.known_without_traits
                    .insert(name.clone(), without_traits.iter().cloned().collect());
                let mut fields = Vec::new();
                let mut members = Vec::new();
                for member in body {
                    match member {
                        ClassMember::Field(f) => {
                            fields.push(f.name.clone());
                            members.push(f.name.clone());
                            self.known_field_types.insert(
                                (name.clone(), f.name.clone()),
                                self.map_type_expr(Some(&f.type_annotation)),
                            );
                            self.known_field_defaults
                                .insert((name.clone(), f.name.clone()), f.default.clone());
                        }
                        ClassMember::Method(m) => {
                            members.push(m.name.clone());
                            self.known_methods.insert(m.name.clone(), name.clone());
                            self.known_method_param_names.insert(
                                (name.clone(), m.name.clone()),
                                m.params.iter().skip(1).map(|p| p.name.clone()).collect(),
                            );
                            self.known_method_param_types.insert(
                                (name.clone(), m.name.clone()),
                                m.params
                                    .iter()
                                    .skip(1)
                                    .map(|p| self.map_type_expr(p.type_annotation.as_ref()))
                                    .collect(),
                            );
                            self.known_method_return_types.insert(
                                (name.clone(), m.name.clone()),
                                self.map_type_expr(m.return_type.as_ref()),
                            );
                            self.known_method_defaults.insert(
                                (name.clone(), m.name.clone()),
                                m.params.iter().skip(1).map(|p| p.default.clone()).collect(),
                            );
                            if let Some((index, param)) = m
                                .params
                                .iter()
                                .skip(1)
                                .enumerate()
                                .find(|(_, param)| param.is_gather)
                            {
                                if let Some(TypeExpr::Named { name: bundle, .. }) =
                                    &param.type_annotation
                                {
                                    self.known_method_gather.insert(
                                        (name.clone(), m.name.clone()),
                                        (index, bundle.clone()),
                                    );
                                }
                            }
                            if m.decorators.iter().any(|decorator| matches!(decorator, Expr::Ident { name: decorator_name, .. } if decorator_name == "contextmanager")) {
                                self.contextmanager_methods.insert((name.clone(), m.name.clone()));
                            }
                        }
                        ClassMember::Getter(g) => {
                            members.push(g.name.clone());
                            self.known_getters
                                .insert((name.clone(), g.name.clone()), g.name.clone());
                        }
                        ClassMember::ClassMethod(m) => {
                            members.push(m.name.clone());
                            self.known_class_methods
                                .insert((name.clone(), m.name.clone()), m.name.clone());
                            self.known_method_param_names.insert(
                                (name.clone(), m.name.clone()),
                                m.params.iter().skip(1).map(|p| p.name.clone()).collect(),
                            );
                            self.known_method_param_types.insert(
                                (name.clone(), m.name.clone()),
                                m.params
                                    .iter()
                                    .skip(1)
                                    .map(|p| self.map_type_expr(p.type_annotation.as_ref()))
                                    .collect(),
                            );
                            self.known_method_return_types.insert(
                                (name.clone(), m.name.clone()),
                                self.map_type_expr(m.return_type.as_ref()),
                            );
                            self.known_method_defaults.insert(
                                (name.clone(), m.name.clone()),
                                m.params.iter().skip(1).map(|p| p.default.clone()).collect(),
                            );
                            if let Some((index, param)) = m
                                .params
                                .iter()
                                .skip(1)
                                .enumerate()
                                .find(|(_, param)| param.is_gather)
                            {
                                if let Some(TypeExpr::Named { name: bundle, .. }) =
                                    &param.type_annotation
                                {
                                    self.known_method_gather.insert(
                                        (name.clone(), m.name.clone()),
                                        (index, bundle.clone()),
                                    );
                                }
                            }
                        }
                        ClassMember::ClassVar(field) => {
                            members.push(field.name.clone());
                            self.known_class_vars.insert(
                                (name.clone(), field.name.clone()),
                                self.map_type_expr(Some(&field.type_annotation)),
                            );
                        }
                        ClassMember::Factory(f) => {
                            members.push(f.name.clone());
                            self.known_factories
                                .insert((name.clone(), f.name.clone()), f.name.clone());
                        }
                        ClassMember::Setter(s) => {
                            members.push(s.name.clone());
                            self.known_setters
                                .insert((name.clone(), s.name.clone()), s.name.clone());
                            self.known_setter_types.insert(
                                (name.clone(), s.name.clone()),
                                self.map_type_expr(s.param.type_annotation.as_ref()),
                            );
                        }
                        _ => {}
                    }
                }
                self.known_classes.insert(name.clone(), fields);
                self.known_class_members.insert(name.clone(), members);
            } else if let Stmt::Function(f) = stmt {
                let is_contextmanager = f.decorators.iter().any(|decorator| {
                    matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager")
                });
                let ret_ty = if f.is_async || is_contextmanager {
                    self.async_fns.insert(f.name.clone());
                    "LucidVal".to_string()
                } else {
                    self.map_type_expr(f.return_type.as_ref())
                };
                self.known_fns.insert(f.name.clone(), ret_ty);
                self.known_fn_params.insert(
                    f.name.clone(),
                    f.params
                        .iter()
                        .map(|param| self.map_type_expr(param.type_annotation.as_ref()))
                        .collect(),
                );
                self.known_fn_param_names.insert(
                    f.name.clone(),
                    f.params.iter().map(|param| param.name.clone()).collect(),
                );
                self.known_fn_defaults.insert(
                    f.name.clone(),
                    f.params.iter().map(|param| param.default.clone()).collect(),
                );
                if let Some(index) = f
                    .params
                    .iter()
                    .position(|param| param.is_variadic_positional || param.is_variadic_keyword)
                {
                    self.known_fn_variadic.insert(f.name.clone(), index);
                }
                if f.params.iter().any(|param| param.is_variadic_keyword) {
                    self.known_fn_keyword_variadic.insert(f.name.clone());
                }
                if let Some((index, param)) = f
                    .params
                    .iter()
                    .enumerate()
                    .find(|(_, param)| param.is_gather)
                {
                    if let Some(TypeExpr::Named { name: bundle, .. }) = &param.type_annotation {
                        self.known_fn_gather
                            .insert(f.name.clone(), (index, bundle.clone()));
                    }
                }
                if f.is_dispatch {
                    if let Some(param) = f.params.first() {
                        if let Some(TypeExpr::Named {
                            name: type_name, ..
                        }) = &param.type_annotation
                        {
                            let emitted_name = self.dispatch_name(f, type_name);
                            self.dispatch_fns
                                .entry(f.name.clone())
                                .or_default()
                                .push((type_name.clone(), emitted_name));
                            let signature = f
                                .params
                                .iter()
                                .map(|param| self.map_type_expr(param.type_annotation.as_ref()))
                                .collect::<Vec<_>>();
                            let emitted_name = self.dispatch_name(f, type_name);
                            self.dispatch_signatures
                                .entry(f.name.clone())
                                .or_default()
                                .push((signature, emitted_name));
                        }
                    }
                }
            }
        }

        // Resolve the single class parent only after every class has been
        // collected. A class may list traits alongside its parent, and the
        // parent may be declared later in the file; selecting the first named
        // base would confuse a trait for a class and emit invalid ancestry
        // metadata.
        for stmt in &module.statements {
            let Stmt::ClassDef { name, bases, .. } = Self::unwrap_export(stmt) else {
                continue;
            };
            if let Some(parent) = bases.iter().find_map(|base| match base {
                TypeExpr::Named {
                    name: base_name, ..
                } if self.known_classes.contains_key(base_name) => Some(base_name.clone()),
                _ => None,
            }) {
                self.known_parents.insert(name.clone(), parent);
            }
        }

        // Native structs use a flat layout.  Expand each derived class with
        // its inherited fields before emitting C declarations and constructors.
        let class_names: Vec<String> = self.known_classes.keys().cloned().collect();
        for class in class_names {
            let mut chain = Vec::new();
            let mut current = self.known_parents.get(&class).cloned();
            let mut seen = HashSet::new();
            while let Some(parent) = current {
                if !seen.insert(parent.clone()) {
                    break;
                }
                chain.push(parent.clone());
                current = self.known_parents.get(&parent).cloned();
            }
            chain.reverse();
            let own = self.known_classes.get(&class).cloned().unwrap_or_default();
            let mut fields = Vec::new();
            for parent in chain {
                for field in self.known_classes.get(&parent).cloned().unwrap_or_default() {
                    if let Some(ty) = self
                        .known_field_types
                        .get(&(parent.clone(), field.clone()))
                        .cloned()
                    {
                        self.known_field_types
                            .entry((class.clone(), field.clone()))
                            .or_insert(ty);
                    }
                    if let Some(default) = self
                        .known_field_defaults
                        .get(&(parent.clone(), field.clone()))
                        .cloned()
                    {
                        self.known_field_defaults
                            .entry((class.clone(), field.clone()))
                            .or_insert(default);
                    }
                    fields.push(field);
                }
            }
            fields.extend(own);
            self.known_classes.insert(class, fields);
        }

        // Resolve simple top-level function aliases before emitting any
        // expressions.  This keeps alias calls independent of declaration
        // order and of the global-variable predeclaration pass.
        for stmt in &module.statements {
            let (target, value) = match stmt {
                Stmt::VarDef {
                    pattern: Pattern::Ident(target, _),
                    value: Some(value),
                    ..
                }
                | Stmt::Assignment {
                    target: Expr::Ident { name: target, .. },
                    value,
                    ..
                } => (target, value),
                _ => continue,
            };
            if let Expr::Ident { name: source, .. } = value {
                let resolved = self
                    .function_aliases
                    .get(source)
                    .cloned()
                    .unwrap_or_else(|| source.clone());
                if self.known_fn_params.contains_key(&resolved) {
                    self.function_aliases.insert(target.clone(), resolved);
                }
            }
        }

        self.emit_preamble();

        // Class variables are file-scope storage used by generated methods,
        // so declare them before emitting any class functions.
        let class_var_decls: Vec<_> = self
            .known_class_vars
            .iter()
            .map(|((class, field), ty)| (class.clone(), field.clone(), ty.clone()))
            .collect();
        for (class, field, ty) in &class_var_decls {
            self.emit_line(&format!("static {ty} lucid_classvar_{class}_{field};"));
        }
        if !class_var_decls.is_empty() {
            self.emit_line("");
        }

        // 1. Emit class forward declarations
        for stmt in &module.statements {
            if let Stmt::ClassDef { name, .. } = Self::unwrap_export(stmt) {
                self.emit_line(&format!("typedef struct {name} {name};"));
                self.emit_line(&format!("static void {name}_freeze(void* raw);"));
            }
        }
        self.emit_line("");
        self.emit_line("static LucidVal lucid_dynamic_attr(LucidVal value, const char* attr, LucidVal fallback, bool has_default);");
        self.emit_line("static bool lucid_dynamic_has_attr(LucidVal value, const char* attr);");
        self.emit_line("static void lucid_dynamic_set_attr(LucidVal object, const char* attr, LucidVal value);");
        self.emit_line("");

        // 2. Emit class struct definitions & constructors
        for stmt in &module.statements {
            if let Stmt::ClassDef { name, body, .. } = Self::unwrap_export(stmt) {
                self.emit_class_def(name, body)?;
            }
        }

        // Union-returning calls are represented as LucidVal in native code.
        // Provide one checked attribute path for those erased objects instead
        // of emitting a C `->field` access against the wrapper value.
        let mut dynamic_classes: Vec<String> = self.known_classes.keys().cloned().collect();
        dynamic_classes.sort();
        self.emit_line("static LucidVal lucid_dynamic_attr(LucidVal value, const char* attr, LucidVal fallback, bool has_default) {");
        self.indent += 1;
        self.emit_line("if (value.type != LUCID_TYPE_PTR || !value.ptr) { if (has_default) return fallback; fprintf(stderr, \"attribute access requires an object\\n\"); exit(1); }");
        self.emit_line("const char* class_name = lucid_object_class_name(value.ptr);");
        self.emit_line("if (!class_name) { if (has_default) return fallback; fprintf(stderr, \"unknown object in attribute access\\n\"); exit(1); }");
        for class in dynamic_classes {
            self.emit_line(&format!("if (strcmp(class_name, \"{class}\") == 0) {{"));
            self.indent += 1;
            self.emit_line(&format!("{class}* self = ({class}*)value.ptr;"));
            let mut getter_owner = Some(class.clone());
            while let Some(owner) = getter_owner.clone() {
                let getter_names: Vec<String> = self
                    .known_getters
                    .keys()
                    .filter(|(candidate, _)| candidate == &owner)
                    .map(|(_, getter)| getter.clone())
                    .collect();
                for getter in getter_names {
                    self.emit_line(&format!(
                        "if (strcmp(attr, \"{getter}\") == 0) return lucid_wrap({owner}_{getter}_get(({owner}*)value.ptr));"
                    ));
                }
                getter_owner = self.known_parents.get(&owner).cloned();
            }
            let fields = self.known_classes.get(&class).cloned().unwrap_or_default();
            for field in fields {
                self.emit_line(&format!(
                    "if (strcmp(attr, \"{field}\") == 0) return lucid_wrap(self->{field});"
                ));
            }
            self.indent -= 1;
            self.emit_line("}");
        }
        self.emit_line("if (has_default) return fallback; fprintf(stderr, \"object has no requested attribute\\n\"); exit(1); return lucid_none();");
        self.indent -= 1;
        self.emit_line("}");
        self.emit_line("static bool lucid_dynamic_has_attr(LucidVal value, const char* attr) {");
        self.indent += 1;
        self.emit_line("if (value.type != LUCID_TYPE_PTR || !value.ptr) return false;");
        self.emit_line("const char* class_name = lucid_object_class_name(value.ptr);");
        self.emit_line("if (!class_name) return false;");
        for class in self.known_classes.keys().cloned().collect::<Vec<_>>() {
            self.emit_line(&format!("if (strcmp(class_name, \"{class}\") == 0) {{"));
            self.indent += 1;
            let fields = self.known_classes.get(&class).cloned().unwrap_or_default();
            for field in fields {
                self.emit_line(&format!("if (strcmp(attr, \"{field}\") == 0) return true;"));
            }
            let mut owner = Some(class.clone());
            while let Some(candidate) = owner {
                let getters: Vec<String> = self
                    .known_getters
                    .keys()
                    .filter(|(class_name, _)| class_name == &candidate)
                    .map(|(_, getter)| getter.clone())
                    .collect();
                for getter in getters {
                    self.emit_line(&format!(
                        "if (strcmp(attr, \"{getter}\") == 0) return true;"
                    ));
                }
                let methods: Vec<String> = self
                    .known_method_param_names
                    .keys()
                    .filter(|(class_name, _)| class_name == &candidate)
                    .map(|(_, method)| method.clone())
                    .collect();
                for method in methods {
                    self.emit_line(&format!(
                        "if (strcmp(attr, \"{method}\") == 0) return true;"
                    ));
                }
                owner = self.known_parents.get(&candidate).cloned();
            }
            self.indent -= 1;
            self.emit_line("}");
        }
        self.emit_line("return false;");
        self.indent -= 1;
        self.emit_line("}");
        self.emit_line("static void lucid_dynamic_set_attr(LucidVal object, const char* attr, LucidVal value) {");
        self.indent += 1;
        self.emit_line("if (object.type != LUCID_TYPE_PTR || !object.ptr) { fprintf(stderr, \"attribute assignment requires an object\\n\"); exit(1); }");
        self.emit_line("if (lucid_object_frozen(object.ptr)) { fprintf(stderr, \"cannot mutate frozen object\\n\"); exit(1); }");
        self.emit_line("const char* class_name = lucid_object_class_name(object.ptr);");
        let mut setter_classes: Vec<String> = self.known_classes.keys().cloned().collect();
        setter_classes.sort();
        for class in setter_classes {
            self.emit_line(&format!(
                "if (class_name && strcmp(class_name, \"{class}\") == 0) {{"
            ));
            self.indent += 1;
            let mut setter_owner = Some(class.clone());
            while let Some(owner) = setter_owner.clone() {
                let setters: Vec<String> = self
                    .known_setters
                    .keys()
                    .filter(|(class_name, _)| class_name == &owner)
                    .map(|(_, setter)| setter.clone())
                    .collect();
                for setter in setters {
                    let ty = self
                        .known_setter_types
                        .get(&(owner.clone(), setter.clone()))
                        .cloned()
                        .unwrap_or_else(|| "LucidVal".to_string());
                    let converted = match ty.as_str() {
                        "LucidVal" => "value".to_string(),
                        "int64_t" => "lucid_as_int(value)".to_string(),
                        "double" => "lucid_as_float(value)".to_string(),
                        "bool" => "lucid_as_bool(value)".to_string(),
                        "const char*" => "lucid_as_str(value)".to_string(),
                        "LucidList*" => "lucid_as_list(value)".to_string(),
                        "LucidDict*" => "lucid_as_dict(value)".to_string(),
                        "LucidSet*" => "lucid_as_set(value)".to_string(),
                        other => format!("({other})lucid_as_ptr(value)"),
                    };
                    self.emit_line(&format!(
                        "if (strcmp(attr, \"{setter}\") == 0) {{ {owner}_{setter}_set(({owner}*)object.ptr, {converted}); return; }}"
                    ));
                }
                setter_owner = self.known_parents.get(&owner).cloned();
            }
            let fields = self.known_classes.get(&class).cloned().unwrap_or_default();
            for field in fields {
                let ty = self
                    .known_field_types
                    .get(&(class.clone(), field.clone()))
                    .cloned()
                    .unwrap_or_else(|| "LucidVal".to_string());
                let converted = match ty.as_str() {
                    "LucidVal" => "value".to_string(),
                    "int64_t" => "lucid_as_int(value)".to_string(),
                    "double" => "lucid_as_float(value)".to_string(),
                    "bool" => "lucid_as_bool(value)".to_string(),
                    "const char*" => "lucid_as_str(value)".to_string(),
                    "LucidList*" => "lucid_as_list(value)".to_string(),
                    "LucidDict*" => "lucid_as_dict(value)".to_string(),
                    "LucidSet*" => "lucid_as_set(value)".to_string(),
                    other => format!("({other})lucid_as_ptr(value)"),
                };
                self.emit_line(&format!(
                    "if (strcmp(attr, \"{field}\") == 0) {{ (({class}*)object.ptr)->{field} = {converted}; return; }}"
                ));
            }
            self.indent -= 1;
            self.emit_line("}");
        }
        self.emit_line("fprintf(stderr, \"object has no settable attribute\\n\"); exit(1);");
        self.indent -= 1;
        self.emit_line("}");
        self.emit_line("");

        // Collect top-level statements and global variables
        let top_level_stmts: Vec<&Stmt> = module
            .statements
            .iter()
            .filter(|s| {
                let s = Self::unwrap_export(s);
                !matches!(
                    s,
                    Stmt::Function(_)
                        | Stmt::ClassDef { .. }
                        | Stmt::InterfaceDef { .. }
                        | Stmt::TraitDef { .. }
                )
            })
            .collect();

        let mut top_vars = HashMap::new();
        for stmt in &top_level_stmts {
            self.collect_vars_from_stmt(stmt, &mut top_vars);
        }
        self.global_vars = top_vars.clone();

        // Emit global variable declarations at file scope
        for (name, ty) in &top_vars {
            self.emit_line(&format!("static {ty} lucid_var_{name};"));
            self.emit_line(&format!("static bool lucid_alive_{name} = false;"));
            self.alive_declarations.insert(name.clone());
        }
        self.emit_line("");

        // 3. Emit function prototypes
        for stmt in &module.statements {
            if let Stmt::Function(f) = Self::unwrap_export(stmt) {
                let is_contextmanager = f.decorators.iter().any(|decorator| {
                    matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager")
                });
                let ret_ty = if f.is_async || is_contextmanager {
                    "LucidVal".to_string()
                } else {
                    self.map_type_expr(f.return_type.as_ref())
                };
                let params = self.emit_param_list(&f.params);
                self.emit_line(&format!(
                    "{ret_ty} {}({params});",
                    self.dispatch_name(f, &f.name)
                ));
            }
        }
        self.emit_line("");

        // 4. Emit function bodies
        let top_function_aliases = self.function_aliases.clone();
        for stmt in &module.statements {
            if let Stmt::Function(f) = Self::unwrap_export(stmt) {
                self.anonymous_bindings.clear();
                self.partial_bindings.clear();
                self.function_aliases.clear();
                self.emit_function(f)?;
            }
        }
        self.function_aliases = top_function_aliases;

        // 5. Emit main() for top-level code
        self.emit_line("int main(int argc, char** argv) {");
        self.indent += 1;
        self.emit_line("(void)argc; (void)argv;");
        self.var_types.clear();
        self.deleted_bindings.clear();
        self.alive_declarations.clear();
        for name in self.global_vars.keys() {
            self.alive_declarations.insert(name.clone());
        }
        self.anonymous_bindings.clear();
        self.partial_bindings.clear();

        // Class-member defaults are initialized once, when the module starts.
        for stmt in &module.statements {
            if let Stmt::ClassDef { name, body, .. } = Self::unwrap_export(stmt) {
                for member in body {
                    if let ClassMember::ClassVar(field) = member {
                        if let Some(default) = &field.default {
                            let value = self.emit_expr(default)?;
                            self.emit_line(&format!(
                                "lucid_classvar_{name}_{} = {};",
                                field.name, value
                            ));
                        }
                    }
                }
            }
        }

        for stmt in top_level_stmts {
            self.emit_stmt(stmt)?;
        }

        self.emit_line("return 0;");
        self.indent -= 1;
        self.emit_line("}");

        Ok(self.buffer.clone())
    }

    fn emit_preamble(&mut self) {
        self.buffer.push_str(r#"#define _POSIX_C_SOURCE 199309L
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
#include <ctype.h>
#include <errno.h>
#include <string.h>
#include <math.h>
#include <time.h>
#include <setjmp.h>
#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif
#ifndef M_E
#define M_E 2.71828182845904523536
#endif

typedef struct LucidList LucidList;
typedef struct LucidDict LucidDict;
typedef struct LucidSet LucidSet;
typedef struct LucidFuture LucidFuture;
typedef struct LucidContext LucidContext;
typedef struct LucidExceptionFrame LucidExceptionFrame;
typedef void (*LucidObjectFreezer)(void*);
typedef bool (*LucidObjectTruthy)(void*);
typedef struct { void* ptr; const char* class_name; bool frozen; LucidObjectFreezer freezer; LucidObjectTruthy truthy; } LucidObjectTag;
static LucidObjectTag* lucid_object_tags = NULL;
static size_t lucid_object_tag_count = 0;
static size_t lucid_object_tag_capacity = 0;
static inline void lucid_register_object(void* ptr, const char* class_name, LucidObjectFreezer freezer, LucidObjectTruthy truthy) {
    if (!ptr) return;
    if (lucid_object_tag_count == SIZE_MAX) {
        fprintf(stderr, "too many registered objects\n"); exit(1);
    }
    if (lucid_object_tag_count == lucid_object_tag_capacity) {
        if (lucid_object_tag_capacity > SIZE_MAX / 2) {
            fprintf(stderr, "too many registered objects\n"); exit(1);
        }
        size_t next = lucid_object_tag_capacity ? lucid_object_tag_capacity * 2 : 64;
        if (next > SIZE_MAX / sizeof(LucidObjectTag)) {
            fprintf(stderr, "object registry allocation overflow\n"); exit(1);
        }
        LucidObjectTag* grown = (LucidObjectTag*)realloc(lucid_object_tags, sizeof(LucidObjectTag) * next);
        if (!grown) { fprintf(stderr, "out of memory registering object\n"); exit(1); }
        lucid_object_tags = grown;
        lucid_object_tag_capacity = next;
    }
    lucid_object_tags[lucid_object_tag_count++] = (LucidObjectTag){ptr, class_name, false, freezer, truthy};
}
static inline bool lucid_object_is(void* ptr, const char* class_name) {
    // A derived object has one registry entry for its concrete class and one
    // for each ancestor. Keep scanning entries for the same pointer: the
    // first entry is the concrete class, so returning false there would make
    // every erased parent check fail.
    for (size_t i = 0; i < lucid_object_tag_count; ++i)
        if (lucid_object_tags[i].ptr == ptr && strcmp(lucid_object_tags[i].class_name, class_name) == 0)
            return true;
    return false;
}
static inline const char* lucid_object_class_name(void* ptr) {
    for (size_t i = 0; i < lucid_object_tag_count; ++i)
        if (lucid_object_tags[i].ptr == ptr) return lucid_object_tags[i].class_name;
    return NULL;
}
static inline bool lucid_object_frozen(void* ptr) {
    for (size_t i = 0; i < lucid_object_tag_count; ++i) if (lucid_object_tags[i].ptr == ptr) return lucid_object_tags[i].frozen;
    return false;
}
static inline void lucid_freeze_object(void* ptr) {
    for (size_t i = 0; i < lucid_object_tag_count; ++i) if (lucid_object_tags[i].ptr == ptr) {
        if (lucid_object_tags[i].frozen) return;
        lucid_object_tags[i].frozen = true;
        if (lucid_object_tags[i].freezer) lucid_object_tags[i].freezer(ptr);
        return;
    }
}
static inline bool lucid_object_truthy(void* ptr) {
    for (size_t i = 0; i < lucid_object_tag_count; ++i)
        if (lucid_object_tags[i].ptr == ptr && lucid_object_tags[i].truthy)
            return lucid_object_tags[i].truthy(ptr);
    return true;
}

typedef enum {
    LUCID_TYPE_NONE = 0,
    LUCID_TYPE_INT,
    LUCID_TYPE_FLOAT,
    LUCID_TYPE_COMPLEX,
    LUCID_TYPE_BIGINT,
    LUCID_TYPE_BOOL,
    LUCID_TYPE_STR,
    LUCID_TYPE_LIST,
    LUCID_TYPE_DICT,
    LUCID_TYPE_SET,
    LUCID_TYPE_PTR,
    LUCID_TYPE_FUTURE,
    LUCID_TYPE_CONTEXT
} LucidValType;

typedef struct {
    int64_t i;
    double f;
    double real;
    double imag;
    const char* bigint;
    bool b;
    const char* s;
    LucidList* list;
    LucidDict* dict;
    LucidSet* set;
    LucidFuture* future;
    LucidContext* context;
    void* ptr;
    LucidValType type;
} LucidVal;

struct LucidList {
    LucidVal* items;
    int64_t len;
    int64_t cap;
    bool frozen;
};

struct LucidDict {
    LucidVal* keys;
    LucidVal* values;
    int64_t len;
    int64_t cap;
    bool frozen;
};

struct LucidSet {
    LucidVal* items;
    int64_t len;
    int64_t cap;
    bool frozen;
};

struct LucidFuture {
    LucidVal (*thunk)(LucidList*);
    LucidList* args;
    bool awaited;
};

struct LucidContext {
    LucidVal value;
    void (*teardown)(LucidList*);
    void (*error_teardown)(LucidList*);
    LucidList* args;
    bool exited;
};
static bool lucid_context_failed = false;

struct LucidExceptionFrame {
    jmp_buf jump;
    LucidExceptionFrame* previous;
};
static LucidExceptionFrame* lucid_current_exception = NULL;
static LucidVal lucid_pending_exception;

static inline LucidList* lucid_list_new(int64_t cap);
static inline void lucid_list_append(LucidList* l, LucidVal v);
static inline const char* lucid_as_str(LucidVal v);
static inline void lucid_print_val(LucidVal v);
static inline void lucid_raise_value(LucidVal v) {
    lucid_pending_exception = v;
    if (lucid_current_exception) longjmp(lucid_current_exception->jump, 1);
    (void)v;
    fprintf(stderr, "raised invariant\n");
    exit(1);
}

static inline const char* lucid_str_index(const char* s, int64_t idx) {
    if (!s) { fprintf(stderr, "index %lld out of range\n", (long long)idx); exit(1); }
    int64_t len = 0; for (const unsigned char* p = (const unsigned char*)s; *p; ++len) p += (*p < 0x80 ? 1 : ((*p & 0xe0) == 0xc0 ? 2 : ((*p & 0xf0) == 0xe0 ? 3 : 4)));
    if (idx < 0) idx += len;
    if (idx < 0 || idx >= len) {
        fprintf(stderr, "index %lld out of range\n", (long long)idx);
        exit(1);
    }
    const unsigned char* p = (const unsigned char*)s;
    for (int64_t i = 0; i < idx; ++i) p += (*p < 0x80 ? 1 : ((*p & 0xe0) == 0xc0 ? 2 : ((*p & 0xf0) == 0xe0 ? 3 : 4)));
    int width = *p < 0x80 ? 1 : ((*p & 0xe0) == 0xc0 ? 2 : ((*p & 0xf0) == 0xe0 ? 3 : 4));
    char* out = (char*)malloc((size_t)width + 1); if (!out) return "";
    memcpy(out, p, (size_t)width); out[width] = '\0'; return out;
}

static inline LucidVal lucid_none(void) {
    LucidVal v = {0}; v.type = LUCID_TYPE_NONE; return v;
}
static inline LucidVal lucid_future(LucidVal (*thunk)(LucidList*), LucidList* args) {
    LucidFuture* future = (LucidFuture*)malloc(sizeof(LucidFuture));
    if (!future) { fprintf(stderr, "out of memory allocating future\n"); exit(1); }
    future->thunk = thunk;
    future->args = args;
    future->awaited = false;
    LucidVal v = {0}; v.type = LUCID_TYPE_FUTURE; v.future = future; v.ptr = (void*)future; return v;
}
static inline LucidVal lucid_await(LucidVal value) {
    if (value.type != LUCID_TYPE_FUTURE || !value.future) return value;
    if (value.future->awaited) { fprintf(stderr, "future has already been awaited\n"); exit(1); }
    value.future->awaited = true;
    return value.future->thunk(value.future->args);
}
static inline LucidVal lucid_context(LucidVal value, void (*teardown)(LucidList*), void (*error_teardown)(LucidList*), LucidList* args) {
    LucidContext* context = (LucidContext*)malloc(sizeof(LucidContext));
    if (!context) { fprintf(stderr, "out of memory allocating context\n"); exit(1); }
    context->value = value;
    context->teardown = teardown;
    context->error_teardown = error_teardown;
    context->args = args;
    context->exited = false;
    LucidVal v = {0}; v.type = LUCID_TYPE_CONTEXT; v.context = context; v.ptr = (void*)context; return v;
}
static inline LucidVal lucid_context_value(LucidVal value) {
    return value.type == LUCID_TYPE_CONTEXT && value.context ? value.context->value : value;
}
static inline void lucid_context_exit(LucidVal value, bool failed) {
    if (value.type != LUCID_TYPE_CONTEXT || !value.context || value.context->exited) return;
    value.context->exited = true;
    lucid_context_failed = failed;
    void (*teardown)(LucidList*) = failed && value.context->error_teardown
        ? value.context->error_teardown : value.context->teardown;
    if (teardown) teardown(value.context->args);
    lucid_context_failed = false;
}
static inline LucidVal lucid_int(int64_t i) {
    LucidVal v = {0}; v.type = LUCID_TYPE_INT; v.i = i; v.f = (double)i; v.real = (double)i; v.b = (i != 0); return v;
}
static inline int64_t lucid_checked_add(int64_t a, int64_t b) {
    if ((b > 0 && a > INT64_MAX - b) || (b < 0 && a < INT64_MIN - b)) {
        fprintf(stderr, "integer overflow in +\n"); exit(1);
    }
    return a + b;
}
static inline int64_t lucid_checked_sub(int64_t a, int64_t b) {
    if ((b < 0 && a > INT64_MAX + b) || (b > 0 && a < INT64_MIN + b)) {
        fprintf(stderr, "integer overflow in -\n"); exit(1);
    }
    return a - b;
}
static inline int64_t lucid_checked_mul(int64_t a, int64_t b) {
    if (a == 0 || b == 0) return 0;
    if (a == -1 && b == INT64_MIN || b == -1 && a == INT64_MIN) {
        fprintf(stderr, "integer overflow in *\n"); exit(1);
    }
    if (a > 0) {
        if ((b > 0 && a > INT64_MAX / b) || (b < 0 && b < INT64_MIN / a)) {
            fprintf(stderr, "integer overflow in *\n"); exit(1);
        }
    } else if ((b > 0 && a < INT64_MIN / b) || (b < 0 && a < INT64_MAX / b)) {
        fprintf(stderr, "integer overflow in *\n"); exit(1);
    }
    return a * b;
}
static inline int64_t lucid_checked_neg(int64_t value) {
    if (value == INT64_MIN) {
        fprintf(stderr, "integer overflow in unary -\n"); exit(1);
    }
    return -value;
}
static inline int64_t lucid_checked_shl(int64_t value, int64_t shift) {
    if (shift < 0 || shift >= 64) {
        fprintf(stderr, "invalid shift count\n"); exit(1);
    }
    if (shift == 0) return value;
    uint64_t shifted = ((uint64_t)value) << (unsigned)shift;
    int64_t result = (int64_t)shifted;
    if ((result >> shift) != value) {
        fprintf(stderr, "integer overflow in <<\n"); exit(1);
    }
    return result;
}
static inline int64_t lucid_checked_shr(int64_t value, int64_t shift) {
    if (shift < 0 || shift >= 64) {
        fprintf(stderr, "invalid shift count\n"); exit(1);
    }
    if (shift == 0) return value;
    uint64_t shifted = ((uint64_t)value) >> (unsigned)shift;
    if (value < 0) shifted |= UINT64_MAX << (64u - (unsigned)shift);
    return (int64_t)shifted;
}
static inline int64_t lucid_int_floor_div(int64_t a, int64_t b) {
    if (b == 0) {
        /* Preserve Lucid's existing special-value diagnostics. */
        if (a == 0) return INT64_MIN;
        return a < 0 ? INT64_MIN + 1 : INT64_MAX;
    }
    /* Avoid the one signed division that C cannot represent. */
    if (a == INT64_MIN && b == -1) return INT64_MIN;
    int64_t q = a / b;
    int64_t r = a % b;
    /* Lucid pairs % with floor division, not truncating division. */
    if (r != 0 && ((a < 0) != (b < 0))) --q;
    return q;
}
static inline int64_t lucid_int_mod(int64_t a, int64_t b) {
    if (b == 0 || (a == INT64_MIN && b == -1)) return INT64_MIN;
    int64_t q = lucid_int_floor_div(a, b);
    return a - q * b;
}
static inline double lucid_float_mod(double a, double b) {
    if (b == 0.0) {
        fprintf(stderr, "division by zero in %%\n");
        exit(1);
    }
    if (isinf(b) && isfinite(a)) return a;
    return a - floor(a / b) * b;
}
static inline double lucid_float_floor_div(double a, double b) {
    if (b == 0.0) {
        fprintf(stderr, "division by zero in //\n");
        exit(1);
    }
    return floor(a / b);
}
static inline LucidVal lucid_float(double f) {
    LucidVal v = {0}; v.type = LUCID_TYPE_FLOAT; v.f = f; v.i = (int64_t)f; v.real = f; v.b = (f != 0.0); return v;
}
static inline LucidVal lucid_complex(double real, double imag) {
    LucidVal v = {0}; v.type = LUCID_TYPE_COMPLEX; v.real = real; v.imag = imag; return v;
}
static inline LucidVal lucid_bigint(const char* digits) {
    LucidVal v = {0}; v.type = LUCID_TYPE_BIGINT; v.bigint = digits; v.s = digits; v.ptr = (void*)digits; return v;
}
static inline const char* lucid_bigint_text(LucidVal v, char* buffer) {
    if (v.type == LUCID_TYPE_BIGINT && v.bigint) return v.bigint;
    snprintf(buffer, 64, "%lld", (long long)v.i);
    return buffer;
}
static inline const char* lucid_bigint_mag(const char* s) { return *s == '-' ? s + 1 : s; }
static inline int lucid_bigint_sign(const char* s) { return *s == '-' && s[1] != '0' ? -1 : 1; }
static inline int lucid_bigint_cmp_mag(const char* a, const char* b) {
    a = lucid_bigint_mag(a); b = lucid_bigint_mag(b);
    while (*a == '0' && a[1]) ++a; while (*b == '0' && b[1]) ++b;
    size_t la = strlen(a), lb = strlen(b);
    if (la != lb) return la < lb ? -1 : 1;
    int c = strcmp(a, b); return c < 0 ? -1 : (c > 0 ? 1 : 0);
}
static inline char* lucid_bigint_add_mag(const char* a, const char* b) {
    a = lucid_bigint_mag(a); b = lucid_bigint_mag(b);
    size_t la = strlen(a), lb = strlen(b), n = (la > lb ? la : lb) + 2;
    char* out = (char*)malloc(n); size_t pos = n - 1; out[pos] = '\0'; int carry = 0;
    while (la || lb || carry) { int d = carry; if (la) d += a[--la] - '0'; if (lb) d += b[--lb] - '0'; out[--pos] = (char)('0' + d % 10); carry = d / 10; }
    memmove(out, out + pos, n - pos); return out;
}
static inline char* lucid_bigint_sub_mag(const char* a, const char* b) {
    a = lucid_bigint_mag(a); b = lucid_bigint_mag(b);
    size_t la = strlen(a), lb = strlen(b), n = la + 2;
    char* out = (char*)malloc(n); size_t pos = n - 1; out[pos] = '\0'; int borrow = 0;
    while (la) { int d = (a[--la] - '0') - borrow - (lb ? b[--lb] - '0' : 0); if (d < 0) { d += 10; borrow = 1; } else borrow = 0; out[--pos] = (char)('0' + d); }
    while (pos + 1 < n - 1 && out[pos] == '0') ++pos;
    memmove(out, out + pos, n - pos); return out;
}
static inline char* lucid_bigint_mul_mag(const char* a, const char* b) {
    a = lucid_bigint_mag(a); b = lucid_bigint_mag(b);
    size_t la = strlen(a), lb = strlen(b); int* digits = (int*)calloc(la + lb + 1, sizeof(int));
    for (size_t i = la; i-- > 0;) { int carry = 0; for (size_t j = lb; j-- > 0;) { size_t k = i + j + 1; int t = (a[i] - '0') * (b[j] - '0') + digits[k] + carry; digits[k] = t % 10; carry = t / 10; } digits[i] += carry; }
    size_t total = la + lb;
    size_t first = 0; while (first < total && digits[first] == 0) ++first;
    char* out = (char*)malloc(total + 2); size_t p = 0; if (first == total) out[p++] = '0'; else for (size_t i = first; i < total; ++i) out[p++] = (char)('0' + digits[i]);
    out[p] = '\0'; free(digits); return out;
}
static inline char* lucid_bigint_signed_op(const char* a, const char* b, char op) {
    int sa = lucid_bigint_sign(a), sb = lucid_bigint_sign(b); if (op == '-') sb = -sb;
    char* mag = NULL; int sign = 1;
    if (op == '*') { mag = lucid_bigint_mul_mag(a, b); sign = sa * lucid_bigint_sign(b); }
    else if (sa == sb) { mag = lucid_bigint_add_mag(a, b); sign = sa; }
    else { int cmp = lucid_bigint_cmp_mag(a, b); if (cmp == 0) mag = strdup("0"); else if (cmp > 0) { mag = lucid_bigint_sub_mag(a, b); sign = sa; } else { mag = lucid_bigint_sub_mag(b, a); sign = sb; } }
    if (sign < 0 && strcmp(mag, "0") != 0) { size_t n = strlen(mag); char* out = (char*)malloc(n + 2); out[0] = '-'; memcpy(out + 1, mag, n + 1); free(mag); return out; }
    return mag;
}
static inline int lucid_bigint_cmp(LucidVal a, LucidVal b) {
    char ab[64], bb[64]; const char* as = lucid_bigint_text(a, ab); const char* bs = lucid_bigint_text(b, bb);
    int sa = lucid_bigint_sign(as), sb = lucid_bigint_sign(bs); if (sa != sb) return sa < sb ? -1 : 1;
    int c = lucid_bigint_cmp_mag(as, bs); return sa < 0 ? -c : c;
}
static inline LucidVal lucid_bigint_binop(LucidVal a, LucidVal b, char op) {
    char ab[64], bb[64]; return lucid_bigint(lucid_bigint_signed_op(lucid_bigint_text(a, ab), lucid_bigint_text(b, bb), op));
}
static inline LucidVal lucid_bigint_neg(LucidVal value) { return lucid_bigint_binop(lucid_bigint("0"), value, '-'); }
static inline LucidVal lucid_bigint_divmod(LucidVal a, LucidVal b, bool remainder, bool floor) {
    char ab[64], bb[64]; const char* as = lucid_bigint_text(a, ab); const char* bs = lucid_bigint_text(b, bb);
    if (strcmp(lucid_bigint_mag(bs), "0") == 0) return lucid_bigint("int.nan");
    const char* am = lucid_bigint_mag(as); const char* bm = lucid_bigint_mag(bs);
    size_t n = strlen(am); char* quotient = (char*)malloc(n + 1); char* rem = strdup("0");
    for (size_t i = 0; i < n; ++i) {
        size_t rl = strlen(rem); char* next = (char*)malloc(rl + 2); memcpy(next, rem, rl); next[rl] = am[i]; next[rl + 1] = '\0'; free(rem); rem = next;
        while (rem[0] == '0' && rem[1]) memmove(rem, rem + 1, strlen(rem));
        int digit = 0; while (lucid_bigint_cmp_mag(rem, bm) >= 0) { char* reduced = lucid_bigint_sub_mag(rem, bm); free(rem); rem = reduced; ++digit; }
        quotient[i] = (char)('0' + digit);
    }
    quotient[n] = '\0'; size_t first = 0; while (quotient[first] == '0' && quotient[first + 1]) ++first;
    int sign = lucid_bigint_sign(as) * lucid_bigint_sign(bs);
    bool has_remainder = strcmp(rem, "0") != 0;
    char* selected;
    if (remainder) {
        if (floor && sign < 0 && has_remainder) { char* adjusted = lucid_bigint_sub_mag(bs, rem); free(rem); rem = adjusted; }
        selected = rem;
    } else {
        selected = strdup(quotient + first);
        if (floor && sign < 0 && has_remainder) { char* adjusted = lucid_bigint_add_mag(selected, "1"); free(selected); selected = adjusted; }
        free(rem);
    }
    if (!remainder && sign < 0 && strcmp(selected, "0") != 0) { size_t l = strlen(selected); char* out = (char*)malloc(l + 2); out[0] = '-'; memcpy(out + 1, selected, l + 1); free(selected); selected = out; }
    free(quotient); return lucid_bigint(selected);
}
static inline LucidVal lucid_bigint_pow(LucidVal base, LucidVal exponent) {
    char eb[64]; const char* es = lucid_bigint_text(exponent, eb);
    if (*es == '-') { char ab[64]; return lucid_float(pow(strtod(lucid_bigint_text(base, ab), NULL), strtod(es, NULL))); }
    unsigned long long e = strtoull(lucid_bigint_mag(es), NULL, 10);
    LucidVal result = lucid_bigint("1"), power = base;
    while (e) { if (e & 1ULL) result = lucid_bigint_binop(result, power, '*'); e >>= 1; if (e) power = lucid_bigint_binop(power, power, '*'); }
    return result;
}
static inline LucidVal lucid_complex_from_value(LucidVal value, bool has_imag, LucidVal imag) {
    if (value.type == LUCID_TYPE_COMPLEX && !has_imag) return value;
    double real = value.type == LUCID_TYPE_FLOAT ? value.f : (double)value.i;
    double imaginary = imag.type == LUCID_TYPE_FLOAT ? imag.f : (double)imag.i;
    return lucid_complex(real, has_imag ? imaginary : 0.0);
}
static inline LucidVal lucid_bool(bool b) {
    LucidVal v = {0}; v.type = LUCID_TYPE_BOOL; v.b = b; v.i = b ? 1 : 0; v.f = b ? 1.0 : 0.0; return v;
}
static inline LucidVal lucid_str(const char* s) {
    LucidVal v = {0}; v.type = LUCID_TYPE_STR; v.s = s; v.ptr = (void*)s; return v;
}
static inline LucidVal lucid_env_var(LucidVal name, bool has_default, LucidVal fallback) {
    if (name.type != LUCID_TYPE_STR || !name.s) {
        fprintf(stderr, "env_var() name must be a string\n");
        exit(1);
    }
    const char* value = getenv(name.s);
    if (value) return lucid_str(value);
    return has_default ? fallback : lucid_none();
}
static inline LucidVal lucid_sys_platform(void) {
#if defined(_WIN32)
    return lucid_str("windows");
#elif defined(__APPLE__)
    return lucid_str("macos");
#elif defined(__linux__)
    return lucid_str("linux");
#elif defined(__FreeBSD__)
    return lucid_str("freebsd");
#else
    return lucid_str("unknown");
#endif
}
static inline LucidVal lucid_sys_version(void) { return lucid_str("0.1.0"); }
static inline LucidVal lucid_sys_argv(void) {
    LucidList* values = lucid_list_new(1);
    lucid_list_append(values, lucid_str("lucid"));
    LucidVal result = {0};
    result.type = LUCID_TYPE_LIST;
    result.list = values;
    result.ptr = (void*)values;
    return result;
}
static inline LucidVal lucid_read_file(LucidVal path) {
    if (path.type != LUCID_TYPE_STR || !path.s) {
        fprintf(stderr, "read_file() path must be a string\n");
        exit(1);
    }
    FILE* file = fopen(path.s, "rb");
    if (!file) {
        fprintf(stderr, "read_file('%s') failed\n", path.s);
        exit(1);
    }
    if (fseek(file, 0, SEEK_END) != 0) { fclose(file); exit(1); }
    long size = ftell(file);
    if (size < 0 || fseek(file, 0, SEEK_SET) != 0) { fclose(file); exit(1); }
    char* contents = (char*)malloc((size_t)size + 1);
    if (!contents) { fclose(file); exit(1); }
    size_t read = fread(contents, 1, (size_t)size, file);
    fclose(file);
    if (read != (size_t)size) { free(contents); exit(1); }
    contents[size] = '\0';
    return lucid_str(contents);
}
static inline LucidVal lucid_write_file(LucidVal path, LucidVal contents) {
    if (path.type != LUCID_TYPE_STR || !path.s) {
        fprintf(stderr, "write_file() path must be a string\n");
        exit(1);
    }
    if (contents.type != LUCID_TYPE_STR || !contents.s) {
        fprintf(stderr, "write_file() content must be a string\n");
        exit(1);
    }
    FILE* file = fopen(path.s, "wb");
    if (!file) { fprintf(stderr, "write_file('%s') failed\n", path.s); exit(1); }
    size_t length = strlen(contents.s);
    bool wrote = fwrite(contents.s, 1, length, file) == length;
    bool closed = fclose(file) == 0;
    if (!wrote || !closed) exit(1);
    return lucid_none();
}

static inline const char* lucid_str_concat(const char* a, const char* b) {
    if (!a) a = "";
    if (!b) b = "";
    size_t na = strlen(a), nb = strlen(b);
    char* out = (char*)malloc(na + nb + 1);
    if (!out) return "";
    memcpy(out, a, na);
    memcpy(out + na, b, nb);
    out[na + nb] = '\0';
    return out;
}

static inline LucidList* lucid_str_split(const char* s, const char* sep, bool has_sep) {
    LucidList* out = lucid_list_new(8);
    if (!s) return out;
    if (!has_sep) {
        const char* p = s;
        while (*p) {
            while (*p && isspace((unsigned char)*p)) p++;
            if (!*p) break;
            const char* start = p;
            while (*p && !isspace((unsigned char)*p)) p++;
            size_t n = (size_t)(p - start);
            char* word = (char*)malloc(n + 1);
            if (!word) return out;
            memcpy(word, start, n); word[n] = '\0';
            lucid_list_append(out, lucid_str(word));
        }
        return out;
    }
    if (!sep || !*sep) {
        lucid_list_append(out, lucid_str(s));
        return out;
    }
    size_t sep_len = strlen(sep);
    const char* start = s;
    for (const char* p = s; ; p++) {
        if (*p == '\0' || strncmp(p, sep, sep_len) == 0) {
            size_t n = (size_t)(p - start);
            char* piece = (char*)malloc(n + 1);
            if (!piece) return out;
            memcpy(piece, start, n); piece[n] = '\0';
            lucid_list_append(out, lucid_str(piece));
            if (*p == '\0') break;
            p += sep_len - 1;
            start = p + 1;
        }
    }
    return out;
}

static inline const char* lucid_str_upper(const char* s) {
    if (!s) return "";
    size_t n = strlen(s); char* out = (char*)malloc(n + 1);
    if (!out) return "";
    for (size_t i = 0; i < n; i++) out[i] = (char)toupper((unsigned char)s[i]);
    out[n] = '\0'; return out;
}

static inline const char* lucid_str_lower(const char* s) {
    if (!s) return "";
    size_t n = strlen(s); char* out = (char*)malloc(n + 1);
    if (!out) return "";
    for (size_t i = 0; i < n; i++) out[i] = (char)tolower((unsigned char)s[i]);
    out[n] = '\0'; return out;
}
static inline const char* lucid_str_strip(const char* s) {
    if (!s) return "";
    const char* begin = s; while (*begin && isspace((unsigned char)*begin)) ++begin;
    const char* end = begin + strlen(begin); while (end > begin && isspace((unsigned char)end[-1])) --end;
    size_t n = (size_t)(end - begin); char* out = (char*)malloc(n + 1); if (!out) return "";
    memcpy(out, begin, n); out[n] = '\0'; return out;
}
static inline bool lucid_str_startswith(const char* s, const char* prefix) {
    if (!s || !prefix) return false; size_t n = strlen(prefix); return strncmp(s, prefix, n) == 0;
}
static inline bool lucid_str_endswith(const char* s, const char* suffix) {
    if (!s || !suffix) return false; size_t n = strlen(s), m = strlen(suffix);
    return m <= n && strcmp(s + n - m, suffix) == 0;
}
static inline const char* lucid_str_replace(const char* s, const char* old, const char* replacement) {
    if (!s || !old || !replacement || !*old) return s ? s : "";
    size_t n = strlen(s), m = strlen(old), r = strlen(replacement), count = 0;
    for (const char* p = s; (p = strstr(p, old)); p += m) ++count;
    char* out = (char*)malloc(n - count * m + count * r + 1); if (!out) return "";
    const char* src = s; char* dst = out;
    while (1) { const char* hit = strstr(src, old); if (!hit) { strcpy(dst, src); break; }
        size_t prefix = (size_t)(hit - src); memcpy(dst, src, prefix); dst += prefix;
        memcpy(dst, replacement, r); dst += r; src = hit + m; }
    return out;
}

static inline const char* lucid_to_str(LucidVal v);
static inline const char* lucid_str_join(const char* sep, LucidList* values) {
    if (!sep) sep = "";
    if (!values || values->len == 0) return "";
    size_t total = 1, sep_len = strlen(sep);
    for (int64_t i = 0; i < values->len; i++) {
        const char* item = lucid_to_str(values->items[i]);
        total += strlen(item);
        if (i) total += sep_len;
    }
    char* out = (char*)malloc(total);
    if (!out) return "";
    out[0] = '\0';
    for (int64_t i = 0; i < values->len; i++) {
        if (i) strcat(out, sep);
        strcat(out, lucid_to_str(values->items[i]));
    }
    return out;
}
static inline LucidVal lucid_char_str(char* s) {
    return lucid_str((const char*)s);
}
static inline LucidVal lucid_list_val(LucidList* l) {
    LucidVal v = {0}; v.type = LUCID_TYPE_LIST; v.list = l; v.ptr = (void*)l; return v;
}
static inline LucidVal lucid_dict_val(LucidDict* d) {
    LucidVal v = {0}; v.type = LUCID_TYPE_DICT; v.dict = d; v.ptr = (void*)d; return v;
}
static inline LucidVal lucid_set_val(LucidSet* s) {
    LucidVal v = {0}; v.type = LUCID_TYPE_SET; v.set = s; v.ptr = (void*)s; return v;
}
static inline LucidVal lucid_complex_add(LucidVal a, LucidVal b);
static inline LucidVal lucid_complex_sub(LucidVal a, LucidVal b);
static inline LucidVal lucid_complex_mul(LucidVal a, LucidVal b);
static inline LucidVal lucid_complex_div(LucidVal a, LucidVal b);
static inline LucidVal lucid_complex_pow(LucidVal a, LucidVal b);
static inline LucidList* lucid_list_concat(LucidList* left, LucidList* right);
static inline LucidVal lucid_add_value(LucidVal left, LucidVal right) {
    if (left.type == LUCID_TYPE_COMPLEX || right.type == LUCID_TYPE_COMPLEX)
        return lucid_complex_add(left, right);
    if ((left.type == LUCID_TYPE_BIGINT || left.type == LUCID_TYPE_INT) &&
        (right.type == LUCID_TYPE_BIGINT || right.type == LUCID_TYPE_INT))
        return lucid_bigint_binop(left, right, '+');
    if (left.type == LUCID_TYPE_STR && right.type == LUCID_TYPE_STR)
        return lucid_str(lucid_str_concat(left.s, right.s));
    if (left.type == LUCID_TYPE_LIST && right.type == LUCID_TYPE_LIST)
        return lucid_list_val(lucid_list_concat(left.list, right.list));
    if (left.type == LUCID_TYPE_INT && right.type == LUCID_TYPE_INT)
        return lucid_int(lucid_checked_add(left.i, right.i));
    if ((left.type == LUCID_TYPE_INT || left.type == LUCID_TYPE_FLOAT) &&
        (right.type == LUCID_TYPE_INT || right.type == LUCID_TYPE_FLOAT))
        return lucid_float((left.type == LUCID_TYPE_FLOAT ? left.f : (double)left.i) +
                           (right.type == LUCID_TYPE_FLOAT ? right.f : (double)right.i));
    fprintf(stderr, "unsupported operands for +\n");
    exit(1);
}
static inline LucidVal lucid_ptr_val(void* p) {
    LucidVal v = {0}; v.type = LUCID_TYPE_PTR; v.ptr = p; return v;
}
static inline LucidVal _w_val(LucidVal v) { return v; }

#define lucid_wrap(x) _Generic((x), \
    int: lucid_int, \
    long: lucid_int, \
    long long: lucid_int, \
    float: lucid_float, \
    double: lucid_float, \
    bool: lucid_bool, \
    char*: lucid_char_str, \
    const char*: lucid_str, \
    LucidList*: lucid_list_val, \
    LucidDict*: lucid_dict_val, \
    LucidSet*: lucid_set_val, \
    LucidVal: _w_val, \
    default: lucid_ptr_val \
)(x)

static inline int64_t lucid_as_int(LucidVal v) {
    if (v.type == LUCID_TYPE_INT) return v.i;
    if (v.type == LUCID_TYPE_FLOAT) {
        /* Rust's float-to-integer cast saturates (and maps NaN to zero);
         * a direct C cast is undefined outside the representable range. */
        if (isnan(v.f)) return 0;
        if (v.f >= 9223372036854775807.0) return INT64_MAX;
        if (v.f <= -9223372036854775808.0) return INT64_MIN;
        return (int64_t)v.f;
    }
    if (v.type == LUCID_TYPE_BOOL) return v.b ? 1 : 0;
    if (v.type == LUCID_TYPE_STR && v.s) return (int64_t)strtoll(v.s, NULL, 10);
    if (v.type == LUCID_TYPE_BIGINT && v.bigint) return (int64_t)strtoll(v.bigint, NULL, 10);
    return 0;
}
static inline int64_t lucid_int_builtin(LucidVal v) {
    if (v.type == LUCID_TYPE_STR && v.s) {
        char* end = NULL;
        int64_t result = (int64_t)strtoll(v.s, &end, 10);
        while (end && isspace((unsigned char)*end)) ++end;
        if (!end || end == v.s || *end != '\0') {
            fprintf(stderr, "invalid literal for int()\n"); exit(1);
        }
        return result;
    }
    return lucid_as_int(v);
}
static inline int64_t lucid_round_to_int(double value) {
    return lucid_as_int(lucid_float(round(value)));
}
static inline LucidVal lucid_int_dynamic(LucidVal v) {
    if (v.type == LUCID_TYPE_BIGINT && v.bigint) return v;
    if (v.type == LUCID_TYPE_STR && v.s) {
        char* end = NULL;
        errno = 0;
        long long result = strtoll(v.s, &end, 10);
        while (end && isspace((unsigned char)*end)) ++end;
        if (!end || end == v.s || *end != '\0') {
            fprintf(stderr, "invalid literal for int()\n"); exit(1);
        }
        if (errno == ERANGE) return lucid_bigint(v.s);
        return lucid_int((int64_t)result);
    }
    return lucid_wrap(lucid_int_builtin(v));
}
static inline double lucid_as_float(LucidVal v) {
    if (v.type == LUCID_TYPE_FLOAT) return v.f;
    if (v.type == LUCID_TYPE_INT) return (double)v.i;
    if (v.type == LUCID_TYPE_BIGINT && v.bigint) return strtod(v.bigint, NULL);
    if (v.type == LUCID_TYPE_STR && v.s) return strtod(v.s, NULL);
    return 0.0;
}
static inline double lucid_float_builtin(LucidVal v) {
    if (v.type == LUCID_TYPE_STR && v.s) {
        char* end = NULL;
        double result = strtod(v.s, &end);
        while (end && isspace((unsigned char)*end)) ++end;
        if (!end || end == v.s || *end != '\0') {
            fprintf(stderr, "invalid literal for float()\n"); exit(1);
        }
        return result;
    }
    return lucid_as_float(v);
}
static inline bool lucid_as_bool(LucidVal v) {
    if (v.type == LUCID_TYPE_BOOL) return v.b;
    if (v.type == LUCID_TYPE_INT) return v.i != 0;
    if (v.type == LUCID_TYPE_FLOAT) return v.f != 0.0;
    if (v.type == LUCID_TYPE_BIGINT) {
        const char* digits = v.bigint ? v.bigint : "0";
        if (*digits == '-') ++digits;
        while (*digits == '0') ++digits;
        return *digits != '\0';
    }
    if (v.type == LUCID_TYPE_COMPLEX) return v.real != 0.0 || v.imag != 0.0;
    if (v.type == LUCID_TYPE_STR) return v.s && v.s[0] != '\0';
    if (v.type == LUCID_TYPE_LIST) return v.list && v.list->len > 0;
    if (v.type == LUCID_TYPE_DICT) return v.dict && v.dict->len > 0;
    if (v.type == LUCID_TYPE_SET) return v.set && v.set->len > 0;
    if (v.type == LUCID_TYPE_PTR) return lucid_object_truthy(v.ptr);
    return v.type != LUCID_TYPE_NONE;
}
static inline const char* lucid_as_str(LucidVal v) {
    if (v.type == LUCID_TYPE_STR) return v.s ? v.s : "";
    if (v.type == LUCID_TYPE_BIGINT) return v.bigint ? v.bigint : "0";
    return "";
}
static inline const char* lucid_to_str(LucidVal v) {
    if (v.type == LUCID_TYPE_STR) return v.s ? v.s : "";
    char* buf = (char*)malloc(64);
    if (!buf) return "";
    if (v.type == LUCID_TYPE_INT) {
        if (v.i == INT64_MAX) snprintf(buf, 64, "int.inf");
        else if (v.i == INT64_MIN + 1) snprintf(buf, 64, "-int.inf");
        else if (v.i == INT64_MIN) snprintf(buf, 64, "int.nan");
        else snprintf(buf, 64, "%ld", v.i);
        return buf;
    }
    if (v.type == LUCID_TYPE_BIGINT) { snprintf(buf, 64, "%s", v.bigint ? v.bigint : "0"); return buf; }
    if (v.type == LUCID_TYPE_FLOAT) { snprintf(buf, 64, "%.10g", v.f); return buf; }
    if (v.type == LUCID_TYPE_BOOL) return v.b ? "true" : "false";
    return "none";
}
static inline LucidVal lucid_pow(LucidVal base, LucidVal exponent) {
    if (base.type == LUCID_TYPE_COMPLEX || exponent.type == LUCID_TYPE_COMPLEX)
        return lucid_complex_pow(base, exponent);
    if (base.type == LUCID_TYPE_INT && exponent.type == LUCID_TYPE_INT && exponent.i >= 0) {
        /* Keep exact integer semantics; never route integer powers through
           floating point. Promote a result that exceeds i64 to BigInt. */
        __int128 result = 1, factor = base.i;
        uint64_t power = (uint64_t)exponent.i;
        bool overflow = false;
        while (power) {
            if (power & 1u) {
                __int128 product = result * factor;
                if (product > INT64_MAX || product < INT64_MIN) { overflow = true; break; }
                result = product;
            }
            power >>= 1u;
            if (power) {
                __int128 square = factor * factor;
                if (square > INT64_MAX || square < INT64_MIN) { overflow = true; break; }
                factor = square;
            }
        }
        if (!overflow) return lucid_int((int64_t)result);
        char base_text[64], exponent_text[64];
        snprintf(base_text, sizeof(base_text), "%ld", base.i);
        snprintf(exponent_text, sizeof(exponent_text), "%ld", exponent.i);
        return lucid_bigint_pow(lucid_bigint(base_text), lucid_bigint(exponent_text));
    }
    if ((base.type == LUCID_TYPE_BIGINT || base.type == LUCID_TYPE_INT) &&
        (exponent.type == LUCID_TYPE_BIGINT || exponent.type == LUCID_TYPE_INT))
        return lucid_bigint_pow(base, exponent);
    if ((base.type == LUCID_TYPE_INT || base.type == LUCID_TYPE_FLOAT) &&
        (exponent.type == LUCID_TYPE_INT || exponent.type == LUCID_TYPE_FLOAT))
        return lucid_float(pow(lucid_as_float(base), lucid_as_float(exponent)));
    fprintf(stderr, "pow() arguments must be numeric\n"); exit(1);
}
static inline LucidVal lucid_pow_mod(LucidVal base, LucidVal exponent, LucidVal modulus) {
    if (base.type != LUCID_TYPE_INT || exponent.type != LUCID_TYPE_INT || modulus.type != LUCID_TYPE_INT || modulus.i == 0 || exponent.i < 0) return lucid_int(INT64_MIN);
    __int128 result = 1, power = exponent.i, mod = modulus.i;
    if (mod < 0) mod = -mod;
    __int128 factor = ((__int128)base.i) % mod;
    if (factor < 0) factor += mod;
    while (power) { if (power & 1) result = (result * factor) % mod; factor = (factor * factor) % mod; power >>= 1; }
    return lucid_int((int64_t)result);
}
static int lucid_compare_i64(const void* lhs, const void* rhs) {
    int64_t left = *(const int64_t*)lhs;
    int64_t right = *(const int64_t*)rhs;
    return (left > right) - (left < right);
}
typedef struct {
    const char* key;
    int64_t hash;
} LucidHashEntry;
static int lucid_compare_hash_entries(const void* lhs, const void* rhs) {
    const LucidHashEntry* left = (const LucidHashEntry*)lhs;
    const LucidHashEntry* right = (const LucidHashEntry*)rhs;
    int by_key = strcmp(left->key ? left->key : "", right->key ? right->key : "");
    return by_key ? by_key : lucid_compare_i64(&left->hash, &right->hash);
}
static inline LucidVal lucid_hash(LucidVal value) {
    int64_t hash = 0;
    if (value.type == LUCID_TYPE_INT) hash = value.i;
    else if (value.type == LUCID_TYPE_BIGINT) {
        const char* digits = value.bigint ? value.bigint : "0";
        char* end = NULL;
        long long parsed = strtoll(digits, &end, 10);
        if (end && *end == '\0') {
            hash = (int64_t)parsed;
        } else {
            hash = 17;
            for (const unsigned char* p = (const unsigned char*)digits; *p; ++p)
                hash = hash * 31 + (int64_t)*p;
        }
    }
    else if (value.type == LUCID_TYPE_FLOAT) {
        if (isfinite(value.f) && value.f == trunc(value.f) && value.f >= (double)INT64_MIN && value.f < -(double)INT64_MIN)
            hash = (int64_t)value.f;
        else if (value.f == 0.0) hash = 0;
        else { uint64_t bits; memcpy(&bits, &value.f, sizeof(bits)); hash = (int64_t)bits; }
    }
    else if (value.type == LUCID_TYPE_COMPLEX && value.imag == 0.0) {
        hash = lucid_hash(lucid_float(value.real)).i;
    }
    else if (value.type == LUCID_TYPE_COMPLEX) {
        uint64_t real_bits, imag_bits;
        memcpy(&real_bits, &value.real, sizeof(real_bits));
        memcpy(&imag_bits, &value.imag, sizeof(imag_bits));
        hash = (int64_t)real_bits * 31 + (int64_t)imag_bits;
    }
    else if (value.type == LUCID_TYPE_BOOL) hash = value.b ? 1 : 0;
    else if (value.type == LUCID_TYPE_STR) {
        for (const unsigned char* p = (const unsigned char*)(value.s ? value.s : ""); *p; ++p)
            hash = hash * 31 + (int64_t)*p;
    } else if (value.type == LUCID_TYPE_LIST && value.list && value.list->frozen) {
        hash = 1;
        for (int64_t i = 0; i < value.list->len; ++i) hash = hash * 31 + lucid_hash(value.list->items[i]).i;
    } else if (value.type == LUCID_TYPE_SET && value.set && value.set->frozen) {
        hash = 7;
        int64_t* item_hashes = (int64_t*)malloc(sizeof(int64_t) * (size_t)value.set->len);
        if (!item_hashes && value.set->len) { fprintf(stderr, "out of memory hashing set\n"); exit(1); }
        for (int64_t i = 0; i < value.set->len; ++i) item_hashes[i] = lucid_hash(value.set->items[i]).i;
        qsort(item_hashes, (size_t)value.set->len, sizeof(int64_t), lucid_compare_i64);
        for (int64_t i = 0; i < value.set->len; ++i) hash = hash * 37 + item_hashes[i];
        free(item_hashes);
    } else if (value.type == LUCID_TYPE_DICT && value.dict && value.dict->frozen) {
        hash = 13;
        LucidHashEntry* entries = (LucidHashEntry*)malloc(sizeof(LucidHashEntry) * (size_t)value.dict->len);
        if (!entries && value.dict->len) { fprintf(stderr, "out of memory hashing dict\n"); exit(1); }
        for (int64_t i = 0; i < value.dict->len; ++i) {
            entries[i].key = value.dict->keys[i].s;
            entries[i].hash = lucid_hash(value.dict->keys[i]).i ^ lucid_hash(value.dict->values[i]).i;
        }
        qsort(entries, (size_t)value.dict->len, sizeof(LucidHashEntry), lucid_compare_hash_entries);
        for (int64_t i = 0; i < value.dict->len; ++i) hash = hash * 41 + entries[i].hash;
        free(entries);
    } else if (value.type != LUCID_TYPE_NONE) {
        fprintf(stderr, "unhashable value\n"); exit(1);
    }
    return lucid_int(hash);
}
static inline LucidVal lucid_abs_value(LucidVal value) {
    if (value.type == LUCID_TYPE_INT) return lucid_int(llabs(value.i));
    if (value.type == LUCID_TYPE_FLOAT) return lucid_float(fabs(value.f));
    if (value.type == LUCID_TYPE_COMPLEX) return lucid_float(hypot(value.real, value.imag));
    if (value.type == LUCID_TYPE_BIGINT) {
        const char* digits = value.bigint ? value.bigint : "0";
        return lucid_bigint(digits[0] == '-' ? digits + 1 : digits);
    }
    fprintf(stderr, "bad operand type for abs()\n"); exit(1);
}
static inline LucidVal lucid_format_value(LucidVal value, const char* spec) {
    if (!spec) spec = "";
    char* out = (char*)malloc(128);
    if (!out) { fprintf(stderr, "out of memory formatting value\n"); exit(1); }
    if (value.type == LUCID_TYPE_INT) {
        if (strcmp(spec, "px") == 0) snprintf(out, 128, "0x%llx", (long long)value.i);
        else if (strcmp(spec, "po") == 0) snprintf(out, 128, "0o%llo", (unsigned long long)value.i);
        else if (strcmp(spec, "pb") == 0) { char digits[65]; int pos = 64; unsigned long long n = (unsigned long long)value.i; digits[pos] = '\0'; do { digits[--pos] = (char)('0' + (n & 1)); n >>= 1; } while (n); snprintf(out, 128, "0b%s", digits + pos); }
        else if (strcmp(spec, "x") == 0) snprintf(out, 128, "%llx", (long long)value.i);
        else if (strcmp(spec, "X") == 0) snprintf(out, 128, "%llX", (long long)value.i);
        else if (strcmp(spec, "b") == 0) {
            char digits[65]; int pos = 64; unsigned long long n = (unsigned long long)value.i;
            digits[pos] = '\0'; do { digits[--pos] = (char)('0' + (n & 1)); n >>= 1; } while (n);
            snprintf(out, 128, "%s", digits + pos);
        } else if (strcmp(spec, "o") == 0) snprintf(out, 128, "%llo", (unsigned long long)value.i);
        else if (*spec == '\0') snprintf(out, 128, "%lld", (long long)value.i);
        else { fprintf(stderr, "unsupported format specifier '%s'\n", spec); exit(1); }
    } else if (value.type == LUCID_TYPE_FLOAT) {
        if (strcmp(spec, "f") == 0) snprintf(out, 128, "%.6f", value.f);
        else if (*spec == '\0') snprintf(out, 128, "%.10g", value.f);
        else { fprintf(stderr, "unsupported format specifier '%s'\n", spec); exit(1); }
    } else if (value.type == LUCID_TYPE_STR && *spec == '\0') snprintf(out, 128, "%s", value.s ? value.s : "");
    else if (value.type == LUCID_TYPE_BOOL && *spec == '\0') snprintf(out, 128, "%s", value.b ? "true" : "false");
    else if (value.type == LUCID_TYPE_NONE && *spec == '\0') snprintf(out, 128, "none");
    else { fprintf(stderr, "unsupported format specifier '%s'\n", spec); exit(1); }
    return lucid_str(out);
}
static inline LucidVal lucid_repr_value(LucidVal value) {
    char* out = (char*)malloc(256);
    if (!out) { fprintf(stderr, "out of memory rendering value\n"); exit(1); }
    if (value.type == LUCID_TYPE_STR) snprintf(out, 256, "\"%s\"", value.s ? value.s : "");
    else if (value.type == LUCID_TYPE_INT) {
        if (value.i == INT64_MAX) snprintf(out, 256, "int.inf");
        else if (value.i == INT64_MIN + 1) snprintf(out, 256, "-int.inf");
        else if (value.i == INT64_MIN) snprintf(out, 256, "int.nan");
        else snprintf(out, 256, "%lld", (long long)value.i);
    }
    else if (value.type == LUCID_TYPE_FLOAT) snprintf(out, 256, "%.10g", value.f);
    else if (value.type == LUCID_TYPE_COMPLEX) snprintf(out, 256, "(%g%+gj)", value.real, value.imag);
    else if (value.type == LUCID_TYPE_BIGINT) snprintf(out, 256, "%s", value.bigint ? value.bigint : "0");
    else if (value.type == LUCID_TYPE_BOOL) snprintf(out, 256, "%s", value.b ? "true" : "false");
    else if (value.type == LUCID_TYPE_NONE) snprintf(out, 256, "none");
    else if (value.type == LUCID_TYPE_LIST) snprintf(out, 256, "[list len=%lld]", (long long)(value.list ? value.list->len : 0));
    else snprintf(out, 256, "<value>");
    return lucid_str(out);
}
static inline LucidVal lucid_ord_value(LucidVal value) {
    if (value.type != LUCID_TYPE_STR || !value.s || value.s[0] == '\0') {
        fprintf(stderr, "ord() expected a one-character string\n"); exit(1);
    }
    const unsigned char* p = (const unsigned char*)value.s; int64_t code = 0; int width = 0;
    if (*p < 0x80) { code = *p; width = 1; }
    else if ((*p & 0xe0) == 0xc0) { code = *p & 0x1f; width = 2; }
    else if ((*p & 0xf0) == 0xe0) { code = *p & 0x0f; width = 3; }
    else if ((*p & 0xf8) == 0xf0) { code = *p & 0x07; width = 4; }
    else { fprintf(stderr, "ord() expected valid UTF-8\n"); exit(1); }
    for (int i = 1; i < width; ++i) {
        if ((p[i] & 0xc0) != 0x80) { fprintf(stderr, "ord() expected valid UTF-8\n"); exit(1); }
        code = (code << 6) | (p[i] & 0x3f);
    }
    if (p[width] != '\0') { fprintf(stderr, "ord() expected a one-character string\n"); exit(1); }
    return lucid_int(code);
}
static inline LucidVal lucid_chr_value(LucidVal value) {
    int64_t code = lucid_as_int(value);
    if (code < 0 || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)) { fprintf(stderr, "chr() argument out of range\n"); exit(1); }
    int width = code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
    char* out = (char*)malloc((size_t)width + 1);
    if (!out) { fprintf(stderr, "out of memory creating character\n"); exit(1); }
    if (width == 1) out[0] = (char)code;
    else if (width == 2) { out[0] = (char)(0xc0 | (code >> 6)); out[1] = (char)(0x80 | (code & 0x3f)); }
    else if (width == 3) { out[0] = (char)(0xe0 | (code >> 12)); out[1] = (char)(0x80 | ((code >> 6) & 0x3f)); out[2] = (char)(0x80 | (code & 0x3f)); }
    else { out[0] = (char)(0xf0 | (code >> 18)); out[1] = (char)(0x80 | ((code >> 12) & 0x3f)); out[2] = (char)(0x80 | ((code >> 6) & 0x3f)); out[3] = (char)(0x80 | (code & 0x3f)); }
    out[width] = '\0';
    return lucid_str(out);
}
static inline LucidList* lucid_as_list(LucidVal v) {
    if (v.type == LUCID_TYPE_LIST) return v.list;
    return NULL;
}
static inline LucidDict* lucid_as_dict(LucidVal v) {
    if (v.type == LUCID_TYPE_DICT) return v.dict;
    return NULL;
}
static inline LucidSet* lucid_as_set(LucidVal v) {
    if (v.type == LUCID_TYPE_SET) return v.set;
    return NULL;
}
static inline void* lucid_as_ptr(LucidVal v) {
    return v.ptr;
}

static inline double _n_float(double f) { return f; }
static inline double _n_int(int64_t i) { return (double)i; }
static inline double _n_val(LucidVal v) { return lucid_as_float(v); }
static inline double _n_def(void* p) { (void)p; return 0.0; }

#define lucid_num(x) _Generic((x), \
    float: _n_float, \
    double: _n_float, \
    int: _n_int, \
    long: _n_int, \
    long long: _n_int, \
    LucidVal: _n_val, \
    default: _n_def \
)(x)

static inline LucidVal lucid_complex_add(LucidVal a, LucidVal b) {
    return lucid_complex(a.real + b.real, a.imag + b.imag);
}
static inline LucidVal lucid_complex_sub(LucidVal a, LucidVal b) {
    return lucid_complex(a.real - b.real, a.imag - b.imag);
}
static inline LucidVal lucid_complex_mul(LucidVal a, LucidVal b) {
    return lucid_complex(a.real * b.real - a.imag * b.imag, a.real * b.imag + a.imag * b.real);
}
static inline LucidVal lucid_complex_div(LucidVal a, LucidVal b) {
    double d = b.real * b.real + b.imag * b.imag;
    if (d == 0.0) { fprintf(stderr, "division by zero\n"); exit(1); }
    return lucid_complex((a.real * b.real + a.imag * b.imag) / d, (a.imag * b.real - a.real * b.imag) / d);
}
static inline LucidVal lucid_complex_pow(LucidVal a, LucidVal b) {
    double radius = hypot(a.real, a.imag);
    double angle = atan2(a.imag, a.real);
    double scale = exp(b.real * log(radius) - b.imag * angle);
    double phase = b.imag * log(radius) + b.real * angle;
    return lucid_complex(scale * cos(phase), scale * sin(phase));
}
static inline LucidVal lucid_complex_neg(LucidVal a) { return lucid_complex(-a.real, -a.imag); }

static inline int64_t _i_int(int64_t i) { return i; }
static inline int64_t _i_float(double f) { return (int64_t)f; }
static inline int64_t _i_val(LucidVal v) { return lucid_as_int(v); }
static inline int64_t _i_def(void* p) { (void)p; return 0; }

#define lucid_int_val(x) _Generic((x), \
    int: _i_int, \
    long: _i_int, \
    long long: _i_int, \
    float: _i_float, \
    double: _i_float, \
    LucidVal: _i_val, \
    default: _i_def \
)(x)

static inline bool _b_bool(bool b) { return b; }
static inline bool _b_int(int64_t i) { return i != 0; }
static inline bool _b_float(double f) { return f != 0.0; }
static inline bool _b_val(LucidVal v) { return lucid_as_bool(v); }
static inline bool _b_ptr(void* p) { return p != NULL; }
static inline bool lucid_str_truthy(const char* value) {
    return value != NULL && value[0] != '\0';
}
static inline bool lucid_list_truthy(LucidList* value) {
    return value != NULL && value->len > 0;
}
static inline bool lucid_dict_truthy(LucidDict* value) {
    return value != NULL && value->len > 0;
}
static inline bool lucid_set_truthy(LucidSet* value) {
    return value != NULL && value->len > 0;
}

#define lucid_bool_val(x) _Generic((x), \
    bool: _b_bool, \
    int: _b_int, \
    long: _b_int, \
    long long: _b_int, \
    float: _b_float, \
    double: _b_float, \
    LucidVal: _b_val, \
    default: _b_ptr \
)(x)

static inline bool _is_none_val(LucidVal v) { return v.type == LUCID_TYPE_NONE || (v.type == LUCID_TYPE_PTR && v.ptr == NULL); }
static inline bool _is_none_ptr(void* p) { return p == NULL; }

#define lucid_is_none(x) _Generic((x), \
    LucidVal: _is_none_val, \
    default: _is_none_ptr \
)(x)

static inline bool lucid_eq(LucidVal a, LucidVal b) {
    if (a.type == LUCID_TYPE_NONE && b.type == LUCID_TYPE_NONE) return true;
    if (a.type == LUCID_TYPE_STR && b.type == LUCID_TYPE_STR) return strcmp(a.s, b.s) == 0;
    if (a.type == LUCID_TYPE_FLOAT || b.type == LUCID_TYPE_FLOAT) return a.f == b.f;
    if (a.type == LUCID_TYPE_COMPLEX && b.type == LUCID_TYPE_COMPLEX) return a.real == b.real && a.imag == b.imag;
    if (a.type == LUCID_TYPE_BIGINT || b.type == LUCID_TYPE_BIGINT) return lucid_bigint_cmp(a, b) == 0;
    if (a.type == LUCID_TYPE_INT || b.type == LUCID_TYPE_INT) return a.i == b.i;
    if (a.type == LUCID_TYPE_BOOL && b.type == LUCID_TYPE_BOOL) return a.b == b.b;
    if (a.type == LUCID_TYPE_LIST && b.type == LUCID_TYPE_LIST) {
        if (!a.list || !b.list || a.list->len != b.list->len) return a.list == b.list;
        for (int64_t i = 0; i < a.list->len; ++i)
            if (!lucid_eq(a.list->items[i], b.list->items[i])) return false;
        return true;
    }
    if (a.type == LUCID_TYPE_SET && b.type == LUCID_TYPE_SET) {
        if (!a.set || !b.set || a.set->len != b.set->len) return a.set == b.set;
        for (int64_t i = 0; i < a.set->len; ++i) {
            bool found = false;
            for (int64_t j = 0; j < b.set->len; ++j)
                if (lucid_eq(a.set->items[i], b.set->items[j])) { found = true; break; }
            if (!found) return false;
        }
        return true;
    }
    if (a.type == LUCID_TYPE_DICT && b.type == LUCID_TYPE_DICT) {
        if (!a.dict || !b.dict || a.dict->len != b.dict->len) return a.dict == b.dict;
        for (int64_t i = 0; i < a.dict->len; ++i) {
            bool found = false;
            for (int64_t j = 0; j < b.dict->len; ++j) {
                if (lucid_eq(a.dict->keys[i], b.dict->keys[j]) && lucid_eq(a.dict->values[i], b.dict->values[j])) {
                    found = true; break;
                }
            }
            if (!found) return false;
        }
        return true;
    }
    return a.ptr == b.ptr;
}
// `===`/`!==` preserve object identity for heap values while scalar values
// retain their ordinary value identity.  In particular, comparing two C
// string pointers directly would make equal strings differ merely because
// they came from different allocations.
static inline bool lucid_identity(LucidVal a, LucidVal b) {
    if (a.type != b.type) return false;
    if (a.type == LUCID_TYPE_LIST) return a.list == b.list;
    if (a.type == LUCID_TYPE_SET) return a.set == b.set;
    if (a.type == LUCID_TYPE_DICT) return a.dict == b.dict;
    if (a.type == LUCID_TYPE_STR) return strcmp(a.s ? a.s : "", b.s ? b.s : "") == 0;
    if (a.type == LUCID_TYPE_INT) return a.i == b.i;
    if (a.type == LUCID_TYPE_FLOAT) return a.f == b.f;
    if (a.type == LUCID_TYPE_BOOL) return a.b == b.b;
    if (a.type == LUCID_TYPE_NONE) return true;
    return a.ptr == b.ptr;
}
static inline bool lucid_lt(LucidVal a, LucidVal b) {
    if (a.type == LUCID_TYPE_STR && b.type == LUCID_TYPE_STR)
        return strcmp(a.s ? a.s : "", b.s ? b.s : "") < 0;
    if ((a.type == LUCID_TYPE_BIGINT || a.type == LUCID_TYPE_INT) &&
        (b.type == LUCID_TYPE_BIGINT || b.type == LUCID_TYPE_INT))
        return lucid_bigint_cmp(a, b) < 0;
    if (a.type == LUCID_TYPE_FLOAT || b.type == LUCID_TYPE_FLOAT ||
        a.type == LUCID_TYPE_BIGINT || b.type == LUCID_TYPE_BIGINT)
        return lucid_as_float(a) < lucid_as_float(b);
    fprintf(stderr, "unsupported operands for <\n");
    exit(1);
}
static inline bool lucid_lte(LucidVal a, LucidVal b) {
    if (a.type == LUCID_TYPE_STR && b.type == LUCID_TYPE_STR)
        return strcmp(a.s ? a.s : "", b.s ? b.s : "") <= 0;
    if ((a.type == LUCID_TYPE_BIGINT || a.type == LUCID_TYPE_INT) &&
        (b.type == LUCID_TYPE_BIGINT || b.type == LUCID_TYPE_INT))
        return lucid_bigint_cmp(a, b) <= 0;
    if (a.type == LUCID_TYPE_FLOAT || b.type == LUCID_TYPE_FLOAT ||
        a.type == LUCID_TYPE_BIGINT || b.type == LUCID_TYPE_BIGINT)
        return lucid_as_float(a) <= lucid_as_float(b);
    fprintf(stderr, "unsupported operands for <=\n");
    exit(1);
}
static inline bool lucid_gt(LucidVal a, LucidVal b) {
    if (a.type == LUCID_TYPE_STR && b.type == LUCID_TYPE_STR)
        return strcmp(a.s ? a.s : "", b.s ? b.s : "") > 0;
    if ((a.type == LUCID_TYPE_BIGINT || a.type == LUCID_TYPE_INT) &&
        (b.type == LUCID_TYPE_BIGINT || b.type == LUCID_TYPE_INT))
        return lucid_bigint_cmp(a, b) > 0;
    if (a.type == LUCID_TYPE_FLOAT || b.type == LUCID_TYPE_FLOAT ||
        a.type == LUCID_TYPE_BIGINT || b.type == LUCID_TYPE_BIGINT)
        return lucid_as_float(a) > lucid_as_float(b);
    fprintf(stderr, "unsupported operands for >\n");
    exit(1);
}
static inline bool lucid_gte(LucidVal a, LucidVal b) {
    if (a.type == LUCID_TYPE_STR && b.type == LUCID_TYPE_STR)
        return strcmp(a.s ? a.s : "", b.s ? b.s : "") >= 0;
    if ((a.type == LUCID_TYPE_BIGINT || a.type == LUCID_TYPE_INT) &&
        (b.type == LUCID_TYPE_BIGINT || b.type == LUCID_TYPE_INT))
        return lucid_bigint_cmp(a, b) >= 0;
    if (a.type == LUCID_TYPE_FLOAT || b.type == LUCID_TYPE_FLOAT ||
        a.type == LUCID_TYPE_BIGINT || b.type == LUCID_TYPE_BIGINT)
        return lucid_as_float(a) >= lucid_as_float(b);
    fprintf(stderr, "unsupported operands for >=\n");
    exit(1);
}

static inline LucidList* lucid_list_new(int64_t cap) {
    LucidList* l = (LucidList*)malloc(sizeof(LucidList));
    l->len = 0;
    l->frozen = false;
    l->cap = cap < 8 ? 8 : cap;
    l->items = (LucidVal*)malloc(sizeof(LucidVal) * l->cap);
    return l;
}
static inline LucidList* lucid_iterable_to_list(LucidVal value);

static inline void lucid_list_append(LucidList* l, LucidVal v) {
    if (!l) return;
    if (l->frozen) { fprintf(stderr, "cannot mutate frozen list\n"); exit(1); }
    if (l->len >= l->cap) {
        l->cap = l->cap < 8 ? 8 : l->cap * 2;
        l->items = (LucidVal*)realloc(l->items, sizeof(LucidVal) * l->cap);
    }
    l->items[l->len++] = v;
}
static inline LucidVal lucid_list_pop(LucidList* l, int64_t index, bool has_index) {
    if (!l || l->len == 0) { fprintf(stderr, "pop from empty list\n"); exit(1); }
    if (l->frozen) { fprintf(stderr, "cannot mutate frozen list\n"); exit(1); }
    int64_t i = has_index ? index : l->len - 1; if (i < 0) i += l->len;
    if (i < 0 || i >= l->len) { fprintf(stderr, "list pop index out of range\n"); exit(1); }
    LucidVal out = l->items[i]; for (int64_t j = i + 1; j < l->len; ++j) l->items[j - 1] = l->items[j];
    --l->len; return out;
}
static inline LucidList* lucid_list_pop_range(LucidList* l, int64_t start, int64_t stop) {
    LucidList* out = lucid_list_new(0); if (!l) return out;
    if (l->frozen) { fprintf(stderr, "cannot mutate frozen list\n"); exit(1); }
    int64_t len = l->len; if (start < 0) start = start + len < 0 ? 0 : start + len; else if (start > len) start = len;
    if (stop < 0) stop = stop + len < 0 ? 0 : stop + len; else if (stop > len) stop = len;
    if (start > stop) return out;
    for (int64_t i = start; i < stop; ++i) lucid_list_append(out, l->items[i]);
    for (int64_t i = stop; i < len; ++i) l->items[start + i - stop] = l->items[i];
    l->len -= stop - start; return out;
}
static inline void lucid_list_insert(LucidList* l, int64_t index, LucidVal value) {
    if (!l) return; if (index < 0) index += l->len; if (index < 0) index = 0; if (index > l->len) index = l->len;
    if (l->frozen) { fprintf(stderr, "cannot mutate frozen list\n"); exit(1); }
    if (l->len >= l->cap) { l->cap *= 2; l->items = (LucidVal*)realloc(l->items, sizeof(LucidVal) * l->cap); }
    for (int64_t j = l->len; j > index; --j) l->items[j] = l->items[j - 1]; l->items[index] = value; ++l->len;
}
static inline void lucid_list_extend(LucidList* l, LucidVal value) {
    LucidList* other = lucid_iterable_to_list(value); for (int64_t i = 0; i < other->len; ++i) lucid_list_append(l, other->items[i]);
}
static inline void lucid_list_clear(LucidList* l) { if (l && l->frozen) { fprintf(stderr, "cannot mutate frozen list\n"); exit(1); } if (l) l->len = 0; }
static inline void lucid_list_remove(LucidList* l, LucidVal value) {
    if (!l) return;
    if (l->frozen) { fprintf(stderr, "cannot mutate frozen list\n"); exit(1); }
    for (int64_t i = 0; i < l->len; ++i) if (lucid_eq(l->items[i], value)) {
        for (int64_t j = i + 1; j < l->len; ++j) l->items[j - 1] = l->items[j];
        --l->len; return;
    }
    fprintf(stderr, "list.remove(x): x not in list\n"); exit(1);
}

static inline LucidList* lucid_bytearray(LucidVal value) {
    if (value.type != LUCID_TYPE_STR && value.type != LUCID_TYPE_LIST) {
        fprintf(stderr, "bytearray() cannot convert value\n"); exit(1);
    }
    if (value.type == LUCID_TYPE_STR) {
        const char* s = value.s ? value.s : "";
        LucidList* out = lucid_list_new((int64_t)strlen(s));
        for (const unsigned char* p = (const unsigned char*)s; *p; ++p)
            lucid_list_append(out, lucid_wrap(lucid_int_val((int64_t)*p)));
        return out;
    }
    LucidList* out = lucid_list_new(value.list ? value.list->len : 0);
    if (value.list) for (int64_t i = 0; i < value.list->len; ++i) {
        LucidVal item = value.list->items[i];
        if (item.type != LUCID_TYPE_INT && item.type != LUCID_TYPE_BIGINT) {
            fprintf(stderr, "bytearray() items must be integers in 0..255\n"); exit(1);
        }
        int64_t n = lucid_as_int(item);
        if (item.type == LUCID_TYPE_BIGINT && item.bigint &&
            (item.bigint[0] == '-' || lucid_bigint_cmp_mag(item.bigint, "255") > 0)) {
            fprintf(stderr, "bytearray() items must be integers in 0..255\n"); exit(1);
        }
        if (n < 0 || n > 255) {
            fprintf(stderr, "bytearray() items must be integers in 0..255\n"); exit(1);
        }
        lucid_list_append(out, lucid_wrap(lucid_int_val(n)));
    }
    return out;
}
static inline LucidVal lucid_bytes(LucidVal value) {
    if (value.type == LUCID_TYPE_STR) return value;
    if (value.type != LUCID_TYPE_LIST || !value.list) {
        fprintf(stderr, "bytes() cannot convert value\n"); exit(1);
    }
    LucidList* l = value.list; char* out = (char*)malloc((size_t)l->len + 1);
    for (int64_t i = 0; i < l->len; ++i) {
        LucidVal item = l->items[i];
        if (item.type != LUCID_TYPE_INT && item.type != LUCID_TYPE_BIGINT) {
            free(out); fprintf(stderr, "bytes() list items must be integers in 0..255\n"); exit(1);
        }
        int64_t n = lucid_as_int(item);
        if ((item.type == LUCID_TYPE_BIGINT && item.bigint &&
             (item.bigint[0] == '-' || lucid_bigint_cmp_mag(item.bigint, "255") > 0)) ||
            n < 0 || n > 255) {
            free(out); fprintf(stderr, "bytes() list items must be integers in 0..255\n"); exit(1);
        }
        out[i] = (char)(unsigned char)n;
    }
    out[l->len] = '\0'; return lucid_str(out);
}

static inline LucidDict* lucid_dict_new(int64_t cap) {
    LucidDict* d = (LucidDict*)malloc(sizeof(LucidDict));
    d->len = 0; d->cap = cap < 8 ? 8 : cap;
    d->frozen = false;
    d->keys = (LucidVal*)malloc(sizeof(LucidVal) * d->cap);
    d->values = (LucidVal*)malloc(sizeof(LucidVal) * d->cap);
    return d;
}
static inline void lucid_dict_set(LucidDict* d, LucidVal key, LucidVal value) {
    if (!d) return;
    if (d->frozen) { fprintf(stderr, "cannot mutate frozen dict\n"); exit(1); }
    for (int64_t i = 0; i < d->len; i++) {
        if (lucid_eq(d->keys[i], key)) { d->values[i] = value; return; }
    }
    if (d->len >= d->cap) {
        d->cap *= 2;
        d->keys = (LucidVal*)realloc(d->keys, sizeof(LucidVal) * d->cap);
        d->values = (LucidVal*)realloc(d->values, sizeof(LucidVal) * d->cap);
    }
    d->keys[d->len] = key; d->values[d->len++] = value;
}
static inline LucidVal lucid_dict_get(LucidDict* d, LucidVal key, LucidVal fallback) {
    if (!d) return fallback;
    for (int64_t i = 0; i < d->len; i++) if (lucid_eq(d->keys[i], key)) return d->values[i];
    return fallback;
}
static inline void lucid_dict_clear(LucidDict* d) {
    if (d && d->frozen) { fprintf(stderr, "cannot mutate frozen dict\n"); exit(1); }
    if (d) d->len = 0;
}
static inline LucidVal lucid_dict_pop(LucidDict* d, LucidVal key) {
    if (!d) { fprintf(stderr, "dict.pop(key): key not found\n"); exit(1); }
    if (d->frozen) { fprintf(stderr, "cannot mutate frozen dict\n"); exit(1); }
    for (int64_t i = 0; i < d->len; ++i) if (lucid_eq(d->keys[i], key)) {
        LucidVal value = d->values[i];
        for (int64_t j = i + 1; j < d->len; ++j) {
            d->keys[j - 1] = d->keys[j]; d->values[j - 1] = d->values[j];
        }
        d->len--; return value;
    }
    fprintf(stderr, "dict.pop(key): key not found\n"); exit(1);
}
static inline LucidVal lucid_dict_field(LucidDict* d, const char* name) {
    return lucid_dict_get(d, lucid_str(name), lucid_none());
}
static inline LucidList* lucid_dict_keys(LucidDict* d) {
    LucidList* out = lucid_list_new(d ? d->len : 0);
    if (d) for (int64_t i = 0; i < d->len; i++) lucid_list_append(out, d->keys[i]);
    return out;
}
static inline LucidList* lucid_dict_values(LucidDict* d) {
    LucidList* out = lucid_list_new(d ? d->len : 0);
    if (d) for (int64_t i = 0; i < d->len; i++) lucid_list_append(out, d->values[i]);
    return out;
}
static inline LucidList* lucid_dict_items(LucidDict* d) {
    LucidList* out = lucid_list_new(d ? d->len : 0);
    if (d) for (int64_t i = 0; i < d->len; ++i) {
        LucidList* pair = lucid_list_new(2); lucid_list_append(pair, d->keys[i]); lucid_list_append(pair, d->values[i]);
        lucid_list_append(out, (LucidVal){ .type = LUCID_TYPE_LIST, .list = pair });
    }
    return out;
}
static inline LucidSet* lucid_set_new(int64_t cap) {
    LucidSet* s = (LucidSet*)malloc(sizeof(LucidSet));
    s->len = 0; s->cap = cap < 8 ? 8 : cap; s->frozen = false;
    s->items = (LucidVal*)malloc(sizeof(LucidVal) * s->cap); return s;
}
static inline void lucid_set_add(LucidSet* s, LucidVal value) {
    if (!s) return;
    if (s->frozen) { fprintf(stderr, "cannot mutate frozen set\n"); exit(1); }
    for (int64_t i = 0; i < s->len; i++) if (lucid_eq(s->items[i], value)) return;
    if (s->len >= s->cap) { s->cap *= 2; s->items = (LucidVal*)realloc(s->items, sizeof(LucidVal) * s->cap); }
    s->items[s->len++] = value;
}
static inline void lucid_freeze_value(LucidVal value) {
    if (value.type == LUCID_TYPE_LIST && value.list) {
        value.list->frozen = true;
        for (int64_t i = 0; i < value.list->len; ++i) lucid_freeze_value(value.list->items[i]);
    } else if (value.type == LUCID_TYPE_DICT && value.dict) {
        value.dict->frozen = true;
        for (int64_t i = 0; i < value.dict->len; ++i) lucid_freeze_value(value.dict->values[i]);
    } else if (value.type == LUCID_TYPE_SET && value.set) {
        value.set->frozen = true;
        for (int64_t i = 0; i < value.set->len; ++i) lucid_freeze_value(value.set->items[i]);
    } else if (value.type == LUCID_TYPE_PTR && value.ptr) {
        lucid_freeze_object(value.ptr);
    }
}
static inline bool lucid_set_contains(LucidSet* s, LucidVal value) {
    if (!s) return false;
    for (int64_t i = 0; i < s->len; i++) if (lucid_eq(s->items[i], value)) return true;
    return false;
}
static inline LucidSet* lucid_set_intersection(LucidSet* a, LucidSet* b) {
    LucidSet* out = lucid_set_new(0); if (!a || !b) return out;
    for (int64_t i = 0; i < a->len; ++i) if (lucid_set_contains(b, a->items[i])) lucid_set_add(out, a->items[i]);
    return out;
}
static inline LucidSet* lucid_set_intersection_value(LucidSet* a, LucidVal other) {
    if (other.type != LUCID_TYPE_SET) { fprintf(stderr, "unsupported operands for &\n"); exit(1); }
    return lucid_set_intersection(a, other.set);
}
static inline LucidSet* lucid_set_union(LucidSet* a, LucidSet* b) {
    LucidSet* out = lucid_set_new(0); if (a) for (int64_t i = 0; i < a->len; ++i) lucid_set_add(out, a->items[i]); if (b) for (int64_t i = 0; i < b->len; ++i) lucid_set_add(out, b->items[i]); return out;
}
static inline LucidSet* lucid_set_union_value(LucidSet* a, LucidVal other) {
    if (other.type != LUCID_TYPE_SET) { fprintf(stderr, "unsupported operands for |\n"); exit(1); }
    return lucid_set_union(a, other.set);
}
static inline LucidSet* lucid_set_difference(LucidSet* a, LucidSet* b) {
    LucidSet* out = lucid_set_new(0); if (!a) return out; for (int64_t i = 0; i < a->len; ++i) if (!b || !lucid_set_contains(b, a->items[i])) lucid_set_add(out, a->items[i]); return out;
}
static inline LucidSet* lucid_set_difference_value(LucidSet* a, LucidVal other) {
    if (other.type != LUCID_TYPE_SET) { fprintf(stderr, "unsupported operands for -\n"); exit(1); }
    return lucid_set_difference(a, other.set);
}
static inline LucidSet* lucid_set_xor(LucidSet* a, LucidSet* b) {
    LucidSet* out = lucid_set_difference(a, b); if (b) for (int64_t i = 0; i < b->len; ++i) if (!a || !lucid_set_contains(a, b->items[i])) lucid_set_add(out, b->items[i]); return out;
}
static inline LucidSet* lucid_set_xor_value(LucidSet* a, LucidVal other) {
    if (other.type != LUCID_TYPE_SET) { fprintf(stderr, "unsupported operands for ^\n"); exit(1); }
    return lucid_set_xor(a, other.set);
}
static inline LucidVal lucid_setop_value(LucidVal left, LucidVal right, char op) {
    if ((left.type == LUCID_TYPE_COMPLEX || right.type == LUCID_TYPE_COMPLEX) && op == '-')
        return lucid_complex_sub(left, right);
    if ((left.type == LUCID_TYPE_BIGINT || left.type == LUCID_TYPE_INT) &&
        (right.type == LUCID_TYPE_BIGINT || right.type == LUCID_TYPE_INT) && op == '-')
        return lucid_bigint_binop(left, right, '-');
    if (left.type == LUCID_TYPE_SET && right.type == LUCID_TYPE_SET) {
        LucidSet* result = NULL;
        if (op == '&') result = lucid_set_intersection(left.set, right.set);
        else if (op == '|') result = lucid_set_union(left.set, right.set);
        else if (op == '-') result = lucid_set_difference(left.set, right.set);
        else if (op == '^') result = lucid_set_xor(left.set, right.set);
        if (result) return lucid_set_val(result);
    }
    if (left.type == LUCID_TYPE_INT && right.type == LUCID_TYPE_INT) {
        if (op == '-') return lucid_int(lucid_checked_sub(left.i, right.i));
        if (op == '&') return lucid_int(left.i & right.i);
        if (op == '|') return lucid_int(left.i | right.i);
        if (op == '^') return lucid_int(left.i ^ right.i);
    }
    fprintf(stderr, "unsupported operands for %c\n", op);
    exit(1);
}
static inline LucidVal lucid_shift_value(LucidVal left, LucidVal right, bool left_shift) {
    if (left.type == LUCID_TYPE_INT && right.type == LUCID_TYPE_INT) {
        return lucid_int(left_shift ? lucid_checked_shl(left.i, right.i)
                                    : lucid_checked_shr(left.i, right.i));
    }
    fprintf(stderr, "unsupported operands for %s\n", left_shift ? "<<" : ">>");
    exit(1);
}
static inline bool lucid_set_disjoint(LucidSet* a, LucidSet* b) { if (!a || !b) return true; for (int64_t i = 0; i < a->len; ++i) if (lucid_set_contains(b, a->items[i])) return false; return true; }
static inline bool lucid_set_disjoint_value(LucidSet* a, LucidVal other) {
    if (other.type == LUCID_TYPE_SET) return lucid_set_disjoint(a, other.set);
    if (other.type == LUCID_TYPE_LIST) {
        if (!a || !other.list) return true;
        for (int64_t i = 0; i < a->len; ++i)
            for (int64_t j = 0; j < other.list->len; ++j)
                if (lucid_eq(a->items[i], other.list->items[j])) return false;
        return true;
    }
    fprintf(stderr, "isdisjoint() argument must be iterable\n"); exit(1);
}
static inline bool lucid_list_contains(LucidList* l, LucidVal value) {
    if (!l) return false;
    for (int64_t i = 0; i < l->len; i++) if (lucid_eq(l->items[i], value)) return true;
    return false;
}
static inline bool lucid_str_contains(const char* haystack, LucidVal needle) {
    return haystack && needle.type == LUCID_TYPE_STR && needle.s
        && strstr(haystack, needle.s) != NULL;
}
static inline bool lucid_dict_contains(LucidDict* d, LucidVal key) {
    if (!d) return false;
    for (int64_t i = 0; i < d->len; i++) if (lucid_eq(d->keys[i], key)) return true;
    return false;
}
static inline bool lucid_contains_value(LucidVal container, LucidVal needle) {
    if (container.type == LUCID_TYPE_LIST) return lucid_list_contains(container.list, needle);
    if (container.type == LUCID_TYPE_SET) return lucid_set_contains(container.set, needle);
    if (container.type == LUCID_TYPE_DICT) return lucid_dict_contains(container.dict, needle);
    if (container.type == LUCID_TYPE_STR) return lucid_str_contains(container.s, needle);
    fprintf(stderr, "'in' operator not supported for value\n");
    exit(1);
}
static inline void lucid_set_remove(LucidSet* s, LucidVal value) {
    if (!s) return;
    if (s->frozen) { fprintf(stderr, "cannot mutate frozen set\n"); exit(1); }
    for (int64_t i = 0; i < s->len; i++) if (lucid_eq(s->items[i], value)) {
        for (int64_t j = i + 1; j < s->len; j++) s->items[j - 1] = s->items[j];
        s->len--; return;
    }
    fprintf(stderr, "set.remove(x): x not in set\n"); exit(1);
}
static inline void lucid_set_discard(LucidSet* s, LucidVal value) {
    if (!s) return;
    if (s->frozen) { fprintf(stderr, "cannot mutate frozen set\n"); exit(1); }
    for (int64_t i = 0; i < s->len; i++) if (lucid_eq(s->items[i], value)) {
        for (int64_t j = i + 1; j < s->len; j++) s->items[j - 1] = s->items[j];
        s->len--; return;
    }
}
static inline LucidVal lucid_set_pop(LucidSet* s) {
    if (s && s->frozen) { fprintf(stderr, "cannot mutate frozen set\n"); exit(1); }
    if (!s || s->len == 0) { fprintf(stderr, "set.pop(): pop from an empty set\n"); exit(1); }
    LucidVal value = s->items[0];
    for (int64_t i = 1; i < s->len; i++) s->items[i - 1] = s->items[i];
    s->len--;
    return value;
}
static inline void lucid_set_clear(LucidSet* s) { if (s && s->frozen) { fprintf(stderr, "cannot mutate frozen set\n"); exit(1); } if (s) s->len = 0; }

static inline LucidSet* lucid_set_from_value(LucidVal value) {
    LucidSet* out = lucid_set_new(value.type == LUCID_TYPE_SET ? value.set->len : 8);
    if (value.type == LUCID_TYPE_SET) {
        for (int64_t i = 0; i < value.set->len; ++i) lucid_set_add(out, value.set->items[i]);
    } else if (value.type == LUCID_TYPE_LIST) {
        for (int64_t i = 0; i < value.list->len; ++i) lucid_set_add(out, value.list->items[i]);
    } else if (value.type == LUCID_TYPE_STR) {
        LucidList* chars = lucid_iterable_to_list(value);
        for (int64_t i = 0; i < chars->len; ++i) lucid_set_add(out, chars->items[i]);
    } else if (value.type == LUCID_TYPE_DICT) {
        LucidList* keys = lucid_dict_keys(value.dict);
        for (int64_t i = 0; i < keys->len; ++i) lucid_set_add(out, keys->items[i]);
    } else {
        fprintf(stderr, "set() argument is not iterable\n"); exit(1);
    }
    return out;
}

static inline LucidDict* lucid_dict_from_value(LucidVal value) {
    LucidDict* out = lucid_dict_new(value.type == LUCID_TYPE_DICT ? value.dict->len : 8);
    if (value.type == LUCID_TYPE_DICT) {
        for (int64_t i = 0; i < value.dict->len; ++i)
            lucid_dict_set(out, value.dict->keys[i], value.dict->values[i]);
    } else if (value.type == LUCID_TYPE_LIST) {
        for (int64_t i = 0; i < value.list->len; ++i) {
            LucidVal pair = value.list->items[i];
            if (pair.type != LUCID_TYPE_LIST || pair.list->len != 2) {
                fprintf(stderr, "dict() sequence elements must each have length 2\n"); exit(1);
            }
            lucid_dict_set(out, pair.list->items[0], pair.list->items[1]);
        }
    } else {
        fprintf(stderr, "dict() argument is not a mapping or sequence of pairs\n"); exit(1);
    }
    return out;
}

static inline LucidVal lucid_list_get(LucidList* l, int64_t idx) {
    if (__builtin_expect(!l, 0)) return lucid_none();
    if (__builtin_expect(idx < 0, 0)) idx += l->len;
    if (__builtin_expect(idx < 0 || idx >= l->len, 0)) {
        fprintf(stderr, "index %lld out of range\n", (long long)idx);
        exit(1);
    }
    return l->items[idx];
}

static inline void lucid_list_set(LucidList* l, int64_t idx, LucidVal v) {
    if (__builtin_expect(!l, 0)) return;
    if (l->frozen) { fprintf(stderr, "cannot mutate frozen list\n"); exit(1); }
    if (__builtin_expect(idx < 0, 0)) idx += l->len;
    if (__builtin_expect(idx < 0 || idx >= l->len, 0)) {
        fprintf(stderr, "index %lld out of range\n", (long long)idx);
        exit(1);
    }
    l->items[idx] = v;
}

static inline bool lucid_dict_contains(LucidDict* d, LucidVal key);

static inline LucidVal lucid_get_index(LucidVal container, int64_t idx) {
    if (container.type == LUCID_TYPE_LIST) {
        return lucid_list_get(container.list, idx);
    }
    if (container.type == LUCID_TYPE_STR) {
        return lucid_str(lucid_str_index(container.s, idx));
    }
    fprintf(stderr, "indexing not supported\n");
    exit(1);
}
static inline LucidVal lucid_get_key(LucidVal container, LucidVal key) {
    if (container.type == LUCID_TYPE_DICT) {
        if (lucid_dict_contains(container.dict, key))
            return lucid_dict_get(container.dict, key, lucid_none());
        fprintf(stderr, "key not found\n");
        exit(1);
    }
    fprintf(stderr, "indexing not supported\n");
    exit(1);
}
static inline LucidVal lucid_get_index_value(LucidVal container, LucidVal index) {
    if (container.type == LUCID_TYPE_DICT)
        return lucid_get_key(container, index);
    if (container.type == LUCID_TYPE_LIST || container.type == LUCID_TYPE_STR) {
        if (index.type != LUCID_TYPE_INT) {
            fprintf(stderr, "indices must be integers\n");
            exit(1);
        }
        return lucid_get_index(container, index.i);
    }
    fprintf(stderr, "indexing not supported\n");
    exit(1);
}
static inline void lucid_set_index_value(LucidVal container, LucidVal index, LucidVal value) {
    if (container.type == LUCID_TYPE_DICT) {
        lucid_dict_set(container.dict, index, value);
        return;
    }
    if (container.type == LUCID_TYPE_LIST) {
        if (index.type != LUCID_TYPE_INT) {
            fprintf(stderr, "list indices must be integers\n");
            exit(1);
        }
        lucid_list_set(container.list, index.i, value);
        return;
    }
    if (container.type == LUCID_TYPE_STR)
        fprintf(stderr, "cannot assign to string index\n");
    else
        fprintf(stderr, "index assignment not supported\n");
    exit(1);
}

static inline LucidList* lucid_list_repeat(LucidVal v, int64_t n) {
    if (n < 0) n = 0;
    LucidList* l = (LucidList*)malloc(sizeof(LucidList));
    l->len = n;
    l->cap = n < 8 ? 8 : n;
    l->items = (LucidVal*)malloc(sizeof(LucidVal) * l->cap);
    for (int64_t i = 0; i < n; i++) {
        l->items[i] = v;
    }
    return l;
}
static inline LucidList* lucid_list_repeat_value(LucidList* source, int64_t count) {
    if (!source || count <= 0) return lucid_list_new(0);
    if (source->len > 0 && count > INT64_MAX / source->len) exit(1);
    if ((uint64_t)count > (SIZE_MAX - 1) / (size_t)(source->len ? source->len : 1)) exit(1);
    int64_t length = source->len * count;
    LucidList* result = lucid_list_new(length);
    for (int64_t repeat = 0; repeat < count; ++repeat)
        for (int64_t index = 0; index < source->len; ++index)
            lucid_list_append(result, source->items[index]);
    return result;
}
static inline LucidList* lucid_list_concat(LucidList* left, LucidList* right) {
    int64_t left_len = left ? left->len : 0;
    int64_t right_len = right ? right->len : 0;
    if (right_len > INT64_MAX - left_len) exit(1);
    LucidList* result = lucid_list_new(left_len + right_len);
    for (int64_t index = 0; left && index < left_len; ++index)
        lucid_list_append(result, left->items[index]);
    for (int64_t index = 0; right && index < right_len; ++index)
        lucid_list_append(result, right->items[index]);
    return result;
}
static inline const char* lucid_str_repeat(const char* value, int64_t count) {
    if (!value || count <= 0) return "";
    size_t length = strlen(value);
    if ((uint64_t)count > (SIZE_MAX - 1) / (length ? length : 1)) exit(1);
    size_t total = length * (size_t)count;
    char* result = (char*)malloc(total + 1);
    if (!result) exit(1);
    for (int64_t i = 0; i < count; ++i) memcpy(result + (size_t)i * length, value, length);
    result[total] = '\0';
    return result;
}
static inline LucidVal lucid_mul_value(LucidVal left, LucidVal right) {
    if (left.type == LUCID_TYPE_COMPLEX || right.type == LUCID_TYPE_COMPLEX)
        return lucid_complex_mul(left, right);
    if ((left.type == LUCID_TYPE_BIGINT || left.type == LUCID_TYPE_INT) &&
        (right.type == LUCID_TYPE_BIGINT || right.type == LUCID_TYPE_INT))
        return lucid_bigint_binop(left, right, '*');
    LucidVal sequence = left;
    LucidVal count_value = right;
    if (left.type == LUCID_TYPE_INT && right.type == LUCID_TYPE_LIST) {
        sequence = right;
        count_value = left;
    }
    if (sequence.type == LUCID_TYPE_STR && count_value.type == LUCID_TYPE_INT)
        return lucid_str(lucid_str_repeat(sequence.s, count_value.i));
    if (sequence.type == LUCID_TYPE_LIST && count_value.type == LUCID_TYPE_INT)
        return lucid_list_val(lucid_list_repeat_value(sequence.list, count_value.i));
    if (left.type == LUCID_TYPE_INT && right.type == LUCID_TYPE_INT)
        return lucid_int(lucid_checked_mul(left.i, right.i));
    if ((left.type == LUCID_TYPE_INT || left.type == LUCID_TYPE_FLOAT) &&
        (right.type == LUCID_TYPE_INT || right.type == LUCID_TYPE_FLOAT))
        return lucid_float((left.type == LUCID_TYPE_FLOAT ? left.f : (double)left.i) *
                           (right.type == LUCID_TYPE_FLOAT ? right.f : (double)right.i));
    fprintf(stderr, "unsupported operands for *\n");
    exit(1);
}
static inline LucidVal lucid_div_value(LucidVal left, LucidVal right) {
    if (left.type == LUCID_TYPE_COMPLEX || right.type == LUCID_TYPE_COMPLEX)
        return lucid_complex_div(left, right);
    bool left_numeric = left.type == LUCID_TYPE_INT || left.type == LUCID_TYPE_FLOAT || left.type == LUCID_TYPE_BIGINT;
    bool right_numeric = right.type == LUCID_TYPE_INT || right.type == LUCID_TYPE_FLOAT || right.type == LUCID_TYPE_BIGINT;
    if (left_numeric && right_numeric) {
        double lhs = left.type == LUCID_TYPE_INT ? (double)left.i : lucid_as_float(left);
        double rhs = right.type == LUCID_TYPE_INT ? (double)right.i : lucid_as_float(right);
        return lucid_float(lhs / rhs);
    }
    fprintf(stderr, "unsupported operands for /\n");
    exit(1);
}
static inline LucidVal lucid_unary_value(LucidVal value, char op) {
    if (op == '+' && (value.type == LUCID_TYPE_INT || value.type == LUCID_TYPE_FLOAT || value.type == LUCID_TYPE_COMPLEX || value.type == LUCID_TYPE_BIGINT))
        return value;
    if (op == '-' && value.type == LUCID_TYPE_INT)
        return lucid_int(lucid_checked_neg(value.i));
    if (op == '-' && value.type == LUCID_TYPE_FLOAT)
        return lucid_float(-value.f);
    if (op == '-' && value.type == LUCID_TYPE_COMPLEX)
        return lucid_complex(-value.real, -value.imag);
    if (op == '-' && value.type == LUCID_TYPE_BIGINT)
        return lucid_bigint_neg(value);
    if (op == '~' && value.type == LUCID_TYPE_INT)
        return lucid_int(~value.i);
    if (op == '~' && value.type == LUCID_TYPE_BIGINT)
        return lucid_bigint_binop(lucid_bigint_neg(value), lucid_bigint("1"), '-');
    fprintf(stderr, "unsupported unary operand\n");
    exit(1);
}
static inline LucidVal lucid_floor_div_value(LucidVal left, LucidVal right) {
    if ((left.type == LUCID_TYPE_BIGINT || left.type == LUCID_TYPE_INT) &&
        (right.type == LUCID_TYPE_BIGINT || right.type == LUCID_TYPE_INT))
        return lucid_bigint_divmod(left, right, false, true);
    bool left_int = left.type == LUCID_TYPE_INT;
    bool right_int = right.type == LUCID_TYPE_INT;
    bool left_numeric = left_int || left.type == LUCID_TYPE_FLOAT;
    bool right_numeric = right_int || right.type == LUCID_TYPE_FLOAT;
    if (left_numeric && right_numeric) {
        if (left_int && right_int)
            return lucid_int(lucid_int_floor_div(left.i, right.i));
        double lhs = left_int ? (double)left.i : left.f;
        double rhs = right_int ? (double)right.i : right.f;
        return lucid_float(lucid_float_floor_div(lhs, rhs));
    }
    fprintf(stderr, "unsupported operands for //\n");
    exit(1);
}
static inline LucidVal lucid_mod_value(LucidVal left, LucidVal right) {
    if ((left.type == LUCID_TYPE_BIGINT || left.type == LUCID_TYPE_INT) &&
        (right.type == LUCID_TYPE_BIGINT || right.type == LUCID_TYPE_INT))
        return lucid_bigint_divmod(left, right, true, true);
    bool left_int = left.type == LUCID_TYPE_INT;
    bool right_int = right.type == LUCID_TYPE_INT;
    bool left_numeric = left_int || left.type == LUCID_TYPE_FLOAT;
    bool right_numeric = right_int || right.type == LUCID_TYPE_FLOAT;
    if (left_numeric && right_numeric) {
        if (left_int && right_int) return lucid_int(lucid_int_mod(left.i, right.i));
        double lhs = left_int ? (double)left.i : left.f;
        double rhs = right_int ? (double)right.i : right.f;
        return lucid_float(lucid_float_mod(lhs, rhs));
    }
    fprintf(stderr, "unsupported operands for %%\n");
    exit(1);
}

static inline bool lucid_checked_range_advance(int64_t current, int64_t step, int64_t* next) {
    if (step > 0 && current > INT64_MAX - step) return false;
    if (step < 0 && current < INT64_MIN - step) return false;
    *next = current + step;
    return true;
}

static inline int64_t lucid_range_count(int64_t start, int64_t stop, int64_t step) {
    if (step == 0) return 0;
    if ((step > 0 && start >= stop) || (step < 0 && start <= stop)) return 0;
    uint64_t distance = step > 0
        ? (uint64_t)stop - (uint64_t)start
        : (uint64_t)start - (uint64_t)stop;
    uint64_t magnitude = step > 0
        ? (uint64_t)step
        : (uint64_t)(-(step + 1)) + 1;
    uint64_t count = distance / magnitude + (distance % magnitude != 0);
    return count > (uint64_t)INT64_MAX ? INT64_MAX : (int64_t)count;
}

static inline LucidList* lucid_range_to_list(int64_t start, int64_t stop, int64_t step) {
    if (step == 0) { fprintf(stderr, "range() step cannot be zero\n"); exit(1); }
    int64_t count = lucid_range_count(start, stop, step);
    LucidList* l = lucid_list_new(count);
    for (int64_t i = start; (step > 0 ? i < stop : i > stop); ) {
        lucid_list_append(l, lucid_int(i));
        int64_t next;
        if (!lucid_checked_range_advance(i, step, &next)) break;
        i = next;
    }
    return l;
}

static inline LucidList* lucid_iterable_to_list(LucidVal value) {
    if (value.type == LUCID_TYPE_LIST && value.list) {
        LucidList* out = lucid_list_new(value.list->len);
        for (int64_t i = 0; i < value.list->len; ++i) lucid_list_append(out, value.list->items[i]);
        return out;
    }
    if (value.type == LUCID_TYPE_SET && value.set) {
        LucidList* out = lucid_list_new(value.set->len);
        for (int64_t i = 0; i < value.set->len; ++i) lucid_list_append(out, value.set->items[i]);
        return out;
    }
    if (value.type == LUCID_TYPE_DICT && value.dict) {
        return lucid_dict_keys(value.dict);
    }
    if (value.type == LUCID_TYPE_STR && value.s) {
        int64_t count = 0; for (const unsigned char* p = (const unsigned char*)value.s; *p; ++count) p += (*p < 0x80 ? 1 : ((*p & 0xe0) == 0xc0 ? 2 : ((*p & 0xf0) == 0xe0 ? 3 : 4)));
        LucidList* out = lucid_list_new(count);
        for (const unsigned char* p = (const unsigned char*)value.s; *p;) {
            int width = *p < 0x80 ? 1 : ((*p & 0xe0) == 0xc0 ? 2 : ((*p & 0xf0) == 0xe0 ? 3 : 4));
            char* ch = (char*)malloc((size_t)width + 1); if (!ch) exit(1);
            memcpy(ch, p, (size_t)width); ch[width] = '\0'; lucid_list_append(out, lucid_str(ch)); p += width;
        }
        return out;
    }
    fprintf(stderr, "value is not iterable\n"); exit(1);
}
static inline LucidList* lucid_iter_value(LucidVal value) {
    if (value.type == LUCID_TYPE_LIST) return value.list;
    return lucid_iterable_to_list(value);
}

static inline LucidList* lucid_reversed(LucidVal value) {
    LucidList* source = lucid_iterable_to_list(value);
    LucidList* out = lucid_list_new(source ? source->len : 0);
    if (source) for (int64_t i = source->len - 1; i >= 0; --i) lucid_list_append(out, source->items[i]);
    return out;
}

static inline LucidList* lucid_enumerate(LucidVal value, int64_t start) {
    LucidList* source = lucid_iterable_to_list(value);
    LucidList* out = lucid_list_new(source->len);
    for (int64_t i = 0; i < source->len; ++i) {
        LucidList* pair = lucid_list_new(2);
        lucid_list_append(pair, lucid_int(start + i));
        lucid_list_append(pair, source->items[i]);
        lucid_list_append(out, (LucidVal){ .type = LUCID_TYPE_LIST, .list = pair });
    }
    return out;
}

static inline LucidList* lucid_zip(LucidList* inputs) {
    if (!inputs) return lucid_list_new(0);
    int64_t length = -1;
    for (int64_t i = 0; i < inputs->len; ++i) {
        LucidVal input = inputs->items[i];
        LucidList* sequence = lucid_iterable_to_list(input);
        inputs->items[i] = (LucidVal){ .type = LUCID_TYPE_LIST, .list = sequence };
        if (length < 0) length = sequence->len;
        if (sequence->len != length) {
            fprintf(stderr, "zip() arguments have different lengths\n"); exit(1);
        }
    }
    if (length < 0) length = 0;
    LucidList* out = lucid_list_new(length);
    for (int64_t row = 0; row < length; ++row) {
        LucidList* values = lucid_list_new(inputs->len);
        for (int64_t col = 0; col < inputs->len; ++col)
            lucid_list_append(values, inputs->items[col].list->items[row]);
        lucid_list_append(out, (LucidVal){ .type = LUCID_TYPE_LIST, .list = values });
    }
    return out;
}

static inline LucidList* lucid_list_from_value(LucidVal value) {
    return lucid_iterable_to_list(value);
}
static inline LucidVal lucid_next_value(LucidVal value, bool has_default, LucidVal fallback) {
    if (value.type == LUCID_TYPE_LIST && value.list && value.list->len > 0) {
        LucidVal first = value.list->items[0];
        for (int64_t i = 1; i < value.list->len; ++i) value.list->items[i - 1] = value.list->items[i];
        value.list->len--;
        return first;
    }
    if (has_default) return fallback;
    fprintf(stderr, "stop iteration\n"); exit(1);
}

static inline bool lucid_any(LucidVal value) {
    LucidList* items = lucid_iterable_to_list(value);
    for (int64_t i = 0; i < items->len; ++i)
        if (lucid_bool_val(items->items[i])) return true;
    return false;
}

static inline bool lucid_all(LucidVal value) {
    LucidList* items = lucid_iterable_to_list(value);
    for (int64_t i = 0; i < items->len; ++i)
        if (!lucid_bool_val(items->items[i])) return false;
    return true;
}

static int lucid_sorted_compare(const void* left, const void* right) {
    const LucidVal* a = (const LucidVal*)left;
    const LucidVal* b = (const LucidVal*)right;
    if (a->type == LUCID_TYPE_STR && b->type == LUCID_TYPE_STR)
        return strcmp(a->s ? a->s : "", b->s ? b->s : "");
    double av = lucid_as_float(*a);
    double bv = lucid_as_float(*b);
    return av < bv ? -1 : (av > bv ? 1 : 0);
}

static inline LucidList* lucid_sorted(LucidVal value) {
    if (value.type == LUCID_TYPE_SET && value.set) {
        LucidList* out = lucid_list_new(value.set->len);
        for (int64_t i = 0; i < value.set->len; ++i)
            lucid_list_append(out, value.set->items[i]);
        qsort(out->items, (size_t)out->len, sizeof(LucidVal), lucid_sorted_compare);
        return out;
    }
    if (value.type != LUCID_TYPE_LIST || !value.list) {
        fprintf(stderr, "sorted() argument is not iterable\n"); exit(1);
    }
    LucidList* out = lucid_list_new(value.list->len);
    for (int64_t i = 0; i < value.list->len; ++i)
        lucid_list_append(out, value.list->items[i]);
    qsort(out->items, (size_t)out->len, sizeof(LucidVal), lucid_sorted_compare);
    return out;
}

static inline LucidList* lucid_list_slice(LucidList* l, int64_t start, int64_t stop, int64_t step) {
    if (!l) return lucid_list_new(0);
    if (step == 0) { fprintf(stderr, "slice step cannot be zero\n"); exit(1); }
    bool default_start = start == INT64_MIN;
    bool default_stop = stop == INT64_MIN;
    if (default_start) start = step > 0 ? 0 : l->len - 1;
    if (default_stop) stop = step > 0 ? l->len : -1;
    if (start < 0) start += l->len;
    if (stop < 0 && !(default_stop && step < 0)) stop += l->len;
    if (step > 0) {
        if (start < 0) start = 0; if (start > l->len) start = l->len;
        if (stop < 0) stop = 0; if (stop > l->len) stop = l->len;
    } else {
        if (start >= l->len) start = l->len - 1; if (start < -1) start = -1;
        if (stop >= l->len) stop = l->len - 1;
    }
    int64_t count = lucid_range_count(start, stop, step);
    LucidList* res = lucid_list_new(count);
    for (int64_t i = start; (step > 0 ? i < stop : i > stop); ) {
        lucid_list_append(res, l->items[i]);
        int64_t next;
        if (!lucid_checked_range_advance(i, step, &next)) break;
        i = next;
    }
    return res;
}
static inline const char* lucid_str_slice(const char* source, int64_t start, int64_t stop, int64_t step) {
    if (!source) return "";
    if (step == 0) { fprintf(stderr, "slice step cannot be zero\n"); exit(1); }
    int64_t len = 0; for (const unsigned char* p = (const unsigned char*)source; *p; ++len) p += (*p < 0x80 ? 1 : ((*p & 0xe0) == 0xc0 ? 2 : ((*p & 0xf0) == 0xe0 ? 3 : 4)));
    bool default_start = start == INT64_MIN;
    bool default_stop = stop == INT64_MIN;
    if (default_start) start = step > 0 ? 0 : len - 1;
    if (default_stop) stop = step > 0 ? len : -1;
    if (start < 0) start += len;
    if (stop < 0 && !(default_stop && step < 0)) stop += len;
    if (step > 0) {
        if (start < 0) start = 0; if (start > len) start = len;
        if (stop < 0) stop = 0; if (stop > len) stop = len;
    } else {
        if (start >= len) start = len - 1; if (start < -1) start = -1;
        if (stop >= len) stop = len - 1;
    }
    int64_t count = lucid_range_count(start, stop, step);
    size_t bytes = strlen(source); char* out = (char*)malloc(bytes + 1);
    if (!out) { fprintf(stderr, "out of memory slicing string\n"); exit(1); }
    int64_t pos = 0;
    for (int64_t i = start; (step > 0 ? i < stop : i > stop); ) {
        const unsigned char* p = (const unsigned char*)source;
        for (int64_t j = 0; j < i; ++j) p += (*p < 0x80 ? 1 : ((*p & 0xe0) == 0xc0 ? 2 : ((*p & 0xf0) == 0xe0 ? 3 : 4)));
        int width = *p < 0x80 ? 1 : ((*p & 0xe0) == 0xc0 ? 2 : ((*p & 0xf0) == 0xe0 ? 3 : 4));
        memcpy(out + pos, p, (size_t)width); pos += width;
        int64_t next;
        if (!lucid_checked_range_advance(i, step, &next)) break;
        i = next;
    }
    out[pos] = '\0';
    return out;
}
static inline LucidVal lucid_slice_value(LucidVal value, int64_t start, int64_t stop, int64_t step) {
    if (value.type == LUCID_TYPE_STR)
        return lucid_str(lucid_str_slice(value.s, start, stop, step));
    if (value.type == LUCID_TYPE_LIST)
        return lucid_list_val(lucid_list_slice(value.list, start, stop, step));
    fprintf(stderr, "slicing not supported\n");
    exit(1);
}

static inline int64_t _len_list(LucidList* l) { return l ? l->len : 0; }
static inline int64_t _len_str(const char* s) {
    int64_t n = 0; if (!s) return 0;
    for (const unsigned char* p = (const unsigned char*)s; *p; ++n) p += (*p < 0x80 ? 1 : ((*p & 0xe0) == 0xc0 ? 2 : ((*p & 0xf0) == 0xe0 ? 3 : 4)));
    return n;
}
static inline int64_t _len_val(LucidVal v) {
    if (v.type == LUCID_TYPE_LIST) return v.list ? v.list->len : 0;
    if (v.type == LUCID_TYPE_STR) return _len_str(v.s);
    if (v.type == LUCID_TYPE_DICT) return v.dict ? v.dict->len : 0;
    if (v.type == LUCID_TYPE_SET) return v.set ? v.set->len : 0;
    return 0;
}
static inline int64_t _len_def(void* p) { (void)p; return 0; }

#define lucid_len(x) _Generic((x), \
    LucidList*: _len_list, \
    char*: _len_str, \
    const char*: _len_str, \
    LucidVal: _len_val, \
    default: _len_def \
)(x)

static inline int64_t lucid_list_sum(LucidList* l) {
    if (!l) return 0;
    int64_t s = 0;
    for (int64_t i = 0; i < l->len; i++) {
        s += lucid_as_int(l->items[i]);
    }
    return s;
}
static inline int64_t lucid_sum_value(LucidVal value) {
    return lucid_list_sum(lucid_list_from_value(value));
}
static inline double lucid_sum_float_value(LucidVal value) {
    LucidList* items = lucid_list_from_value(value);
    double total = 0.0;
    if (!items) return total;
    for (int64_t i = 0; i < items->len; ++i) total += lucid_as_float(items->items[i]);
    return total;
}
static inline LucidVal lucid_sum_bigint_value(LucidVal value) {
    LucidList* items = lucid_list_from_value(value);
    LucidVal total = lucid_bigint("0");
    if (!items) return total;
    for (int64_t i = 0; i < items->len; ++i) total = lucid_bigint_binop(total, items->items[i], '+');
    return total;
}

static inline LucidVal lucid_list_min(LucidList* l) {
    if (!l || l->len == 0) return lucid_none();
    LucidVal best = l->items[0];
    for (int64_t i = 1; i < l->len; i++)
        if (lucid_as_float(l->items[i]) < lucid_as_float(best)) best = l->items[i];
    return best;
}

static inline LucidVal lucid_list_max(LucidList* l) {
    if (!l || l->len == 0) return lucid_none();
    LucidVal best = l->items[0];
    for (int64_t i = 1; i < l->len; i++)
        if (lucid_as_float(l->items[i]) > lucid_as_float(best)) best = l->items[i];
    return best;
}
static inline LucidVal lucid_min_value(LucidVal value) {
    return lucid_list_min(lucid_list_from_value(value));
}
static inline LucidVal lucid_max_value(LucidVal value) {
    return lucid_list_max(lucid_list_from_value(value));
}

static inline double lucid_round_places(double val, int64_t places) {
    double factor = pow(10.0, (double)places);
    return round(val * factor) / factor;
}

static inline double lucid_time_now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec + (double)ts.tv_nsec * 1e-9;
}

static inline void lucid_print_val(LucidVal v) {
    switch (v.type) {
        case LUCID_TYPE_INT:
            if (v.i == INT64_MAX) printf("int.inf");
            else if (v.i == INT64_MIN + 1) printf("-int.inf");
            else if (v.i == INT64_MIN) printf("int.nan");
            else printf("%ld", v.i);
            break;
        case LUCID_TYPE_FLOAT:
            // Converting an out-of-range double to int64_t is undefined (and
            // commonly wraps to INT64_MIN).  Keep integral-looking values in
            // floating formatting once they approach the exact int64 limit.
            if (fabs(v.f - round(v.f)) < 1e-12 &&
                v.f >= -9223372036854774784.0 && v.f < 9223372036854774784.0) {
                printf("%ld", (int64_t)round(v.f));
            } else {
                printf("%.10g", v.f);
            }
            break;
        case LUCID_TYPE_COMPLEX: printf("(%g%+gj)", v.real, v.imag); break;
        case LUCID_TYPE_BIGINT: printf("%s", v.bigint ? v.bigint : "0"); break;
        case LUCID_TYPE_BOOL: printf("%s", v.b ? "true" : "false"); break;
        case LUCID_TYPE_STR: printf("%s", v.s ? v.s : ""); break;
        case LUCID_TYPE_NONE: printf("none"); break;
        case LUCID_TYPE_LIST: printf("[list len=%ld]", v.list ? v.list->len : 0); break;
        case LUCID_TYPE_DICT: printf("[dict len=%ld]", v.dict ? v.dict->len : 0); break;
        case LUCID_TYPE_SET: printf("[set len=%ld]", v.set ? v.set->len : 0); break;
        case LUCID_TYPE_PTR: printf("[obj %p]", v.ptr); break;
        case LUCID_TYPE_FUTURE: printf("<future>"); break;
        case LUCID_TYPE_CONTEXT: printf("<context>"); break;
    }
}

"#);
    }

    fn map_type_expr(&self, te: Option<&TypeExpr>) -> String {
        match te {
            Some(TypeExpr::Named { name, .. }) => match name.as_str() {
                "int" => "int64_t".to_string(),
                "float" => "double".to_string(),
                "complex" => "LucidVal".to_string(),
                "bool" => "bool".to_string(),
                "str" => "const char*".to_string(),
                "none" | "None" => "void".to_string(),
                "list" => "LucidList*".to_string(),
                "dict" => "LucidDict*".to_string(),
                "set" => "LucidSet*".to_string(),
                // `object` is the universal value view.  It must use the
                // tagged runtime value rather than becoming an undefined C
                // pointer type (`object*`).
                "object" | "Any" => "LucidVal".to_string(),
                "Bytes" | "ByteArray" | "MemoryView" | "SourceLocation" | "VarName" => {
                    "const char*".to_string()
                }
                other => format!("{other}*"),
            },
            // A union has no single C layout.  Preserve the full Lucid value
            // tag rather than borrowing the first arm's spelling (which can
            // produce invalid signatures such as `int*` for `int | none`).
            Some(TypeExpr::Union { .. }) => "LucidVal".to_string(),
            None => "LucidVal".to_string(),
            _ => "LucidVal".to_string(),
        }
    }

    fn emit_param_list(&self, params: &[Param]) -> String {
        if params.is_empty() {
            return "void".to_string();
        }
        let parts: Vec<String> = params
            .iter()
            .map(|p| {
                let ty = if p.is_variadic_keyword {
                    "LucidDict*".into()
                } else if p.is_variadic_positional {
                    "LucidList*".into()
                } else {
                    self.map_type_expr(p.type_annotation.as_ref())
                };
                format!("{ty} lucid_var_{}", p.name)
            })
            .collect();
        parts.join(", ")
    }

    fn future_arg_expr(&self, ty: &str, index: usize) -> String {
        match ty {
            "int64_t" => format!("lucid_as_int(args->items[{index}])"),
            "double" => format!("lucid_as_float(args->items[{index}])"),
            "bool" => format!("lucid_as_bool(args->items[{index}])"),
            "const char*" => format!("lucid_as_str(args->items[{index}])"),
            "LucidList*" => format!("lucid_as_list(args->items[{index}])"),
            "LucidVal" => format!("args->items[{index}]"),
            other => format!("({other})lucid_as_ptr(args->items[{index}])"),
        }
    }

    fn dispatch_name(&self, f: &FunctionDef, _fallback: &str) -> String {
        if !f.is_dispatch {
            return format!("lucid_fn_{}", f.name);
        }
        let suffix = f
            .params
            .first()
            .and_then(|p| match &p.type_annotation {
                Some(TypeExpr::Named { name, .. }) => Some(name.as_str()),
                _ => None,
            })
            .unwrap_or("Any");
        format!(
            "lucid_fn_{}__{}",
            Self::mangle_component(&f.name),
            Self::mangle_component(suffix)
        )
    }

    fn mangle_component(name: &str) -> String {
        let mut out = String::new();
        for byte in name.bytes() {
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                out.push(byte as char);
            } else {
                out.push_str(&format!("_{byte:02x}"));
            }
        }
        if out.is_empty() {
            "anonymous".to_string()
        } else {
            out
        }
    }

    fn binary_op_name(op: &BinaryOp) -> Option<&'static str> {
        Some(match op {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::FloorDiv => "//",
            BinaryOp::Mod => "%",
            BinaryOp::Pow => "**",
            BinaryOp::BitAnd => "&",
            BinaryOp::BitOr => "|",
            BinaryOp::BitXor => "^",
            BinaryOp::Shl => "<<",
            BinaryOp::Shr => ">>",
            _ => return None,
        })
    }

    fn infer_expr_type(&self, expr: &Expr, vars: &HashMap<String, String>) -> String {
        match expr {
            Expr::Literal { value, .. } => match value {
                LiteralValue::Int(_) => "int64_t".to_string(),
                LiteralValue::BigInt(_) => "LucidVal".to_string(),
                LiteralValue::Float(_) => "double".to_string(),
                LiteralValue::Complex(_) => "LucidVal".to_string(),
                LiteralValue::Bool(_) => "bool".to_string(),
                LiteralValue::Str(_) => "const char*".to_string(),
                LiteralValue::None => "LucidVal".to_string(),
                _ => "LucidVal".to_string(),
            },
            Expr::Type(_) | Expr::Skip(_) | Expr::AnonymousDef { .. } => "LucidVal".to_string(),
            Expr::List { .. } | Expr::ListComp { .. } => "LucidList*".to_string(),
            Expr::Dict { .. } | Expr::DictComp { .. } => "LucidDict*".to_string(),
            Expr::Record { .. } => "LucidDict*".to_string(),
            Expr::Set { .. } | Expr::SetComp { .. } => "LucidSet*".to_string(),
            Expr::Call { func, args, .. } => {
                if let Expr::Ident { name, .. } = &**func {
                    if self.known_classes.contains_key(name) {
                        return format!("{name}*");
                    }
                    if let Some(ret) = self.known_fns.get(name) {
                        return ret.clone();
                    }
                    match name.as_str() {
                        "len" | "sum" => "int64_t".to_string(),
                        "int" => args
                            .first()
                            .map(|arg| {
                                let arg_type = self.infer_expr_type(&arg.value, vars);
                                if matches!(arg_type.as_str(), "LucidVal" | "const char*") {
                                    "LucidVal".to_string()
                                } else {
                                    "int64_t".to_string()
                                }
                            })
                            .unwrap_or_else(|| "int64_t".to_string()),
                        "any" | "all" => "bool".to_string(),
                        "iter" => args
                            .first()
                            .map(|a| self.infer_expr_type(&a.value, vars))
                            .unwrap_or_else(|| "LucidVal".to_string()),
                        "next" => "LucidVal".to_string(),
                        // The runtime hash builtin returns a Lucid integer
                        // value (which may be widened in the future), not a
                        // raw C scalar.
                        "hash" => "LucidVal".to_string(),
                        "format" => "const char*".to_string(),
                        "repr" => "const char*".to_string(),
                        "locals" => "LucidDict*".to_string(),
                        "ord" => "int64_t".to_string(),
                        "chr" => "LucidVal".to_string(),
                        "bin" | "oct" | "hex" => "const char*".to_string(),
                        "abs" => args
                            .first()
                            .map(|a| self.infer_expr_type(&a.value, vars))
                            .unwrap_or_else(|| "double".to_string()),
                        "round" => {
                            if args.len() == 1 {
                                "int64_t".to_string()
                            } else {
                                "double".to_string()
                            }
                        }
                        "list" | "sorted" => "LucidList*".to_string(),
                        "min" | "max" => args
                            .first()
                            .and_then(|arg| self.indexed_element_class(&arg.value))
                            .map(|class| format!("{class}*"))
                            .unwrap_or_else(|| "LucidVal".to_string()),
                        "str" => "const char*".to_string(),
                        "set" => "LucidSet*".to_string(),
                        "dict" => "LucidDict*".to_string(),
                        "slice" => "LucidDict*".to_string(),
                        "range" => "LucidList*".to_string(),
                        "freeze" => args
                            .first()
                            .map(|a| self.infer_expr_type(&a.value, vars))
                            .unwrap_or_else(|| "LucidVal".to_string()),
                        _ => "LucidVal".to_string(),
                    }
                } else if let Expr::Attribute {
                    value: obj, attr, ..
                } = &**func
                {
                    if let Expr::Ident { name: obj_name, .. } = &**obj {
                        if obj_name == "time" && matches!(attr.as_str(), "time" | "monotonic") {
                            return "double".to_string();
                        }
                        if obj_name == "sys" && matches!(attr.as_str(), "platform" | "version") {
                            return "const char*".to_string();
                        }
                    }
                    if attr == "next" {
                        return "double".to_string();
                    }
                    if attr == "replace" {
                        if let Expr::Ident { name, .. } = &**obj {
                            if self.known_classes.contains_key(name) {
                                return format!("{name}*");
                            }
                        }
                    }
                    "LucidVal".to_string()
                } else {
                    "LucidVal".to_string()
                }
            }
            Expr::Index { .. } => "LucidVal".to_string(),
            Expr::Binary {
                op, left, right, ..
            } => {
                let l_ty = self.infer_expr_type(left, vars);
                let r_ty = self.infer_expr_type(right, vars);
                let has_bigint = self.expr_is_bigint(left) || self.expr_is_bigint(right);
                // BigInt normally stays boxed, but a mixed BigInt/float
                // operation follows the language's numeric promotion rule
                // and produces a native double.  Keep this check before the
                // generic BigInt fallback so assignments receive the right
                // C type instead of truncating through int64_t.
                if has_bigint && (l_ty == "double" || r_ty == "double") {
                    return match op {
                        BinaryOp::Eq
                        | BinaryOp::NotEq
                        | BinaryOp::Lt
                        | BinaryOp::LtEq
                        | BinaryOp::Gt
                        | BinaryOp::GtEq
                        | BinaryOp::Is
                        | BinaryOp::IsNot
                        | BinaryOp::Identity
                        | BinaryOp::NotIdentity => "bool".to_string(),
                        _ => "double".to_string(),
                    };
                }
                if has_bigint || self.expr_is_complex(left) || self.expr_is_complex(right) {
                    return "LucidVal".to_string();
                }
                if *op == BinaryOp::Add && l_ty == "LucidList*" && r_ty == "LucidList*" {
                    return "LucidList*".to_string();
                }
                if *op == BinaryOp::Add && l_ty == "LucidVal" && r_ty == "LucidVal" {
                    return "LucidVal".to_string();
                }
                if *op == BinaryOp::Mul && l_ty == "LucidVal" && r_ty == "LucidVal" {
                    return "LucidVal".to_string();
                }
                if matches!(
                    op,
                    BinaryOp::Sub | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor
                ) && l_ty == "LucidVal"
                    && r_ty == "LucidVal"
                {
                    return "LucidVal".to_string();
                }
                if *op == BinaryOp::Div && l_ty == "LucidVal" && r_ty == "LucidVal" {
                    return "LucidVal".to_string();
                }
                if matches!(op, BinaryOp::FloorDiv | BinaryOp::Mod)
                    && l_ty == "LucidVal"
                    && r_ty == "LucidVal"
                {
                    return "LucidVal".to_string();
                }
                if *op == BinaryOp::Pow && l_ty == "LucidVal" && r_ty == "LucidVal" {
                    return "LucidVal".to_string();
                }
                if matches!(op, BinaryOp::Shl | BinaryOp::Shr)
                    && l_ty == "LucidVal"
                    && r_ty == "LucidVal"
                {
                    return "LucidVal".to_string();
                }
                if *op == BinaryOp::Mul && (l_ty == "LucidList*" || r_ty == "LucidList*") {
                    return "LucidList*".to_string();
                }
                if let Some(operator) = Self::binary_op_name(op) {
                    if self.dispatch_fns.contains_key(operator) {
                        if let Some(ret) = self.known_fns.get(operator) {
                            return ret.clone();
                        }
                    }
                }
                match op {
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::LtEq
                    | BinaryOp::Gt
                    | BinaryOp::GtEq
                    | BinaryOp::Is
                    | BinaryOp::IsNot
                    | BinaryOp::Identity
                    | BinaryOp::NotIdentity => "bool".to_string(),
                    BinaryOp::Div | BinaryOp::Pow => "double".to_string(),
                    BinaryOp::Mod
                    | BinaryOp::FloorDiv
                    | BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr => "int64_t".to_string(),
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                        if *op == BinaryOp::Add && l_ty == "const char*" && r_ty == "const char*" {
                            "const char*".to_string()
                        } else if l_ty == "double" || r_ty == "double" {
                            "double".to_string()
                        } else if l_ty == "int64_t" && r_ty == "int64_t" {
                            "int64_t".to_string()
                        } else {
                            "double".to_string()
                        }
                    }
                    _ => "LucidVal".to_string(),
                }
            }
            Expr::Unary { op, expr, .. } => match op {
                UnaryOp::Not => "bool".to_string(),
                _ => self.infer_expr_type(expr, vars),
            },
            Expr::Ident { name, .. } => {
                if name == "pi" && !self.global_vars.contains_key(name) {
                    return "double".to_string();
                }
                if name == "e" && !self.global_vars.contains_key(name) {
                    return "double".to_string();
                }
                if matches!(name.as_str(), "platform" | "version")
                    && !self.global_vars.contains_key(name)
                {
                    return "const char*".to_string();
                }
                if name == "argv" && !self.global_vars.contains_key(name) {
                    return "LucidList*".to_string();
                }
                if let Some(ty) = vars
                    .get(name)
                    .or_else(|| self.var_types.get(name))
                    .or_else(|| self.global_vars.get(name))
                {
                    ty.clone()
                } else {
                    "LucidVal".to_string()
                }
            }
            Expr::Attribute { value, attr, .. } => {
                if let Expr::Ident { name, .. } = &**value {
                    if name == "float" && (attr == "inf" || attr == "nan") {
                        return "double".to_string();
                    }
                    if name == "int" && (attr == "inf" || attr == "nan") {
                        return "int64_t".to_string();
                    }
                    if name == "complex" && attr == "nan" {
                        return "LucidVal".to_string();
                    }
                    if name == "math" && (attr == "pi" || attr == "e") {
                        return "double".to_string();
                    }
                }
                "LucidVal".to_string()
            }
            _ => "LucidVal".to_string(),
        }
    }

    fn collect_vars_from_stmt(&mut self, stmt: &Stmt, vars: &mut HashMap<String, String>) {
        match stmt {
            Stmt::VarDef {
                pattern,
                type_annotation,
                value,
                ..
            } => {
                if let Pattern::Ident(name, _) = pattern {
                    if let Some(Expr::Attribute { value, attr, .. }) = value {
                        if matches!(&**value, Expr::Ident { name: base, .. } if base == "str")
                            && matches!(attr.as_str(), "bin" | "oct" | "hex")
                        {
                            self.function_aliases
                                .insert(name.clone(), format!("__str_base_{attr}"));
                            return;
                        }
                    }
                    let ty = if let Some(ta) = type_annotation {
                        self.map_type_expr(Some(ta))
                    } else if let Some(val) = value {
                        self.infer_expr_type(val, vars)
                    } else {
                        "LucidVal".to_string()
                    };
                    self.record_list_element_class(name, value.as_ref(), type_annotation.as_ref());
                    if let Some(val) = value {
                        if self.expr_is_complex(val) {
                            self.complex_names.insert(name.clone());
                        }
                        if self.expr_is_bigint(val) {
                            self.bigint_names.insert(name.clone());
                        }
                        if ty == "LucidVal" && matches!(val, Expr::Call { .. }) {
                            self.dynamic_vars.insert(name.clone());
                        }
                    }
                    vars.insert(name.clone(), ty);
                } else {
                    self.collect_pattern_vars(pattern, vars);
                }
            }
            Stmt::Assignment { target, value, .. } => {
                if let Expr::Ident { name, .. } = target {
                    if let Expr::Attribute {
                        value: object,
                        attr,
                        ..
                    } = value
                    {
                        if matches!(&**object, Expr::Ident { name: base, .. } if base == "str")
                            && matches!(attr.as_str(), "bin" | "oct" | "hex")
                        {
                            self.function_aliases
                                .insert(name.clone(), format!("__str_base_{attr}"));
                            return;
                        }
                    }
                    if !vars.contains_key(name) {
                        let ty = self.infer_expr_type(value, vars);
                        self.record_list_element_class(name, Some(value), None);
                        if self.expr_is_complex(value) {
                            self.complex_names.insert(name.clone());
                        }
                        if self.expr_is_bigint(value) {
                            self.bigint_names.insert(name.clone());
                        }
                        if ty == "LucidVal" && matches!(value, Expr::Call { .. }) {
                            self.dynamic_vars.insert(name.clone());
                        }
                        vars.insert(name.clone(), ty);
                    }
                } else if let Expr::Record { fields, .. } = target {
                    for (_, field) in fields {
                        let name = match field {
                            Expr::Ident { name, .. } => name,
                            Expr::Unary { expr, .. } => match &**expr {
                                Expr::Ident { name, .. } => name,
                                _ => continue,
                            },
                            _ => continue,
                        };
                        vars.entry(name.clone())
                            .or_insert_with(|| "LucidVal".to_string());
                    }
                }
            }
            Stmt::For {
                target,
                iterable,
                body,
                if_broken,
                ..
            } => {
                self.collect_pattern_vars(target, vars);
                if let Pattern::Ident(name, _) = target {
                    let ty = if matches!(iterable, Expr::Call { func, .. } if matches!(&**func, Expr::Ident { name, .. } if name == "range"))
                    {
                        "int64_t".to_string()
                    } else {
                        "LucidVal".to_string()
                    };
                    vars.insert(name.clone(), ty);
                }
                for s in body {
                    self.collect_vars_from_stmt(s, vars);
                }
                if let Some(clause) = if_broken {
                    for s in clause {
                        self.collect_vars_from_stmt(s, vars);
                    }
                }
            }
            Stmt::If {
                then_branch,
                elif_branches,
                else_branch,
                ..
            } => {
                for s in then_branch {
                    self.collect_vars_from_stmt(s, vars);
                }
                for (_, elif_stmts) in elif_branches {
                    for s in elif_stmts {
                        self.collect_vars_from_stmt(s, vars);
                    }
                }
                if let Some(else_stmts) = else_branch {
                    for s in else_stmts {
                        self.collect_vars_from_stmt(s, vars);
                    }
                }
            }
            Stmt::While {
                body, if_broken, ..
            } => {
                for s in body {
                    self.collect_vars_from_stmt(s, vars);
                }
                if let Some(clause) = if_broken {
                    for s in clause {
                        self.collect_vars_from_stmt(s, vars);
                    }
                }
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    self.collect_pattern_vars(&arm.pattern, vars);
                    for s in &arm.body {
                        self.collect_vars_from_stmt(s, vars);
                    }
                }
            }
            Stmt::Try {
                body,
                handlers,
                finally_body,
                ..
            } => {
                for s in body {
                    self.collect_vars_from_stmt(s, vars);
                }
                for handler in handlers {
                    if let Some(name) = &handler.name {
                        vars.entry(name.clone())
                            .or_insert_with(|| "LucidVal".to_string());
                    }
                    for s in &handler.body {
                        self.collect_vars_from_stmt(s, vars);
                    }
                }
                if let Some(cleanup) = finally_body {
                    for s in cleanup {
                        self.collect_vars_from_stmt(s, vars);
                    }
                }
            }
            Stmt::With { items, body, .. } => {
                for item in items {
                    if let Some(target) = &item.target {
                        self.collect_pattern_vars(target, vars);
                    }
                }
                for s in body {
                    self.collect_vars_from_stmt(s, vars);
                }
            }
            _ => {}
        }
    }

    fn collect_pattern_vars(&self, pattern: &Pattern, vars: &mut HashMap<String, String>) {
        match pattern {
            Pattern::Ident(name, _)
                if !matches!(name.as_str(), "int" | "float" | "bool" | "str" | "none") =>
            {
                vars.entry(name.clone())
                    .or_insert_with(|| "LucidVal".to_string());
            }
            Pattern::ClassDestructure { fields, .. } | Pattern::RecordDestructure(fields, _) => {
                for (_, nested) in fields {
                    self.collect_pattern_vars(nested, vars);
                }
            }
            Pattern::Tuple(items, _) => {
                for item in items {
                    self.collect_pattern_vars(item, vars);
                }
            }
            _ => {}
        }
    }

    fn emit_pattern_condition(
        &mut self,
        pattern: &Pattern,
        subject: &str,
    ) -> Result<String, CodegenError> {
        Ok(match pattern {
            Pattern::Wildcard(_) => "true".to_string(),
            Pattern::Ident(name, _) => match name.as_str() {
                "int" => format!("{subject}.type == LUCID_TYPE_INT"),
                "float" => format!("{subject}.type == LUCID_TYPE_FLOAT"),
                "bool" => format!("{subject}.type == LUCID_TYPE_BOOL"),
                "str" => format!("{subject}.type == LUCID_TYPE_STR"),
                "none" | "None" => format!("{subject}.type == LUCID_TYPE_NONE"),
                name if self.known_classes.contains_key(name) => {
                    let names = self.class_pattern_names(name);
                    names
                        .into_iter()
                        .map(|class_name| {
                            format!("lucid_object_is(lucid_as_ptr({subject}), \"{class_name}\")")
                        })
                        .collect::<Vec<_>>()
                        .join(" || ")
                }
                _ => "true".to_string(),
            },
            Pattern::Literal(value, _) => {
                let literal = self.emit_expr(&Expr::Literal {
                    value: value.clone(),
                    span: lucid_syntax::token::Span::default(),
                })?;
                format!("lucid_eq({subject}, lucid_wrap({literal}))")
            }
            Pattern::ClassDestructure { class_name, .. } => {
                let names = self.class_pattern_names(class_name);
                names
                    .into_iter()
                    .map(|name| format!("lucid_object_is(lucid_as_ptr({subject}), \"{name}\")"))
                    .collect::<Vec<_>>()
                    .join(" || ")
            }
            Pattern::Tuple(_, _) => format!("({subject}.type == LUCID_TYPE_LIST)"),
            Pattern::Star(_, _) => "true".to_string(),
            Pattern::RecordDestructure(fields, _) => {
                let checks = fields.iter().filter_map(|(name, _)| name.as_ref().map(|name| format!("lucid_dict_contains((LucidDict*)lucid_as_ptr({subject}), lucid_str(\"{}\"))", name.replace('"', "\\\"")))).collect::<Vec<_>>();
                if checks.is_empty() {
                    format!("({subject}.type == LUCID_TYPE_DICT)")
                } else {
                    format!(
                        "(({subject}.type == LUCID_TYPE_DICT) && {})",
                        checks.join(" && ")
                    )
                }
            }
            Pattern::Type(type_expr, _) => match type_expr {
                TypeExpr::Named { name, .. } => match name.as_str() {
                    "int" => format!("{subject}.type == LUCID_TYPE_INT"),
                    "float" => format!("{subject}.type == LUCID_TYPE_FLOAT"),
                    "bool" => format!("{subject}.type == LUCID_TYPE_BOOL"),
                    "str" => format!("{subject}.type == LUCID_TYPE_STR"),
                    "none" | "None" => format!("{subject}.type == LUCID_TYPE_NONE"),
                    name if self.known_classes.contains_key(name) => self
                        .class_pattern_names(name)
                        .into_iter()
                        .map(|class_name| {
                            format!("lucid_object_is(lucid_as_ptr({subject}), \"{class_name}\")")
                        })
                        .collect::<Vec<_>>()
                        .join(" || "),
                    _ => "true".to_string(),
                },
                _ => "true".to_string(),
            },
        })
    }

    fn class_pattern_names(&self, expected: &str) -> Vec<String> {
        self.known_classes
            .keys()
            .filter(|candidate| {
                let mut current = Some(candidate.as_str());
                let mut seen = HashSet::new();
                while let Some(name) = current {
                    if !seen.insert(name) {
                        break;
                    }
                    if name == expected {
                        return true;
                    }
                    current = self.known_parents.get(name).map(String::as_str);
                }
                false
            })
            .cloned()
            .collect()
    }

    fn class_is_subtype(&self, actual: &str, expected: &str) -> bool {
        let mut current = Some(actual);
        let mut seen = HashSet::new();
        while let Some(name) = current {
            if !seen.insert(name) {
                break;
            }
            if name == expected {
                return true;
            }
            current = self.known_parents.get(name).map(String::as_str);
        }
        false
    }

    fn emit_pattern_bindings(&mut self, pattern: &Pattern, subject: &str) {
        match pattern {
            Pattern::Ident(name, _)
                if !matches!(
                    name.as_str(),
                    "int" | "float" | "bool" | "str" | "none" | "None"
                ) =>
            {
                self.emit_line(&format!("lucid_var_{name} = {subject};"));
            }
            Pattern::ClassDestructure {
                class_name, fields, ..
            } => {
                let declared = self
                    .known_classes
                    .get(class_name)
                    .cloned()
                    .unwrap_or_default();
                for (index, (field_name, nested)) in fields.iter().enumerate() {
                    let name = field_name.clone().or_else(|| declared.get(index).cloned());
                    if let Some(name) = name {
                        let field =
                            format!("(({class_name}*)lucid_as_ptr(lucid_wrap({subject})))->{name}");
                        self.emit_pattern_bindings(nested, &format!("lucid_wrap({field})"));
                    }
                }
            }
            Pattern::RecordDestructure(fields, _) => {
                for (field_name, nested) in fields {
                    if let Some(name) = field_name {
                        let field = format!(
                            "lucid_dict_get((LucidDict*)lucid_as_ptr(lucid_wrap({subject})), lucid_str(\"{}\"))",
                            name.replace('"', "\\\"")
                        );
                        self.emit_pattern_bindings(nested, &field);
                    }
                }
            }
            Pattern::Tuple(items, _) => {
                let star_index = items.iter().position(|p| matches!(p, Pattern::Star(_, _)));
                if let Some(star) = star_index {
                    for (index, item) in items[..star].iter().enumerate() {
                        let element = format!(
                            "lucid_list_get(lucid_as_list(lucid_wrap({subject})), {index})"
                        );
                        self.emit_pattern_bindings(item, &element);
                    }
                    let tail_count = items.len() - star - 1;
                    if let Pattern::Star(nested, _) = &items[star] {
                        let rest = format!(
                            "lucid_list_slice(lucid_as_list(lucid_wrap({subject})), {star}, (int64_t)lucid_as_list(lucid_wrap({subject}))->len - {tail_count}, 1)"
                        );
                        self.emit_pattern_bindings(nested, &rest);
                    }
                    for (offset, item) in items[star + 1..].iter().enumerate() {
                        let index = format!(
                            "(int64_t)lucid_as_list(lucid_wrap({subject}))->len - {}",
                            tail_count - offset
                        );
                        let element = format!(
                            "lucid_list_get(lucid_as_list(lucid_wrap({subject})), {index})"
                        );
                        self.emit_pattern_bindings(item, &element);
                    }
                } else {
                    for (index, item) in items.iter().enumerate() {
                        let element = format!(
                            "lucid_list_get(lucid_as_list(lucid_wrap({subject})), {index})"
                        );
                        self.emit_pattern_bindings(item, &element);
                    }
                }
            }
            Pattern::Star(nested, _) => self.emit_pattern_bindings(nested, subject),
            _ => {}
        }
    }

    fn pattern_binding_lines(&self, pattern: &Pattern, subject: &str, lines: &mut Vec<String>) {
        match pattern {
            Pattern::Ident(name, _)
                if !matches!(
                    name.as_str(),
                    "int" | "float" | "bool" | "str" | "none" | "None"
                ) =>
            {
                lines.push(format!("lucid_var_{name} = {subject};"));
            }
            Pattern::ClassDestructure {
                class_name, fields, ..
            } => {
                let declared = self
                    .known_classes
                    .get(class_name)
                    .cloned()
                    .unwrap_or_default();
                for (index, (field_name, nested)) in fields.iter().enumerate() {
                    if let Some(name) = field_name.clone().or_else(|| declared.get(index).cloned())
                    {
                        let field = format!(
                            "lucid_wrap((({class_name}*)lucid_as_ptr(lucid_wrap({subject})))->{name})"
                        );
                        self.pattern_binding_lines(nested, &field, lines);
                    }
                }
            }
            Pattern::RecordDestructure(fields, _) => {
                for (field_name, nested) in fields {
                    if let Some(name) = field_name {
                        let field = format!(
                            "lucid_dict_get((LucidDict*)lucid_as_ptr(lucid_wrap({subject})), lucid_str(\"{}\"))",
                            name.replace('"', "\\\"")
                        );
                        self.pattern_binding_lines(nested, &field, lines);
                    }
                }
            }
            Pattern::Tuple(items, _) => {
                let star_index = items.iter().position(|p| matches!(p, Pattern::Star(_, _)));
                if let Some(star) = star_index {
                    for (index, nested) in items[..star].iter().enumerate() {
                        let element = format!(
                            "lucid_list_get(lucid_as_list(lucid_wrap({subject})), {index})"
                        );
                        self.pattern_binding_lines(nested, &element, lines);
                    }
                    let tail_count = items.len() - star - 1;
                    if let Pattern::Star(nested, _) = &items[star] {
                        let rest = format!(
                            "lucid_wrap(lucid_list_slice(lucid_as_list(lucid_wrap({subject})), {star}, (int64_t)lucid_as_list(lucid_wrap({subject}))->len - {tail_count}, 1))"
                        );
                        self.pattern_binding_lines(nested, &rest, lines);
                    }
                    for (offset, nested) in items[star + 1..].iter().enumerate() {
                        let index = format!(
                            "(int64_t)lucid_as_list(lucid_wrap({subject}))->len - {}",
                            tail_count - offset
                        );
                        let element = format!(
                            "lucid_list_get(lucid_as_list(lucid_wrap({subject})), {index})"
                        );
                        self.pattern_binding_lines(nested, &element, lines);
                    }
                } else {
                    for (index, nested) in items.iter().enumerate() {
                        let element = format!(
                            "lucid_list_get(lucid_as_list(lucid_wrap({subject})), {index})"
                        );
                        self.pattern_binding_lines(nested, &element, lines);
                    }
                }
            }
            Pattern::Star(nested, _) => self.pattern_binding_lines(nested, subject, lines),
            _ => {}
        }
    }

    fn exception_condition(&self, exception_type: &TypeExpr) -> String {
        let name = match exception_type {
            TypeExpr::Named { name, .. } => name.as_str(),
            _ => "Any",
        };
        match name {
            "Any" | "Exception" | "BaseException" => "true".to_string(),
            "int" => "lucid_pending_exception.type == LUCID_TYPE_INT".to_string(),
            "float" => "lucid_pending_exception.type == LUCID_TYPE_FLOAT".to_string(),
            "bool" => "lucid_pending_exception.type == LUCID_TYPE_BOOL".to_string(),
            "str" => "lucid_pending_exception.type == LUCID_TYPE_STR".to_string(),
            "none" | "None" => "lucid_pending_exception.type == LUCID_TYPE_NONE".to_string(),
            _ => "true".to_string(),
        }
    }

    fn expr_is_val(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Index { .. } => true,
            Expr::Ident { name, .. } => self
                .var_types
                .get(name)
                .or_else(|| self.global_vars.get(name))
                .map(|ty| ty == "LucidVal")
                .unwrap_or(false),
            Expr::Call { func, .. } => {
                if let Expr::Ident { name, .. } = &**func {
                    self.known_fns
                        .get(name)
                        .map(|ty| ty == "LucidVal")
                        .unwrap_or(false)
                } else {
                    false
                }
            }
            Expr::Attribute { .. } => true,
            Expr::Literal {
                value: LiteralValue::Complex(_),
                ..
            } => true,
            Expr::Literal {
                value: LiteralValue::BigInt(_),
                ..
            } => true,
            _ => false,
        }
    }

    fn expr_is_complex(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Literal {
                value: LiteralValue::Complex(_),
                ..
            } => true,
            Expr::Ident { name, .. } => self.complex_names.contains(name),
            Expr::Binary { left, right, .. } => {
                self.expr_is_complex(left) || self.expr_is_complex(right)
            }
            Expr::Call { func, .. } => {
                matches!(&**func, Expr::Ident { name, .. } if name == "complex")
            }
            _ => false,
        }
    }

    fn expr_is_dynamic_value(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Ident { name, .. } => self.dynamic_vars.contains(name),
            Expr::Call { .. } | Expr::Binary { .. } => {
                self.infer_expr_type(expr, &HashMap::new()) == "LucidVal"
            }
            _ => false,
        }
    }

    fn expr_is_bigint(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Literal {
                value: LiteralValue::BigInt(_),
                ..
            } => true,
            Expr::Ident { name, .. } => self.bigint_names.contains(name),
            Expr::Binary { left, right, .. } => {
                self.expr_is_bigint(left) || self.expr_is_bigint(right)
            }
            Expr::Unary { expr, .. } => self.expr_is_bigint(expr),
            _ => false,
        }
    }

    fn emit_class_def(&mut self, name: &str, body: &[ClassMember]) -> Result<(), CodegenError> {
        self.emit_line(&format!("struct {name} {{"));
        self.indent += 1;

        let field_names = self.known_classes.get(name).cloned().unwrap_or_default();
        let mut fields = Vec::new();
        for field_name in field_names {
            let ty = self
                .known_field_types
                .get(&(name.to_string(), field_name.clone()))
                .cloned()
                .unwrap_or_else(|| "LucidVal".to_string());
            self.emit_line(&format!("{ty} {field_name};"));
            fields.push((field_name, ty));
        }
        self.indent -= 1;
        self.emit_line("};");

        // Register a class-specific deep freezer so freezing an object also
        // freezes mutable values reachable through its declared fields.
        self.emit_line(&format!("static void {name}_freeze(void* raw) {{"));
        self.indent += 1;
        self.emit_line(&format!("{name}* self = ({name}*)raw;"));
        for (field_name, ty) in &fields {
            let wrapped = match ty.as_str() {
                "LucidVal" => format!("self->{field_name}"),
                "LucidList*" => format!("lucid_wrap(self->{field_name})"),
                "LucidDict*" => format!("lucid_wrap(self->{field_name})"),
                "LucidSet*" => format!("lucid_wrap(self->{field_name})"),
                ty if ty.ends_with('*') => format!("lucid_wrap(self->{field_name})"),
                _ => continue,
            };
            self.emit_line(&format!("lucid_freeze_value({wrapped});"));
        }
        self.indent -= 1;
        self.emit_line("}");

        // Dynamic `Any` values retain class truthiness through the object tag
        // registry. Emit a prototype before the constructor stores the
        // callback; the implementation itself is emitted with the methods.
        let truthy_owner = self
            .method_owner(name, "__bool__")
            .filter(|owner| {
                self.known_method_return_types
                    .get(&(owner.clone(), "__bool__".to_string()))
                    .is_some_and(|ty| ty == "bool")
            })
            .map(|owner| (owner, true))
            .or_else(|| {
                self.method_owner(name, "__len__")
                    .filter(|owner| {
                        self.known_method_return_types
                            .get(&(owner.clone(), "__len__".to_string()))
                            .is_some_and(|ty| ty == "int64_t")
                    })
                    .map(|owner| (owner, false))
            });
        if let Some((owner, is_bool)) = &truthy_owner {
            let ret_ty = if *is_bool { "bool" } else { "int64_t" };
            self.emit_line(&format!(
                "{ret_ty} {owner}___{}({owner}* self);",
                if *is_bool { "bool__" } else { "len__" }
            ));
            if !*is_bool {
                self.emit_line(&format!(
                    "static bool {name}_truthy(void* raw) {{ return {owner}___len__(({owner}*)raw) != 0; }}"
                ));
            }
        }

        // Emit constructor
        let param_list: Vec<String> = fields
            .iter()
            .map(|(fname, fty)| format!("{fty} {fname}"))
            .collect();
        self.emit_line(&format!("{name}* {name}_new({}) {{", param_list.join(", ")));
        self.indent += 1;
        self.emit_line(&format!("{name}* self = ({name}*)malloc(sizeof({name}));"));
        let truthy_callback = truthy_owner
            .as_ref()
            .map(|(owner, is_bool)| {
                if *is_bool {
                    format!("(LucidObjectTruthy){owner}___bool__")
                } else {
                    format!("{name}_truthy")
                }
            })
            .unwrap_or_else(|| "NULL".to_string());
        self.emit_line(&format!(
            "lucid_register_object(self, \"{name}\", {name}_freeze, {truthy_callback});"
        ));
        // Dynamic `is` checks need the complete class hierarchy, not only the
        // concrete allocation name. Register each ancestor as an alias so a
        // value erased to `Any` still satisfies parent-type tests.
        let mut ancestor = self.known_parents.get(name).cloned();
        let mut seen_ancestors = HashSet::new();
        while let Some(parent) = ancestor {
            if !seen_ancestors.insert(parent.clone()) {
                break;
            }
            self.emit_line(&format!(
                "lucid_register_object(self, \"{parent}\", {parent}_freeze, NULL);"
            ));
            ancestor = self.known_parents.get(&parent).cloned();
        }
        for (fname, _) in &fields {
            self.emit_line(&format!("self->{fname} = {fname};"));
        }
        self.emit_line("return self;");
        self.indent -= 1;
        self.emit_line("}");

        // Factories are C entry points whose first Lucid parameter is implicit
        // in the native representation. construct(...) inside a factory
        // allocates this exact class via the generated constructor.
        for m in body {
            if let ClassMember::Factory(factory) = m {
                let ret_ty = format!("{name}*");
                let params: Vec<String> = factory
                    .params
                    .iter()
                    .skip(1)
                    .map(|p| {
                        format!(
                            "{} lucid_var_{}",
                            self.map_type_expr(p.type_annotation.as_ref()),
                            p.name
                        )
                    })
                    .collect();
                self.emit_line(&format!(
                    "{ret_ty} {name}_factory_{}({}) {{",
                    factory.name,
                    params.join(", ")
                ));
                self.indent += 1;
                self.in_function = true;
                self.current_fn_ret_type = Some(ret_ty);
                self.current_class = Some(name.to_string());
                for p in factory.params.iter().skip(1) {
                    self.var_types.insert(
                        p.name.clone(),
                        self.map_type_expr(p.type_annotation.as_ref()),
                    );
                }
                for statement in &factory.body {
                    self.emit_stmt(statement)?;
                }
                self.current_class = None;
                self.current_fn_ret_type = None;
                self.in_function = false;
                self.indent -= 1;
                self.emit_line("}");
            }
        }

        // Emit methods
        for m in body {
            if let ClassMember::Method(method) = m {
                if method.decorators.iter().any(|decorator| {
                    matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager")
                }) {
                    self.emit_contextmanager_method(name, method)?;
                    continue;
                }
                let ret_ty = self.map_type_expr(method.return_type.as_ref());
                let mut method_params = vec![format!("{name}* self")];
                for p in method.params.iter().skip(1) {
                    let ty = self.map_type_expr(p.type_annotation.as_ref());
                    method_params.push(format!("{ty} lucid_var_{}", p.name));
                }
                let public_name = format!("{name}_{}", method.name);
                let impl_name = if method.is_async {
                    format!("{public_name}_impl")
                } else {
                    public_name.clone()
                };
                self.emit_line(&format!(
                    "{ret_ty} {impl_name}({}) {{",
                    method_params.join(", ")
                ));
                self.indent += 1;
                self.in_function = true;
                self.current_fn_ret_type = Some(ret_ty.clone());
                self.current_fn_async = false;
                self.current_class = Some(name.to_string());
                self.var_types.insert("self".into(), format!("{name}*"));
                let mut pattern_vars = HashMap::new();
                for param in &method.params {
                    if let Some(pattern) = &param.pattern {
                        self.collect_pattern_vars(pattern, &mut pattern_vars);
                    }
                }
                for (pattern_name, pattern_ty) in &pattern_vars {
                    self.var_types
                        .insert(pattern_name.clone(), pattern_ty.clone());
                    self.emit_line(&format!(
                        "LucidVal lucid_var_{pattern_name} = lucid_none();"
                    ));
                }
                for param in &method.params {
                    if let Some(pattern) = &param.pattern {
                        self.emit_pattern_bindings(
                            pattern,
                            &format!("lucid_wrap(lucid_var_{})", param.name),
                        );
                    }
                }
                for s in &method.body {
                    self.emit_stmt(s)?;
                }
                self.current_fn_ret_type = None;
                self.current_fn_async = false;
                self.in_function = false;
                self.current_class = None;
                self.indent -= 1;
                self.emit_line("}");
                if method.is_async {
                    let thunk_name = format!("{public_name}_thunk");
                    self.emit_line(&format!("static LucidVal {thunk_name}(LucidList* args) {{"));
                    self.indent += 1;
                    let mut call_args = vec![format!("({name}*)lucid_as_ptr(args->items[0])")];
                    for (index, param) in method.params.iter().skip(1).enumerate() {
                        let ty = self.map_type_expr(param.type_annotation.as_ref());
                        call_args.push(self.future_arg_expr(&ty, index + 1));
                    }
                    self.emit_line(&format!(
                        "return lucid_wrap({impl_name}({}));",
                        call_args.join(", ")
                    ));
                    self.indent -= 1;
                    self.emit_line("}");
                    self.emit_line(&format!(
                        "LucidVal {public_name}({}) {{",
                        method_params.join(", ")
                    ));
                    self.indent += 1;
                    self.emit_line(&format!(
                        "LucidList* _args = lucid_list_new({});",
                        method_params.len()
                    ));
                    self.emit_line("lucid_list_append(_args, lucid_ptr_val(self));");
                    for param in method.params.iter().skip(1) {
                        self.emit_line(&format!(
                            "lucid_list_append(_args, lucid_wrap(lucid_var_{}));",
                            param.name
                        ));
                    }
                    self.emit_line(&format!("return lucid_future({thunk_name}, _args);"));
                    self.indent -= 1;
                    self.emit_line("}");
                }
            }
            if let ClassMember::Getter(getter) = m {
                let ret_ty = self.map_type_expr(getter.return_type.as_ref());
                self.emit_line(&format!(
                    "{ret_ty} {name}_{}_get({name}* self) {{",
                    getter.name
                ));
                self.indent += 1;
                self.in_function = true;
                self.current_fn_ret_type = Some(ret_ty.clone());
                self.var_types.insert("self".into(), format!("{name}*"));
                for statement in &getter.body {
                    self.emit_stmt(statement)?;
                }
                self.current_fn_ret_type = None;
                self.in_function = false;
                self.indent -= 1;
                self.emit_line("}");
            }
            if let ClassMember::ClassMethod(method) = m {
                if method.decorators.iter().any(|decorator| {
                    matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager")
                }) {
                    self.emit_contextmanager_classmethod(name, method)?;
                    continue;
                }
                let ret_ty = self.map_type_expr(method.return_type.as_ref());
                let mut params = Vec::new();
                for param in method.params.iter().skip(1) {
                    params.push(format!(
                        "{} lucid_var_{}",
                        self.map_type_expr(param.type_annotation.as_ref()),
                        param.name
                    ));
                }
                let public_name = format!("{name}_{}", method.name);
                let impl_name = if method.is_async {
                    format!("{public_name}_impl")
                } else {
                    public_name.clone()
                };
                self.emit_line(&format!("{ret_ty} {impl_name}({}) {{", params.join(", ")));
                self.indent += 1;
                self.in_function = true;
                self.current_fn_ret_type = Some(ret_ty.clone());
                self.current_fn_async = false;
                self.var_types.clear();
                self.current_class = Some(name.to_string());
                self.var_types.insert("cls".into(), format!("{name}*"));
                for param in method.params.iter().skip(1) {
                    self.record_list_element_class(
                        param.name.as_str(),
                        None,
                        param.type_annotation.as_ref(),
                    );
                    self.var_types.insert(
                        param.name.clone(),
                        self.map_type_expr(param.type_annotation.as_ref()),
                    );
                }
                let mut pattern_vars = HashMap::new();
                for param in &method.params {
                    if let Some(pattern) = &param.pattern {
                        self.collect_pattern_vars(pattern, &mut pattern_vars);
                    }
                }
                for pattern_name in pattern_vars.keys() {
                    self.var_types
                        .insert(pattern_name.clone(), "LucidVal".into());
                    self.emit_line(&format!(
                        "LucidVal lucid_var_{pattern_name} = lucid_none();"
                    ));
                }
                for param in &method.params {
                    if let Some(pattern) = &param.pattern {
                        self.emit_pattern_bindings(
                            pattern,
                            &format!("lucid_wrap(lucid_var_{})", param.name),
                        );
                    }
                }
                for statement in &method.body {
                    self.emit_stmt(statement)?;
                }
                self.current_fn_ret_type = None;
                self.current_fn_async = false;
                self.in_function = false;
                self.current_class = None;
                self.indent -= 1;
                self.emit_line("}");
                if method.is_async {
                    let thunk_name = format!("{public_name}_thunk");
                    self.emit_line(&format!("static LucidVal {thunk_name}(LucidList* args) {{"));
                    self.indent += 1;
                    let mut call_args = Vec::new();
                    for (index, param) in method.params.iter().skip(1).enumerate() {
                        let ty = self.map_type_expr(param.type_annotation.as_ref());
                        call_args.push(self.future_arg_expr(&ty, index));
                    }
                    self.emit_line(&format!(
                        "return lucid_wrap({impl_name}({}));",
                        call_args.join(", ")
                    ));
                    self.indent -= 1;
                    self.emit_line("}");
                    self.emit_line(&format!("LucidVal {public_name}({}) {{", params.join(", ")));
                    self.indent += 1;
                    self.emit_line(&format!(
                        "LucidList* _args = lucid_list_new({});",
                        params.len()
                    ));
                    for param in method.params.iter().skip(1) {
                        self.emit_line(&format!(
                            "lucid_list_append(_args, lucid_wrap(lucid_var_{}));",
                            param.name
                        ));
                    }
                    self.emit_line(&format!("return lucid_future({thunk_name}, _args);"));
                    self.indent -= 1;
                    self.emit_line("}");
                }
            }
            if let ClassMember::Setter(setter) = m {
                let value_type = self.map_type_expr(setter.param.type_annotation.as_ref());
                self.emit_line(&format!(
                    "void {name}_{}_set({name}* self, {value_type} lucid_var_{}) {{",
                    setter.name, setter.param.name
                ));
                self.indent += 1;
                self.in_function = true;
                self.in_setter = true;
                self.current_fn_ret_type = Some("void".to_string());
                self.var_types.insert("self".into(), format!("{name}*"));
                self.var_types.insert(setter.param.name.clone(), value_type);
                for statement in &setter.body {
                    self.emit_stmt(statement)?;
                }
                self.current_fn_ret_type = None;
                self.in_function = false;
                self.in_setter = false;
                self.indent -= 1;
                self.emit_line("}");
            }
        }

        self.emit_line("");
        Ok(())
    }

    fn emit_contextmanager_method(
        &mut self,
        class_name: &str,
        method: &FunctionDef,
    ) -> Result<(), CodegenError> {
        let public_name = format!("{class_name}_{}", method.name);
        let params = {
            let mut parts = vec![format!("{class_name}* self")];
            for param in method.params.iter().skip(1) {
                parts.push(format!(
                    "{} lucid_var_{}",
                    self.map_type_expr(param.type_annotation.as_ref()),
                    param.name
                ));
            }
            parts.join(", ")
        };
        let yield_index = method
            .body
            .iter()
            .position(|statement| matches!(statement, Stmt::Yield { .. }))
            .ok_or_else(|| CodegenError {
                message: format!("contextmanager '{}' must yield", method.name),
            })?;
        let mut local_vars = HashMap::new();
        for statement in &method.body {
            self.collect_vars_from_stmt(statement, &mut local_vars);
        }
        for param in method.params.iter().skip(1) {
            local_vars.remove(&param.name);
        }
        let mut local_names: Vec<String> = local_vars.keys().cloned().collect();
        local_names.sort();
        let teardown_name = format!("{public_name}_teardown");
        self.emit_line(&format!("static void {teardown_name}(LucidList* args) {{"));
        self.indent += 1;
        self.in_function = true;
        self.current_fn_ret_type = Some("void".into());
        self.var_types.clear();
        self.var_types
            .insert("self".into(), format!("{class_name}*"));
        self.emit_line(&format!(
            "{class_name}* self = ({class_name}*)lucid_as_ptr(args->items[0]);"
        ));
        for (index, param) in method.params.iter().skip(1).enumerate() {
            let ty = self.map_type_expr(param.type_annotation.as_ref());
            self.var_types.insert(param.name.clone(), ty.clone());
            if matches!(param.type_annotation, Some(TypeExpr::Named { ref name, .. }) if name == "complex")
            {
                self.complex_names.insert(param.name.clone());
            }
            self.emit_line(&format!(
                "{ty} lucid_var_{} = {};",
                param.name,
                self.future_arg_expr(&ty, index + 1)
            ));
        }
        for (offset, name) in local_names.iter().enumerate() {
            let Some(ty) = local_vars.get(name) else {
                return Err(CodegenError {
                    message: format!("missing inferred type for local '{name}'"),
                });
            };
            self.var_types.insert(name.clone(), ty.clone());
            self.emit_line(&format!(
                "{ty} lucid_var_{name} = {};",
                self.future_arg_expr(ty, method.params.len() + offset)
            ));
        }
        for statement in &method.body[yield_index + 1..] {
            self.emit_stmt(statement)?;
        }
        self.emit_line("return;");
        self.indent -= 1;
        self.emit_line("}");
        self.current_fn_ret_type = None;
        self.in_function = false;

        self.emit_line(&format!("LucidVal {public_name}({params}) {{"));
        self.indent += 1;
        self.in_function = true;
        self.current_fn_ret_type = Some("LucidVal".into());
        self.var_types.clear();
        self.var_types
            .insert("self".into(), format!("{class_name}*"));
        for param in method.params.iter().skip(1) {
            self.var_types.insert(
                param.name.clone(),
                self.map_type_expr(param.type_annotation.as_ref()),
            );
        }
        for name in &local_names {
            let Some(ty) = local_vars.get(name) else {
                return Err(CodegenError {
                    message: format!("missing inferred type for local '{name}'"),
                });
            };
            self.var_types.insert(name.clone(), ty.clone());
            let init = match ty.as_str() {
                "int64_t" => "0",
                "double" => "0.0",
                "bool" => "false",
                "const char*" => "\"\"",
                "LucidList*" => "NULL",
                "LucidVal" => "lucid_none()",
                _ => "NULL",
            };
            self.emit_line(&format!("{ty} lucid_var_{name} = {init};"));
        }
        for statement in &method.body[..yield_index] {
            self.emit_stmt(statement)?;
        }
        let yield_value = match &method.body[yield_index] {
            Stmt::Yield { value, .. } => self.emit_expr(value)?,
            _ => {
                return Err(CodegenError {
                    message: "contextmanager yield location is invalid".to_string(),
                });
            }
        };
        self.emit_line(&format!(
            "LucidList* _args = lucid_list_new({});",
            method.params.len() + local_names.len()
        ));
        self.emit_line("lucid_list_append(_args, lucid_ptr_val(self));");
        for param in method.params.iter().skip(1) {
            self.emit_line(&format!(
                "lucid_list_append(_args, lucid_wrap(lucid_var_{}));",
                param.name
            ));
        }
        for name in &local_names {
            self.emit_line(&format!(
                "lucid_list_append(_args, lucid_wrap(lucid_var_{name}));"
            ));
        }
        self.emit_line(&format!(
            "return lucid_context(lucid_wrap({yield_value}), {teardown_name}, NULL, _args);"
        ));
        self.indent -= 1;
        self.emit_line("}");
        self.current_fn_ret_type = None;
        self.in_function = false;
        self.var_types.clear();
        Ok(())
    }

    fn emit_contextmanager_classmethod(
        &mut self,
        class_name: &str,
        method: &FunctionDef,
    ) -> Result<(), CodegenError> {
        let public_name = format!("{class_name}_{}", method.name);
        let params: Vec<&Param> = method.params.iter().skip(1).collect();
        let signature = params
            .iter()
            .map(|param| {
                format!(
                    "{} lucid_var_{}",
                    self.map_type_expr(param.type_annotation.as_ref()),
                    param.name
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let yield_index = method
            .body
            .iter()
            .position(|statement| matches!(statement, Stmt::Yield { .. }))
            .ok_or_else(|| CodegenError {
                message: format!("contextmanager '{}' must yield", method.name),
            })?;
        let teardown_name = format!("{public_name}_teardown");
        self.emit_line(&format!("static void {teardown_name}(LucidList* args) {{"));
        self.indent += 1;
        self.in_function = true;
        self.current_fn_ret_type = Some("void".into());
        self.var_types.clear();
        self.emit_line(&format!("{class_name}* cls = NULL;"));
        for (index, param) in params.iter().enumerate() {
            let ty = self.map_type_expr(param.type_annotation.as_ref());
            self.var_types.insert(param.name.clone(), ty.clone());
            if matches!(param.type_annotation, Some(TypeExpr::Named { ref name, .. }) if name == "complex")
            {
                self.complex_names.insert(param.name.clone());
            }
            self.emit_line(&format!(
                "{ty} lucid_var_{} = {};",
                param.name,
                self.future_arg_expr(&ty, index)
            ));
        }
        for statement in &method.body[yield_index + 1..] {
            self.emit_stmt(statement)?;
        }
        self.emit_line("return;");
        self.indent -= 1;
        self.emit_line("}");
        self.current_fn_ret_type = None;
        self.in_function = false;
        self.emit_line(&format!("LucidVal {public_name}({signature}) {{"));
        self.indent += 1;
        self.in_function = true;
        self.current_fn_ret_type = Some("LucidVal".into());
        self.var_types.clear();
        self.emit_line(&format!("{class_name}* cls = NULL;"));
        for param in &params {
            self.var_types.insert(
                param.name.clone(),
                self.map_type_expr(param.type_annotation.as_ref()),
            );
        }
        for statement in &method.body[..yield_index] {
            self.emit_stmt(statement)?;
        }
        let yield_value = match &method.body[yield_index] {
            Stmt::Yield { value, .. } => self.emit_expr(value)?,
            _ => {
                return Err(CodegenError {
                    message: "contextmanager yield location is invalid".to_string(),
                });
            }
        };
        self.emit_line(&format!(
            "LucidList* _args = lucid_list_new({});",
            params.len()
        ));
        for param in &params {
            self.emit_line(&format!(
                "lucid_list_append(_args, lucid_wrap(lucid_var_{}));",
                param.name
            ));
        }
        self.emit_line(&format!(
            "return lucid_context(lucid_wrap({yield_value}), {teardown_name}, NULL, _args);"
        ));
        self.indent -= 1;
        self.emit_line("}");
        self.current_fn_ret_type = None;
        self.in_function = false;
        self.var_types.clear();
        Ok(())
    }

    fn emit_function(&mut self, f: &FunctionDef) -> Result<(), CodegenError> {
        if f.decorators.iter().any(
            |decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager"),
        ) {
            return self.emit_contextmanager_function(f);
        }
        let ret_ty = self.map_type_expr(f.return_type.as_ref());
        let params = self.emit_param_list(&f.params);
        let fn_name = self.dispatch_name(f, &f.name);
        let impl_name = if f.is_async {
            format!("{fn_name}_impl")
        } else {
            fn_name.clone()
        };
        self.emit_line(&format!("{ret_ty} {impl_name}({}) {{", params));
        self.indent += 1;
        self.in_function = true;
        self.current_fn_ret_type = Some(ret_ty.clone());
        self.current_fn_async = false;
        self.var_types.clear();
        self.deleted_bindings.clear();

        // Register params
        for p in &f.params {
            let ty = if p.is_variadic_keyword {
                "LucidDict*".into()
            } else if p.is_variadic_positional {
                "LucidList*".into()
            } else {
                self.map_type_expr(p.type_annotation.as_ref())
            };
            self.var_types.insert(p.name.clone(), ty);
            self.emit_line(&format!("bool lucid_alive_{} = true;", p.name));
            self.alive_declarations.insert(p.name.clone());
            self.record_list_element_class(p.name.as_str(), None, p.type_annotation.as_ref());
            if matches!(p.type_annotation, Some(TypeExpr::Named { ref name, .. }) if name == "complex")
            {
                self.complex_names.insert(p.name.clone());
            }
        }

        // Collect all local variables
        let mut local_vars = HashMap::new();
        for p in &f.params {
            if let Some(pattern) = &p.pattern {
                self.collect_pattern_vars(pattern, &mut local_vars);
            }
        }
        for s in &f.body {
            self.collect_vars_from_stmt(s, &mut local_vars);
        }

        // Remove params from local_vars
        for p in &f.params {
            local_vars.remove(&p.name);
        }

        // Pre-declare locals
        for (name, ty) in &local_vars {
            self.var_types.insert(name.clone(), ty.clone());
            let init_val = match ty.as_str() {
                "int64_t" => "0",
                "double" => "0.0",
                "bool" => "false",
                "const char*" => "\"\"",
                "LucidList*" => "NULL",
                "LucidVal" => "lucid_none()",
                _ => "NULL",
            };
            self.emit_line(&format!("{ty} lucid_var_{name} = {init_val};"));
            self.emit_line(&format!("bool lucid_alive_{name} = false;"));
            self.alive_declarations.insert(name.clone());
        }

        for p in &f.params {
            if let Some(pattern) = &p.pattern {
                self.emit_pattern_bindings(pattern, &format!("lucid_wrap(lucid_var_{})", p.name));
            }
        }

        for s in &f.body {
            self.emit_stmt(s)?;
        }

        self.in_function = false;
        self.current_fn_ret_type = None;
        self.current_fn_async = false;
        self.indent -= 1;
        self.emit_line("}\n");

        if f.is_async {
            let thunk_name = format!("{fn_name}_thunk");
            self.emit_line(&format!("static LucidVal {thunk_name}(LucidList* args) {{"));
            self.indent += 1;
            let mut call_args = Vec::new();
            for (index, param) in f.params.iter().enumerate() {
                let ty = if param.is_variadic_keyword {
                    "LucidDict*".into()
                } else if param.is_variadic_positional {
                    "LucidList*".into()
                } else {
                    self.map_type_expr(param.type_annotation.as_ref())
                };
                let arg = match ty.as_str() {
                    "int64_t" => format!("lucid_as_int(args->items[{index}])"),
                    "double" => format!("lucid_as_float(args->items[{index}])"),
                    "bool" => format!("lucid_as_bool(args->items[{index}])"),
                    "const char*" => format!("lucid_as_str(args->items[{index}])"),
                    "LucidList*" => format!("lucid_as_list(args->items[{index}])"),
                    "LucidVal" => format!("args->items[{index}]"),
                    other => format!("({other})lucid_as_ptr(args->items[{index}])"),
                };
                call_args.push(arg);
            }
            let call = format!("{impl_name}({})", call_args.join(", "));
            if ret_ty == "void" {
                self.emit_line(&format!("{call};"));
                self.emit_line("return lucid_none();");
            } else {
                self.emit_line(&format!("return lucid_wrap({call});"));
            }
            self.indent -= 1;
            self.emit_line("}");

            self.emit_line(&format!("LucidVal {fn_name}({params}) {{"));
            self.indent += 1;
            self.emit_line(&format!(
                "LucidList* _args = lucid_list_new({});",
                f.params.len()
            ));
            for param in &f.params {
                self.emit_line(&format!(
                    "lucid_list_append(_args, lucid_wrap(lucid_var_{}));",
                    param.name
                ));
            }
            self.emit_line(&format!("return lucid_future({thunk_name}, _args);"));
            self.indent -= 1;
            self.emit_line("}");
        }
        Ok(())
    }

    fn emit_contextmanager_function(&mut self, f: &FunctionDef) -> Result<(), CodegenError> {
        if !f
            .body
            .iter()
            .any(|statement| matches!(statement, Stmt::Yield { .. }))
        {
            if let Some((try_index, try_stmt)) =
                f.body
                    .iter()
                    .enumerate()
                    .find_map(|(index, statement)| match statement {
                        Stmt::Try {
                            body,
                            handlers,
                            finally_body,
                            ..
                        } if body
                            .iter()
                            .any(|nested| matches!(nested, Stmt::Yield { .. })) =>
                        {
                            if !handlers.is_empty() {
                                let mut errors = Vec::new();
                                for handler in handlers {
                                    errors.extend_from_slice(&handler.body);
                                }
                                self.context_error_bodies.insert(f.name.clone(), errors);
                            }
                            Some((index, (body, finally_body)))
                        }
                        _ => None,
                    })
            {
                let (try_body, finally_body) = try_stmt;
                let mut flattened = f.body[..try_index].to_vec();
                flattened.extend_from_slice(try_body);
                if let Some(finally_body) = finally_body {
                    flattened.extend_from_slice(finally_body);
                }
                let mut flattened_function = f.clone();
                flattened_function.body = flattened;
                return self.emit_contextmanager_function(&flattened_function);
            }
        }
        let fn_name = self.dispatch_name(f, &f.name);
        let params = self.emit_param_list(&f.params);
        let yield_index = f
            .body
            .iter()
            .position(|statement| matches!(statement, Stmt::Yield { .. }))
            .ok_or_else(|| CodegenError {
                message: format!("contextmanager '{}' must yield", f.name),
            })?;
        let mut local_vars = HashMap::new();
        for statement in &f.body {
            self.collect_vars_from_stmt(statement, &mut local_vars);
        }
        for param in &f.params {
            local_vars.remove(&param.name);
        }
        let mut local_names: Vec<String> = local_vars.keys().cloned().collect();
        local_names.sort();
        let teardown_name = format!("{fn_name}_teardown");
        self.emit_line(&format!("static void {teardown_name}(LucidList* args) {{"));
        self.indent += 1;
        self.in_function = true;
        self.current_fn_ret_type = Some("void".into());
        self.var_types.clear();
        self.alive_declarations.clear();
        for (index, param) in f.params.iter().enumerate() {
            let ty = self.map_type_expr(param.type_annotation.as_ref());
            self.var_types.insert(param.name.clone(), ty.clone());
            self.emit_line(&format!("bool lucid_alive_{} = true;", param.name));
            self.alive_declarations.insert(param.name.clone());
            if matches!(param.type_annotation, Some(TypeExpr::Named { ref name, .. }) if name == "complex")
            {
                self.complex_names.insert(param.name.clone());
            }
            self.emit_line(&format!(
                "{ty} lucid_var_{} = {};",
                param.name,
                self.future_arg_expr(&ty, index)
            ));
        }
        for (offset, name) in local_names.iter().enumerate() {
            let Some(ty) = local_vars.get(name) else {
                return Err(CodegenError {
                    message: format!("missing inferred type for local '{name}'"),
                });
            };
            self.var_types.insert(name.clone(), ty.clone());
            let init = match ty.as_str() {
                "int64_t" => "0",
                "double" => "0.0",
                "bool" => "false",
                "const char*" => "\"\"",
                "LucidList*" => "NULL",
                "LucidVal" => "lucid_none()",
                _ => "NULL",
            };
            self.emit_line(&format!("{ty} lucid_var_{name} = {};", init));
            self.emit_line(&format!("bool lucid_alive_{name} = false;"));
            self.alive_declarations.insert(name.clone());
            self.emit_line(&format!(
                "lucid_var_{name} = {};",
                self.future_arg_expr(ty, f.params.len() + offset)
            ));
        }
        for statement in &f.body[yield_index + 1..] {
            self.emit_stmt(statement)?;
        }
        if let Some(error_body) = self.context_error_bodies.get(&f.name).cloned() {
            self.emit_line("if (lucid_context_failed) {");
            self.indent += 1;
            for statement in &error_body {
                self.emit_stmt(statement)?;
            }
            self.indent -= 1;
            self.emit_line("}");
        }
        self.emit_line("return;");
        self.indent -= 1;
        self.emit_line("}");
        self.current_fn_ret_type = None;
        self.in_function = false;

        self.emit_line(&format!("LucidVal {fn_name}({params}) {{"));
        self.indent += 1;
        self.in_function = true;
        self.current_fn_ret_type = Some("LucidVal".into());
        self.var_types.clear();
        self.alive_declarations.clear();
        for param in &f.params {
            self.var_types.insert(
                param.name.clone(),
                self.map_type_expr(param.type_annotation.as_ref()),
            );
            self.emit_line(&format!("bool lucid_alive_{} = true;", param.name));
            self.alive_declarations.insert(param.name.clone());
        }
        for name in &local_names {
            let Some(ty) = local_vars.get(name) else {
                return Err(CodegenError {
                    message: format!("missing inferred type for local '{name}'"),
                });
            };
            self.var_types.insert(name.clone(), ty.clone());
            let init = match ty.as_str() {
                "int64_t" => "0",
                "double" => "0.0",
                "bool" => "false",
                "const char*" => "\"\"",
                "LucidList*" => "NULL",
                "LucidVal" => "lucid_none()",
                _ => "NULL",
            };
            self.emit_line(&format!("{ty} lucid_var_{name} = {};", init));
            self.emit_line(&format!("bool lucid_alive_{name} = false;"));
            self.alive_declarations.insert(name.clone());
        }
        for statement in &f.body[..yield_index] {
            self.emit_stmt(statement)?;
        }
        let yield_value = match &f.body[yield_index] {
            Stmt::Yield { value, .. } => self.emit_expr(value)?,
            _ => {
                return Err(CodegenError {
                    message: "contextmanager yield location is invalid".to_string(),
                });
            }
        };
        self.emit_line(&format!(
            "LucidList* _args = lucid_list_new({});",
            f.params.len() + local_names.len()
        ));
        for param in &f.params {
            self.emit_line(&format!(
                "lucid_list_append(_args, lucid_wrap(lucid_var_{}));",
                param.name
            ));
        }
        for name in &local_names {
            self.emit_line(&format!(
                "lucid_list_append(_args, lucid_wrap(lucid_var_{name}));"
            ));
        }
        self.emit_line(&format!(
            "return lucid_context(lucid_wrap({yield_value}), {teardown_name}, NULL, _args);"
        ));
        self.indent -= 1;
        self.emit_line("}");
        self.current_fn_ret_type = None;
        self.in_function = false;
        self.var_types.clear();
        Ok(())
    }

    fn emit_stmt(&mut self, stmt: &Stmt) -> Result<(), CodegenError> {
        match stmt {
            Stmt::VarDef { pattern, value, .. } => {
                if let Pattern::Ident(name, _) = pattern {
                    self.deleted_bindings.remove(name);
                    if let Some(Expr::Attribute {
                        value: object,
                        attr,
                        ..
                    }) = value
                    {
                        if matches!(&**object, Expr::Ident { name: base, .. } if base == "str")
                            && matches!(attr.as_str(), "bin" | "oct" | "hex")
                        {
                            self.function_aliases
                                .insert(name.clone(), format!("__str_base_{attr}"));
                            return Ok(());
                        }
                    }
                    self.ensure_alive_binding(name);
                    if self.current_fn_ret_type.as_deref() != Some("void") {
                        self.emit_line(&format!("lucid_alive_{name} = true;"));
                    }
                    if let Some(Expr::AnonymousDef { params, body, .. }) = value {
                        if let [
                            Stmt::Return {
                                value: Some(body_expr),
                                ..
                            },
                        ] = body.as_slice()
                        {
                            let parameter_specs = params
                                .iter()
                                .map(|param| {
                                    (
                                        param.name.clone(),
                                        self.map_type_expr(param.type_annotation.as_ref()),
                                    )
                                })
                                .collect();
                            self.anonymous_bindings
                                .insert(name.clone(), (parameter_specs, body_expr.clone()));
                            return Ok(());
                        }
                    }
                    if let Some(Expr::Ident { name: source, .. }) = value {
                        let resolved = self
                            .function_aliases
                            .get(source)
                            .cloned()
                            .unwrap_or_else(|| source.clone());
                        if self.known_fn_params.contains_key(&resolved) {
                            self.function_aliases.insert(name.clone(), resolved);
                            return Ok(());
                        }
                    }
                    if let Some(Expr::Call {
                        func: inner_func,
                        args: inner_args,
                        ..
                    }) = value
                    {
                        if let Expr::Ident { name: source, .. } = &**inner_func {
                            let resolved = self
                                .function_aliases
                                .get(source)
                                .cloned()
                                .unwrap_or_else(|| source.clone());
                            if self.known_fn_params.contains_key(&resolved)
                                && inner_args.iter().any(|arg| matches!(&arg.value, Expr::Ident { name, .. } if name == "_"))
                            {
                                self.partial_bindings.insert(name.clone(), (resolved, inner_args.clone()));
                                return Ok(());
                            }
                        }
                    }
                    if let Some(val_expr) = value {
                        let previous = self.capture_assignment.replace(name.clone());
                        let val_code = self.emit_expr(val_expr)?;
                        self.capture_assignment = previous;
                        self.emit_line(&format!("lucid_var_{name} = {val_code};"));
                    }
                } else if let Some(val_expr) = value {
                    let val_code = self.emit_expr(val_expr)?;
                    let subject = self.new_temp();
                    self.emit_line(&format!("LucidVal {subject} = lucid_wrap({val_code});"));
                    self.emit_pattern_bindings(pattern, &subject);
                }
                Ok(())
            }
            Stmt::Assignment { target, value, .. } => {
                match target {
                    Expr::Ident { name, .. } => {
                        self.deleted_bindings.remove(name);
                        self.ensure_alive_binding(name);
                        if self.current_fn_ret_type.as_deref() != Some("void") {
                            self.emit_line(&format!("lucid_alive_{name} = true;"));
                        }
                        if let Expr::Attribute {
                            value: object,
                            attr,
                            ..
                        } = value
                        {
                            if matches!(&**object, Expr::Ident { name: base, .. } if base == "str")
                                && matches!(attr.as_str(), "bin" | "oct" | "hex")
                            {
                                self.function_aliases
                                    .insert(name.clone(), format!("__str_base_{attr}"));
                                return Ok(());
                            }
                        }
                        if let Expr::AnonymousDef { params, body, .. } = value {
                            if let [
                                Stmt::Return {
                                    value: Some(body_expr),
                                    ..
                                },
                            ] = body.as_slice()
                            {
                                let parameter_specs = params
                                    .iter()
                                    .map(|param| {
                                        (
                                            param.name.clone(),
                                            self.map_type_expr(param.type_annotation.as_ref()),
                                        )
                                    })
                                    .collect();
                                self.anonymous_bindings
                                    .insert(name.clone(), (parameter_specs, body_expr.clone()));
                                return Ok(());
                            }
                        }
                        if let Expr::Ident { name: source, .. } = value {
                            let resolved = self
                                .function_aliases
                                .get(source)
                                .cloned()
                                .unwrap_or_else(|| source.clone());
                            if self.known_fn_params.contains_key(&resolved) {
                                self.function_aliases.insert(name.clone(), resolved);
                                return Ok(());
                            }
                        }
                        if let Expr::Call {
                            func: inner_func,
                            args: inner_args,
                            ..
                        } = value
                        {
                            if let Expr::Ident { name: source, .. } = &**inner_func {
                                let resolved = self
                                    .function_aliases
                                    .get(source)
                                    .cloned()
                                    .unwrap_or_else(|| source.clone());
                                if self.known_fn_params.contains_key(&resolved)
                                    && inner_args.iter().any(|arg| matches!(&arg.value, Expr::Ident { name, .. } if name == "_"))
                                {
                                    self.partial_bindings.insert(name.clone(), (resolved, inner_args.clone()));
                                    return Ok(());
                                }
                            }
                        }
                        let previous = self.capture_assignment.replace(name.clone());
                        let val_code = self.emit_expr(value)?;
                        self.capture_assignment = previous;
                        let var_ty = self
                            .var_types
                            .get(name)
                            .or_else(|| self.global_vars.get(name))
                            .cloned()
                            .unwrap_or_else(|| "LucidVal".to_string());
                        if var_ty == "LucidVal" {
                            self.emit_line(&format!("lucid_var_{name} = lucid_wrap({val_code});"));
                        } else if var_ty == "int64_t" && self.expr_is_val(value) {
                            self.emit_line(&format!(
                                "lucid_var_{name} = lucid_int_val({val_code});"
                            ));
                        } else if var_ty == "double" && self.expr_is_val(value) {
                            self.emit_line(&format!("lucid_var_{name} = lucid_num({val_code});"));
                        } else if var_ty == "const char*" && self.expr_is_val(value) {
                            self.emit_line(&format!(
                                "lucid_var_{name} = lucid_as_str({val_code});"
                            ));
                        } else if var_ty == "LucidList*" && self.expr_is_val(value) {
                            self.emit_line(&format!(
                                "lucid_var_{name} = lucid_as_list({val_code});"
                            ));
                        } else {
                            self.emit_line(&format!("lucid_var_{name} = {val_code};"));
                        }
                    }
                    Expr::Index {
                        value: arr_expr,
                        index,
                        ..
                    } => {
                        let idx_code = self.emit_expr(index)?;
                        let val_code = self.emit_expr(value)?;
                        let receiver_type = self.infer_expr_type(arr_expr, &HashMap::new());
                        let receiver_class = receiver_type.trim_end_matches('*').to_string();
                        if let Some(owner) = self.method_owner(&receiver_class, "__setitem__") {
                            let arr_code = self.emit_expr(arr_expr)?;
                            self.emit_line(&format!("{owner}___setitem__(({owner}*)({arr_code}), {idx_code}, {val_code});"));
                            return Ok(());
                        }
                        if receiver_type == "LucidVal" {
                            let arr_code = self.emit_expr(arr_expr)?;
                            self.emit_line(&format!(
                                "lucid_set_index_value(lucid_wrap({arr_code}), lucid_wrap({idx_code}), lucid_wrap({val_code}));"
                            ));
                            return Ok(());
                        }
                        // Check for 2D index assignment: a[i][j] = val
                        if let Expr::Index {
                            value: inner_arr,
                            index: inner_idx,
                            ..
                        } = &**arr_expr
                        {
                            let inner_code = self.emit_expr(inner_arr)?;
                            let inner_idx_code = self.emit_expr(inner_idx)?;
                            self.emit_line(&format!("lucid_list_set(lucid_as_list(lucid_get_index(lucid_wrap({inner_code}), lucid_int_val({inner_idx_code}))), lucid_int_val({idx_code}), lucid_wrap({val_code}));"));
                        } else {
                            let arr_code = self.emit_expr(arr_expr)?;
                            self.emit_line(&format!("lucid_list_set(lucid_as_list(lucid_wrap({arr_code})), lucid_int_val({idx_code}), lucid_wrap({val_code}));"));
                        }
                    }
                    Expr::Attribute {
                        value: obj_expr,
                        attr,
                        ..
                    } => {
                        if let Expr::Ident { name: receiver, .. } = &**obj_expr {
                            let class_name = if self.known_classes.contains_key(receiver) {
                                Some(receiver.clone())
                            } else if receiver == "cls" {
                                self.current_class.clone()
                            } else if receiver == "self" {
                                self.current_class.clone()
                            } else {
                                None
                            };
                            if let Some(class_name) = class_name {
                                let mut owner = Some(class_name.clone());
                                while let Some(candidate) = owner.clone() {
                                    if self
                                        .known_class_vars
                                        .contains_key(&(candidate.clone(), attr.clone()))
                                    {
                                        let val_code = self.emit_expr(value)?;
                                        self.emit_line(&format!(
                                            "lucid_classvar_{candidate}_{attr} = {val_code};"
                                        ));
                                        return Ok(());
                                    }
                                    owner = self.known_parents.get(&candidate).cloned();
                                }
                            }
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let val_code = self.emit_expr(value)?;
                        self.emit_line(&format!("if (lucid_object_frozen((void*)lucid_as_ptr(lucid_wrap({obj_code})))) {{ fprintf(stderr, \"cannot mutate frozen object\\n\"); exit(1); }}"));
                        let class_name = self
                            .infer_expr_type(obj_expr, &HashMap::new())
                            .trim_end_matches('*')
                            .to_string();
                        let mut setter_owner = Some(class_name.clone());
                        while let Some(owner) = setter_owner.clone() {
                            if !self.in_setter
                                && self
                                    .known_setters
                                    .contains_key(&(owner.clone(), attr.clone()))
                            {
                                let receiver = if owner != class_name {
                                    format!("({owner}*)({obj_code})")
                                } else {
                                    obj_code.clone()
                                };
                                self.emit_line(&format!(
                                    "{owner}_{attr}_set({receiver}, {val_code});"
                                ));
                                return Ok(());
                            }
                            setter_owner = self.known_parents.get(&owner).cloned();
                        }
                        if !self.in_setter {
                            self.emit_line(&format!("{obj_code}->{attr} = {val_code};"));
                        } else {
                            self.emit_line(&format!("{obj_code}->{attr} = {val_code};"));
                        }
                    }
                    Expr::Record { fields, .. } => {
                        let val_code = self.emit_expr(value)?;
                        let source = self.new_temp();
                        self.emit_line(&format!("LucidVal {source} = lucid_wrap({val_code});"));
                        let star = fields.iter().position(|(_, expr)| {
                            matches!(
                                expr,
                                Expr::Unary {
                                    op: UnaryOp::Spread,
                                    ..
                                }
                            )
                        });
                        let tail_count = star.map(|index| fields.len() - index - 1).unwrap_or(0);
                        for (index, (_, target_expr)) in fields.iter().enumerate() {
                            if let Expr::Ident { name, .. } = target_expr {
                                let element = if let Some(star_index) = star {
                                    if index > star_index {
                                        format!(
                                            "lucid_list_get(lucid_as_list({source}), (int64_t)lucid_as_list({source})->len - {})",
                                            tail_count - (index - star_index - 1)
                                        )
                                    } else {
                                        format!("lucid_list_get(lucid_as_list({source}), {index})")
                                    }
                                } else {
                                    format!("lucid_list_get(lucid_as_list({source}), {index})")
                                };
                                self.emit_line(&format!("lucid_var_{name} = {element};"));
                            } else if Some(index) == star {
                                if let Expr::Unary { expr: nested, .. } = target_expr {
                                    if let Expr::Ident { name, .. } = &**nested {
                                        self.emit_line(&format!("lucid_var_{name} = lucid_wrap(lucid_list_slice(lucid_as_list({source}), {index}, (int64_t)lucid_as_list({source})->len - {tail_count}, 1));"));
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            Stmt::AugAssign {
                target, op, value, ..
            } => {
                let op_str = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    _ => "+",
                };
                match target {
                    Expr::Ident { name, .. } => {
                        let val_code = self.emit_expr(value)?;
                        if self.expr_is_val(value) {
                            let var_ty = self
                                .var_types
                                .get(name)
                                .or_else(|| self.global_vars.get(name))
                                .cloned()
                                .unwrap_or_else(|| "double".to_string());
                            if var_ty == "int64_t" {
                                self.emit_line(&format!(
                                    "lucid_var_{name} {op_str}= lucid_int_val({val_code});"
                                ));
                            } else {
                                self.emit_line(&format!(
                                    "lucid_var_{name} {op_str}= lucid_num({val_code});"
                                ));
                            }
                        } else {
                            self.emit_line(&format!("lucid_var_{name} {op_str}= {val_code};"));
                        }
                    }
                    Expr::Index {
                        value: arr_expr,
                        index,
                        ..
                    } => {
                        let idx_code = self.emit_expr(index)?;
                        let val_code = self.emit_expr(value)?;
                        if let Expr::Index {
                            value: inner_arr,
                            index: inner_idx,
                            ..
                        } = &**arr_expr
                        {
                            let inner_code = self.emit_expr(inner_arr)?;
                            let inner_idx_code = self.emit_expr(inner_idx)?;
                            self.emit_line(&format!("{{ LucidList* _sub = lucid_as_list(lucid_get_index(lucid_wrap({inner_code}), lucid_int_val({inner_idx_code}))); int64_t _i = lucid_int_val({idx_code}); LucidVal _cur = lucid_list_get(_sub, _i); lucid_list_set(_sub, _i, lucid_float(lucid_as_float(_cur) {op_str} lucid_as_float(lucid_wrap({val_code})))); }}"));
                        } else {
                            let arr_code = self.emit_expr(arr_expr)?;
                            self.emit_line(&format!("{{ LucidList* _l = lucid_as_list(lucid_wrap({arr_code})); int64_t _i = lucid_int_val({idx_code}); LucidVal _cur = lucid_list_get(_l, _i); lucid_list_set(_l, _i, lucid_float(lucid_as_float(_cur) {op_str} lucid_as_float(lucid_wrap({val_code})))); }}"));
                        }
                    }
                    Expr::Attribute {
                        value: obj_expr,
                        attr,
                        ..
                    } => {
                        let class_name = if let Expr::Ident { name: receiver, .. } = &**obj_expr {
                            if self.known_classes.contains_key(receiver) {
                                Some(receiver.clone())
                            } else if receiver == "cls" {
                                self.current_class.clone()
                            } else if receiver == "self" {
                                self.current_class.clone()
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        if let Some(class_name) = class_name {
                            if self
                                .known_class_vars
                                .contains_key(&(class_name.clone(), attr.clone()))
                            {
                                let rhs = self.emit_expr(value)?;
                                self.emit_line(&format!(
                                    "lucid_classvar_{class_name}_{attr} {op_str}= {rhs};"
                                ));
                                return Ok(());
                            }
                            if self
                                .known_field_types
                                .contains_key(&(class_name.clone(), attr.clone()))
                            {
                                let obj_code = self.emit_expr(obj_expr)?;
                                let rhs = self.emit_expr(value)?;
                                let field_ty = self
                                    .known_field_types
                                    .get(&(class_name.clone(), attr.clone()))
                                    .cloned()
                                    .unwrap_or_else(|| "LucidVal".into());
                                let rhs_code = if field_ty == "int64_t" {
                                    if self.expr_is_val(value) {
                                        format!("lucid_int_val({rhs})")
                                    } else {
                                        format!("{rhs}")
                                    }
                                } else if field_ty == "double" {
                                    if self.expr_is_val(value) {
                                        format!("lucid_num({rhs})")
                                    } else {
                                        format!("{rhs}")
                                    }
                                } else {
                                    rhs
                                };
                                self.emit_line(&format!(
                                    "{obj_code}->{attr} {op_str}= {rhs_code};"
                                ));
                                return Ok(());
                            }
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            Stmt::If {
                condition,
                then_branch,
                elif_branches,
                else_branch,
                ..
            } => {
                // `del` changes a source binding's lifetime.  Because branch
                // bodies are emitted sequentially, keep each branch's state
                // independent and retain only deletions guaranteed on every
                // runtime path after the conditional.
                let base_deleted = self.deleted_bindings.clone();
                let mut branch_deleted = Vec::new();
                let cond_truth = self.emit_condition(condition)?;
                self.emit_line(&format!("if ({cond_truth}) {{"));
                self.indent += 1;
                for s in then_branch {
                    self.emit_stmt(s)?;
                }
                self.indent -= 1;
                branch_deleted.push(self.deleted_bindings.clone());
                self.deleted_bindings = base_deleted.clone();

                for (elif_cond, elif_stmts) in elif_branches {
                    let elif_truth = self.emit_condition(elif_cond)?;
                    self.emit_line(&format!("}} else if ({elif_truth}) {{"));
                    self.indent += 1;
                    for s in elif_stmts {
                        self.emit_stmt(s)?;
                    }
                    self.indent -= 1;
                    branch_deleted.push(self.deleted_bindings.clone());
                    self.deleted_bindings = base_deleted.clone();
                }

                if let Some(else_stmts) = else_branch {
                    self.emit_line("} else {");
                    self.indent += 1;
                    for s in else_stmts {
                        self.emit_stmt(s)?;
                    }
                    self.indent -= 1;
                    branch_deleted.push(self.deleted_bindings.clone());
                } else {
                    // No matching branch is also a runtime path.
                    branch_deleted.push(base_deleted.clone());
                }
                self.emit_line("}");
                self.deleted_bindings = branch_deleted
                    .into_iter()
                    .reduce(|mut guaranteed, state| {
                        guaranteed.retain(|name| state.contains(name));
                        guaranteed
                    })
                    .unwrap_or(base_deleted);
                Ok(())
            }
            Stmt::Match { subject, arms, .. } => {
                let subject_code = self.emit_expr(subject)?;
                let subject_tmp = self.new_temp();
                self.emit_line(&format!(
                    "LucidVal {subject_tmp} = lucid_wrap({subject_code});"
                ));
                for (index, arm) in arms.iter().enumerate() {
                    let condition = self.emit_pattern_condition(&arm.pattern, &subject_tmp)?;
                    let guarded = if let Some(guard) = &arm.guard {
                        let guard_truth = self.emit_condition(guard)?;
                        format!("({condition}) && ({guard_truth})")
                    } else {
                        condition
                    };
                    if index == 0 {
                        self.emit_line(&format!("if ({guarded}) {{"));
                    } else {
                        self.emit_line(&format!("}} else if ({guarded}) {{"));
                    }
                    self.indent += 1;
                    self.emit_pattern_bindings(&arm.pattern, &subject_tmp);
                    for statement in &arm.body {
                        self.emit_stmt(statement)?;
                    }
                    self.indent -= 1;
                }
                if !arms.is_empty() {
                    self.emit_line("} else {");
                    self.indent += 1;
                    self.emit_line("fprintf(stderr, \"non-exhaustive match\\n\"); exit(1);");
                    self.indent -= 1;
                    self.emit_line("}");
                }
                Ok(())
            }
            Stmt::With { items, body, .. } => {
                let contexts: Vec<String> = (0..items.len())
                    .map(|index| format!("_lucid_context_{index}"))
                    .collect();
                for context_name in &contexts {
                    self.emit_line(&format!("LucidVal {context_name} = lucid_none();"));
                }
                let frame = self.new_temp();
                let jump = self.new_temp();
                self.emit_line(&format!(
                    "LucidExceptionFrame {frame}; {frame}.previous = lucid_current_exception; lucid_current_exception = &{frame};"
                ));
                self.emit_line(&format!("int {jump} = setjmp({frame}.jump);"));
                self.emit_line(&format!("if ({jump} == 0) {{"));
                self.indent += 1;
                for (item, context_name) in items.iter().zip(&contexts) {
                    let context_code = if let Expr::Call { func, .. } = &item.context_expr {
                        if let Expr::Ident {
                            name: class_name, ..
                        } = &**func
                        {
                            if self
                                .contextmanager_methods
                                .contains(&(class_name.clone(), "__cm__".to_string()))
                            {
                                let object_code = self.emit_expr(&item.context_expr)?;
                                format!("{class_name}___cm__({object_code})")
                            } else {
                                self.emit_expr(&item.context_expr)?
                            }
                        } else {
                            self.emit_expr(&item.context_expr)?
                        }
                    } else {
                        let inferred = self
                            .infer_expr_type(&item.context_expr, &self.var_types)
                            .trim_end_matches('*')
                            .to_string();
                        if self
                            .contextmanager_methods
                            .contains(&(inferred.clone(), "__cm__".to_string()))
                        {
                            let object_code = self.emit_expr(&item.context_expr)?;
                            format!("{inferred}___cm__({object_code})")
                        } else {
                            self.emit_expr(&item.context_expr)?
                        }
                    };
                    self.emit_line(&format!("{context_name} = lucid_wrap({context_code});"));
                    if let Some(Pattern::Ident(name, _)) = &item.target {
                        let ty = self
                            .var_types
                            .get(name)
                            .cloned()
                            .unwrap_or_else(|| "LucidVal".to_string());
                        let assignment = match ty.as_str() {
                            "LucidVal" => {
                                format!("lucid_var_{name} = lucid_context_value({context_name});")
                            }
                            "int64_t" => format!(
                                "lucid_var_{name} = lucid_as_int(lucid_context_value({context_name}));"
                            ),
                            "double" => format!(
                                "lucid_var_{name} = lucid_as_float(lucid_context_value({context_name}));"
                            ),
                            "bool" => format!(
                                "lucid_var_{name} = lucid_as_bool(lucid_context_value({context_name}));"
                            ),
                            "const char*" => format!(
                                "lucid_var_{name} = lucid_as_str(lucid_context_value({context_name}));"
                            ),
                            _ => format!(
                                "lucid_var_{name} = ({ty})lucid_as_ptr(lucid_context_value({context_name}));"
                            ),
                        };
                        self.emit_line(&assignment);
                    } else if let Some(pattern) = &item.target {
                        let value_tmp = self.new_temp();
                        self.emit_line(&format!(
                            "LucidVal {value_tmp} = lucid_context_value({context_name});"
                        ));
                        self.emit_pattern_bindings(pattern, &value_tmp);
                    }
                }
                for statement in body {
                    self.emit_stmt(statement)?;
                }
                for context_name in contexts.iter().rev() {
                    self.emit_line(&format!("lucid_context_exit({context_name}, false);"));
                }
                self.emit_line(&format!("lucid_current_exception = {frame}.previous;"));
                self.indent -= 1;
                self.emit_line("} else {");
                self.indent += 1;
                for context_name in contexts.iter().rev() {
                    self.emit_line(&format!("lucid_context_exit({context_name}, true);"));
                }
                self.emit_line(&format!("lucid_current_exception = {frame}.previous;"));
                self.emit_line("lucid_raise_value(lucid_pending_exception);");
                self.indent -= 1;
                self.emit_line("}");
                Ok(())
            }
            Stmt::While {
                condition,
                body,
                if_broken,
                ..
            } => {
                let break_flag = self.new_temp();
                self.emit_line(&format!("bool {break_flag} = false;"));
                let cond_truth = self.emit_condition(condition)?;
                self.emit_line(&format!("while ({cond_truth}) {{"));
                self.indent += 1;
                self.loop_break_flags.push(break_flag.clone());
                for s in body {
                    self.emit_stmt(s)?;
                }
                self.loop_break_flags.pop();
                self.indent -= 1;
                self.emit_line("}");
                if let Some(clause) = if_broken {
                    self.emit_line(&format!("if ({break_flag}) {{"));
                    self.indent += 1;
                    for statement in clause {
                        self.emit_stmt(statement)?;
                    }
                    self.indent -= 1;
                    self.emit_line("}");
                }
                Ok(())
            }
            Stmt::For {
                target,
                iterable,
                body,
                if_broken,
                ..
            } => {
                let var_name = match target {
                    Pattern::Ident(name, _) => name.clone(),
                    Pattern::Wildcard(_) => "_".to_string(),
                    _ => "_".to_string(),
                };

                // Fast path for range(...)
                if let Expr::Call { func, args, .. } = iterable {
                    if let Expr::Ident { name, .. } = &**func {
                        if name == "range" {
                            let break_flag = self.new_temp();
                            self.emit_line(&format!("bool {break_flag} = false;"));
                            let (start, stop, step) = match args.len() {
                                1 => (
                                    "0LL".to_string(),
                                    self.emit_expr(&args[0].value)?,
                                    "1LL".to_string(),
                                ),
                                2 => (
                                    self.emit_expr(&args[0].value)?,
                                    self.emit_expr(&args[1].value)?,
                                    "1LL".to_string(),
                                ),
                                3 => (
                                    self.emit_expr(&args[0].value)?,
                                    self.emit_expr(&args[1].value)?,
                                    self.emit_expr(&args[2].value)?,
                                ),
                                _ => ("0LL".to_string(), "0LL".to_string(), "1LL".to_string()),
                            };
                            let step_tmp = self.new_temp();
                            let next_tmp = self.new_temp();
                            let start_tmp = self.new_temp();
                            let stop_tmp = self.new_temp();
                            self.emit_line(&format!("int64_t {start_tmp} = {start};"));
                            self.emit_line(&format!("int64_t {stop_tmp} = {stop};"));
                            self.emit_line(&format!("int64_t {step_tmp} = {step};"));
                            self.emit_line(&format!("int64_t {next_tmp};"));
                            self.emit_line(&format!("if ({step_tmp} == 0) {{ fprintf(stderr, \"range() step cannot be zero\\n\"); exit(1); }}"));
                            self.emit_line(&format!("for (lucid_var_{var_name} = {start_tmp}; ({step_tmp} > 0 ? lucid_var_{var_name} < {stop_tmp} : lucid_var_{var_name} > {stop_tmp}); ) {{"));
                            self.indent += 1;
                            self.loop_break_flags.push(break_flag.clone());
                            for s in body {
                                self.emit_stmt(s)?;
                            }
                            self.emit_line(&format!("if (!lucid_checked_range_advance(lucid_var_{var_name}, {step_tmp}, &{next_tmp})) {{ fprintf(stderr, \"range step overflow\\n\"); exit(1); }}"));
                            self.emit_line(&format!("lucid_var_{var_name} = {next_tmp};"));
                            self.loop_break_flags.pop();
                            self.indent -= 1;
                            self.emit_line("}");
                            if let Some(clause) = if_broken {
                                self.emit_line(&format!("if ({break_flag}) {{"));
                                self.indent += 1;
                                for statement in clause {
                                    self.emit_stmt(statement)?;
                                }
                                self.indent -= 1;
                                self.emit_line("}");
                            }
                            return Ok(());
                        }
                    }
                }

                // Consume user-defined iterator objects through `next()`;
                // the result is represented as LucidVal so the terminal
                // iteration sentinel and yielded values share one ABI.
                let iterator_type = self.infer_expr_type(iterable, &self.var_types);
                let iterator_class = iterator_type.trim_end_matches('*').to_string();
                if let Some(next_owner) = self.method_owner(&iterator_class, "next") {
                    let iter_code = self.emit_expr(iterable)?;
                    let iter_object = if let Some(iter_owner) =
                        self.method_owner(&iterator_class, "__iter__")
                    {
                        format!(
                            "({next_owner}*)lucid_as_ptr(lucid_wrap({iter_owner}___iter__(({iter_owner}*)({iter_code}))))"
                        )
                    } else {
                        iter_code
                    };
                    let item_tmp = self.new_temp();
                    let break_flag = self.new_temp();
                    self.emit_line(&format!("bool {break_flag} = false;"));
                    self.emit_line("for (;;) {");
                    self.indent += 1;
                    self.emit_line(&format!(
                        "LucidVal {item_tmp} = lucid_wrap({next_owner}_next({iter_object}));"
                    ));
                    self.emit_line(&format!("if ({item_tmp}.type == LUCID_TYPE_STR && {item_tmp}.s && strcmp({item_tmp}.s, \"iteration.done\") == 0) break;"));
                    self.loop_break_flags.push(break_flag.clone());
                    self.emit_pattern_bindings(target, &item_tmp);
                    for s in body {
                        self.emit_stmt(s)?;
                    }
                    self.loop_break_flags.pop();
                    self.indent -= 1;
                    self.emit_line("}");
                    if let Some(clause) = if_broken {
                        self.emit_line(&format!("if ({break_flag}) {{"));
                        self.indent += 1;
                        for statement in clause {
                            self.emit_stmt(statement)?;
                        }
                        self.indent -= 1;
                        self.emit_line("}");
                    }
                    return Ok(());
                }

                // Generic list iteration: for item in list
                let iter_code = self.emit_expr(iterable)?;
                let iterable_type = self.infer_expr_type(iterable, &HashMap::new());
                let iterable_class = iterable_type.trim_end_matches('*').to_string();
                let iter_code = if let Some(owner) = self.method_owner(&iterable_class, "__iter__")
                {
                    format!("{owner}___iter__(({owner}*)({iter_code}))")
                } else {
                    iter_code
                };
                let tmp_idx = self.new_temp();
                let tmp_list = self.new_temp();
                let break_flag = self.new_temp();
                self.emit_line(&format!("bool {break_flag} = false;"));
                self.emit_line(&format!(
                    "LucidList* {tmp_list} = lucid_iterable_to_list(lucid_wrap({iter_code}));"
                ));
                self.emit_line(&format!("for (int64_t {tmp_idx} = 0; {tmp_list} && {tmp_idx} < {tmp_list}->len; {tmp_idx}++) {{"));
                self.indent += 1;
                self.loop_break_flags.push(break_flag.clone());
                let item_tmp = self.new_temp();
                self.emit_line(&format!(
                    "LucidVal {item_tmp} = {tmp_list}->items[{tmp_idx}];"
                ));
                self.emit_pattern_bindings(target, &item_tmp);
                for s in body {
                    self.emit_stmt(s)?;
                }
                self.loop_break_flags.pop();
                self.indent -= 1;
                self.emit_line("}");
                if let Some(clause) = if_broken {
                    self.emit_line(&format!("if ({break_flag}) {{"));
                    self.indent += 1;
                    for statement in clause {
                        self.emit_stmt(statement)?;
                    }
                    self.indent -= 1;
                    self.emit_line("}");
                }
                Ok(())
            }
            Stmt::Try {
                body,
                handlers,
                finally_body,
                ..
            } => {
                let frame = self.new_temp();
                let jump = self.new_temp();
                let handler_jump = self.new_temp();
                self.emit_line(&format!("LucidExceptionFrame {frame}; {frame}.previous = lucid_current_exception; lucid_current_exception = &{frame}; int {jump} = setjmp({frame}.jump); int {handler_jump} = 0;"));
                self.emit_line(&format!("if ({jump} == 0) {{"));
                self.indent += 1;
                for statement in body {
                    self.emit_stmt(statement)?;
                }
                self.indent -= 1;
                if !handlers.is_empty() {
                    self.emit_line("} else {");
                    self.indent += 1;
                    self.emit_line(&format!("{handler_jump} = setjmp({frame}.jump);"));
                    self.emit_line(&format!("if ({handler_jump} == 0) {{"));
                    self.indent += 1;
                    for (index, handler) in handlers.iter().enumerate() {
                        let condition = self.exception_condition(&handler.exception_type);
                        if index == 0 {
                            self.emit_line(&format!("if ({condition}) {{"));
                        } else {
                            self.emit_line(&format!("}} else if ({condition}) {{"));
                        }
                        self.indent += 1;
                        if let Some(name) = &handler.name {
                            self.emit_line(&format!(
                                "LucidVal lucid_var_{name} = lucid_pending_exception;"
                            ));
                        }
                        for statement in &handler.body {
                            self.emit_stmt(statement)?;
                        }
                        self.indent -= 1;
                    }
                    self.emit_line("} else {");
                    self.indent += 1;
                    self.emit_line("lucid_raise_value(lucid_pending_exception);");
                    self.indent -= 1;
                    self.emit_line("}");
                    self.indent -= 1;
                    self.emit_line("}");
                    self.indent -= 1;
                }
                self.emit_line("}");
                self.emit_line(&format!("lucid_current_exception = {frame}.previous;"));
                if let Some(cleanup) = finally_body {
                    for statement in cleanup {
                        self.emit_stmt(statement)?;
                    }
                }
                if !handlers.is_empty() {
                    self.emit_line(&format!(
                        "if ({handler_jump} != 0) lucid_raise_value(lucid_pending_exception);"
                    ));
                }
                Ok(())
            }
            Stmt::Return { value, .. } => {
                if let Some(val_expr) = value {
                    let val_code = self.emit_expr(val_expr)?;
                    if self.current_fn_async {
                        self.emit_line(&format!("return lucid_future(lucid_wrap({val_code}));"));
                        return Ok(());
                    }
                    if let Some(ret_ty) = &self.current_fn_ret_type {
                        match ret_ty.as_str() {
                            "int64_t" => self.emit_line(&format!(
                                "return lucid_as_int(lucid_wrap({val_code}));"
                            )),
                            "double" => self.emit_line(&format!(
                                "return lucid_as_float(lucid_wrap({val_code}));"
                            )),
                            "bool" => self.emit_line(&format!(
                                "return lucid_as_bool(lucid_wrap({val_code}));"
                            )),
                            "const char*" => self.emit_line(&format!(
                                "return lucid_as_str(lucid_wrap({val_code}));"
                            )),
                            "LucidList*" => self.emit_line(&format!(
                                "return lucid_as_list(lucid_wrap({val_code}));"
                            )),
                            "LucidVal" => {
                                self.emit_line(&format!("return lucid_wrap({val_code});"))
                            }
                            "void" => self.emit_line("return;"),
                            other => self.emit_line(&format!(
                                "return ({other})lucid_as_ptr(lucid_wrap({val_code}));"
                            )),
                        }
                    } else {
                        self.emit_line(&format!("return {val_code};"));
                    }
                } else {
                    if self.current_fn_async {
                        self.emit_line("return lucid_future(lucid_none());");
                    } else {
                        self.emit_line("return;");
                    }
                }
                Ok(())
            }
            Stmt::Assert {
                condition, message, ..
            } => {
                if let Some(message) = message {
                    let message_code = if let Expr::AnonymousDef { params, body, .. } = message {
                        if params.is_empty() {
                            if let [
                                Stmt::Return {
                                    value: Some(expr), ..
                                },
                            ] = body.as_slice()
                            {
                                self.emit_expr(expr)?
                            } else {
                                return Err(CodegenError {
                                    message: "lazy assertion messages must return an expression"
                                        .into(),
                                });
                            }
                        } else {
                            return Err(CodegenError {
                                message: "assertion message callable must take no arguments".into(),
                            });
                        }
                    } else {
                        self.emit_expr(message)?
                    };
                    let truth = self.emit_condition(condition)?;
                    self.emit_line(&format!("if (!({truth})) {{ fprintf(stderr, \"%s\\n\", lucid_as_str(lucid_wrap({message_code}))); exit(1); }}"));
                } else {
                    let truth = self.emit_condition(condition)?;
                    self.emit_line(&format!(
                        "if (!({truth})) {{ fprintf(stderr, \"assertion failed\\n\"); exit(1); }}"
                    ));
                }
                Ok(())
            }
            Stmt::Delete { names, .. } => {
                // The checker proves that a deleted name is never read before
                // it is rebound. Native locals therefore need no tombstone
                // representation: preserve the lifetime boundary as an
                // explicit no-op and let the subsequent definition overwrite
                // the reserved slot.
                for name in names {
                    self.deleted_bindings.insert(name.clone());
                    self.ensure_alive_binding(name);
                    if self.current_fn_ret_type.as_deref() != Some("void") {
                        self.emit_line(&format!("lucid_alive_{name} = false;"));
                    }
                }
                self.emit_line(&format!("/* del {} */", names.join(", ")));
                Ok(())
            }
            Stmt::Raise { exception, .. } => {
                let exception_code = self.emit_expr(exception)?;
                self.emit_line(&format!("lucid_raise_value(lucid_wrap({exception_code}));"));
                Ok(())
            }
            Stmt::Yield { value, .. } => {
                let value_code = self.emit_expr(value)?;
                self.emit_line(&format!("(void)({value_code});"));
                Ok(())
            }
            Stmt::Break(_) => {
                if let Some(flag) = self.loop_break_flags.last().cloned() {
                    self.emit_line(&format!("{flag} = true;"));
                }
                self.emit_line("break;");
                Ok(())
            }
            Stmt::Continue(_) => {
                self.emit_line("continue;");
                Ok(())
            }
            Stmt::Expr(expr) => {
                let code = self.emit_expr(expr)?;
                self.emit_line(&format!("{code};"));
                Ok(())
            }
            // Imports are resolved by the CLI's project loader before native
            // code generation.  The merged definitions remain in the AST so
            // source locations and declaration order stay intact, but the
            // import statements themselves have no C-side operation.
            Stmt::Import { .. } | Stmt::FromImport { .. } | Stmt::Pass(_) => Ok(()),
            other => Err(CodegenError {
                message: format!("native backend does not support statement form: {other:?}"),
            }),
        }
    }

    fn emit_expr(&mut self, expr: &Expr) -> Result<String, CodegenError> {
        match expr {
            Expr::Literal { value, .. } => Ok(match value {
                LiteralValue::Int(n) => format!("{n}LL"),
                LiteralValue::BigInt(n) => {
                    format!("lucid_bigint(\"{}\")", bigint_literal_decimal(n))
                }
                LiteralValue::Float(f) => {
                    let s = format!("{f}");
                    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
                        format!("{s}.0")
                    } else {
                        s
                    }
                }
                LiteralValue::Complex(f) => format!("lucid_complex(0.0, {f})"),
                LiteralValue::Str(s) => format!("\"{}\"", c_escape_string(s)),
                LiteralValue::Bool(b) => {
                    if *b {
                        "true".to_string()
                    } else {
                        "false".to_string()
                    }
                }
                LiteralValue::None => "lucid_none()".to_string(),
                LiteralValue::Sentinel(name) => {
                    // Native values use the same tagged string representation
                    // for named sentinels.  This keeps sentinel identity
                    // stable across generated code and lets sentinel values
                    // participate in the existing `iteration.done` protocol.
                    format!("lucid_str(\"{}\")", c_escape_string(name))
                }
                LiteralValue::Ellipsis => {
                    return Err(CodegenError {
                        message: "ellipsis is documentation syntax, not a runtime value".into(),
                    });
                }
            }),
            Expr::Type(type_expr) => Ok(format!(
                "lucid_str(\"__type:{}\")",
                c_escape_string(&type_form_name(type_expr))
            )),
            Expr::Skip(_) => Ok("lucid_none()".to_string()),
            Expr::Ident { name, .. } => {
                if self
                    .from_imports
                    .get(name)
                    .is_some_and(|module| module == "math")
                {
                    match name.as_str() {
                        "pi" => return Ok("M_PI".to_string()),
                        "e" => return Ok("M_E".to_string()),
                        _ => {}
                    }
                }
                match name.as_str() {
                    "true" => Ok("true".to_string()),
                    "false" => Ok("false".to_string()),
                    "none" | "None" => Ok("lucid_none()".to_string()),
                    "pi" if !self.global_vars.contains_key(name) => Ok("M_PI".to_string()),
                    "e" if !self.global_vars.contains_key(name) => Ok("M_E".to_string()),
                    "platform" if !self.global_vars.contains_key(name) => {
                        Ok("lucid_sys_platform()".to_string())
                    }
                    "version" if !self.global_vars.contains_key(name) => {
                        Ok("lucid_sys_version()".to_string())
                    }
                    "argv" if !self.global_vars.contains_key(name) => {
                        Ok("lucid_sys_argv()".to_string())
                    }
                    "done" if !self.global_vars.contains_key(name) => {
                        Ok("lucid_str(\"iteration.done\")".to_string())
                    }
                    "self" => Ok("self".to_string()),
                    _ => Ok(format!("lucid_var_{name}")),
                }
            }
            Expr::Binary {
                op, left, right, ..
            } => {
                if let Some(operator) = Self::binary_op_name(op) {
                    if let Some(signatures) = self.dispatch_signatures.get(operator) {
                        let actual = [left, right]
                            .iter()
                            .map(|expr| {
                                self.infer_expr_type(expr, &HashMap::new())
                                    .trim_end_matches('*')
                                    .to_string()
                            })
                            .collect::<Vec<_>>();
                        if let Some(emitted) = signatures
                            .iter()
                            .find(|(types, _)| {
                                types.len() >= 2
                                    && types[0].trim_end_matches('*') == actual[0]
                                    && types[1].trim_end_matches('*') == actual[1]
                            })
                            .map(|(_, emitted)| emitted.clone())
                        {
                            let left_code = self.emit_expr(left)?;
                            let right_code = self.emit_expr(right)?;
                            return Ok(format!("{emitted}({left_code}, {right_code})"));
                        }
                    }
                    if let Some(candidates) = self.dispatch_fns.get(operator) {
                        let actual = self.infer_expr_type(left, &HashMap::new());
                        let actual = actual.trim_end_matches('*');
                        if let Some((_, emitted)) =
                            candidates.iter().find(|(ty, _)| ty == actual).cloned()
                        {
                            let left_code = self.emit_expr(left)?;
                            let right_code = self.emit_expr(right)?;
                            return Ok(format!("{emitted}({left_code}, {right_code})"));
                        }
                    }
                }
                // Check for list repeat: list * n and n * list.
                if *op == BinaryOp::Mul {
                    let left_type = self.infer_expr_type(left, &HashMap::new());
                    let right_type = self.infer_expr_type(right, &HashMap::new());
                    if left_type == "LucidList*" && right_type == "int64_t" {
                        let list_code = self.emit_expr(left)?;
                        let count_code = self.emit_expr(right)?;
                        return Ok(format!(
                            "lucid_list_repeat_value({list_code}, lucid_int_val(lucid_wrap({count_code})))"
                        ));
                    }
                    if left_type == "int64_t" && right_type == "LucidList*" {
                        let list_code = self.emit_expr(right)?;
                        let count_code = self.emit_expr(left)?;
                        return Ok(format!(
                            "lucid_list_repeat_value({list_code}, lucid_int_val(lucid_wrap({count_code})))"
                        ));
                    }
                }

                let l_is_val = self.expr_is_val(left);
                let r_is_val = self.expr_is_val(right);
                let l_str = self.emit_expr(left)?;
                let r_str = self.emit_expr(right)?;
                let l_ty = self.infer_expr_type(left, &HashMap::new());
                let r_ty = self.infer_expr_type(right, &HashMap::new());

                if *op == BinaryOp::Mul
                    && matches!(l_ty.as_str(), "const char*" | "char*")
                    && r_ty == "int64_t"
                {
                    return Ok(format!(
                        "lucid_str_repeat({l_str}, lucid_int_val(lucid_wrap({r_str})))"
                    ));
                }

                if self.expr_is_complex(left) || self.expr_is_complex(right) {
                    let lhs = format!("lucid_wrap({l_str})");
                    let rhs = format!("lucid_wrap({r_str})");
                    return Ok(match op {
                        BinaryOp::Add => format!("lucid_complex_add({lhs}, {rhs})"),
                        BinaryOp::Sub => format!("lucid_complex_sub({lhs}, {rhs})"),
                        BinaryOp::Mul => format!("lucid_complex_mul({lhs}, {rhs})"),
                        BinaryOp::Div => format!("lucid_complex_div({lhs}, {rhs})"),
                        BinaryOp::Pow => format!("lucid_complex_pow({lhs}, {rhs})"),
                        BinaryOp::Eq => format!("lucid_eq({lhs}, {rhs})"),
                        BinaryOp::NotEq => format!("(!lucid_eq({lhs}, {rhs}))"),
                        BinaryOp::Is => format!("((bool)({lhs}.type == LUCID_TYPE_COMPLEX))"),
                        BinaryOp::IsNot => format!("((bool)({lhs}.type != LUCID_TYPE_COMPLEX))"),
                        _ => {
                            return Err(CodegenError {
                                message: "complex values do not support this operator".to_string(),
                            });
                        }
                    });
                }

                if self.expr_is_bigint(left) || self.expr_is_bigint(right) {
                    let lhs = format!("lucid_wrap({l_str})");
                    let rhs = format!("lucid_wrap({r_str})");
                    // A mixed arbitrary-precision/float operation follows
                    // the language's numeric promotion rule. Do not route a
                    // float through the decimal BigInt helpers: those
                    // helpers intentionally truncate scalar payloads.
                    if l_ty == "double" || r_ty == "double" {
                        let lhs = format!("lucid_as_float({lhs})");
                        let rhs = format!("lucid_as_float({rhs})");
                        return match op {
                            BinaryOp::Add => Ok(format!("({lhs} + {rhs})")),
                            BinaryOp::Sub => Ok(format!("({lhs} - {rhs})")),
                            BinaryOp::Mul => Ok(format!("({lhs} * {rhs})")),
                            BinaryOp::Div => Ok(format!("({lhs} / {rhs})")),
                            BinaryOp::FloorDiv => {
                                Ok(format!("lucid_float_floor_div({lhs}, {rhs})"))
                            }
                            BinaryOp::Mod => Ok(format!("lucid_float_mod({lhs}, {rhs})")),
                            BinaryOp::Pow => Ok(format!("pow({lhs}, {rhs})")),
                            BinaryOp::Eq => Ok(format!("({lhs} == {rhs})")),
                            BinaryOp::NotEq => Ok(format!("({lhs} != {rhs})")),
                            BinaryOp::Lt => Ok(format!("({lhs} < {rhs})")),
                            BinaryOp::LtEq => Ok(format!("({lhs} <= {rhs})")),
                            BinaryOp::Gt => Ok(format!("({lhs} > {rhs})")),
                            BinaryOp::GtEq => Ok(format!("({lhs} >= {rhs})")),
                            _ => Err(CodegenError {
                                message: "unsupported mixed BigInt/float operation".to_string(),
                            }),
                        };
                    }
                    return match op {
                        BinaryOp::Add => Ok(format!("lucid_bigint_binop({lhs}, {rhs}, '+')")),
                        BinaryOp::Sub => Ok(format!("lucid_bigint_binop({lhs}, {rhs}, '-')")),
                        BinaryOp::Mul => Ok(format!("lucid_bigint_binop({lhs}, {rhs}, '*')")),
                        BinaryOp::Div => {
                            Ok(format!("(lucid_as_float({lhs}) / lucid_as_float({rhs}))"))
                        }
                        BinaryOp::Pow => Ok(format!("lucid_bigint_pow({lhs}, {rhs})")),
                        BinaryOp::FloorDiv => {
                            Ok(format!("lucid_bigint_divmod({lhs}, {rhs}, false, true)"))
                        }
                        BinaryOp::Mod => {
                            Ok(format!("lucid_bigint_divmod({lhs}, {rhs}, true, true)"))
                        }
                        BinaryOp::Eq => Ok(format!("lucid_eq({lhs}, {rhs})")),
                        BinaryOp::NotEq => Ok(format!("(!lucid_eq({lhs}, {rhs}))")),
                        BinaryOp::Lt => Ok(format!("(lucid_bigint_cmp({lhs}, {rhs}) < 0)")),
                        BinaryOp::LtEq => Ok(format!("(lucid_bigint_cmp({lhs}, {rhs}) <= 0)")),
                        BinaryOp::Gt => Ok(format!("(lucid_bigint_cmp({lhs}, {rhs}) > 0)")),
                        BinaryOp::GtEq => Ok(format!("(lucid_bigint_cmp({lhs}, {rhs}) >= 0)")),
                        _ => Err(CodegenError {
                            message: "unsupported operation on native big integers".to_string(),
                        }),
                    };
                }

                if matches!(op, BinaryOp::Sub)
                    && self.infer_expr_type(left, &HashMap::new()) == "LucidSet*"
                {
                    return Ok(format!(
                        "lucid_set_difference_value({l_str}, lucid_wrap({r_str}))"
                    ));
                }
                if matches!(
                    op,
                    BinaryOp::Sub | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor
                ) && self.expr_is_dynamic_value(left)
                    && self.expr_is_dynamic_value(right)
                {
                    let symbol = match op {
                        BinaryOp::Sub => '-',
                        BinaryOp::BitAnd => '&',
                        BinaryOp::BitOr => '|',
                        BinaryOp::BitXor => '^',
                        _ => {
                            return Err(CodegenError {
                                message: "invalid dynamic set operation".to_string(),
                            });
                        }
                    };
                    return Ok(format!(
                        "lucid_setop_value(lucid_wrap({l_str}), lucid_wrap({r_str}), '{symbol}')"
                    ));
                }
                if *op == BinaryOp::Div
                    && self.expr_is_dynamic_value(left)
                    && self.expr_is_dynamic_value(right)
                {
                    return Ok(format!(
                        "lucid_div_value(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                    ));
                }
                if matches!(op, BinaryOp::FloorDiv | BinaryOp::Mod)
                    && self.expr_is_dynamic_value(left)
                    && self.expr_is_dynamic_value(right)
                {
                    let helper = if *op == BinaryOp::FloorDiv {
                        "lucid_floor_div_value"
                    } else {
                        "lucid_mod_value"
                    };
                    return Ok(format!(
                        "{helper}(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                    ));
                }
                if *op == BinaryOp::Pow
                    && self.expr_is_dynamic_value(left)
                    && self.expr_is_dynamic_value(right)
                {
                    return Ok(format!(
                        "lucid_pow(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                    ));
                }
                if matches!(op, BinaryOp::Shl | BinaryOp::Shr)
                    && self.expr_is_dynamic_value(left)
                    && self.expr_is_dynamic_value(right)
                {
                    return Ok(format!(
                        "lucid_shift_value(lucid_wrap({l_str}), lucid_wrap({r_str}), {})",
                        *op == BinaryOp::Shl
                    ));
                }

                match op {
                    BinaryOp::Add => {
                        let l_ty = self.infer_expr_type(left, &HashMap::new());
                        let r_ty = self.infer_expr_type(right, &HashMap::new());
                        if l_ty == "const char*" && r_ty == "const char*" {
                            Ok(format!("lucid_str_concat({l_str}, {r_str})"))
                        } else if l_ty == "LucidList*" && r_ty == "LucidList*" {
                            Ok(format!("lucid_list_concat({l_str}, {r_str})"))
                        } else if self.expr_is_dynamic_value(left)
                            && self.expr_is_dynamic_value(right)
                        {
                            Ok(format!(
                                "lucid_add_value(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                            ))
                        } else if l_is_val || r_is_val {
                            Ok(format!("(lucid_num({l_str}) + lucid_num({r_str}))"))
                        } else if l_ty == "int64_t" && r_ty == "int64_t" {
                            Ok(format!("lucid_checked_add({l_str}, {r_str})"))
                        } else {
                            Ok(format!("({l_str} + {r_str})"))
                        }
                    }
                    BinaryOp::Sub => {
                        if l_is_val || r_is_val {
                            Ok(format!("(lucid_num({l_str}) - lucid_num({r_str}))"))
                        } else if l_ty == "int64_t" && r_ty == "int64_t" {
                            Ok(format!("lucid_checked_sub({l_str}, {r_str})"))
                        } else {
                            Ok(format!("({l_str} - {r_str})"))
                        }
                    }
                    BinaryOp::Mul => {
                        if self.expr_is_dynamic_value(left) && self.expr_is_dynamic_value(right) {
                            Ok(format!(
                                "lucid_mul_value(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                            ))
                        } else if l_is_val || r_is_val {
                            Ok(format!("(lucid_num({l_str}) * lucid_num({r_str}))"))
                        } else if l_ty == "int64_t" && r_ty == "int64_t" {
                            Ok(format!("lucid_checked_mul({l_str}, {r_str})"))
                        } else {
                            Ok(format!("({l_str} * {r_str})"))
                        }
                    }
                    BinaryOp::Div => Ok(format!("((double){l_str} / (double){r_str})")),
                    BinaryOp::FloorDiv if l_ty == "double" || r_ty == "double" => Ok(format!(
                        "lucid_float_floor_div(lucid_num({l_str}), lucid_num({r_str}))"
                    )),
                    BinaryOp::FloorDiv => Ok(format!(
                        "lucid_int_floor_div(lucid_int_val({l_str}), lucid_int_val({r_str}))"
                    )),
                    BinaryOp::Mod if l_ty == "double" || r_ty == "double" => Ok(format!(
                        "lucid_float_mod(lucid_num({l_str}), lucid_num({r_str}))"
                    )),
                    BinaryOp::Mod => Ok(format!(
                        "lucid_int_mod(lucid_int_val({l_str}), lucid_int_val({r_str}))"
                    )),
                    BinaryOp::Pow if l_ty == "int64_t" && r_ty == "int64_t" => Ok(format!(
                        "lucid_pow(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                    )),
                    BinaryOp::Pow => Ok(format!("pow((double){l_str}, (double){r_str})")),
                    BinaryOp::Eq => {
                        let l_ty = self.infer_expr_type(left, &HashMap::new());
                        let r_ty = self.infer_expr_type(right, &HashMap::new());
                        let aggregate =
                            matches!(l_ty.as_str(), "LucidList*" | "LucidSet*" | "LucidDict*")
                                || matches!(
                                    r_ty.as_str(),
                                    "LucidList*" | "LucidSet*" | "LucidDict*"
                                );
                        if l_is_val
                            || r_is_val
                            || l_ty == "LucidVal"
                            || r_ty == "LucidVal"
                            || aggregate
                            || matches!(
                                **left,
                                Expr::Literal {
                                    value: LiteralValue::Str(_),
                                    ..
                                }
                            )
                            || matches!(
                                **right,
                                Expr::Literal {
                                    value: LiteralValue::Str(_),
                                    ..
                                }
                            )
                        {
                            Ok(format!(
                                "lucid_eq(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                            ))
                        } else {
                            Ok(format!("({l_str} == {r_str})"))
                        }
                    }
                    BinaryOp::NotEq => {
                        let l_ty = self.infer_expr_type(left, &HashMap::new());
                        let r_ty = self.infer_expr_type(right, &HashMap::new());
                        let aggregate =
                            matches!(l_ty.as_str(), "LucidList*" | "LucidSet*" | "LucidDict*")
                                || matches!(
                                    r_ty.as_str(),
                                    "LucidList*" | "LucidSet*" | "LucidDict*"
                                );
                        if l_is_val
                            || r_is_val
                            || l_ty == "LucidVal"
                            || r_ty == "LucidVal"
                            || aggregate
                            || matches!(
                                **left,
                                Expr::Literal {
                                    value: LiteralValue::Str(_),
                                    ..
                                }
                            )
                            || matches!(
                                **right,
                                Expr::Literal {
                                    value: LiteralValue::Str(_),
                                    ..
                                }
                            )
                        {
                            Ok(format!(
                                "(!lucid_eq(lucid_wrap({l_str}), lucid_wrap({r_str})))"
                            ))
                        } else {
                            Ok(format!("({l_str} != {r_str})"))
                        }
                    }
                    BinaryOp::Lt => {
                        if l_is_val || r_is_val {
                            Ok(format!(
                                "lucid_lt(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                            ))
                        } else {
                            Ok(format!("({l_str} < {r_str})"))
                        }
                    }
                    BinaryOp::LtEq => {
                        if l_is_val || r_is_val {
                            Ok(format!(
                                "lucid_lte(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                            ))
                        } else {
                            Ok(format!("({l_str} <= {r_str})"))
                        }
                    }
                    BinaryOp::Gt => {
                        if l_is_val || r_is_val {
                            Ok(format!(
                                "lucid_gt(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                            ))
                        } else {
                            Ok(format!("({l_str} > {r_str})"))
                        }
                    }
                    BinaryOp::GtEq => {
                        if l_is_val || r_is_val {
                            Ok(format!(
                                "lucid_gte(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                            ))
                        } else {
                            Ok(format!("({l_str} >= {r_str})"))
                        }
                    }
                    BinaryOp::BitAnd
                        if self.infer_expr_type(left, &HashMap::new()) == "LucidSet*" =>
                    {
                        Ok(format!(
                            "lucid_set_intersection_value({l_str}, lucid_wrap({r_str}))"
                        ))
                    }
                    BinaryOp::BitOr
                        if self.infer_expr_type(left, &HashMap::new()) == "LucidSet*" =>
                    {
                        Ok(format!(
                            "lucid_set_union_value({l_str}, lucid_wrap({r_str}))"
                        ))
                    }
                    BinaryOp::BitXor
                        if self.infer_expr_type(left, &HashMap::new()) == "LucidSet*" =>
                    {
                        Ok(format!("lucid_set_xor_value({l_str}, lucid_wrap({r_str}))"))
                    }
                    BinaryOp::BitAnd => Ok(format!("((int64_t){l_str} & (int64_t){r_str})")),
                    BinaryOp::BitOr => Ok(format!("((int64_t){l_str} | (int64_t){r_str})")),
                    BinaryOp::BitXor => Ok(format!("((int64_t){l_str} ^ (int64_t){r_str})")),
                    BinaryOp::Shl => Ok(format!(
                        "lucid_checked_shl((int64_t){l_str}, (int64_t){r_str})"
                    )),
                    BinaryOp::Shr => Ok(format!(
                        "lucid_checked_shr((int64_t){l_str}, (int64_t){r_str})"
                    )),
                    BinaryOp::And => {
                        let l_tmp = self.new_temp();
                        let l_truth = self.truthy_native(left, &l_tmp);
                        Ok(format!(
                            "({{ __typeof__({l_str}) {l_tmp} = {l_str}; ({l_truth}) ? lucid_wrap({r_str}) : lucid_wrap({l_tmp}); }})"
                        ))
                    }
                    BinaryOp::Or => {
                        let l_tmp = self.new_temp();
                        let l_truth = self.truthy_native(left, &l_tmp);
                        Ok(format!(
                            "({{ __typeof__({l_str}) {l_tmp} = {l_str}; ({l_truth}) ? lucid_wrap({l_tmp}) : lucid_wrap({r_str}); }})"
                        ))
                    }
                    BinaryOp::Identity => Ok(format!(
                        "lucid_identity(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                    )),
                    BinaryOp::Is => {
                        if is_none_expr(right) {
                            Ok(format!("lucid_is_none({l_str})"))
                        } else if let Some(type_name) = match &**right {
                            Expr::Ident { name, .. } => Some(name.as_str()),
                            Expr::Type(TypeExpr::Named { name, .. }) => Some(name.as_str()),
                            _ => None,
                        } {
                            let left_ty = self.infer_expr_type(left, &HashMap::new());
                            let condition = match type_name {
                                "Callable" => {
                                    const BUILTINS: &[&str] = &[
                                        "abs",
                                        "all",
                                        "any",
                                        "bool",
                                        "bytes",
                                        "bytearray",
                                        "chr",
                                        "dict",
                                        "enumerate",
                                        "env_var",
                                        "fields",
                                        "float",
                                        "freeze",
                                        "format",
                                        "getattr",
                                        "hasattr",
                                        "hash",
                                        "help",
                                        "int",
                                        "iter",
                                        "len",
                                        "list",
                                        "locals",
                                        "map",
                                        "max",
                                        "memoryview",
                                        "min",
                                        "ord",
                                        "pow",
                                        "print",
                                        "range",
                                        "repr",
                                        "reversed",
                                        "round",
                                        "set",
                                        "setattr",
                                        "slice",
                                        "sorted",
                                        "str",
                                        "sum",
                                        "time",
                                        "trust",
                                        "read_file",
                                        "write_file",
                                        "zip",
                                    ];
                                    let callable = match &**left {
                                        Expr::AnonymousDef { .. } => true,
                                        Expr::Call { args, .. }
                                            if args
                                                .iter()
                                                .any(|arg| matches!(arg.value, Expr::Skip(_))) =>
                                        {
                                            true
                                        }
                                        Expr::Ident { name, .. } => {
                                            self.known_fns.contains_key(name)
                                                || self.function_aliases.contains_key(name)
                                                || self.anonymous_bindings.contains_key(name)
                                                || self.partial_bindings.contains_key(name)
                                                || BUILTINS.contains(&name.as_str())
                                        }
                                        _ => false,
                                    };
                                    format!("((bool){})", callable)
                                }
                                "class" => {
                                    if left_ty.ends_with('*') {
                                        "((bool)1)".to_string()
                                    } else {
                                        format!("(!lucid_is_none(lucid_wrap({l_str})))")
                                    }
                                }
                                "trait" | "interface" => "((bool)0)".to_string(),
                                "Sized" => {
                                    if matches!(
                                        left_ty.as_str(),
                                        "const char*" | "LucidList*" | "LucidDict*" | "LucidSet*"
                                    ) {
                                        "((bool)1)".into()
                                    } else {
                                        "((bool)0)".into()
                                    }
                                }
                                "Container" => {
                                    if matches!(
                                        left_ty.as_str(),
                                        "const char*" | "LucidList*" | "LucidDict*" | "LucidSet*"
                                    ) {
                                        "((bool)1)".into()
                                    } else {
                                        "((bool)0)".into()
                                    }
                                }
                                "Iterable" | "Collection" => {
                                    if matches!(
                                        left_ty.as_str(),
                                        "LucidList*" | "LucidSet*" | "LucidDict*"
                                    ) {
                                        "((bool)1)".into()
                                    } else {
                                        "((bool)0)".into()
                                    }
                                }
                                "Sequence" | "Reversible" => {
                                    if left_ty == "LucidList*" {
                                        "((bool)1)".into()
                                    } else {
                                        "((bool)0)".into()
                                    }
                                }
                                "Set" => {
                                    if left_ty == "LucidSet*" {
                                        "((bool)1)".into()
                                    } else {
                                        "((bool)0)".into()
                                    }
                                }
                                "Buffer" => {
                                    if matches!(left_ty.as_str(), "const char*" | "LucidList*") {
                                        "((bool)1)".into()
                                    } else {
                                        "((bool)0)".into()
                                    }
                                }
                                "Shape" => {
                                    if left_ty == "LucidList*" {
                                        "((bool)1)".into()
                                    } else {
                                        "((bool)0)".into()
                                    }
                                }
                                "Eq" | "Ord" | "Hashable" => {
                                    let capability = type_name;
                                    if is_none_expr(left) {
                                        "((bool)0)".into()
                                    } else if self
                                        .known_classes
                                        .contains_key(left_ty.trim_end_matches('*'))
                                    {
                                        format!(
                                            "((bool){})",
                                            self.class_has_capability(
                                                left_ty.trim_end_matches('*'),
                                                capability
                                            )
                                        )
                                    } else {
                                        "((bool)1)".into()
                                    }
                                }
                                "int" => {
                                    if left_ty == "int64_t" {
                                        "((bool)1)".into()
                                    } else {
                                        format!("(lucid_wrap({l_str}).type == LUCID_TYPE_INT)")
                                    }
                                }
                                "float" => {
                                    if left_ty == "double" {
                                        "((bool)1)".into()
                                    } else {
                                        format!("(lucid_wrap({l_str}).type == LUCID_TYPE_FLOAT)")
                                    }
                                }
                                "complex" => {
                                    format!("(lucid_wrap({l_str}).type == LUCID_TYPE_COMPLEX)")
                                }
                                "bool" => {
                                    if left_ty == "bool" {
                                        "((bool)1)".into()
                                    } else {
                                        format!("(lucid_wrap({l_str}).type == LUCID_TYPE_BOOL)")
                                    }
                                }
                                "str" => {
                                    if left_ty == "const char*" {
                                        "((bool)1)".into()
                                    } else {
                                        format!("(lucid_wrap({l_str}).type == LUCID_TYPE_STR)")
                                    }
                                }
                                name if self.known_classes.contains_key(name) => {
                                    if left_ty.ends_with('*') {
                                        format!(
                                            "((bool){})",
                                            self.class_is_subtype(
                                                left_ty.trim_end_matches('*'),
                                                name
                                            )
                                        )
                                    } else {
                                        format!(
                                            "lucid_object_is(lucid_as_ptr(lucid_wrap({l_str})), \"{name}\")"
                                        )
                                    }
                                }
                                _ => "((bool)0)".into(),
                            };
                            Ok(condition)
                        } else {
                            Ok(format!(
                                "lucid_identity(lucid_wrap({l_str}), lucid_wrap({r_str}))"
                            ))
                        }
                    }
                    BinaryOp::NotIdentity => Ok(format!(
                        "(!lucid_identity(lucid_wrap({l_str}), lucid_wrap({r_str})))"
                    )),
                    BinaryOp::IsNot => {
                        if is_none_expr(right) {
                            Ok(format!("(!lucid_is_none({l_str}))"))
                        } else if let Some(type_name) = match &**right {
                            Expr::Ident { name, .. } => Some(name.as_str()),
                            Expr::Type(TypeExpr::Named { name, .. }) => Some(name.as_str()),
                            _ => None,
                        } {
                            let left_ty = self.infer_expr_type(left, &HashMap::new());
                            let condition = match type_name {
                                "Callable" => {
                                    const BUILTINS: &[&str] = &[
                                        "abs",
                                        "all",
                                        "any",
                                        "bool",
                                        "bytes",
                                        "bytearray",
                                        "chr",
                                        "dict",
                                        "enumerate",
                                        "env_var",
                                        "fields",
                                        "float",
                                        "freeze",
                                        "format",
                                        "getattr",
                                        "hasattr",
                                        "hash",
                                        "help",
                                        "int",
                                        "iter",
                                        "len",
                                        "list",
                                        "locals",
                                        "map",
                                        "max",
                                        "memoryview",
                                        "min",
                                        "ord",
                                        "pow",
                                        "print",
                                        "range",
                                        "repr",
                                        "reversed",
                                        "round",
                                        "set",
                                        "setattr",
                                        "slice",
                                        "sorted",
                                        "str",
                                        "sum",
                                        "time",
                                        "trust",
                                        "read_file",
                                        "write_file",
                                        "zip",
                                    ];
                                    let callable = match &**left {
                                        Expr::AnonymousDef { .. } => true,
                                        Expr::Call { args, .. }
                                            if args
                                                .iter()
                                                .any(|arg| matches!(arg.value, Expr::Skip(_))) =>
                                        {
                                            true
                                        }
                                        Expr::Ident { name, .. } => {
                                            self.known_fns.contains_key(name)
                                                || self.function_aliases.contains_key(name)
                                                || self.anonymous_bindings.contains_key(name)
                                                || self.partial_bindings.contains_key(name)
                                                || BUILTINS.contains(&name.as_str())
                                        }
                                        _ => false,
                                    };
                                    format!("((bool){})", !callable)
                                }
                                "class" => {
                                    if left_ty.ends_with('*') {
                                        "((bool)0)".into()
                                    } else {
                                        format!("lucid_is_none(lucid_wrap({l_str}))")
                                    }
                                }
                                "trait" | "interface" => "((bool)1)".into(),
                                "Sized" | "Container" => {
                                    if matches!(
                                        left_ty.as_str(),
                                        "const char*" | "LucidList*" | "LucidDict*" | "LucidSet*"
                                    ) {
                                        "((bool)0)".into()
                                    } else {
                                        "((bool)1)".into()
                                    }
                                }
                                "Eq" | "Ord" | "Hashable" => {
                                    let capability = type_name;
                                    if is_none_expr(left) {
                                        "((bool)1)".into()
                                    } else if self
                                        .known_classes
                                        .contains_key(left_ty.trim_end_matches('*'))
                                    {
                                        format!(
                                            "((bool)!{})",
                                            self.class_has_capability(
                                                left_ty.trim_end_matches('*'),
                                                capability
                                            )
                                        )
                                    } else {
                                        "((bool)0)".into()
                                    }
                                }
                                "Iterable" | "Collection" => {
                                    if matches!(
                                        left_ty.as_str(),
                                        "LucidList*" | "LucidSet*" | "LucidDict*"
                                    ) {
                                        "((bool)0)".into()
                                    } else {
                                        "((bool)1)".into()
                                    }
                                }
                                "Sequence" | "Reversible" => {
                                    if left_ty == "LucidList*" {
                                        "((bool)0)".into()
                                    } else {
                                        "((bool)1)".into()
                                    }
                                }
                                "Set" => {
                                    if left_ty == "LucidSet*" {
                                        "((bool)0)".into()
                                    } else {
                                        "((bool)1)".into()
                                    }
                                }
                                "Buffer" | "Shape" => {
                                    if matches!(left_ty.as_str(), "const char*" | "LucidList*") {
                                        "((bool)0)".into()
                                    } else {
                                        "((bool)1)".into()
                                    }
                                }
                                "int" => format!("(lucid_wrap({l_str}).type != LUCID_TYPE_INT)"),
                                "float" => {
                                    format!("(lucid_wrap({l_str}).type != LUCID_TYPE_FLOAT)")
                                }
                                "complex" => {
                                    format!("(lucid_wrap({l_str}).type != LUCID_TYPE_COMPLEX)")
                                }
                                "bool" => format!("(lucid_wrap({l_str}).type != LUCID_TYPE_BOOL)"),
                                "str" => format!("(lucid_wrap({l_str}).type != LUCID_TYPE_STR)"),
                                name if self.known_classes.contains_key(name) => {
                                    if left_ty.ends_with('*') {
                                        format!(
                                            "((bool){})",
                                            !self.class_is_subtype(
                                                left_ty.trim_end_matches('*'),
                                                name
                                            )
                                        )
                                    } else {
                                        format!(
                                            "(!lucid_object_is(lucid_as_ptr(lucid_wrap({l_str})), \"{name}\"))"
                                        )
                                    }
                                }
                                _ => "((bool)1)".into(),
                            };
                            Ok(condition)
                        } else {
                            Ok(format!(
                                "(!lucid_identity(lucid_wrap({l_str}), lucid_wrap({r_str})))"
                            ))
                        }
                    }
                    BinaryOp::In | BinaryOp::NotIn => {
                        let r_ty = self.infer_expr_type(right, &HashMap::new());
                        let r_class = r_ty.trim_end_matches('*').to_string();
                        if let Some(owner) = self.method_owner(&r_class, "__contains__") {
                            let call =
                                format!("{owner}___contains__(({owner}*)({r_str}), {l_str})");
                            return if matches!(op, BinaryOp::NotIn) {
                                Ok(format!("((bool)(!{call}))"))
                            } else {
                                Ok(format!("((bool)({call}))"))
                            };
                        }
                        if matches!(r_ty.as_str(), "const char*" | "char*") {
                            let check = format!("lucid_str_contains({r_str}, lucid_wrap({l_str}))");
                            return if matches!(op, BinaryOp::NotIn) {
                                Ok(format!("((bool)(!{check}))"))
                            } else {
                                Ok(format!("((bool)({check}))"))
                            };
                        }
                        if r_ty == "LucidVal" {
                            let check = format!(
                                "lucid_contains_value(lucid_wrap({r_str}), lucid_wrap({l_str}))"
                            );
                            return if matches!(op, BinaryOp::NotIn) {
                                Ok(format!("((bool)(!{check}))"))
                            } else {
                                Ok(format!("((bool)({check}))"))
                            };
                        }
                        let helper = match r_ty.as_str() {
                            "LucidSet*" => "lucid_set_contains",
                            "LucidDict*" => "lucid_dict_contains",
                            _ => "lucid_list_contains",
                        };
                        let check = format!("{helper}({r_str}, lucid_wrap({l_str}))");
                        if matches!(op, BinaryOp::NotIn) {
                            Ok(format!("((bool)(!{check}))"))
                        } else {
                            Ok(format!("((bool)({check}))"))
                        }
                    }
                }
            }
            Expr::Unary { op, expr, .. } => {
                let e_str = self.emit_expr(expr)?;
                if self.expr_is_dynamic_value(expr) {
                    let symbol = match op {
                        UnaryOp::Pos => '+',
                        UnaryOp::Neg => '-',
                        UnaryOp::Invert => '~',
                        UnaryOp::Not => {
                            return Ok(format!("(!{})", self.truthy_native(expr, &e_str)));
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "unsupported dynamic unary operator".to_string(),
                            });
                        }
                    };
                    return Ok(format!(
                        "lucid_unary_value(lucid_wrap({e_str}), '{symbol}')"
                    ));
                }
                if self.expr_is_bigint(expr) {
                    return match op {
                        UnaryOp::Neg => Ok(format!("lucid_bigint_neg(lucid_wrap({e_str}))")),
                        UnaryOp::Pos => Ok(format!("lucid_wrap({e_str})")),
                        UnaryOp::Invert => Ok(format!(
                            "lucid_bigint_binop(lucid_bigint_neg(lucid_wrap({e_str})), lucid_bigint(\"1\"), '-')"
                        )),
                        _ => Err(CodegenError {
                            message: "unsupported unary operation on native big integer"
                                .to_string(),
                        }),
                    };
                }
                if self.expr_is_complex(expr) {
                    return Ok(match op {
                        UnaryOp::Neg => format!("lucid_complex_neg(lucid_wrap({e_str}))"),
                        UnaryOp::Pos => format!("lucid_wrap({e_str})"),
                        _ => format!("lucid_wrap({e_str})"),
                    });
                }
                match op {
                    UnaryOp::Pos => Ok(format!("(+{e_str})")),
                    UnaryOp::Neg if self.infer_expr_type(expr, &HashMap::new()) == "int64_t" => {
                        Ok(format!("lucid_checked_neg({e_str})"))
                    }
                    UnaryOp::Neg => Ok(format!("(-{e_str})")),
                    UnaryOp::Not => Ok(format!("(!({}))", self.truthy_native(expr, &e_str))),
                    UnaryOp::Invert => Ok(format!("(~(int64_t){e_str})")),
                    _ => Ok(e_str),
                }
            }
            Expr::Call { func, args, span } => {
                if let Expr::Attribute { value, attr, .. } = &**func {
                    if matches!(&**value, Expr::Ident { name, .. } if name == "str")
                        && matches!(attr.as_str(), "bin" | "oct" | "hex")
                    {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: format!("str.{attr}() takes exactly one argument"),
                            });
                        }
                        let value = self.emit_expr(&args[0].value)?;
                        let spec = match attr.as_str() {
                            "bin" => "pb",
                            "oct" => "po",
                            _ => "px",
                        };
                        return Ok(format!(
                            "lucid_as_str(lucid_format_value(lucid_wrap({value}), \"{spec}\"))"
                        ));
                    }
                }
                if let Expr::Ident { name, .. } = &**func {
                    if self
                        .from_imports
                        .get(name)
                        .is_some_and(|module| module == "math")
                        && matches!(
                            name.as_str(),
                            "sqrt" | "sin" | "cos" | "tan" | "floor" | "ceil"
                        )
                    {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: format!("{name}() takes exactly one argument"),
                            });
                        }
                        let arg = self.emit_expr(&args[0].value)?;
                        let numeric = format!("lucid_as_float(lucid_wrap({arg}))");
                        return Ok(match name.as_str() {
                            "sqrt" => format!("sqrt({numeric})"),
                            "sin" => format!("sin({numeric})"),
                            "cos" => format!("cos({numeric})"),
                            "tan" => format!("tan({numeric})"),
                            "floor" => format!("floor({numeric})"),
                            _ => format!("ceil({numeric})"),
                        });
                    }
                    if self
                        .from_imports
                        .get(name)
                        .is_some_and(|module| module == "time")
                        && matches!(name.as_str(), "time" | "monotonic")
                    {
                        if !args.is_empty() {
                            return Err(CodegenError {
                                message: format!("{name}() takes no arguments"),
                            });
                        }
                        return Ok("lucid_time_now()".to_string());
                    }
                    if let Some(alias) = self.function_aliases.get(name).cloned() {
                        if let Some(factory) = alias.strip_prefix("__str_base_") {
                            let spec = match factory {
                                "bin" => "pb",
                                "oct" => "po",
                                _ => "px",
                            };
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: format!("{alias}() takes exactly one argument"),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            return Ok(format!(
                                "lucid_as_str(lucid_format_value(lucid_wrap({value}), \"{spec}\"))"
                            ));
                        }
                    }
                    if let Some((target, partial_args)) = self.partial_bindings.get(name).cloned() {
                        let supplied: Vec<&Arg> = args
                            .iter()
                            .filter(|arg| !matches!(arg.value, Expr::Skip(_)))
                            .collect();
                        let mut next = supplied.into_iter();
                        let mut rendered = Vec::with_capacity(partial_args.len());
                        for arg in partial_args {
                            if matches!(&arg.value, Expr::Ident { name, .. } if name == "_") {
                                let replacement = next.next().ok_or_else(|| CodegenError {
                                    message: "partial function called with too few arguments"
                                        .into(),
                                })?;
                                rendered.push(self.emit_expr(&replacement.value)?);
                            } else {
                                rendered.push(self.emit_expr(&arg.value)?);
                            }
                        }
                        if next.next().is_some() {
                            return Err(CodegenError {
                                message: "partial function called with too many arguments".into(),
                            });
                        }
                        return Ok(format!("lucid_fn_{target}({})", rendered.join(", ")));
                    }
                }
                // Inline expression-bodied anonymous functions with any
                // number of typed parameters.  Native functions use concrete
                // C signatures, so this preserves first-class call behavior
                // without introducing an untyped function-pointer ABI.
                if let Expr::AnonymousDef { params, body, .. } = &**func {
                    if params.len() > 1 {
                        let body_expr = match body.as_slice() {
                            [
                                Stmt::Return {
                                    value: Some(expr), ..
                                },
                            ] => expr,
                            _ => {
                                return Err(CodegenError {
                                    message: "native multi-argument anonymous calls require an expression-bodied function".into(),
                                });
                            }
                        };
                        let effective_args: Vec<&Arg> = args
                            .iter()
                            .filter(|arg| !matches!(arg.value, Expr::Skip(_)))
                            .collect();
                        if effective_args.len() != params.len() {
                            return Err(CodegenError {
                                message: format!(
                                    "anonymous function expects {} arguments, got {}",
                                    params.len(),
                                    effective_args.len()
                                ),
                            });
                        }
                        let mut declarations = Vec::with_capacity(params.len());
                        let mut previous = Vec::with_capacity(params.len());
                        for (param, arg) in params.iter().zip(effective_args.iter()) {
                            let ty = self.map_type_expr(param.type_annotation.as_ref());
                            let arg_code = self.emit_expr(&arg.value)?;
                            let converted = match ty.as_str() {
                                "int64_t" => format!("lucid_as_int(lucid_wrap({arg_code}))"),
                                "double" => format!("lucid_as_float(lucid_wrap({arg_code}))"),
                                "bool" => format!("(bool)lucid_as_bool(lucid_wrap({arg_code}))"),
                                "const char*" => format!("lucid_as_str(lucid_wrap({arg_code}))"),
                                "LucidList*" => format!("lucid_as_list(lucid_wrap({arg_code}))"),
                                "LucidDict*" => format!("lucid_as_dict(lucid_wrap({arg_code}))"),
                                "LucidSet*" => format!("lucid_as_set(lucid_wrap({arg_code}))"),
                                _ => format!("lucid_wrap({arg_code})"),
                            };
                            let binding = format!("lucid_var_{}", param.name);
                            declarations.push(format!("{ty} {binding} = {converted};"));
                            previous.push((
                                param.name.clone(),
                                self.var_types.insert(param.name.clone(), ty),
                            ));
                        }
                        let rendered = self.emit_expr(body_expr)?;
                        for (name, old) in previous {
                            if let Some(old) = old {
                                self.var_types.insert(name, old);
                            } else {
                                self.var_types.remove(&name);
                            }
                        }
                        return Ok(format!("({{ {} {}; }})", declarations.join(" "), rendered));
                    }
                }
                let anonymous = match &**func {
                    Expr::AnonymousDef { params, body, .. } if params.len() <= 1 => {
                        match body.as_slice() {
                            [
                                Stmt::Return {
                                    value: Some(body_expr),
                                    ..
                                },
                            ] => Some((
                                params
                                    .iter()
                                    .map(|param| {
                                        (
                                            param.name.clone(),
                                            self.map_type_expr(param.type_annotation.as_ref()),
                                        )
                                    })
                                    .collect(),
                                body_expr.clone(),
                            )),
                            _ => None,
                        }
                    }
                    Expr::Ident { name, .. } => self.anonymous_bindings.get(name).cloned(),
                    _ => None,
                };
                if let Some((parameter_specs, body_expr)) = anonymous {
                    let effective_args: Vec<&Arg> = args
                        .iter()
                        .filter(|arg| !matches!(arg.value, Expr::Skip(_)))
                        .collect();
                    let expected_args = parameter_specs.len();
                    if effective_args.len() != expected_args {
                        return Err(CodegenError {
                            message: format!(
                                "native anonymous function calls require {} arguments",
                                expected_args
                            ),
                        });
                    }
                    let mut declarations = Vec::with_capacity(parameter_specs.len());
                    let mut previous = Vec::with_capacity(parameter_specs.len());
                    for ((param_name, param_type), arg) in
                        parameter_specs.iter().zip(effective_args.iter())
                    {
                        let argument = self.emit_expr(&arg.value)?;
                        let converted = match param_type.as_str() {
                            "int64_t" => format!("lucid_as_int(lucid_wrap({argument}))"),
                            "double" => format!("lucid_as_float(lucid_wrap({argument}))"),
                            "bool" => format!("(bool)lucid_as_bool(lucid_wrap({argument}))"),
                            "const char*" => format!("lucid_as_str(lucid_wrap({argument}))"),
                            "LucidList*" => format!("lucid_as_list(lucid_wrap({argument}))"),
                            "LucidDict*" => format!("lucid_as_dict(lucid_wrap({argument}))"),
                            "LucidSet*" => format!("lucid_as_set(lucid_wrap({argument}))"),
                            _ => format!("lucid_wrap({argument})"),
                        };
                        declarations.push(format!(
                            "{param_type} lucid_var_{param_name} = {converted};"
                        ));
                        previous.push((
                            param_name.clone(),
                            self.var_types
                                .insert(param_name.clone(), param_type.clone()),
                        ));
                    }
                    let body_code = self.emit_expr(&body_expr)?;
                    for (name, old) in previous {
                        if let Some(old) = old {
                            self.var_types.insert(name, old);
                        } else {
                            self.var_types.remove(&name);
                        }
                    }
                    return Ok(format!("({{ {} {}; }})", declarations.join(" "), body_code));
                }
                if let Expr::Attribute { value, attr, .. } = &**func {
                    if let Expr::Ident { name, .. } = &**value {
                        if name == "SourceLocation" && attr == "caller" {
                            return Ok(format!("\"unknown:{}\"", span.line));
                        }
                        if name == "VarName" && attr == "from_assignment" {
                            return Ok(format!(
                                "\"{}\"",
                                self.capture_assignment.as_deref().unwrap_or("")
                            ));
                        }
                    }
                }
                // Built-in functions
                if let Expr::Ident { name, .. } = &**func {
                    match name.as_str() {
                        "print" => {
                            let mut print_parts = Vec::new();
                            let mut printed = 0usize;
                            for a in args {
                                if matches!(a.value, Expr::Skip(_)) {
                                    continue;
                                }
                                let arg_str = self.emit_expr(&a.value)?;
                                let arg_str =
                                    if self.infer_expr_type(&a.value, &HashMap::new()) == "bool" {
                                        format!("lucid_bool({arg_str})")
                                    } else {
                                        arg_str
                                    };
                                if printed > 0 {
                                    print_parts.push("printf(\" \");".to_string());
                                }
                                print_parts
                                    .push(format!("lucid_print_val(lucid_wrap({arg_str}));"));
                                printed += 1;
                            }
                            print_parts.push("printf(\"\\n\");".to_string());
                            return Ok(format!("({{ {} lucid_none(); }})", print_parts.join(" ")));
                        }
                        "getattr" | "hasattr" | "setattr" => {
                            if (name == "setattr" && args.len() != 3)
                                || (name == "hasattr" && args.len() != 2)
                                || (name == "getattr" && !(2..=3).contains(&args.len()))
                            {
                                return Err(CodegenError {
                                    message: if name == "setattr" {
                                        "setattr() takes exactly three arguments".to_string()
                                    } else {
                                        format!("{name}() takes exactly two arguments")
                                    },
                                });
                            }
                            let attr_index = 1;
                            let attr_name = match &args[attr_index].value {
                                Expr::Literal {
                                    value: LiteralValue::Str(field),
                                    ..
                                } => field,
                                _ => {
                                    let object = self.emit_expr(&args[0].value)?;
                                    let object_tmp = self.new_temp();
                                    self.emit_line(&format!(
                                        "LucidVal {object_tmp} = lucid_wrap({object});"
                                    ));
                                    let attr = self.emit_expr(&args[attr_index].value)?;
                                    let attr_tmp = self.new_temp();
                                    self.emit_line(&format!(
                                        "const char* {attr_tmp} = lucid_as_str(lucid_wrap({attr}));"
                                    ));
                                    if name == "hasattr" {
                                        return Ok(format!(
                                            "lucid_dynamic_has_attr({object_tmp}, {attr_tmp})"
                                        ));
                                    }
                                    if name == "setattr" {
                                        let value = self.emit_expr(&args[2].value)?;
                                        let value_tmp = self.new_temp();
                                        self.emit_line(&format!(
                                            "LucidVal {value_tmp} = lucid_wrap({value});"
                                        ));
                                        return Ok(format!(
                                            "(lucid_dynamic_set_attr({object_tmp}, {attr_tmp}, {value_tmp}), lucid_none())"
                                        ));
                                    }
                                    let (fallback, has_default) = if args.len() == 3 {
                                        let fallback = self.emit_expr(&args[2].value)?;
                                        let fallback_tmp = self.new_temp();
                                        self.emit_line(&format!(
                                            "LucidVal {fallback_tmp} = lucid_wrap({fallback});"
                                        ));
                                        (fallback_tmp, "true")
                                    } else {
                                        ("lucid_none()".to_string(), "false")
                                    };
                                    return Ok(format!(
                                        "lucid_dynamic_attr({object_tmp}, {attr_tmp}, {fallback}, {has_default})"
                                    ));
                                }
                            };
                            let mut object = self.emit_expr(&args[0].value)?;
                            let class_name = self
                                .infer_expr_type(&args[0].value, &HashMap::new())
                                .trim_end_matches('*')
                                .to_string();
                            if name == "getattr" && class_name == "LucidVal" {
                                let object_tmp = self.new_temp();
                                self.emit_line(&format!(
                                    "LucidVal {object_tmp} = lucid_wrap({object});"
                                ));
                                let (fallback, has_default) = if args.len() == 3 {
                                    let fallback_tmp = self.new_temp();
                                    let fallback_expr = self.emit_expr(&args[2].value)?;
                                    self.emit_line(&format!(
                                        "LucidVal {fallback_tmp} = lucid_wrap({fallback_expr});"
                                    ));
                                    (fallback_tmp, "true")
                                } else {
                                    ("lucid_none()".to_string(), "false")
                                };
                                return Ok(format!(
                                    "lucid_dynamic_attr({object_tmp}, \"{}\", {fallback}, {has_default})",
                                    c_escape_string(attr_name)
                                ));
                            }
                            if name == "hasattr" && class_name == "LucidVal" {
                                return Ok(format!(
                                    "lucid_dynamic_has_attr(lucid_wrap({object}), \"{}\")",
                                    c_escape_string(attr_name)
                                ));
                            }
                            if name == "setattr" && class_name == "LucidVal" {
                                let value = self.emit_expr(&args[2].value)?;
                                return Ok(format!(
                                    "(lucid_dynamic_set_attr(lucid_wrap({object}), \"{}\", lucid_wrap({value})), lucid_none())",
                                    c_escape_string(attr_name)
                                ));
                            }
                            if name == "setattr" && self.known_classes.contains_key(&class_name) {
                                let object_tmp = self.new_temp();
                                self.emit_line(&format!("{class_name}* {object_tmp} = {object};"));
                                object = object_tmp;
                            }
                            let mut getter_owner = Some(class_name.clone());
                            while let Some(owner) = getter_owner.clone() {
                                if self
                                    .known_getters
                                    .contains_key(&(owner.clone(), attr_name.clone()))
                                {
                                    if name == "hasattr" {
                                        return Ok("true".to_string());
                                    }
                                    if name == "getattr" {
                                        return Ok(format!(
                                            "lucid_wrap({owner}_{attr_name}_get(({owner}*)({object})))"
                                        ));
                                    }
                                }
                                getter_owner = self.known_parents.get(&owner).cloned();
                            }
                            if name == "setattr" {
                                let mut setter_owner = Some(class_name.clone());
                                while let Some(owner) = setter_owner.clone() {
                                    if self
                                        .known_setters
                                        .contains_key(&(owner.clone(), attr_name.clone()))
                                    {
                                        let value = self.emit_expr(&args[2].value)?;
                                        let setter_type = self
                                            .known_setter_types
                                            .get(&(owner.clone(), attr_name.clone()))
                                            .cloned()
                                            .unwrap_or_else(|| "LucidVal".to_string());
                                        let converted = match setter_type.as_str() {
                                            "int64_t" => {
                                                format!("lucid_as_int(lucid_wrap({value}))")
                                            }
                                            "double" => {
                                                format!("lucid_as_float(lucid_wrap({value}))")
                                            }
                                            "bool" => format!("lucid_as_bool(lucid_wrap({value}))"),
                                            "const char*" => {
                                                format!("lucid_as_str(lucid_wrap({value}))")
                                            }
                                            "LucidList*" => {
                                                format!("lucid_as_list(lucid_wrap({value}))")
                                            }
                                            "LucidDict*" => {
                                                format!("lucid_as_dict(lucid_wrap({value}))")
                                            }
                                            "LucidSet*" => {
                                                format!("lucid_as_set(lucid_wrap({value}))")
                                            }
                                            "LucidVal" => format!("lucid_wrap({value})"),
                                            other => format!(
                                                "({other})lucid_as_ptr(lucid_wrap({value}))"
                                            ),
                                        };
                                        let receiver = if owner == class_name {
                                            object.clone()
                                        } else {
                                            format!("({owner}*)({object})")
                                        };
                                        return Ok(format!(
                                            "(lucid_object_frozen((void*)({object})) ? (fprintf(stderr, \"cannot mutate frozen object\\n\"), exit(1), lucid_none()) : ({owner}_{attr_name}_set({receiver}, {converted}), lucid_none()))"
                                        ));
                                    }
                                    setter_owner = self.known_parents.get(&owner).cloned();
                                }
                            }
                            let present =
                                self.known_classes.get(&class_name).is_some_and(|fields| {
                                    fields.iter().any(|field| field == attr_name)
                                });
                            let method_present =
                                self.method_owner(&class_name, attr_name).is_some();
                            if name == "hasattr" {
                                return Ok(if present || method_present {
                                    "true"
                                } else {
                                    "false"
                                }
                                .to_string());
                            }
                            if !present {
                                if name == "getattr" && args.len() == 3 {
                                    let fallback = self.emit_expr(&args[2].value)?;
                                    return Ok(format!(
                                        "({{ (void)({object}); lucid_wrap({fallback}); }})"
                                    ));
                                }
                                return Err(CodegenError {
                                    message: format!(
                                        "unknown attribute '{attr_name}' on class '{class_name}'"
                                    ),
                                });
                            }
                            if name == "getattr" {
                                return Ok(format!("lucid_wrap(({object})->{attr_name})"));
                            }
                            let field_type = self
                                .known_field_types
                                .get(&(class_name.clone(), attr_name.clone()))
                                .cloned()
                                .unwrap_or_else(|| "LucidVal".to_string());
                            let value = self.emit_expr(&args[2].value)?;
                            let converted = match field_type.as_str() {
                                "int64_t" => format!("lucid_as_int(lucid_wrap({value}))"),
                                "double" => format!("lucid_as_float(lucid_wrap({value}))"),
                                "bool" => format!("lucid_as_bool(lucid_wrap({value}))"),
                                "const char*" => format!("lucid_as_str(lucid_wrap({value}))"),
                                "LucidList*" => format!("lucid_as_list(lucid_wrap({value}))"),
                                "LucidDict*" => format!("lucid_as_dict(lucid_wrap({value}))"),
                                "LucidSet*" => format!("lucid_as_set(lucid_wrap({value}))"),
                                "LucidVal" => format!("lucid_wrap({value})"),
                                other => format!("({other})lucid_as_ptr(lucid_wrap({value}))"),
                            };
                            self.emit_line(&format!(
                                "if (lucid_object_frozen((void*)({object}))) {{ fprintf(stderr, \"cannot mutate frozen object\\n\"); exit(1); }}"
                            ));
                            self.emit_line(&format!("({object})->{attr_name} = {converted};"));
                            return Ok("lucid_none()".to_string());
                        }
                        "fields" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "fields() takes exactly one argument".to_string(),
                                });
                            }
                            if let Expr::Ident {
                                name: class_name, ..
                            } = &args[0].value
                            {
                                if let Some(fields) =
                                    self.known_class_members.get(class_name).cloned()
                                {
                                    let list = self.new_temp();
                                    self.emit_line(&format!(
                                        "LucidList* {list} = lucid_list_new({});",
                                        fields.len()
                                    ));
                                    for field in fields {
                                        self.emit_line(&format!(
                                            "lucid_list_append({list}, lucid_str(\"{field}\"));"
                                        ));
                                    }
                                    return Ok(list);
                                }
                            }
                            let object = self.emit_expr(&args[0].value)?;
                            let class_name = self
                                .infer_expr_type(&args[0].value, &HashMap::new())
                                .trim_end_matches('*')
                                .to_string();
                            let fields = self.known_classes.get(&class_name).cloned().ok_or_else(|| CodegenError {
                                message: format!("fields() unsupported for native value of type '{class_name}'"),
                            })?;
                            let list = self.new_temp();
                            self.emit_line(&format!(
                                "LucidList* {list} = lucid_list_new({});",
                                fields.len()
                            ));
                            for field in fields {
                                self.emit_line(&format!(
                                    "lucid_list_append({list}, lucid_str(\"{field}\"));"
                                ));
                            }
                            self.emit_line(&format!("(void)({object});"));
                            return Ok(list);
                        }
                        "len" => {
                            if let Some(a) = args.first() {
                                let arg_str = self.emit_expr(&a.value)?;
                                let arg_type = self.infer_expr_type(&a.value, &HashMap::new());
                                if matches!(
                                    arg_type.as_str(),
                                    "const char*" | "char*" | "LucidList*"
                                ) {
                                    return Ok(format!("lucid_len({arg_str})"));
                                }
                                let class_name = arg_type.trim_end_matches('*');
                                if let Some(owner) = self.method_owner(class_name, "__len__") {
                                    return Ok(format!("{owner}___len__(({owner}*)({arg_str}))"));
                                }
                                return Ok(format!("lucid_len(lucid_wrap({arg_str}))"));
                            }
                        }
                        "reversed" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "reversed() takes exactly one argument".to_string(),
                                });
                            }
                            let arg_str = self.emit_expr(&args[0].value)?;
                            let arg_type = self.infer_expr_type(&args[0].value, &HashMap::new());
                            let class_name = arg_type.trim_end_matches('*');
                            if let Some(owner) = self.method_owner(class_name, "__reversed__") {
                                return Ok(format!("{owner}___reversed__(({owner}*)({arg_str}))"));
                            }
                            return Ok(format!("lucid_reversed(lucid_wrap({arg_str}))"));
                        }
                        "enumerate" => {
                            if !(1..=2).contains(&args.len()) {
                                return Err(CodegenError {
                                    message: "enumerate() takes one or two arguments".to_string(),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            let value_type = self.infer_expr_type(&args[0].value, &HashMap::new());
                            let value_class = value_type.trim_end_matches('*');
                            let value = if let Some(owner) = self.method_owner(value_class, "next")
                            {
                                self.native_drain_iterator(owner.as_str(), &value)
                            } else if let Some(owner) = self.method_owner(value_class, "__iter__") {
                                format!("{owner}___iter__(({owner}*)({value}))")
                            } else {
                                value
                            };
                            let start = if args.len() == 2 {
                                self.emit_expr(&args[1].value)?
                            } else {
                                "0LL".to_string()
                            };
                            return Ok(format!(
                                "lucid_enumerate(lucid_wrap({value}), (int64_t)({start}))"
                            ));
                        }
                        "any" | "all" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: format!("{name}() takes exactly one argument"),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            let value_type = self.infer_expr_type(&args[0].value, &HashMap::new());
                            let value_class = value_type.trim_end_matches('*');
                            let value = if let Some(owner) = self.method_owner(value_class, "next")
                            {
                                self.native_drain_iterator(owner.as_str(), &value)
                            } else if let Some(owner) = self.method_owner(value_class, "__iter__") {
                                format!("{owner}___iter__(({owner}*)({value}))")
                            } else {
                                value
                            };
                            if let Some(element_class) = self.indexed_element_class(&args[0].value)
                            {
                                if self.method_owner(&element_class, "__bool__").is_some()
                                    || self.method_owner(&element_class, "__len__").is_some()
                                {
                                    return Ok(self.native_any_all_custom(
                                        &value,
                                        &element_class,
                                        name == "any",
                                    ));
                                }
                            }
                            let helper = if name == "any" {
                                "lucid_any"
                            } else {
                                "lucid_all"
                            };
                            return Ok(format!("{helper}(lucid_wrap({value}))"));
                        }
                        "sorted" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "sorted() takes exactly one argument".to_string(),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            if let Some(element_class) = self.indexed_element_class(&args[0].value)
                            {
                                if let Some(owner) = self.method_owner(&element_class, "__lt__") {
                                    return Ok(self.native_sorted_custom(&value, &owner));
                                }
                            }
                            let value_type = self.infer_expr_type(&args[0].value, &HashMap::new());
                            let value_class = value_type.trim_end_matches('*');
                            let value = if let Some(owner) = self.method_owner(value_class, "next")
                            {
                                self.native_drain_iterator(owner.as_str(), &value)
                            } else if let Some(owner) = self.method_owner(value_class, "__iter__") {
                                format!("{owner}___iter__(({owner}*)({value}))")
                            } else {
                                value
                            };
                            return Ok(format!("lucid_sorted(lucid_wrap({value}))"));
                        }
                        "pow" => {
                            if args.len() == 3 {
                                let base = self.emit_expr(&args[0].value)?;
                                let exponent = self.emit_expr(&args[1].value)?;
                                let modulus = self.emit_expr(&args[2].value)?;
                                return Ok(format!(
                                    "lucid_pow_mod(lucid_wrap({base}), lucid_wrap({exponent}), lucid_wrap({modulus}))"
                                ));
                            }
                            if args.len() != 2 {
                                return Err(CodegenError {
                                    message: "pow() takes two or three arguments".to_string(),
                                });
                            }
                            let base = self.emit_expr(&args[0].value)?;
                            let exponent = self.emit_expr(&args[1].value)?;
                            return Ok(format!(
                                "lucid_pow(lucid_wrap({base}), lucid_wrap({exponent}))"
                            ));
                        }
                        "iter" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "iter() takes exactly one argument".to_string(),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            let value_type = self.infer_expr_type(&args[0].value, &HashMap::new());
                            let class_name = value_type.trim_end_matches('*');
                            if let Some(owner) = self.method_owner(class_name, "__iter__") {
                                return Ok(format!("{owner}___iter__(({owner}*)({value}))"));
                            }
                            return Ok(format!("lucid_iter_value(lucid_wrap({value}))"));
                        }
                        "next" => {
                            if !(1..=2).contains(&args.len()) {
                                return Err(CodegenError {
                                    message: "next() takes one or two arguments".to_string(),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            let value_type = self.infer_expr_type(&args[0].value, &HashMap::new());
                            let class_name = value_type.trim_end_matches('*');
                            if let Some(owner) = self.method_owner(class_name, "next") {
                                return Ok(format!("{owner}_next(({owner}*)({value}))"));
                            }
                            let fallback = if args.len() == 2 {
                                self.emit_expr(&args[1].value)?
                            } else {
                                "lucid_none()".to_string()
                            };
                            return Ok(format!(
                                "lucid_next_value(lucid_wrap({value}), {}, lucid_wrap({fallback}))",
                                args.len() == 2
                            ));
                        }
                        "hash" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "hash() takes exactly one argument".to_string(),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            let value_type = self.infer_expr_type(&args[0].value, &HashMap::new());
                            let class_name = value_type.trim_end_matches('*');
                            if let Some(owner) = self.method_owner(class_name, "__hash__") {
                                return Ok(format!("{owner}___hash__(({owner}*)({value}))"));
                            }
                            return Ok(format!("lucid_hash(lucid_wrap({value}))"));
                        }
                        "format" => {
                            if !(1..=2).contains(&args.len()) {
                                return Err(CodegenError {
                                    message: "format() takes one or two arguments".to_string(),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            let spec = if args.len() == 2 {
                                self.emit_expr(&args[1].value)?
                            } else {
                                "\"\"".to_string()
                            };
                            return Ok(format!(
                                "lucid_as_str(lucid_format_value(lucid_wrap({value}), {spec}))"
                            ));
                        }
                        "repr" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "repr() takes exactly one argument".to_string(),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            return Ok(format!(
                                "lucid_as_str(lucid_repr_value(lucid_wrap({value})))"
                            ));
                        }
                        "ord" | "chr" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: format!("{name}() takes exactly one argument"),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            let helper = if name == "ord" {
                                "lucid_ord_value"
                            } else {
                                "lucid_chr_value"
                            };
                            return Ok(format!("{helper}(lucid_wrap({value}))"));
                        }
                        "bin" | "oct" | "hex" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: format!("{name}() takes exactly one argument"),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            let spec = match name.as_str() {
                                "bin" => "b",
                                "oct" => "o",
                                _ => "x",
                            };
                            return Ok(format!(
                                "lucid_as_str(lucid_format_value(lucid_wrap({value}), \"{spec}\"))"
                            ));
                        }
                        "locals" => {
                            if !args.is_empty() {
                                return Err(CodegenError {
                                    message: "locals() takes no arguments".to_string(),
                                });
                            }
                            let dict = self.new_temp();
                            let bindings: Vec<(String, String)> = if self.in_function {
                                self.var_types
                                    .iter()
                                    .map(|(name, ty)| (name.clone(), ty.clone()))
                                    .collect()
                            } else {
                                self.global_vars
                                    .iter()
                                    .map(|(name, ty)| (name.clone(), ty.clone()))
                                    .collect()
                            };
                            self.emit_line(&format!(
                                "LucidDict* {dict} = lucid_dict_new({});",
                                bindings.len()
                            ));
                            for (binding, _) in bindings {
                                self.emit_line(&format!(
                                    "if (lucid_alive_{binding}) lucid_dict_set({dict}, lucid_str(\"{binding}\"), lucid_wrap(lucid_var_{binding}));"
                                ));
                            }
                            return Ok(dict);
                        }
                        "help" => {
                            if args.len() > 1 {
                                return Err(CodegenError {
                                    message: "help() takes zero or one argument".into(),
                                });
                            }
                            return Ok("lucid_none()".into());
                        }
                        "env_var" => {
                            if !(1..=2).contains(&args.len()) {
                                return Err(CodegenError {
                                    message: "env_var() takes one or two arguments".into(),
                                });
                            }
                            let name = self.emit_expr(&args[0].value)?;
                            let fallback = args
                                .get(1)
                                .map(|arg| self.emit_expr(&arg.value))
                                .transpose()?
                                .unwrap_or_else(|| "lucid_none()".into());
                            return Ok(format!(
                                "lucid_env_var(lucid_wrap({name}), {}, lucid_wrap({fallback}))",
                                args.len() == 2
                            ));
                        }
                        "read_file" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "read_file() takes exactly one argument".into(),
                                });
                            }
                            let path = self.emit_expr(&args[0].value)?;
                            return Ok(format!("lucid_read_file(lucid_wrap({path}))"));
                        }
                        "write_file" => {
                            if args.len() != 2 {
                                return Err(CodegenError {
                                    message: "write_file() takes exactly two arguments".into(),
                                });
                            }
                            let path = self.emit_expr(&args[0].value)?;
                            let contents = self.emit_expr(&args[1].value)?;
                            return Ok(format!(
                                "lucid_write_file(lucid_wrap({path}), lucid_wrap({contents}))"
                            ));
                        }
                        "slice" => {
                            if !(1..=3).contains(&args.len()) {
                                return Err(CodegenError {
                                    message: "slice() takes one to three arguments".into(),
                                });
                            }
                            let start = self.emit_expr(&args[0].value)?;
                            let stop = args
                                .get(1)
                                .map(|a| self.emit_expr(&a.value))
                                .transpose()?
                                .unwrap_or_else(|| "lucid_none()".into());
                            let step = args
                                .get(2)
                                .map(|a| self.emit_expr(&a.value))
                                .transpose()?
                                .unwrap_or_else(|| "lucid_none()".into());
                            return Ok(format!(
                                "({{ LucidDict* _slice = lucid_dict_new(3); lucid_dict_set(_slice, lucid_str(\"start\"), lucid_wrap({start})); lucid_dict_set(_slice, lucid_str(\"stop\"), lucid_wrap({stop})); lucid_dict_set(_slice, lucid_str(\"step\"), lucid_wrap({step})); _slice; }})"
                            ));
                        }
                        "zip" => {
                            let bundle = self.new_temp();
                            self.emit_line(&format!(
                                "LucidList* {bundle} = lucid_list_new({});",
                                args.len()
                            ));
                            for arg in args {
                                let value = self.emit_expr(&arg.value)?;
                                let arg_type = self.infer_expr_type(&arg.value, &HashMap::new());
                                let class_name = arg_type.trim_end_matches('*');
                                let value =
                                    if let Some(owner) = self.method_owner(class_name, "next") {
                                        self.native_drain_iterator(owner.as_str(), &value)
                                    } else if let Some(owner) =
                                        self.method_owner(class_name, "__iter__")
                                    {
                                        format!("{owner}___iter__(({owner}*)({value}))")
                                    } else {
                                        value
                                    };
                                self.emit_line(&format!(
                                    "lucid_list_append({bundle}, lucid_wrap({value}));"
                                ));
                            }
                            return Ok(format!("lucid_zip({bundle})"));
                        }
                        "map" => {
                            if args.len() != 2 {
                                return Err(CodegenError {
                                    message: "map() takes exactly two arguments".to_string(),
                                });
                            }
                            let (parameter_types, call_body) = match &args[0].value {
                                Expr::Ident {
                                    name: function_name,
                                    ..
                                } => {
                                    if let Some((specs, body_expr)) =
                                        self.anonymous_bindings.get(function_name).cloned()
                                    {
                                        if specs.len() != 1 {
                                            return Err(CodegenError {
                                                message: "native map() requires a unary function"
                                                    .into(),
                                            });
                                        }
                                        let (param_name, parameter_type) = specs
                                            .into_iter()
                                            .next()
                                            .ok_or_else(|| CodegenError {
                                                message: "native map() requires a unary function"
                                                    .into(),
                                            })?;
                                        (vec![parameter_type], Some((param_name, body_expr)))
                                    } else {
                                        let resolved = self
                                            .function_aliases
                                            .get(function_name)
                                            .map(String::as_str)
                                            .unwrap_or(function_name.as_str());
                                        let parameter_types = self
                                            .known_fn_params
                                            .get(resolved)
                                            .cloned()
                                            .ok_or_else(|| CodegenError {
                                                message: format!(
                                                    "unknown map function '{function_name}'"
                                                ),
                                            })?;
                                        if parameter_types.len() != 1 {
                                            return Err(CodegenError {
                                                message: "native map() requires a unary function"
                                                    .to_string(),
                                            });
                                        }
                                        (parameter_types, None)
                                    }
                                }
                                Expr::AnonymousDef { params, body, .. } => {
                                    if params.len() != 1 {
                                        return Err(CodegenError {
                                            message: "native map() requires a unary function"
                                                .to_string(),
                                        });
                                    }
                                    let expr = match body.as_slice() {
                                        [
                                            Stmt::Return {
                                                value: Some(expr), ..
                                            },
                                        ] => expr.clone(),
                                        _ => {
                                            return Err(CodegenError {
                                                message: "native map() supports only an expression-bodied anonymous function".to_string(),
                                            });
                                        }
                                    };
                                    let parameter_type =
                                        self.map_type_expr(params[0].type_annotation.as_ref());
                                    (vec![parameter_type], Some((params[0].name.clone(), expr)))
                                }
                                _ => {
                                    return Err(CodegenError {
                                        message: "native map() requires a named or expression-bodied anonymous unary function".to_string(),
                                    });
                                }
                            };
                            let source = self.emit_expr(&args[1].value)?;
                            let source_type = self.infer_expr_type(&args[1].value, &HashMap::new());
                            let source_class = source_type.trim_end_matches('*');
                            let source = if let Some(owner) =
                                self.method_owner(source_class, "next")
                            {
                                self.native_drain_iterator(owner.as_str(), &source)
                            } else if let Some(owner) = self.method_owner(source_class, "__iter__")
                            {
                                format!("{owner}___iter__(({owner}*)({source}))")
                            } else {
                                source
                            };
                            let source_tmp = self.new_temp();
                            let result_tmp = self.new_temp();
                            let index_tmp = self.new_temp();
                            self.emit_line(&format!(
                                "LucidList* {source_tmp} = lucid_list_from_value(lucid_wrap({source}));"
                            ));
                            self.emit_line(&format!(
                                "LucidList* {result_tmp} = lucid_list_new({source_tmp} ? {source_tmp}->len : 0);"
                            ));
                            self.emit_line(&format!(
                                "for (int64_t {index_tmp} = 0; {source_tmp} && {index_tmp} < {source_tmp}->len; ++{index_tmp}) {{"
                            ));
                            self.indent += 1;
                            let argument = match parameter_types[0].as_str() {
                                "int64_t" => {
                                    format!("lucid_as_int({source_tmp}->items[{index_tmp}])")
                                }
                                "double" => {
                                    format!("lucid_as_float({source_tmp}->items[{index_tmp}])")
                                }
                                "bool" => {
                                    format!("lucid_as_bool({source_tmp}->items[{index_tmp}])")
                                }
                                "const char*" => {
                                    format!("lucid_as_str({source_tmp}->items[{index_tmp}])")
                                }
                                "LucidList*" => {
                                    format!("lucid_as_list({source_tmp}->items[{index_tmp}])")
                                }
                                "LucidDict*" => {
                                    format!("lucid_as_dict({source_tmp}->items[{index_tmp}])")
                                }
                                "LucidSet*" => {
                                    format!("lucid_as_set({source_tmp}->items[{index_tmp}])")
                                }
                                _ => format!("{source_tmp}->items[{index_tmp}]"),
                            };
                            let call = if let Some((param_name, body_expr)) = &call_body {
                                let param_type = &parameter_types[0];
                                let binding = format!("lucid_var_{param_name}");
                                let converted = match param_type.as_str() {
                                    "int64_t" => {
                                        format!("lucid_as_int({source_tmp}->items[{index_tmp}])")
                                    }
                                    "double" => {
                                        format!("lucid_as_float({source_tmp}->items[{index_tmp}])")
                                    }
                                    "bool" => format!(
                                        "(bool)lucid_as_bool({source_tmp}->items[{index_tmp}])"
                                    ),
                                    "const char*" => {
                                        format!("lucid_as_str({source_tmp}->items[{index_tmp}])")
                                    }
                                    "LucidList*" => {
                                        format!("lucid_as_list({source_tmp}->items[{index_tmp}])")
                                    }
                                    "LucidDict*" => {
                                        format!("lucid_as_dict({source_tmp}->items[{index_tmp}])")
                                    }
                                    "LucidSet*" => {
                                        format!("lucid_as_set({source_tmp}->items[{index_tmp}])")
                                    }
                                    _ => format!("{source_tmp}->items[{index_tmp}]"),
                                };
                                self.emit_line(&format!("{param_type} {binding} = {converted};"));
                                let previous = self
                                    .var_types
                                    .insert(param_name.clone(), param_type.clone());
                                let body = self.emit_expr(body_expr)?;
                                if let Some(previous) = previous {
                                    self.var_types.insert(param_name.clone(), previous);
                                } else {
                                    self.var_types.remove(param_name);
                                }
                                body
                            } else {
                                let function_name = match &args[0].value {
                                    Expr::Ident { name, .. } => name,
                                    _ => {
                                        return Err(CodegenError {
                                            message: "function reference must be an identifier"
                                                .to_string(),
                                        });
                                    }
                                };
                                let function_name = self
                                    .function_aliases
                                    .get(function_name)
                                    .map(String::as_str)
                                    .unwrap_or(function_name.as_str());
                                format!("lucid_fn_{function_name}({argument})")
                            };
                            self.emit_line(&format!(
                                "lucid_list_append({result_tmp}, lucid_wrap({call}));"
                            ));
                            self.indent -= 1;
                            self.emit_line("}");
                            return Ok(result_tmp);
                        }
                        "time" | "monotonic" => {
                            if !args.is_empty() {
                                return Err(CodegenError {
                                    message: format!("{name}() takes no arguments"),
                                });
                            }
                            return Ok("lucid_time_now()".to_string());
                        }
                        "abs" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "abs() takes exactly one argument".to_string(),
                                });
                            }
                            let arg_str = self.emit_expr(&args[0].value)?;
                            return Ok(format!("lucid_abs_value(lucid_wrap({arg_str}))"));
                        }
                        "sqrt" | "sin" | "cos" | "tan" | "floor" | "ceil" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: format!("{name}() takes exactly one argument"),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            let numeric = format!("lucid_as_float(lucid_wrap({value}))");
                            return Ok(match name.as_str() {
                                "sqrt" => format!("sqrt({numeric})"),
                                "sin" => format!("sin({numeric})"),
                                "cos" => format!("cos({numeric})"),
                                "tan" => format!("tan({numeric})"),
                                "floor" => format!("floor({numeric})"),
                                _ => format!("ceil({numeric})"),
                            });
                        }
                        "round" => {
                            if args.len() > 2 {
                                return Err(CodegenError {
                                    message: "round() takes one or two arguments".to_string(),
                                });
                            }
                            if args.len() == 1 {
                                let arg_str = self.emit_expr(&args[0].value)?;
                                return Ok(format!(
                                    "lucid_round_to_int(lucid_as_float(lucid_wrap({arg_str})))"
                                ));
                            } else if args.len() >= 2 {
                                let arg0 = self.emit_expr(&args[0].value)?;
                                let arg1 = self.emit_expr(&args[1].value)?;
                                if self.infer_expr_type(&args[0].value, &HashMap::new())
                                    == "int64_t"
                                {
                                    return Ok(format!("lucid_as_int(lucid_wrap({arg0}))"));
                                }
                                return Ok(format!(
                                    "({{ LucidVal _round_value = lucid_wrap({arg0}); LucidVal _round_places = lucid_wrap({arg1}); if (_round_places.type != LUCID_TYPE_INT) {{ fprintf(stderr, \"round() ndigits must be an int\\n\"); exit(1); }} lucid_round_places(lucid_as_float(_round_value), _round_places.i); }})"
                                ));
                            }
                        }
                        "int" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "int() takes exactly one argument".into(),
                                });
                            }
                            if let Some(a) = args.first() {
                                let arg_str = self.emit_expr(&a.value)?;
                                if let Expr::Literal {
                                    value: LiteralValue::BigInt(value),
                                    ..
                                } = &a.value
                                {
                                    return Ok(format!(
                                        "lucid_bigint(\"{}\")",
                                        bigint_literal_decimal(value)
                                    ));
                                }
                                let arg_type = self.infer_expr_type(&a.value, &HashMap::new());
                                if matches!(arg_type.as_str(), "LucidVal" | "const char*") {
                                    return Ok(format!("lucid_int_dynamic(lucid_wrap({arg_str}))"));
                                }
                                return Ok(format!("lucid_int_builtin(lucid_wrap({arg_str}))"));
                            }
                        }
                        "bool" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "bool() takes exactly one argument".into(),
                                });
                            }
                            return Ok(format!(
                                "((bool)({}))",
                                self.emit_condition(&args[0].value)?
                            ));
                        }
                        "float" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "float() takes exactly one argument".into(),
                                });
                            }
                            if let Some(a) = args.first() {
                                let arg_str = self.emit_expr(&a.value)?;
                                return Ok(format!("lucid_float_builtin(lucid_wrap({arg_str}))"));
                            }
                        }
                        "complex" => {
                            if args.len() > 2 {
                                return Err(CodegenError {
                                    message: "complex() takes at most two arguments".to_string(),
                                });
                            }
                            let real = if args.is_empty() {
                                "lucid_int(0)".to_string()
                            } else {
                                format!("lucid_wrap({})", self.emit_expr(&args[0].value)?)
                            };
                            let imag = if args.len() == 2 {
                                format!("lucid_wrap({})", self.emit_expr(&args[1].value)?)
                            } else {
                                "lucid_int(0)".to_string()
                            };
                            return Ok(format!(
                                "lucid_complex_from_value({real}, {}, {imag})",
                                args.len() == 2
                            ));
                        }
                        "sum" => {
                            if let Some(a) = args.first() {
                                let arg_str = self.emit_expr(&a.value)?;
                                if args.len() > 2 {
                                    return Err(CodegenError {
                                        message: "sum() takes one or two arguments".into(),
                                    });
                                }
                                let start_code = args
                                    .get(1)
                                    .map(|arg| self.emit_expr(&arg.value))
                                    .transpose()?;
                                let is_bigint_list = matches!(&a.value, Expr::List { elements, .. } if elements.iter().any(|element| self.expr_is_bigint(element)));
                                let is_float_list = matches!(&a.value, Expr::List { elements, .. } if elements.iter().any(|element| matches!(element, Expr::Literal { value: LiteralValue::Float(_), .. })));
                                let arg_type = self.infer_expr_type(&a.value, &HashMap::new());
                                let class_name = arg_type.trim_end_matches('*');
                                if let Some(owner) = self.method_owner(class_name, "next") {
                                    let total = format!(
                                        "lucid_sum_value(lucid_wrap({}))",
                                        self.native_drain_iterator(owner.as_str(), &arg_str)
                                    );
                                    return Ok(if let Some(start) = start_code.clone() {
                                        format!("(lucid_as_int(lucid_wrap({start})) + {total})")
                                    } else {
                                        total
                                    });
                                }
                                if let Some(owner) = self.method_owner(class_name, "__iter__") {
                                    let total = format!(
                                        "lucid_sum_value(lucid_wrap({owner}___iter__(({owner}*)({arg_str}))))"
                                    );
                                    return Ok(if let Some(start) = start_code.clone() {
                                        format!("(lucid_as_int(lucid_wrap({start})) + {total})")
                                    } else {
                                        total
                                    });
                                }
                                return Ok(if is_bigint_list {
                                    let total =
                                        format!("lucid_sum_bigint_value(lucid_wrap({arg_str}))");
                                    if let Some(start) = start_code {
                                        format!(
                                            "lucid_bigint_binop(lucid_wrap({start}), {total}, '+')"
                                        )
                                    } else {
                                        total
                                    }
                                } else if is_float_list {
                                    let total =
                                        format!("lucid_sum_float_value(lucid_wrap({arg_str}))");
                                    if let Some(start) = start_code {
                                        format!("(lucid_as_float(lucid_wrap({start})) + {total})")
                                    } else {
                                        total
                                    }
                                } else {
                                    let total = format!("lucid_sum_value(lucid_wrap({arg_str}))");
                                    if let Some(start) = start_code {
                                        format!("(lucid_as_int(lucid_wrap({start})) + {total})")
                                    } else {
                                        total
                                    }
                                });
                            }
                        }
                        "min" | "max" => {
                            if let Some(a) = args.first() {
                                let arg_str = self.emit_expr(&a.value)?;
                                let arg_type = self.infer_expr_type(&a.value, &HashMap::new());
                                let arg_class = arg_type.trim_end_matches('*');
                                let arg_str = if let Some(owner) =
                                    self.method_owner(arg_class, "next")
                                {
                                    self.native_drain_iterator(owner.as_str(), &arg_str)
                                } else if let Some(owner) = self.method_owner(arg_class, "__iter__")
                                {
                                    format!("{owner}___iter__(({owner}*)({arg_str}))")
                                } else {
                                    arg_str
                                };
                                if let Some(element_class) = self.indexed_element_class(&a.value) {
                                    if let Some(owner) = self.method_owner(&element_class, "__lt__")
                                    {
                                        return Ok(self.native_min_max_custom(
                                            &arg_str,
                                            &owner,
                                            name == "min",
                                        ));
                                    }
                                }
                                return Ok(format!(
                                    "{}(lucid_wrap({arg_str}))",
                                    if name == "min" {
                                        "lucid_min_value"
                                    } else {
                                        "lucid_max_value"
                                    }
                                ));
                            }
                        }
                        "freeze" => {
                            if let Some(a) = args.first() {
                                let code = self.emit_expr(&a.value)?;
                                let ty = self.infer_expr_type(&a.value, &HashMap::new());
                                return Ok(format!(
                                    "({{ {ty} _freeze_value = {code}; lucid_freeze_value(lucid_wrap(_freeze_value)); _freeze_value; }})"
                                ));
                            }
                        }
                        "list" => {
                            if args.is_empty() {
                                return Ok("lucid_list_new(0)".to_string());
                            }
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "list() takes zero or one argument".to_string(),
                                });
                            }
                            if let Some(a) = args.first() {
                                if let Expr::Call {
                                    func: inner_func,
                                    args: inner_args,
                                    ..
                                } = &a.value
                                {
                                    if let Expr::Ident {
                                        name: inner_name, ..
                                    } = &**inner_func
                                    {
                                        if inner_name == "range" {
                                            let (start, stop, step) = match inner_args.len() {
                                                1 => (
                                                    "0LL".to_string(),
                                                    self.emit_expr(&inner_args[0].value)?,
                                                    "1LL".to_string(),
                                                ),
                                                2 => (
                                                    self.emit_expr(&inner_args[0].value)?,
                                                    self.emit_expr(&inner_args[1].value)?,
                                                    "1LL".to_string(),
                                                ),
                                                3 => (
                                                    self.emit_expr(&inner_args[0].value)?,
                                                    self.emit_expr(&inner_args[1].value)?,
                                                    self.emit_expr(&inner_args[2].value)?,
                                                ),
                                                _ => (
                                                    "0LL".to_string(),
                                                    "0LL".to_string(),
                                                    "1LL".to_string(),
                                                ),
                                            };
                                            return Ok(format!(
                                                "lucid_range_to_list({start}, {stop}, {step})"
                                            ));
                                        }
                                    }
                                }
                                let arg_str = self.emit_expr(&a.value)?;
                                let arg_type = self.infer_expr_type(&a.value, &HashMap::new());
                                let class_name = arg_type.trim_end_matches('*');
                                if let Some(owner) = self.method_owner(class_name, "next") {
                                    return Ok(self.native_drain_iterator(&owner, &arg_str));
                                }
                                if let Some(owner) = self.method_owner(class_name, "__iter__") {
                                    return Ok(format!(
                                        "lucid_list_from_value(lucid_wrap({owner}___iter__(({owner}*)({arg_str}))))"
                                    ));
                                }
                                return Ok(format!("lucid_list_from_value(lucid_wrap({arg_str}))"));
                            }
                        }
                        "set" => {
                            if args.is_empty() {
                                return Ok("lucid_set_new(0)".to_string());
                            }
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "set() takes zero or one argument".to_string(),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            let value_type = self.infer_expr_type(&args[0].value, &HashMap::new());
                            let class_name = value_type.trim_end_matches('*');
                            if let Some(owner) = self.method_owner(class_name, "next") {
                                return Ok(format!(
                                    "lucid_set_from_value(lucid_wrap({}))",
                                    self.native_drain_iterator(&owner, &value)
                                ));
                            }
                            if let Some(owner) = self.method_owner(class_name, "__iter__") {
                                return Ok(format!(
                                    "lucid_set_from_value(lucid_wrap({owner}___iter__(({owner}*)({value}))))"
                                ));
                            }
                            return Ok(format!("lucid_set_from_value(lucid_wrap({value}))"));
                        }
                        "dict" => {
                            if args.is_empty() {
                                return Ok("lucid_dict_new(0)".to_string());
                            }
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "dict() takes zero or one argument".to_string(),
                                });
                            }
                            let value = self.emit_expr(&args[0].value)?;
                            let value_type = self.infer_expr_type(&args[0].value, &HashMap::new());
                            let class_name = value_type.trim_end_matches('*');
                            if let Some(owner) = self.method_owner(class_name, "next") {
                                return Ok(format!(
                                    "lucid_dict_from_value(lucid_wrap({}))",
                                    self.native_drain_iterator(&owner, &value)
                                ));
                            }
                            if let Some(owner) = self.method_owner(class_name, "__iter__") {
                                return Ok(format!(
                                    "lucid_dict_from_value(lucid_wrap({owner}___iter__(({owner}*)({value}))))"
                                ));
                            }
                            return Ok(format!("lucid_dict_from_value(lucid_wrap({value}))"));
                        }
                        "range" => {
                            let (start, stop, step) = match args.len() {
                                1 => (
                                    "0LL".to_string(),
                                    self.emit_expr(&args[0].value)?,
                                    "1LL".to_string(),
                                ),
                                2 => (
                                    self.emit_expr(&args[0].value)?,
                                    self.emit_expr(&args[1].value)?,
                                    "1LL".to_string(),
                                ),
                                3 => (
                                    self.emit_expr(&args[0].value)?,
                                    self.emit_expr(&args[1].value)?,
                                    self.emit_expr(&args[2].value)?,
                                ),
                                _ => {
                                    return Err(CodegenError {
                                        message: "range() takes one to three arguments".to_string(),
                                    });
                                }
                            };
                            return Ok(format!("lucid_range_to_list({start}, {stop}, {step})"));
                        }
                        "str" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "str() takes exactly one argument".into(),
                                });
                            }
                            if let Some(a) = args.first() {
                                let value = self.emit_expr(&a.value)?;
                                return Ok(format!("lucid_to_str(lucid_wrap({value}))"));
                            }
                        }
                        "bytes" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "bytes() takes exactly one argument".into(),
                                });
                            }
                            if let Some(a) = args.first() {
                                let value = self.emit_expr(&a.value)?;
                                return Ok(format!("lucid_bytes(lucid_wrap({value}))"));
                            }
                        }
                        "bytearray" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "bytearray() takes exactly one argument".into(),
                                });
                            }
                            if let Some(a) = args.first() {
                                let value = self.emit_expr(&a.value)?;
                                return Ok(format!("lucid_bytearray(lucid_wrap({value}))"));
                            }
                        }
                        "memoryview" => {
                            if args.len() != 1 {
                                return Err(CodegenError {
                                    message: "memoryview() takes exactly one argument".into(),
                                });
                            }
                            if let Some(a) = args.first() {
                                return self.emit_expr(&a.value);
                            }
                        }
                        _ => {}
                    }

                    // Class constructor: Node(...) or RandomGen(...)
                    if let Some(field_names) = self.known_classes.get(name).cloned() {
                        let mut arg_map = HashMap::new();
                        for (i, a) in args.iter().enumerate() {
                            if let Some(arg_name) = &a.name {
                                arg_map.insert(arg_name.clone(), &a.value);
                            } else if i < field_names.len() {
                                arg_map.insert(field_names[i].clone(), &a.value);
                            }
                        }

                        let mut c_args = Vec::new();
                        for fname in &field_names {
                            if let Some(val_expr) = arg_map.get(fname) {
                                if is_none_expr(val_expr) {
                                    c_args.push("NULL".to_string());
                                } else {
                                    c_args.push(self.emit_expr(val_expr)?);
                                }
                            } else if let Some(Some(default)) = self
                                .known_field_defaults
                                .get(&(name.clone(), fname.clone()))
                                .cloned()
                            {
                                c_args.push(self.emit_expr(&default)?);
                            } else {
                                c_args.push("NULL".to_string());
                            }
                        }
                        return Ok(format!("{name}_new({})", c_args.join(", ")));
                    }

                    // User function call
                    // A runtime list spread can still target a statically
                    // known fixed-arity function.  Materialize each element
                    // at the call boundary and coerce it according to the
                    // callee's declared parameter type.  This is the native
                    // equivalent of `f(*values)`; bundle (`***`) and mapping
                    // spreads remain handled by their dedicated ABI path.
                    let resolved_name = self
                        .function_aliases
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| name.clone());
                    // A `***rest: Arguments|Parameters` parameter gathers
                    // every argument that does not fill the function's
                    // ordinary fixed prefix.  Materialize that bundle at
                    // the native call boundary; forwarding an existing
                    // bundle is handled by the branch below.
                    if let Some((gather_index, bundle_name)) =
                        self.known_fn_gather.get(&resolved_name).cloned()
                    {
                        let is_role_bundle = bundle_name == "Arguments"
                            || bundle_name == "Parameters"
                            || bundle_name.ends_with("Arguments")
                            || bundle_name.ends_with("Parameters");
                        if is_role_bundle {
                            let param_names = self
                                .known_fn_param_names
                                .get(&resolved_name)
                                .cloned()
                                .unwrap_or_default();
                            let mut fixed: Vec<Option<&Arg>> = vec![None; gather_index];
                            let mut positional = 0usize;
                            let mut rest: Vec<&Arg> = Vec::new();
                            for arg in args
                                .iter()
                                .filter(|arg| !matches!(arg.value, Expr::Skip(_)))
                            {
                                if arg.is_spread || arg.is_dict_spread || arg.is_gather_spread {
                                    return Err(CodegenError { message: "native gather calls require statically materialized arguments".into() });
                                }
                                if let Some(arg_name) = &arg.name {
                                    if let Some(index) = param_names
                                        .iter()
                                        .take(gather_index)
                                        .position(|name| name == arg_name)
                                    {
                                        fixed[index] = Some(arg);
                                    } else {
                                        rest.push(arg);
                                    }
                                } else if positional < gather_index {
                                    fixed[positional] = Some(arg);
                                    positional += 1;
                                } else {
                                    rest.push(arg);
                                }
                            }
                            let defaults = self
                                .known_fn_defaults
                                .get(&resolved_name)
                                .cloned()
                                .unwrap_or_default();
                            let mut fixed_code = Vec::new();
                            for index in 0..gather_index {
                                if let Some(arg) = fixed[index] {
                                    fixed_code.push(self.emit_expr(&arg.value)?);
                                } else if let Some(Some(default)) = defaults.get(index) {
                                    fixed_code.push(self.emit_expr(default)?);
                                } else {
                                    fixed_code.push("0".into());
                                }
                            }
                            let positional_rest = rest
                                .iter()
                                .filter(|arg| arg.name.is_none())
                                .map(|arg| self.emit_expr(&arg.value))
                                .collect::<Result<Vec<_>, _>>()?;
                            let keyword_rest = rest
                                .iter()
                                .filter_map(|arg| {
                                    arg.name.as_ref().map(|key| {
                                        let value = self.emit_expr(&arg.value)?;
                                        Ok::<String, CodegenError>(format!("lucid_dict_set(_kwargs, lucid_str(\"{}\"), lucid_wrap({value}));", c_escape_string(key)))
                                    })
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            let pargs = "lucid_list_new(0)";
                            let vpargs = format!(
                                "({{ LucidList* _vpargs = lucid_list_new({}); {} _vpargs; }})",
                                positional_rest.len(),
                                positional_rest
                                    .iter()
                                    .map(|value| format!(
                                        "lucid_list_append(_vpargs, lucid_wrap({value}));"
                                    ))
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            );
                            let kwargs = format!(
                                "({{ LucidDict* _kwargs = lucid_dict_new({}); {} _kwargs; }})",
                                keyword_rest.len(),
                                keyword_rest.join(" ")
                            );
                            let fields = self
                                .known_classes
                                .get(&bundle_name)
                                .cloned()
                                .unwrap_or_else(|| vec!["vpargs".into(), "kwargs".into()]);
                            let bundle_args = fields
                                .iter()
                                .map(|field| match field.as_str() {
                                    "pargs" => pargs.to_string(),
                                    "vpargs" => vpargs.clone(),
                                    "kwargs" => kwargs.clone(),
                                    _ => "NULL".into(),
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            let bundle = format!("{bundle_name}_new({bundle_args})");
                            let fn_name = self
                                .dispatch_fns
                                .get(&resolved_name)
                                .and_then(|candidates| {
                                    candidates.first().map(|(_, emitted)| emitted.clone())
                                })
                                .unwrap_or_else(|| format!("lucid_fn_{resolved_name}"));
                            fixed_code.push(bundle);
                            return Ok(format!("{fn_name}({})", fixed_code.join(", ")));
                        }
                    }
                    if self.known_fn_keyword_variadic.contains(&resolved_name)
                        && self.known_fn_variadic.get(&resolved_name) == Some(&0)
                        && self
                            .known_fn_params
                            .get(&resolved_name)
                            .is_some_and(|params| params.len() == 1)
                    {
                        let entries = args
                            .iter()
                            .filter(|arg| {
                                arg.name.is_some()
                                    && !arg.is_spread
                                    && !arg.is_dict_spread
                                    && !arg.is_gather_spread
                            })
                            .filter_map(|arg| {
                                arg.name.as_ref().map(|key| {
                                    self.emit_expr(&arg.value).map(|value| {
                                        format!("lucid_dict_set(_kwargs, lucid_str(\"{}\"), lucid_wrap({value}));", c_escape_string(key))
                                    })
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        if entries.len()
                            != args
                                .iter()
                                .filter(|arg| !matches!(arg.value, Expr::Skip(_)))
                                .count()
                        {
                            return Err(CodegenError {
                                message: "native **kwargs calls require named arguments".into(),
                            });
                        }
                        let packed = format!(
                            "({{ LucidDict* _kwargs = lucid_dict_new({}); {} _kwargs; }})",
                            entries.len(),
                            entries.join(" ")
                        );
                        let fn_name = self
                            .dispatch_fns
                            .get(&resolved_name)
                            .and_then(|candidates| {
                                candidates.first().map(|(_, emitted)| emitted.clone())
                            })
                            .unwrap_or_else(|| format!("lucid_fn_{resolved_name}"));
                        return Ok(format!("{fn_name}({packed})"));
                    }
                    if self.known_fn_keyword_variadic.contains(&resolved_name)
                        && let Some(&variadic_index) = self.known_fn_variadic.get(&resolved_name)
                        && variadic_index > 0
                        && self
                            .known_fn_params
                            .get(&resolved_name)
                            .is_some_and(|params| params.len() == variadic_index + 1)
                        && args.len() >= variadic_index
                    {
                        let fixed = args
                            .iter()
                            .take(variadic_index)
                            .map(|arg| self.emit_expr(&arg.value))
                            .collect::<Result<Vec<_>, _>>()?;
                        let named = args.iter().skip(variadic_index).map(|arg| {
                            let key = arg.name.as_ref().ok_or_else(|| CodegenError { message: "mixed **kwargs calls require named trailing arguments".into() })?;
                            let value = self.emit_expr(&arg.value)?;
                            Ok::<String, CodegenError>(format!("lucid_dict_set(_kwargs, lucid_str(\"{}\"), lucid_wrap({value}));", c_escape_string(key)))
                        }).collect::<Result<Vec<_>, _>>()?;
                        let packed = format!(
                            "({{ LucidDict* _kwargs = lucid_dict_new({}); {} _kwargs; }})",
                            named.len(),
                            named.join(" ")
                        );
                        let fn_name = self
                            .dispatch_fns
                            .get(&resolved_name)
                            .and_then(|candidates| {
                                candidates.first().map(|(_, emitted)| emitted.clone())
                            })
                            .unwrap_or_else(|| format!("lucid_fn_{resolved_name}"));
                        return Ok(format!("{fn_name}({}, {packed})", fixed.join(", ")));
                    }
                    if self.known_fn_variadic.get(&resolved_name) == Some(&0)
                        && self
                            .known_fn_params
                            .get(&resolved_name)
                            .is_some_and(|params| params.len() == 1)
                    {
                        let values = args
                            .iter()
                            .filter(|arg| !matches!(arg.value, Expr::Skip(_)))
                            .map(|arg| self.emit_expr(&arg.value))
                            .collect::<Result<Vec<_>, _>>()?;
                        let packed = format!(
                            "({{ LucidList* _vargs = lucid_list_new({}); {} _vargs; }})",
                            values.len(),
                            values
                                .iter()
                                .map(|value| format!(
                                    "lucid_list_append(_vargs, lucid_wrap({value}));"
                                ))
                                .collect::<Vec<_>>()
                                .join(" ")
                        );
                        let fn_name = self
                            .dispatch_fns
                            .get(&resolved_name)
                            .and_then(|candidates| {
                                candidates.first().map(|(_, emitted)| emitted.clone())
                            })
                            .unwrap_or_else(|| format!("lucid_fn_{resolved_name}"));
                        return Ok(format!("{fn_name}({packed})"));
                    }
                    if let Some(&variadic_index) = self.known_fn_variadic.get(&resolved_name) {
                        if variadic_index > 0
                            && args.iter().all(|arg| {
                                arg.name.is_none()
                                    && !arg.is_spread
                                    && !arg.is_dict_spread
                                    && !arg.is_gather_spread
                            })
                            && args.len() >= variadic_index
                            && self
                                .known_fn_params
                                .get(&resolved_name)
                                .is_some_and(|params| params.len() == variadic_index + 1)
                        {
                            let fixed = args
                                .iter()
                                .take(variadic_index)
                                .map(|arg| self.emit_expr(&arg.value))
                                .collect::<Result<Vec<_>, _>>()?;
                            let rest = args
                                .iter()
                                .skip(variadic_index)
                                .map(|arg| self.emit_expr(&arg.value))
                                .collect::<Result<Vec<_>, _>>()?;
                            let packed = format!(
                                "({{ LucidList* _vargs = lucid_list_new({}); {} _vargs; }})",
                                rest.len(),
                                rest.iter()
                                    .map(|value| format!(
                                        "lucid_list_append(_vargs, lucid_wrap({value}));"
                                    ))
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            );
                            let fn_name = self
                                .dispatch_fns
                                .get(&resolved_name)
                                .and_then(|candidates| {
                                    candidates.first().map(|(_, emitted)| emitted.clone())
                                })
                                .unwrap_or_else(|| format!("lucid_fn_{resolved_name}"));
                            return Ok(format!("{fn_name}({}, {packed})", fixed.join(", ")));
                        }
                    }
                    if args.len() == 1
                        && args[0].is_spread
                        && !args[0].is_dict_spread
                        && !args[0].is_gather_spread
                    {
                        if !matches!(&args[0].value, Expr::List { .. }) {
                            if let Some(param_types) =
                                self.known_fn_params.get(&resolved_name).cloned()
                            {
                                let spread = self.emit_expr(&args[0].value)?;
                                let list = format!("lucid_as_list(lucid_wrap({spread}))");
                                let convert = |item: String, ty: &str| -> String {
                                    match ty {
                                        "int64_t" => format!("lucid_as_int({item})"),
                                        "double" => format!("lucid_as_float({item})"),
                                        "bool" => format!("lucid_as_bool({item})"),
                                        "const char*" => format!("lucid_as_str({item})"),
                                        "LucidList*" => format!("lucid_as_list({item})"),
                                        "LucidVal" => item,
                                        other => format!("({other})lucid_as_ptr({item})"),
                                    }
                                };
                                let rendered = param_types
                                    .iter()
                                    .enumerate()
                                    .map(|(index, ty)| {
                                        convert(format!("lucid_list_get({list}, {index}LL)"), ty)
                                    })
                                    .collect::<Vec<_>>();
                                let fn_name = if let Some(candidates) =
                                    self.dispatch_fns.get(&resolved_name)
                                {
                                    candidates
                                        .first()
                                        .map(|(_, emitted)| emitted.clone())
                                        .unwrap_or_else(|| format!("lucid_fn_{resolved_name}"))
                                } else {
                                    format!("lucid_fn_{resolved_name}")
                                };
                                return Ok(format!("{fn_name}({})", rendered.join(", ")));
                            }
                        }
                    }
                    if args.len() == 1
                        && args[0].is_dict_spread
                        && !args[0].is_spread
                        && !args[0].is_gather_spread
                    {
                        if !matches!(&args[0].value, Expr::Dict { .. }) {
                            if let Some(param_types) =
                                self.known_fn_params.get(&resolved_name).cloned()
                            {
                                let param_names = self
                                    .known_fn_param_names
                                    .get(&resolved_name)
                                    .cloned()
                                    .unwrap_or_default();
                                let mapping = self.emit_expr(&args[0].value)?;
                                let dict = format!("lucid_as_dict(lucid_wrap({mapping}))");
                                let convert = |item: String, ty: &str| -> String {
                                    match ty {
                                        "int64_t" => format!("lucid_as_int({item})"),
                                        "double" => format!("lucid_as_float({item})"),
                                        "bool" => format!("lucid_as_bool({item})"),
                                        "const char*" => format!("lucid_as_str({item})"),
                                        "LucidList*" => format!("lucid_as_list({item})"),
                                        "LucidVal" => item,
                                        other => format!("({other})lucid_as_ptr({item})"),
                                    }
                                };
                                let rendered = param_types
                                    .iter()
                                    .enumerate()
                                    .map(|(index, ty)| {
                                        let key = param_names.get(index).cloned().unwrap_or_else(|| index.to_string());
                                        convert(format!("lucid_dict_get({dict}, lucid_str(\"{}\"), lucid_none())", key.replace('"', "\\\"")), ty)
                                    })
                                    .collect::<Vec<_>>();
                                let fn_name = if let Some(candidates) =
                                    self.dispatch_fns.get(&resolved_name)
                                {
                                    candidates
                                        .first()
                                        .map(|(_, emitted)| emitted.clone())
                                        .unwrap_or_else(|| format!("lucid_fn_{resolved_name}"))
                                } else {
                                    format!("lucid_fn_{resolved_name}")
                                };
                                return Ok(format!("{fn_name}({})", rendered.join(", ")));
                            }
                        }
                    }
                    // A bundle can be forwarded without unpacking when the
                    // callee explicitly gathers it.  Both sides then use the
                    // same generated class representation and the call is a
                    // normal typed pointer pass.
                    if args.len() == 1 && args[0].is_gather_spread {
                        if let Some(param_types) = self.known_fn_params.get(&resolved_name).cloned()
                        {
                            if param_types.len() == 1 {
                                let value = self.emit_expr(&args[0].value)?;
                                let fn_name = format!("lucid_fn_{resolved_name}");
                                return Ok(format!("{fn_name}({value})"));
                            }
                            let bundle_ty = self.infer_expr_type(&args[0].value, &HashMap::new());
                            let bundle_name = bundle_ty.trim_end_matches('*');
                            let is_bundle = bundle_name == "Arguments"
                                || bundle_name == "Parameters"
                                || bundle_name.ends_with("Arguments")
                                || bundle_name.ends_with("Parameters");
                            if is_bundle {
                                let value = self.emit_expr(&args[0].value)?;
                                let object =
                                    format!("(({bundle_name}*)lucid_as_ptr(lucid_wrap({value})))");
                                let names = self
                                    .known_fn_param_names
                                    .get(&resolved_name)
                                    .cloned()
                                    .unwrap_or_default();
                                let defaults = self
                                    .known_fn_defaults
                                    .get(&resolved_name)
                                    .cloned()
                                    .unwrap_or_default();
                                let has_pargs = self
                                    .known_classes
                                    .get(bundle_name)
                                    .map(|fields| fields.iter().any(|field| field == "pargs"))
                                    .unwrap_or(false);
                                let pargs_prefix = if has_pargs { 1usize } else { 0usize };
                                let convert = |item: String, ty: &str| -> String {
                                    match ty {
                                        "int64_t" => format!("lucid_as_int({item})"),
                                        "double" => format!("lucid_as_float({item})"),
                                        "bool" => format!("lucid_as_bool({item})"),
                                        "const char*" => format!("lucid_as_str({item})"),
                                        "LucidList*" => format!("lucid_as_list({item})"),
                                        "LucidVal" => item,
                                        other => format!("({other})lucid_as_ptr({item})"),
                                    }
                                };
                                let rendered = param_types.iter().enumerate().map(|(index, ty)| {
                                    let key = names.get(index).cloned().unwrap_or_else(|| index.to_string());
                                    let positional = if has_pargs {
                                        format!("({object}->pargs && {object}->pargs->len > {index}LL ? lucid_list_get({object}->pargs, {index}LL) : lucid_list_get({object}->vpargs, {}LL))", index.saturating_sub(pargs_prefix))
                                    } else {
                                        format!("lucid_list_get({object}->vpargs, {index}LL)")
                                    };
                                    let keyword = format!("lucid_dict_get({object}->kwargs, lucid_str(\"{}\"), lucid_none())", key.replace('"', "\\\""));
                                    let has_keyword = format!("lucid_dict_contains({object}->kwargs, lucid_str(\"{}\"))", key.replace('"', "\\\""));
                                    let available = if has_pargs {
                                        format!("(({object}->pargs && {object}->pargs->len > {index}LL) || ({object}->vpargs && {object}->vpargs->len > {}LL))", index.saturating_sub(pargs_prefix))
                                    } else {
                                        format!("{object}->vpargs && {object}->vpargs->len > {index}LL")
                                    };
                                    let selected = if let Some(Some(default)) = defaults.get(index) {
                                        let default_code = self.emit_expr(default)?;
                                        format!("({available} ? {positional} : ({has_keyword} ? {keyword} : {default_code}))")
                                    } else {
                                        format!("({available} ? {positional} : {keyword})")
                                    };
                                    Ok::<String, CodegenError>(convert(selected, ty))
                                }).collect::<Result<Vec<_>, _>>()?;
                                let fn_name = if let Some(candidates) =
                                    self.dispatch_fns.get(&resolved_name)
                                {
                                    candidates
                                        .first()
                                        .map(|(_, emitted)| emitted.clone())
                                        .unwrap_or_else(|| format!("lucid_fn_{resolved_name}"))
                                } else {
                                    format!("lucid_fn_{resolved_name}")
                                };
                                return Ok(format!("{fn_name}({})", rendered.join(", ")));
                            }
                            // Ordinary class values participate in `***`
                            // forwarding through their declared field order
                            // (the interpreter uses the same rule).  This is
                            // useful for small record-like argument objects
                            // and keeps native calls semantically aligned.
                            if let Some(fields) = self.known_classes.get(bundle_name).cloned() {
                                let value = self.emit_expr(&args[0].value)?;
                                let object =
                                    format!("(({bundle_name}*)lucid_as_ptr(lucid_wrap({value})))");
                                let convert = |item: String, ty: &str| -> String {
                                    match ty {
                                        "int64_t" => format!("lucid_as_int({item})"),
                                        "double" => format!("lucid_as_float({item})"),
                                        "bool" => format!("lucid_as_bool({item})"),
                                        "const char*" => format!("lucid_as_str({item})"),
                                        "LucidList*" => format!("lucid_as_list({item})"),
                                        "LucidVal" => item,
                                        other => format!("({other})lucid_as_ptr({item})"),
                                    }
                                };
                                let rendered = fields
                                    .iter()
                                    .enumerate()
                                    .map(|(index, field)| {
                                        let item = format!("lucid_wrap({object}->{field})");
                                        let ty = param_types
                                            .get(index)
                                            .map(String::as_str)
                                            .unwrap_or("LucidVal");
                                        convert(item, ty)
                                    })
                                    .collect::<Vec<_>>();
                                let fn_name = if let Some(candidates) =
                                    self.dispatch_fns.get(&resolved_name)
                                {
                                    candidates
                                        .first()
                                        .map(|(_, emitted)| emitted.clone())
                                        .unwrap_or_else(|| format!("lucid_fn_{resolved_name}"))
                                } else {
                                    format!("lucid_fn_{resolved_name}")
                                };
                                return Ok(format!("{fn_name}({})", rendered.join(", ")));
                            }
                        }
                    }
                    let mut expanded_args = Vec::new();
                    for arg in args {
                        if arg.is_spread {
                            if let Expr::List { elements, .. } = &arg.value {
                                for value in elements {
                                    expanded_args.push(Arg {
                                        name: None,
                                        value: value.clone(),
                                        is_spread: false,
                                        is_dict_spread: false,
                                        is_gather_spread: false,
                                        span: arg.span,
                                    });
                                }
                                continue;
                            }
                        }
                        if arg.is_dict_spread {
                            if let Expr::Dict { entries, .. } = &arg.value {
                                for (key, value) in entries {
                                    let name = match key {
                                        Expr::Literal {
                                            value: LiteralValue::Str(name),
                                            ..
                                        } => Some(name.clone()),
                                        _ => None,
                                    };
                                    expanded_args.push(Arg {
                                        name,
                                        value: value.clone(),
                                        is_spread: false,
                                        is_dict_spread: false,
                                        is_gather_spread: false,
                                        span: arg.span,
                                    });
                                }
                                continue;
                            }
                        }
                        if arg.is_spread || arg.is_dict_spread || arg.is_gather_spread {
                            return Err(CodegenError { message: "native backend requires spread arguments to be statically materialized".into() });
                        }
                        expanded_args.push(arg.clone());
                    }
                    let raw_args: Vec<&Arg> = expanded_args
                        .iter()
                        .filter(|a| !matches!(a.value, Expr::Skip(_)))
                        .collect();
                    let (effective_args, arg_strs): (Vec<&Arg>, Vec<String>) =
                        if let Some(param_names) =
                            self.known_fn_param_names.get(&resolved_name).cloned()
                        {
                            let defaults = self
                                .known_fn_defaults
                                .get(&resolved_name)
                                .cloned()
                                .unwrap_or_default();
                            let parameter_types = self
                                .known_fn_params
                                .get(&resolved_name)
                                .cloned()
                                .unwrap_or_default();
                            let adapt_argument = |rendered: String, index: usize| {
                                if parameter_types.get(index).map(String::as_str)
                                    == Some("LucidVal")
                                {
                                    format!("lucid_wrap({rendered})")
                                } else {
                                    rendered
                                }
                            };
                            let mut slots: Vec<Option<&Arg>> = vec![None; param_names.len()];
                            let mut positional = 0usize;
                            let mut extras = Vec::new();
                            for arg in raw_args {
                                if let Some(arg_name) = &arg.name {
                                    if let Some(index) =
                                        param_names.iter().position(|name| name == arg_name)
                                    {
                                        slots[index] = Some(arg);
                                    } else {
                                        extras.push(arg);
                                    }
                                } else if positional < slots.len() {
                                    slots[positional] = Some(arg);
                                    positional += 1;
                                } else {
                                    extras.push(arg);
                                }
                            }
                            let effective: Vec<&Arg> = slots
                                .iter()
                                .flatten()
                                .copied()
                                .chain(extras.iter().copied())
                                .collect();
                            let mut rendered = Vec::with_capacity(slots.len() + extras.len());
                            for (index, slot) in slots.into_iter().enumerate() {
                                if let Some(arg) = slot {
                                    rendered
                                        .push(adapt_argument(self.emit_expr(&arg.value)?, index));
                                } else if let Some(Some(default)) = defaults.get(index) {
                                    rendered.push(adapt_argument(self.emit_expr(default)?, index));
                                } else {
                                    return Err(CodegenError {
                                        message: format!(
                                            "missing required argument '{}'",
                                            param_names[index]
                                        ),
                                    });
                                }
                            }
                            for arg in extras {
                                rendered.push(self.emit_expr(&arg.value)?);
                            }
                            (effective, rendered)
                        } else {
                            let rendered = raw_args
                                .iter()
                                .map(|a| self.emit_expr(&a.value))
                                .collect::<Result<Vec<_>, _>>()?;
                            (raw_args, rendered)
                        };
                    let fn_name = if let Some(candidates) = self.dispatch_fns.get(&resolved_name) {
                        if let Some(signatures) = self.dispatch_signatures.get(&resolved_name) {
                            let actual = effective_args
                                .iter()
                                .map(|a| {
                                    self.infer_expr_type(&a.value, &HashMap::new())
                                        .trim_end_matches('*')
                                        .to_string()
                                })
                                .collect::<Vec<_>>();
                            let mut best: Option<(usize, String)> = None;
                            let mut tied = false;
                            for (types, emitted) in signatures {
                                if types.len() != actual.len() {
                                    continue;
                                }
                                let Some(distance) = types
                                    .iter()
                                    .zip(&actual)
                                    .map(|(expected, got)| {
                                        self.dispatch_type_distance(
                                            got,
                                            expected.trim_end_matches('*'),
                                        )
                                    })
                                    .collect::<Option<Vec<_>>>()
                                    .map(|distances| {
                                        distances.into_iter().fold(0usize, usize::saturating_add)
                                    })
                                else {
                                    continue;
                                };
                                match &best {
                                    None => best = Some((distance, emitted.clone())),
                                    Some((best_distance, _)) if distance < *best_distance => {
                                        best = Some((distance, emitted.clone()));
                                        tied = false;
                                    }
                                    Some((best_distance, _)) if distance == *best_distance => {
                                        tied = true;
                                    }
                                    _ => {}
                                }
                            }
                            if tied {
                                return Err(CodegenError {
                                    message: format!(
                                        "ambiguous dispatch call to '{resolved_name}'"
                                    ),
                                });
                            }
                            best.map(|(_, emitted)| emitted).ok_or_else(|| CodegenError {
                                message: format!(
                                    "no dispatch overload of '{resolved_name}' accepts the supplied arguments"
                                ),
                            })?
                        } else {
                            let actual = effective_args
                                .first()
                                .map(|a| self.infer_expr_type(&a.value, &HashMap::new()))
                                .unwrap_or_else(|| "Any".to_string());
                            let actual = actual.trim_end_matches('*');
                            candidates
                                .iter()
                                .find(|(ty, _)| ty == actual)
                                .map(|(_, emitted)| emitted.clone())
                                .ok_or_else(|| CodegenError {
                                    message: format!(
                                        "no dispatch overload of '{resolved_name}' accepts the supplied arguments"
                                    ),
                                })?
                        }
                    } else {
                        format!("lucid_fn_{resolved_name}")
                    };
                    let call_args =
                        if let Some(signatures) = self.dispatch_signatures.get(&resolved_name) {
                            if let Some((types, _)) =
                                signatures.iter().find(|(_, emitted)| emitted == &fn_name)
                            {
                                arg_strs
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, value)| {
                                        match types.get(index).map(String::as_str) {
                                            Some("LucidVal") => format!("lucid_wrap({value})"),
                                            Some(ty) if ty.ends_with('*') => {
                                                format!("({ty})({value})")
                                            }
                                            _ => value,
                                        }
                                    })
                                    .collect::<Vec<_>>()
                            } else {
                                arg_strs
                            }
                        } else {
                            arg_strs
                        };
                    return Ok(format!("{fn_name}({})", call_args.join(", ")));
                }

                // Method call: obj.method(...)
                if let Expr::Attribute {
                    value: obj_expr,
                    attr,
                    ..
                } = &**func
                {
                    // The standard `time` module is represented as an
                    // imported attribute call, not as a user-defined method.
                    // Handle both clocks before type/module inference can
                    // fall through to a `lucid_fn_*` symbol.
                    if matches!(&**obj_expr, Expr::Ident { name, .. } if name == "time")
                        && matches!(attr.as_str(), "time" | "monotonic")
                    {
                        if !args.is_empty() {
                            return Err(CodegenError {
                                message: format!("time.{attr}() takes no arguments"),
                            });
                        }
                        return Ok("lucid_time_now()".to_string());
                    }
                    if let Expr::Ident {
                        name: class_name, ..
                    } = &**obj_expr
                    {
                        if class_name == "super" {
                            let current =
                                self.current_class.clone().ok_or_else(|| CodegenError {
                                    message: "super used outside a class method".into(),
                                })?;
                            let parent =
                                self.known_parents.get(&current).cloned().ok_or_else(|| {
                                    CodegenError {
                                        message: format!("class '{current}' has no parent"),
                                    }
                                })?;
                            if !self
                                .known_method_param_names
                                .contains_key(&(parent.clone(), attr.clone()))
                            {
                                return Err(CodegenError {
                                    message: format!(
                                        "parent class '{parent}' has no method '{attr}'"
                                    ),
                                });
                            }
                            let is_instance = self.var_types.contains_key("self");
                            if !is_instance
                                && !self
                                    .known_class_methods
                                    .contains_key(&(parent.clone(), attr.clone()))
                            {
                                return Err(CodegenError {
                                    message: format!(
                                        "parent class '{parent}' has no class method '{attr}'"
                                    ),
                                });
                            }
                            let mut rendered = if is_instance {
                                vec![format!("({parent}*)self")]
                            } else {
                                Vec::new()
                            };
                            let param_names = self
                                .known_method_param_names
                                .get(&(parent.clone(), attr.clone()))
                                .cloned()
                                .unwrap_or_default();
                            let defaults = self
                                .known_method_defaults
                                .get(&(parent.clone(), attr.clone()))
                                .cloned()
                                .unwrap_or_default();
                            let mut slots: Vec<Option<&Arg>> = vec![None; param_names.len()];
                            let mut positional = 0usize;
                            for arg in args.iter().filter(|a| !matches!(a.value, Expr::Skip(_))) {
                                if let Some(name) = &arg.name {
                                    if let Some(index) = param_names.iter().position(|p| p == name)
                                    {
                                        slots[index] = Some(arg);
                                    }
                                } else if positional < slots.len() {
                                    slots[positional] = Some(arg);
                                    positional += 1;
                                }
                            }
                            for (index, slot) in slots.into_iter().enumerate() {
                                if let Some(arg) = slot {
                                    rendered.push(self.emit_expr(&arg.value)?);
                                } else if let Some(Some(default)) = defaults.get(index) {
                                    rendered.push(self.emit_expr(default)?);
                                } else {
                                    return Err(CodegenError {
                                        message: format!(
                                            "missing required super method argument '{}'",
                                            param_names[index]
                                        ),
                                    });
                                }
                            }
                            return Ok(format!("{parent}_{attr}({})", rendered.join(", ")));
                        }
                        if attr == "replace" {
                            let Some(instance) = args.first() else {
                                return Err(CodegenError {
                                    message: "replace() requires an instance".into(),
                                });
                            };
                            let instance_code = self.emit_expr(&instance.value)?;
                            let fields = self
                                .known_classes
                                .get(class_name)
                                .cloned()
                                .unwrap_or_default();
                            let overrides: HashMap<String, &Arg> = args
                                .iter()
                                .skip(1)
                                .filter_map(|arg| arg.name.as_ref().map(|name| (name.clone(), arg)))
                                .collect();
                            for name in overrides.keys() {
                                if !fields.iter().any(|field| field == name) {
                                    return Err(CodegenError {
                                        message: format!("replace() got unknown field '{name}'"),
                                    });
                                }
                            }
                            let values = fields.iter().map(|field| {
                                if let Some(arg) = overrides.get(field) {
                                    self.emit_expr(&arg.value)
                                } else {
                                    Ok(format!("(({class_name}*)lucid_as_ptr(lucid_wrap({instance_code})))->{field}"))
                                }
                            }).collect::<Result<Vec<_>, CodegenError>>()?;
                            return Ok(format!("{class_name}_new({})", values.join(", ")));
                        }
                        if self.module_aliases.get(class_name).is_some_and(|module| {
                            module != "math" && module != "sys" && module != "time"
                        }) {
                            let arg_strs: Result<Vec<String>, CodegenError> = args
                                .iter()
                                .filter(|arg| !matches!(arg.value, Expr::Skip(_)))
                                .map(|arg| self.emit_expr(&arg.value))
                                .collect();
                            return Ok(format!("lucid_fn_{}({})", attr, arg_strs?.join(", ")));
                        }
                        let mut class_method_owner = Some(class_name.clone());
                        while let Some(owner) = class_method_owner.clone() {
                            if self
                                .known_class_methods
                                .contains_key(&(owner.clone(), attr.clone()))
                            {
                                let param_names = self
                                    .known_method_param_names
                                    .get(&(owner.clone(), attr.clone()))
                                    .cloned()
                                    .unwrap_or_default();
                                let defaults = self
                                    .known_method_defaults
                                    .get(&(owner.clone(), attr.clone()))
                                    .cloned()
                                    .unwrap_or_default();
                                let mut slots: Vec<Option<&Arg>> = vec![None; param_names.len()];
                                let mut positional = 0usize;
                                for arg in args.iter().filter(|a| !matches!(a.value, Expr::Skip(_)))
                                {
                                    if let Some(name) = &arg.name {
                                        if let Some(index) =
                                            param_names.iter().position(|p| p == name)
                                        {
                                            slots[index] = Some(arg);
                                        }
                                    } else if positional < slots.len() {
                                        slots[positional] = Some(arg);
                                        positional += 1;
                                    }
                                }
                                let mut arg_strs = Vec::with_capacity(slots.len());
                                for (index, slot) in slots.into_iter().enumerate() {
                                    if let Some(arg) = slot {
                                        arg_strs.push(self.emit_expr(&arg.value)?);
                                    } else if let Some(Some(default)) = defaults.get(index) {
                                        arg_strs.push(self.emit_expr(default)?);
                                    } else {
                                        return Err(CodegenError {
                                            message: format!(
                                                "missing required class method argument '{}'",
                                                param_names[index]
                                            ),
                                        });
                                    }
                                }
                                return Ok(format!("{owner}_{attr}({})", arg_strs.join(", ")));
                            }
                            class_method_owner = self.known_parents.get(&owner).cloned();
                        }
                        if self
                            .known_factories
                            .contains_key(&(class_name.clone(), attr.clone()))
                        {
                            let arg_strs: Result<Vec<String>, CodegenError> =
                                args.iter().map(|a| self.emit_expr(&a.value)).collect();
                            return Ok(format!(
                                "{class_name}_factory_{attr}({})",
                                arg_strs?.join(", ")
                            ));
                        }
                    }
                    if let Expr::Ident { name: obj_name, .. } = &**obj_expr {
                        if obj_name == "time" && matches!(attr.as_str(), "time" | "monotonic") {
                            return Ok("lucid_time_now()".to_string());
                        }
                        if obj_name == "sys" {
                            return match attr.as_str() {
                                "platform" => {
                                    Ok(format!("lucid_str(\"{}\")", std::env::consts::OS))
                                }
                                "version" => Ok("lucid_str(\"0.1.0\")".to_string()),
                                _ => Err(CodegenError {
                                    message: format!("unknown sys member '{attr}'"),
                                }),
                            };
                        }
                        if self
                            .module_aliases
                            .get(obj_name)
                            .is_some_and(|module| module == "math")
                            || obj_name == "math"
                        {
                            let arg = if args.len() == 1 {
                                self.emit_expr(&args[0].value)?
                            } else {
                                return Err(CodegenError {
                                    message: format!("math.{attr}() takes exactly one argument"),
                                });
                            };
                            let numeric = format!("lucid_as_float(lucid_wrap({arg}))");
                            return match attr.as_str() {
                                "sqrt" => Ok(format!("sqrt({numeric})")),
                                "sin" => Ok(format!("sin({numeric})")),
                                "cos" => Ok(format!("cos({numeric})")),
                                "tan" => Ok(format!("tan({numeric})")),
                                "floor" => Ok(format!("floor({numeric})")),
                                "ceil" => Ok(format!("ceil({numeric})")),
                                "abs" => Ok(format!("lucid_abs_value(lucid_wrap({arg}))")),
                                _ => Err(CodegenError {
                                    message: format!("unknown math member '{attr}'"),
                                }),
                            };
                        }
                    }
                    let receiver_ty = self.infer_expr_type(obj_expr, &HashMap::new());
                    if attr == "append" && receiver_ty == "LucidList*" {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: "append() takes exactly one argument".into(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let val_code = self.emit_expr(&args[0].value)?;
                        return Ok(format!(
                            "lucid_list_append(lucid_as_list(lucid_wrap({obj_code})), lucid_wrap({val_code}))"
                        ));
                    }
                    if attr == "remove" && receiver_ty == "LucidList*" {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: "remove() takes exactly one argument".into(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let val_code = self.emit_expr(&args[0].value)?;
                        return Ok(format!(
                            "lucid_list_remove(lucid_as_list(lucid_wrap({obj_code})), lucid_wrap({val_code}))"
                        ));
                    }
                    if attr == "pop" && receiver_ty == "LucidSet*" {
                        if !args.is_empty() {
                            return Err(CodegenError {
                                message: "set.pop() takes no arguments".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        return Ok(format!("lucid_set_pop({obj_code})"));
                    }
                    if attr == "pop" && receiver_ty == "LucidDict*" {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: "dict.pop() takes exactly one argument".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let key_code = self.emit_expr(&args[0].value)?;
                        return Ok(format!(
                            "lucid_dict_pop({obj_code}, lucid_wrap({key_code}))"
                        ));
                    }
                    if attr == "pop" && (receiver_ty == "LucidList*" || receiver_ty == "LucidSet*")
                    {
                        if args.len() > 2 {
                            return Err(CodegenError {
                                message: "pop() takes zero, one, or two arguments".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        if args.len() == 2 {
                            let start = self.emit_expr(&args[0].value)?;
                            let stop = self.emit_expr(&args[1].value)?;
                            return Ok(format!(
                                "lucid_list_pop_range(lucid_as_list(lucid_wrap({obj_code})), (int64_t)({start}), (int64_t)({stop}))"
                            ));
                        }
                        let index = if args.is_empty() {
                            "0LL".to_string()
                        } else {
                            self.emit_expr(&args[0].value)?
                        };
                        return Ok(format!(
                            "lucid_list_pop(lucid_as_list(lucid_wrap({obj_code})), (int64_t)({index}), {})",
                            !args.is_empty()
                        ));
                    }
                    if attr == "insert" && receiver_ty == "LucidList*" {
                        if args.len() != 2 {
                            return Err(CodegenError {
                                message: "insert() takes exactly two arguments".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let index = self.emit_expr(&args[0].value)?;
                        let value = self.emit_expr(&args[1].value)?;
                        return Ok(format!(
                            "lucid_list_insert(lucid_as_list(lucid_wrap({obj_code})), (int64_t)({index}), lucid_wrap({value}))"
                        ));
                    }
                    if attr == "extend" && receiver_ty == "LucidList*" {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: "extend() takes exactly one argument".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let value = self.emit_expr(&args[0].value)?;
                        let value_type = self.infer_expr_type(&args[0].value, &HashMap::new());
                        let value_class = value_type.trim_end_matches('*');
                        let value = if let Some(owner) = self.method_owner(value_class, "next") {
                            self.native_drain_iterator(&owner, &value)
                        } else if let Some(owner) = self.method_owner(value_class, "__iter__") {
                            format!("{owner}___iter__(({owner}*)({value}))")
                        } else {
                            value
                        };
                        return Ok(format!(
                            "lucid_list_extend(lucid_as_list(lucid_wrap({obj_code})), lucid_wrap({value}))"
                        ));
                    }
                    if attr == "clear"
                        && (receiver_ty == "LucidList*"
                            || receiver_ty == "LucidSet*"
                            || receiver_ty == "LucidDict*")
                    {
                        if !args.is_empty() {
                            return Err(CodegenError {
                                message: "clear() takes no arguments".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let ty = self.infer_expr_type(obj_expr, &HashMap::new());
                        let helper = if ty == "LucidSet*" {
                            "lucid_set_clear(lucid_as_set(lucid_wrap("
                        } else if ty == "LucidDict*" {
                            "lucid_dict_clear(lucid_as_dict(lucid_wrap("
                        } else {
                            "lucid_list_clear(lucid_as_list(lucid_wrap("
                        };
                        return Ok(format!("{helper}{obj_code})))"));
                    }
                    if (attr == "keys" || attr == "values") && receiver_ty == "LucidDict*" {
                        if !args.is_empty() {
                            return Err(CodegenError {
                                message: format!("{attr}() takes no arguments"),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let helper = if attr == "keys" {
                            "lucid_dict_keys"
                        } else {
                            "lucid_dict_values"
                        };
                        return Ok(format!("{helper}({obj_code})"));
                    }
                    if attr == "items" && receiver_ty == "LucidDict*" {
                        if !args.is_empty() {
                            return Err(CodegenError {
                                message: "items() takes no arguments".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        return Ok(format!("lucid_dict_items({obj_code})"));
                    }
                    if attr == "get" && receiver_ty == "LucidDict*" {
                        if args.is_empty() || args.len() > 2 {
                            return Err(CodegenError {
                                message: "get() takes one or two arguments".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let key_code = self.emit_expr(&args[0].value)?;
                        let fallback = if args.len() == 2 {
                            self.emit_expr(&args[1].value)?
                        } else {
                            "lucid_none()".to_string()
                        };
                        return Ok(format!(
                            "lucid_dict_get({obj_code}, lucid_wrap({key_code}), lucid_wrap({fallback}))"
                        ));
                    }
                    if attr == "contains" && receiver_ty == "LucidDict*" {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: "contains() takes exactly one argument".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let key_code = self.emit_expr(&args[0].value)?;
                        return Ok(format!(
                            "lucid_dict_contains({obj_code}, lucid_wrap({key_code}))"
                        ));
                    }
                    if attr == "add"
                        && self.infer_expr_type(obj_expr, &HashMap::new()) == "LucidSet*"
                    {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: "add() takes one argument".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let value_code = self.emit_expr(&args[0].value)?;
                        return Ok(format!(
                            "lucid_set_add({obj_code}, lucid_wrap({value_code}))"
                        ));
                    }
                    if attr == "contains"
                        && self.infer_expr_type(obj_expr, &HashMap::new()) == "LucidSet*"
                    {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: "contains() takes one argument".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let value_code = self.emit_expr(&args[0].value)?;
                        return Ok(format!(
                            "lucid_set_contains({obj_code}, lucid_wrap({value_code}))"
                        ));
                    }
                    if attr == "isdisjoint"
                        && self.infer_expr_type(obj_expr, &HashMap::new()) == "LucidSet*"
                    {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: "isdisjoint() takes one argument".into(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let value_code = self.emit_expr(&args[0].value)?;
                        return Ok(format!(
                            "lucid_set_disjoint_value({obj_code}, lucid_wrap({value_code}))"
                        ));
                    }
                    if attr == "remove"
                        && self.infer_expr_type(obj_expr, &HashMap::new()) == "LucidSet*"
                    {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: "remove() takes one argument".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let value_code = self.emit_expr(&args[0].value)?;
                        return Ok(format!(
                            "lucid_set_remove({obj_code}, lucid_wrap({value_code}))"
                        ));
                    }
                    if attr == "discard"
                        && self.infer_expr_type(obj_expr, &HashMap::new()) == "LucidSet*"
                    {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: "discard() takes one argument".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let value_code = self.emit_expr(&args[0].value)?;
                        return Ok(format!(
                            "lucid_set_discard({obj_code}, lucid_wrap({value_code}))"
                        ));
                    }
                    if attr == "split" {
                        if args.len() > 1 {
                            return Err(CodegenError {
                                message: "split() takes zero or one argument".into(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        if args.is_empty() {
                            return Ok(format!(
                                "lucid_str_split(lucid_as_str(lucid_wrap({obj_code})), NULL, false)"
                            ));
                        }
                        let sep_code = self.emit_expr(&args[0].value)?;
                        return Ok(format!(
                            "lucid_str_split(lucid_as_str(lucid_wrap({obj_code})), lucid_as_str(lucid_wrap({sep_code})), true)"
                        ));
                    }
                    if attr == "strip" || attr == "trim" {
                        if !args.is_empty() {
                            return Err(CodegenError {
                                message: format!("{attr}() takes no arguments"),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        return Ok(format!(
                            "lucid_str_strip(lucid_as_str(lucid_wrap({obj_code})))"
                        ));
                    }
                    if attr == "startswith" || attr == "endswith" {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: format!("{attr}() takes exactly one argument"),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let arg_code = self.emit_expr(&args[0].value)?;
                        let helper = if attr == "startswith" {
                            "lucid_str_startswith"
                        } else {
                            "lucid_str_endswith"
                        };
                        return Ok(format!(
                            "{helper}(lucid_as_str(lucid_wrap({obj_code})), lucid_as_str(lucid_wrap({arg_code})))"
                        ));
                    }
                    if attr == "replace" {
                        if args.len() != 2 {
                            return Err(CodegenError {
                                message: "replace() takes exactly two arguments".to_string(),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let old_code = self.emit_expr(&args[0].value)?;
                        let new_code = self.emit_expr(&args[1].value)?;
                        return Ok(format!(
                            "lucid_str_replace(lucid_as_str(lucid_wrap({obj_code})), lucid_as_str(lucid_wrap({old_code})), lucid_as_str(lucid_wrap({new_code})))"
                        ));
                    }
                    if attr == "join" {
                        if args.len() != 1 {
                            return Err(CodegenError {
                                message: "join() requires exactly one argument".to_string(),
                            });
                        }
                        let sep_code = self.emit_expr(obj_expr)?;
                        let values_code = self.emit_expr(&args[0].value)?;
                        let values_type = self.infer_expr_type(&args[0].value, &HashMap::new());
                        let values_class = values_type.trim_end_matches('*');
                        let values_code =
                            if let Some(owner) = self.method_owner(values_class, "next") {
                                self.native_drain_iterator(&owner, &values_code)
                            } else {
                                values_code
                            };
                        return Ok(format!(
                            "lucid_str_join(lucid_as_str(lucid_wrap({sep_code})), lucid_iterable_to_list(lucid_wrap({values_code})))"
                        ));
                    }
                    if attr == "upper" || attr == "lower" {
                        if !args.is_empty() {
                            return Err(CodegenError {
                                message: format!("{attr}() takes no arguments"),
                            });
                        }
                        let obj_code = self.emit_expr(obj_expr)?;
                        let helper = if attr == "upper" {
                            "lucid_str_upper"
                        } else {
                            "lucid_str_lower"
                        };
                        return Ok(format!("{helper}(lucid_as_str(lucid_wrap({obj_code})))"));
                    }
                    if receiver_ty == "LucidVal" {
                        if args.iter().any(|arg| {
                            arg.name.is_some()
                                || arg.is_spread
                                || arg.is_dict_spread
                                || arg.is_gather_spread
                        }) {
                            return Err(CodegenError {
                                message: format!(
                                    "dynamic method '{attr}' requires positional, non-spread arguments"
                                ),
                            });
                        }
                        let object = self.new_temp();
                        let class_name = self.new_temp();
                        let result = self.new_temp();
                        let matched = self.new_temp();
                        let mut arg_temps = Vec::new();
                        let mut classes: Vec<String> = self.known_classes.keys().cloned().collect();
                        classes.sort();
                        let mut lines = vec![format!(
                            "LucidVal {object} = lucid_wrap({}); const char* {class_name} = lucid_object_class_name({object}.ptr); LucidVal {result} = lucid_none(); bool {matched} = false;",
                            self.emit_expr(obj_expr)?
                        )];
                        for arg in args {
                            let temp = self.new_temp();
                            lines.push(format!(
                                "LucidVal {temp} = lucid_wrap({});",
                                self.emit_expr(&arg.value)?
                            ));
                            arg_temps.push(temp);
                        }
                        for candidate in classes {
                            if let Some(owner) = self.method_owner(&candidate, attr) {
                                let Some(param_types) = self
                                    .known_method_param_types
                                    .get(&(owner.clone(), attr.clone()))
                                    .cloned()
                                else {
                                    continue;
                                };
                                if param_types.len() != arg_temps.len() {
                                    continue;
                                }
                                let converted = param_types
                                    .iter()
                                    .zip(&arg_temps)
                                    .map(|(ty, temp)| match ty.as_str() {
                                        "LucidVal" => temp.clone(),
                                        "int64_t" => format!("lucid_as_int({temp})"),
                                        "double" => format!("lucid_as_float({temp})"),
                                        "bool" => format!("lucid_as_bool({temp})"),
                                        "const char*" => format!("lucid_as_str({temp})"),
                                        "LucidList*" => format!("lucid_as_list({temp})"),
                                        "LucidDict*" => format!("lucid_as_dict({temp})"),
                                        "LucidSet*" => format!("lucid_as_set({temp})"),
                                        other => format!("({other})lucid_as_ptr({temp})"),
                                    })
                                    .collect::<Vec<_>>();
                                let call_args = if converted.is_empty() {
                                    String::new()
                                } else {
                                    format!(", {}", converted.join(", "))
                                };
                                lines.push(format!(
                                    "if (!{matched} && {class_name} && strcmp({class_name}, \"{candidate}\") == 0) {{ {result} = lucid_wrap({owner}_{attr}(({owner}*){object}.ptr{call_args})); {matched} = true; }}"
                                ));
                            }
                        }
                        lines.push(format!(
                            "if (!{matched}) {{ fprintf(stderr, \"unsupported dynamic method '{attr}'\\n\"); exit(1); }} {result}"
                        ));
                        return Ok(format!("({{ {}; }})", lines.join(" ")));
                    }
                    let receiver_class = receiver_ty
                        .strip_suffix('*')
                        .filter(|class| self.known_classes.contains_key(*class))
                        .map(str::to_string);
                    let receiver_class_for_cast = receiver_class.clone();
                    let method_class = receiver_class
                        .as_deref()
                        .and_then(|class| self.method_owner(class, attr))
                        // A statically known receiver must never fall back to
                        // a method belonging to another class with the same
                        // spelling.  The global table is only a last resort
                        // when type inference could not identify a class.
                        .or_else(|| {
                            receiver_class_for_cast
                                .is_none()
                                .then(|| self.known_methods.get(attr).cloned())
                                .flatten()
                        });
                    if let Some(cls_name) = method_class {
                        let raw_obj_code = self.emit_expr(obj_expr)?;
                        let obj_code = if receiver_class_for_cast
                            .as_deref()
                            .is_some_and(|receiver| receiver != cls_name)
                        {
                            format!("({cls_name}*)({raw_obj_code})")
                        } else {
                            raw_obj_code
                        };
                        let mut all_args = vec![obj_code];
                        if let Some((gather_index, bundle_name)) = self
                            .known_method_gather
                            .get(&(cls_name.clone(), attr.clone()))
                            .cloned()
                        {
                            let param_names = self
                                .known_method_param_names
                                .get(&(cls_name.clone(), attr.clone()))
                                .cloned()
                                .unwrap_or_default();
                            let mut fixed: Vec<Option<&Arg>> = vec![None; gather_index];
                            let mut positional = 0usize;
                            let mut rest: Vec<&Arg> = Vec::new();
                            for arg in args
                                .iter()
                                .filter(|arg| !matches!(arg.value, Expr::Skip(_)))
                            {
                                if arg.is_spread || arg.is_dict_spread || arg.is_gather_spread {
                                    return Err(CodegenError { message: "native method gather calls require statically materialized arguments".into() });
                                }
                                if let Some(arg_name) = &arg.name {
                                    if let Some(index) = param_names
                                        .iter()
                                        .take(gather_index)
                                        .position(|name| name == arg_name)
                                    {
                                        fixed[index] = Some(arg);
                                    } else {
                                        rest.push(arg);
                                    }
                                } else if positional < gather_index {
                                    fixed[positional] = Some(arg);
                                    positional += 1;
                                } else {
                                    rest.push(arg);
                                }
                            }
                            let defaults = self
                                .known_method_defaults
                                .get(&(cls_name.clone(), attr.clone()))
                                .cloned()
                                .unwrap_or_default();
                            for index in 0..gather_index {
                                if let Some(arg) = fixed[index] {
                                    all_args.push(self.emit_expr(&arg.value)?);
                                } else if let Some(Some(default)) = defaults.get(index) {
                                    all_args.push(self.emit_expr(default)?);
                                } else {
                                    return Err(CodegenError {
                                        message: format!(
                                            "missing required method argument '{}'",
                                            param_names[index]
                                        ),
                                    });
                                }
                            }
                            let positional_rest = rest
                                .iter()
                                .filter(|arg| arg.name.is_none())
                                .map(|arg| self.emit_expr(&arg.value))
                                .collect::<Result<Vec<_>, _>>()?;
                            let keyword_rest = rest
                                .iter()
                                .filter_map(|arg| {
                                    arg.name.as_ref().map(|key| {
                                        let value = self.emit_expr(&arg.value)?;
                                        Ok::<String, CodegenError>(format!("lucid_dict_set(_kwargs, lucid_str(\"{}\"), lucid_wrap({value}));", c_escape_string(key)))
                                    })
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            let fields = self
                                .known_classes
                                .get(&bundle_name)
                                .cloned()
                                .unwrap_or_else(|| vec!["vpargs".into(), "kwargs".into()]);
                            let vpargs = format!(
                                "({{ LucidList* _vpargs = lucid_list_new({}); {} _vpargs; }})",
                                positional_rest.len(),
                                positional_rest
                                    .iter()
                                    .map(|value| format!(
                                        "lucid_list_append(_vpargs, lucid_wrap({value}));"
                                    ))
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            );
                            let kwargs = format!(
                                "({{ LucidDict* _kwargs = lucid_dict_new({}); {} _kwargs; }})",
                                keyword_rest.len(),
                                keyword_rest.join(" ")
                            );
                            let bundle_args = fields
                                .iter()
                                .map(|field| match field.as_str() {
                                    "pargs" => "lucid_list_new(0)".into(),
                                    "vpargs" => vpargs.clone(),
                                    "kwargs" => kwargs.clone(),
                                    _ => "NULL".into(),
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            all_args.push(format!("{bundle_name}_new({bundle_args})"));
                            return Ok(format!("{cls_name}_{attr}({})", all_args.join(", ")));
                        }
                        if let Some(param_names) = self
                            .known_method_param_names
                            .get(&(cls_name.clone(), attr.clone()))
                            .cloned()
                        {
                            let defaults = self
                                .known_method_defaults
                                .get(&(cls_name.clone(), attr.clone()))
                                .cloned()
                                .unwrap_or_default();
                            let mut slots: Vec<Option<&Arg>> = vec![None; param_names.len()];
                            let mut positional = 0usize;
                            for arg in args.iter().filter(|a| !matches!(a.value, Expr::Skip(_))) {
                                if let Some(name) = &arg.name {
                                    if let Some(index) = param_names.iter().position(|p| p == name)
                                    {
                                        slots[index] = Some(arg);
                                    }
                                } else if positional < slots.len() {
                                    slots[positional] = Some(arg);
                                    positional += 1;
                                }
                            }
                            for (index, slot) in slots.into_iter().enumerate() {
                                if let Some(arg) = slot {
                                    all_args.push(self.emit_expr(&arg.value)?);
                                } else if let Some(Some(default)) = defaults.get(index) {
                                    all_args.push(self.emit_expr(default)?);
                                } else {
                                    return Err(CodegenError {
                                        message: format!(
                                            "missing required method argument '{}'",
                                            param_names[index]
                                        ),
                                    });
                                }
                            }
                        } else {
                            for a in args {
                                all_args.push(self.emit_expr(&a.value)?);
                            }
                        }
                        return Ok(format!("{cls_name}_{attr}({})", all_args.join(", ")));
                    }
                }

                // A function alias is lowered to the original typed entry
                // point.  This is deliberately resolved after the special
                // builtin/closure paths above, so aliases retain ordinary
                // argument checking and dispatch behavior.
                if let Expr::Ident { name, .. } = &**func {
                    if let Some(target) = self.function_aliases.get(name).cloned() {
                        let arg_strs: Result<Vec<String>, CodegenError> =
                            args.iter().map(|a| self.emit_expr(&a.value)).collect();
                        return Ok(format!("lucid_fn_{}({})", target, arg_strs?.join(", ")));
                    }
                }
                let f_code = self.emit_expr(func)?;
                let arg_strs: Result<Vec<String>, CodegenError> =
                    args.iter().map(|a| self.emit_expr(&a.value)).collect();
                Ok(format!("{f_code}({})", arg_strs?.join(", ")))
            }
            Expr::Propagate { expr, .. } => {
                let value = self.emit_expr(expr)?;
                let temp = self.new_temp();
                Ok(format!(
                    "({{ LucidVal {temp} = lucid_wrap({value}); if ({temp}.type == LUCID_TYPE_PTR) return {temp}; {temp}; }})"
                ))
            }
            Expr::Construct { args, .. } => {
                let class_name = self.current_class.clone().ok_or_else(|| CodegenError {
                    message: "construct is only valid inside a class factory".to_string(),
                })?;
                let fields = self
                    .known_classes
                    .get(&class_name)
                    .cloned()
                    .unwrap_or_default();
                let mut values: Vec<Option<String>> = vec![None; fields.len()];
                for (i, arg) in args.iter().enumerate() {
                    let idx = arg
                        .name
                        .as_ref()
                        .and_then(|n| fields.iter().position(|f| f == n))
                        .unwrap_or(i);
                    if idx < values.len() {
                        values[idx] = Some(self.emit_expr(&arg.value)?);
                    }
                }
                let mut rendered = Vec::with_capacity(values.len());
                for (index, value) in values.into_iter().enumerate() {
                    if let Some(value) = value {
                        rendered.push(value);
                    } else if let Some(field) = fields.get(index) {
                        if let Some(default) = self
                            .known_field_defaults
                            .get(&(class_name.clone(), field.clone()))
                            .cloned()
                            .flatten()
                        {
                            rendered.push(self.emit_expr(&default)?);
                        } else {
                            rendered.push("NULL".to_string());
                        }
                    } else {
                        rendered.push("NULL".to_string());
                    }
                }
                Ok(format!("{class_name}_new({})", rendered.join(", ")))
            }
            Expr::Await { expr, .. } => {
                let value = self.emit_expr(expr)?;
                Ok(format!("lucid_await(lucid_wrap({value}))"))
            }
            Expr::Freeze { expr, .. } => {
                let code = self.emit_expr(expr)?;
                let ty = self.infer_expr_type(expr, &HashMap::new());
                Ok(format!(
                    "({{ {ty} _freeze_value = {code}; lucid_freeze_value(lucid_wrap(_freeze_value)); _freeze_value; }})"
                ))
            }
            Expr::Trust { expr, .. } => self.emit_expr(expr),
            Expr::Index { value, index, .. } => {
                let v_code = self.emit_expr(value)?;
                if let Expr::Slice {
                    start, stop, step, ..
                } = &**index
                {
                    let st = if let Some(s) = start {
                        self.emit_expr(s)?
                    } else {
                        "INT64_MIN".to_string()
                    };
                    let sp = if let Some(s) = stop {
                        self.emit_expr(s)?
                    } else {
                        "INT64_MIN".to_string()
                    };
                    let step_code = if let Some(s) = step {
                        self.emit_expr(s)?
                    } else {
                        "1LL".to_string()
                    };
                    if self.infer_expr_type(value, &HashMap::new()) == "const char*" {
                        return Ok(format!(
                            "lucid_str_slice({v_code}, {st}, {sp}, {step_code})"
                        ));
                    }
                    if self.infer_expr_type(value, &HashMap::new()) == "LucidVal" {
                        return Ok(format!(
                            "lucid_slice_value(lucid_wrap({v_code}), {st}, {sp}, {step_code})"
                        ));
                    }
                    return Ok(format!(
                        "lucid_list_slice(lucid_as_list(lucid_wrap({v_code})), {st}, {sp}, {step_code})"
                    ));
                }
                let idx_code = self.emit_expr(index)?;
                let receiver_type = self.infer_expr_type(value, &HashMap::new());
                let receiver_class = receiver_type.trim_end_matches('*').to_string();
                if let Some(owner) = self.method_owner(&receiver_class, "__getitem__") {
                    return Ok(format!(
                        "{owner}___getitem__(({owner}*)({v_code}), {idx_code})"
                    ));
                }
                if self.infer_expr_type(value, &HashMap::new()) == "LucidDict*" {
                    return Ok(format!(
                        "lucid_get_key(lucid_wrap({v_code}), lucid_wrap({idx_code}))"
                    ));
                }
                if receiver_type == "LucidVal" {
                    return Ok(format!(
                        "lucid_get_index_value(lucid_wrap({v_code}), lucid_wrap({idx_code}))"
                    ));
                }
                Ok(format!(
                    "lucid_get_index(lucid_wrap({v_code}), lucid_int_val({idx_code}))"
                ))
            }
            Expr::Attribute { value, attr, .. } => {
                if let Expr::Ident { name, .. } = &**value {
                    match (name.as_str(), attr.as_str()) {
                        ("float", "inf") => return Ok("INFINITY".to_string()),
                        ("float", "nan") => return Ok("NAN".to_string()),
                        ("int", "inf") => return Ok("INT64_MAX".to_string()),
                        ("int", "nan") => return Ok("INT64_MIN".to_string()),
                        ("complex", "nan") => return Ok("lucid_complex(NAN, NAN)".to_string()),
                        ("iteration", "done") => {
                            return Ok("lucid_str(\"iteration.done\")".to_string());
                        }
                        ("math", "pi") => return Ok("M_PI".to_string()),
                        ("math", "e") => return Ok("M_E".to_string()),
                        ("sys", "platform") => return Ok("lucid_sys_platform()".to_string()),
                        ("sys", "version") => return Ok("lucid_sys_version()".to_string()),
                        ("sys", "argv") => return Ok("lucid_sys_argv()".to_string()),
                        _ => {}
                    }
                    if self
                        .module_aliases
                        .get(name)
                        .is_some_and(|module| module != "math" && module != "sys")
                    {
                        return Ok(format!("lucid_var_{attr}"));
                    }
                    if self
                        .module_aliases
                        .get(name)
                        .is_some_and(|module| module == "math")
                    {
                        return match attr.as_str() {
                            "pi" => Ok("M_PI".to_string()),
                            "e" => Ok("M_E".to_string()),
                            _ => Err(CodegenError {
                                message: format!("unknown math member '{attr}'"),
                            }),
                        };
                    }
                }
                if let Expr::Ident { name: receiver, .. } = &**value {
                    let class_name = if self.known_classes.contains_key(receiver) {
                        Some(receiver.clone())
                    } else if receiver == "cls" {
                        self.current_class.clone()
                    } else {
                        self.var_types
                            .get(receiver)
                            .or_else(|| self.global_vars.get(receiver))
                            .and_then(|ty| ty.strip_suffix('*'))
                            .filter(|name| self.known_classes.contains_key(*name))
                            .map(str::to_string)
                    };
                    if let Some(class_name) = class_name {
                        let mut owner = Some(class_name);
                        while let Some(candidate) = owner.clone() {
                            if self
                                .known_class_vars
                                .contains_key(&(candidate.clone(), attr.clone()))
                            {
                                return Ok(format!("lucid_classvar_{candidate}_{attr}"));
                            }
                            owner = self.known_parents.get(&candidate).cloned();
                        }
                    }
                }
                let v_code = self.emit_expr(value)?;
                if self.infer_expr_type(value, &HashMap::new()) == "const char*" && attr == "chars"
                {
                    return Ok(format!(
                        "({{ LucidList* _chars = lucid_iterable_to_list(lucid_wrap({v_code})); _chars->frozen = true; _chars; }})"
                    ));
                }
                if self.infer_expr_type(value, &HashMap::new()) == "LucidDict*" {
                    return Ok(format!("lucid_dict_field({v_code}, \"{attr}\")"));
                }
                if let Expr::Call { func, args, .. } = &**value {
                    if let Expr::Ident { name, .. } = &**func {
                        if matches!(name.as_str(), "min" | "max") {
                            if let Some(element_class) = args
                                .first()
                                .and_then(|arg| self.indexed_element_class(&arg.value))
                            {
                                return Ok(format!(
                                    "(({element_class}*)lucid_as_ptr(lucid_wrap({v_code})))->{attr}"
                                ));
                            }
                        }
                    }
                }
                let receiver_type = self.infer_expr_type(value, &HashMap::new());
                if receiver_type == "LucidVal" {
                    return Ok(format!(
                        "lucid_dynamic_attr(lucid_wrap({v_code}), \"{}\", lucid_none(), false)",
                        c_escape_string(attr)
                    ));
                }
                let class_name = receiver_type.trim_end_matches('*').to_string();
                if !self.known_classes.contains_key(&class_name) {
                    if let Expr::Index { value: indexed, .. } = &**value {
                        if let Some(element_class) = self.indexed_element_class(indexed) {
                            return Ok(format!(
                                "(({element_class}*)lucid_as_ptr(lucid_wrap({v_code})))->{attr}"
                            ));
                        }
                    }
                }
                let mut getter_owner = Some(class_name.clone());
                while let Some(owner) = getter_owner.clone() {
                    if self
                        .known_getters
                        .contains_key(&(owner.clone(), attr.clone()))
                    {
                        let receiver = if owner != class_name {
                            format!("({owner}*)({v_code})")
                        } else {
                            v_code.clone()
                        };
                        return Ok(format!("{owner}_{attr}_get({receiver})"));
                    }
                    getter_owner = self.known_parents.get(&owner).cloned();
                }
                Ok(format!("{v_code}->{attr}"))
            }
            Expr::List { elements, .. } => {
                let tmp = self.new_temp();
                let mut instrs = Vec::new();
                let count = elements
                    .iter()
                    .filter(|e| !matches!(e, Expr::Skip(_)))
                    .count();
                instrs.push(format!("LucidList* {tmp} = lucid_list_new({});", count));
                for e in elements {
                    if matches!(e, Expr::Skip(_)) {
                        continue;
                    }
                    let elem_code = self.emit_expr(e)?;
                    instrs.push(format!(
                        "lucid_list_append({tmp}, lucid_wrap({elem_code}));"
                    ));
                }
                Ok(format!("({{ {} {tmp}; }})", instrs.join(" ")))
            }
            Expr::Dict { entries, .. } => {
                let tmp = self.new_temp();
                let count = entries
                    .iter()
                    .filter(|(key, value)| {
                        !matches!(key, Expr::Skip(_)) && !matches!(value, Expr::Skip(_))
                    })
                    .count();
                let mut instrs = vec![format!("LucidDict* {tmp} = lucid_dict_new({});", count)];
                for (key, value) in entries {
                    if matches!(key, Expr::Skip(_)) || matches!(value, Expr::Skip(_)) {
                        continue;
                    }
                    let key_code = self.emit_expr(key)?;
                    let value_code = self.emit_expr(value)?;
                    instrs.push(format!(
                        "lucid_dict_set({tmp}, lucid_wrap({key_code}), lucid_wrap({value_code}));"
                    ));
                }
                Ok(format!("({{ {} {tmp}; }})", instrs.join(" ")))
            }
            Expr::Set { elements, .. } => {
                let tmp = self.new_temp();
                let count = elements
                    .iter()
                    .filter(|e| !matches!(e, Expr::Skip(_)))
                    .count();
                let mut instrs = vec![format!("LucidSet* {tmp} = lucid_set_new({});", count)];
                for element in elements {
                    if matches!(element, Expr::Skip(_)) {
                        continue;
                    }
                    let code = self.emit_expr(element)?;
                    instrs.push(format!("lucid_set_add({tmp}, lucid_wrap({code}));"));
                }
                Ok(format!("({{ {} {tmp}; }})", instrs.join(" ")))
            }
            Expr::Record { fields, .. } => {
                let tmp = self.new_temp();
                let mut instrs = vec![format!(
                    "LucidDict* {tmp} = lucid_dict_new({});",
                    fields.len()
                )];
                for (name, value) in fields {
                    let field_name = name.clone().ok_or_else(|| CodegenError {
                        message: "record fields require names in native codegen".to_string(),
                    })?;
                    let value_code = self.emit_expr(value)?;
                    instrs.push(format!(
                        "lucid_dict_set({tmp}, lucid_str(\"{}\"), lucid_wrap({value_code}));",
                        field_name.replace('"', "\\\"")
                    ));
                }
                Ok(format!("({{ {} {tmp}; }})", instrs.join(" ")))
            }
            Expr::ListComp {
                element,
                target,
                iter,
                condition,
                ..
            } => {
                let tmp_list = self.new_temp();
                let var_name = match target {
                    Pattern::Ident(name, _) => name.clone(),
                    _ => self.new_temp(),
                };

                // Check if iter is range(...)
                if let Expr::Call { func, args, .. } = &**iter {
                    if let Expr::Ident { name, .. } = &**func {
                        if name == "range" {
                            let (start, stop, step) = match args.len() {
                                1 => (
                                    "0LL".to_string(),
                                    self.emit_expr(&args[0].value)?,
                                    "1LL".to_string(),
                                ),
                                2 => (
                                    self.emit_expr(&args[0].value)?,
                                    self.emit_expr(&args[1].value)?,
                                    "1LL".to_string(),
                                ),
                                3 => (
                                    self.emit_expr(&args[0].value)?,
                                    self.emit_expr(&args[1].value)?,
                                    self.emit_expr(&args[2].value)?,
                                ),
                                _ => ("0LL".to_string(), "0LL".to_string(), "1LL".to_string()),
                            };
                            let elem_code = self.emit_expr(element)?;
                            let range_var = format!("lucid_var_{var_name}");
                            let next_var = self.new_temp();
                            let step_var = self.new_temp();
                            let start_var = self.new_temp();
                            let stop_var = self.new_temp();
                            let cond_check = if let Some(cond) = condition {
                                format!("if ({}) ", self.emit_condition(cond)?)
                            } else {
                                "".to_string()
                            };
                            return Ok(format!(
                                "({{ LucidList* {tmp_list} = lucid_list_new(8); int64_t {start_var} = {start}; int64_t {stop_var} = {stop}; int64_t {step_var} = {step}; if ({step_var} == 0) {{ fprintf(stderr, \"range() step cannot be zero\\n\"); exit(1); }} for (int64_t {range_var} = {start_var}; ({step_var} > 0 ? {range_var} < {stop_var} : {range_var} > {stop_var}); ) {{ {cond_check}lucid_list_append({tmp_list}, lucid_wrap({elem_code})); int64_t {next_var}; if (!lucid_checked_range_advance({range_var}, {step_var}, &{next_var})) {{ fprintf(stderr, \"range step overflow\\n\"); exit(1); }} {range_var} = {next_var}; }} {tmp_list}; }})"
                            ));
                        }
                    }
                }

                let iter_code = self.emit_expr(iter)?;
                let tmp_i = self.new_temp();
                let tmp_iter_l = self.new_temp();
                let mut binding_names = HashMap::new();
                self.collect_pattern_vars(target, &mut binding_names);
                for name in binding_names.keys() {
                    self.var_types.insert(name.clone(), "LucidVal".to_string());
                }
                let declarations = binding_names
                    .keys()
                    .map(|name| format!("LucidVal lucid_var_{name} = lucid_none();"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut binding_lines = Vec::new();
                self.pattern_binding_lines(
                    target,
                    &format!("{tmp_iter_l}->items[{tmp_i}]"),
                    &mut binding_lines,
                );
                let binding_code = binding_lines.join(" ");
                let elem_code = self.emit_expr(element)?;
                let cond_check = if let Some(cond) = condition {
                    format!("if ({}) ", self.emit_condition(cond)?)
                } else {
                    "".to_string()
                };
                let iter_type = self.infer_expr_type(iter, &HashMap::new());
                let iter_class = iter_type.trim_end_matches('*');
                let iter_materialized = if let Some(owner) = self.method_owner(iter_class, "next") {
                    self.native_drain_iterator(owner.as_str(), &iter_code)
                } else if let Some(owner) = self.method_owner(iter_class, "__iter__") {
                    format!(
                        "lucid_iterable_to_list(lucid_wrap({owner}___iter__(({owner}*)({iter_code}))))"
                    )
                } else {
                    format!("lucid_iterable_to_list(lucid_wrap({iter_code}))")
                };
                Ok(format!(
                    "({{ LucidList* {tmp_iter_l} = {iter_materialized}; LucidList* {tmp_list} = lucid_list_new({tmp_iter_l} ? {tmp_iter_l}->len : 8); for (int64_t {tmp_i} = 0; {tmp_iter_l} && {tmp_i} < {tmp_iter_l}->len; {tmp_i}++) {{ {declarations} {binding_code} {cond_check}lucid_list_append({tmp_list}, lucid_wrap({elem_code})); }} {tmp_list}; }})"
                ))
            }
            Expr::SetComp {
                element,
                target,
                iter,
                condition,
                ..
            } => {
                let tmp = self.new_temp();
                let iter_code = self.emit_expr(iter)?;
                let mut binding_names = HashMap::new();
                self.collect_pattern_vars(target, &mut binding_names);
                for name in binding_names.keys() {
                    self.var_types.insert(name.clone(), "LucidVal".to_string());
                }
                let declarations = binding_names
                    .keys()
                    .map(|name| format!("LucidVal lucid_var_{name} = lucid_none();"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut binding_lines = Vec::new();
                self.pattern_binding_lines(target, "_it->items[_i]", &mut binding_lines);
                let binding_code = binding_lines.join(" ");
                let elem_code = self.emit_expr(element)?;
                let cond = if let Some(c) = condition {
                    format!("if ({}) ", self.emit_condition(c)?)
                } else {
                    String::new()
                };
                let iter_type = self.infer_expr_type(iter, &HashMap::new());
                let iter_class = iter_type.trim_end_matches('*');
                let iter_materialized = if let Some(owner) = self.method_owner(iter_class, "next") {
                    self.native_drain_iterator(owner.as_str(), &iter_code)
                } else if let Some(owner) = self.method_owner(iter_class, "__iter__") {
                    format!(
                        "lucid_iterable_to_list(lucid_wrap({owner}___iter__(({owner}*)({iter_code}))))"
                    )
                } else {
                    format!("lucid_iterable_to_list(lucid_wrap({iter_code}))")
                };
                Ok(format!(
                    "({{ LucidSet* {tmp} = lucid_set_new(8); LucidList* _it = {iter_materialized}; for (int64_t _i = 0; _it && _i < _it->len; _i++) {{ {declarations} {binding_code} {cond}lucid_set_add({tmp}, lucid_wrap({elem_code})); }} {tmp}; }})"
                ))
            }
            Expr::DictComp {
                key,
                value,
                target,
                iter,
                condition,
                ..
            } => {
                let tmp = self.new_temp();
                let iter_code = self.emit_expr(iter)?;
                let mut binding_names = HashMap::new();
                self.collect_pattern_vars(target, &mut binding_names);
                for name in binding_names.keys() {
                    self.var_types.insert(name.clone(), "LucidVal".to_string());
                }
                let declarations = binding_names
                    .keys()
                    .map(|name| format!("LucidVal lucid_var_{name} = lucid_none();"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut binding_lines = Vec::new();
                self.pattern_binding_lines(target, "_it->items[_i]", &mut binding_lines);
                let binding_code = binding_lines.join(" ");
                let key_code = self.emit_expr(key)?;
                let value_code = self.emit_expr(value)?;
                let cond = if let Some(c) = condition {
                    format!("if ({}) ", self.emit_condition(c)?)
                } else {
                    String::new()
                };
                let iter_type = self.infer_expr_type(iter, &HashMap::new());
                let iter_class = iter_type.trim_end_matches('*');
                let iter_materialized = if let Some(owner) = self.method_owner(iter_class, "next") {
                    self.native_drain_iterator(owner.as_str(), &iter_code)
                } else if let Some(owner) = self.method_owner(iter_class, "__iter__") {
                    format!(
                        "lucid_iterable_to_list(lucid_wrap({owner}___iter__(({owner}*)({iter_code}))))"
                    )
                } else {
                    format!("lucid_iterable_to_list(lucid_wrap({iter_code}))")
                };
                Ok(format!(
                    "({{ LucidDict* {tmp} = lucid_dict_new(8); LucidList* _it = {iter_materialized}; for (int64_t _i = 0; _it && _i < _it->len; _i++) {{ {declarations} {binding_code} {cond}lucid_dict_set({tmp}, lucid_wrap({key_code}), lucid_wrap({value_code})); }} {tmp}; }})"
                ))
            }
            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let th = self.emit_expr(then_branch)?;
                let el = self.emit_expr(else_branch)?;
                Ok(format!(
                    "(({}) ? ({th}) : ({el}))",
                    self.emit_condition(condition)?
                ))
            }
            other => Err(CodegenError {
                message: format!("native backend does not support expression form: {other:?}"),
            }),
        }
    }
}

/// Compile Lucid AST module to a native executable binary using GCC
pub fn compile_to_native(
    module: &Module,
    output_binary: &Path,
    opt_level: usize,
) -> Result<(), CodegenError> {
    compile_to_native_entry(module, output_binary, opt_level, None)
}

/// Compile a module to a native executable whose process entry invokes a
/// public zero-argument Lucid function. The ordinary module initializer still
/// runs first; the selected function's return value is printed using the same
/// tagged-value formatter as top-level evaluation.
pub fn compile_to_native_entry(
    module: &Module,
    output_binary: &Path,
    opt_level: usize,
    entry: Option<&str>,
) -> Result<(), CodegenError> {
    let mut generator = CCodeGenerator::new();
    let c_code = generator.generate(module)?;
    let c_code = if let Some(entry) = entry {
        let target = entry.rsplit('.').next().unwrap_or(entry);
        let function = module.statements.iter().find_map(|statement| {
            let Stmt::Function(function) = CCodeGenerator::unwrap_export(statement) else {
                return None;
            };
            (function.name == target).then_some(function)
        });
        let Some(function) = function else {
            return Err(CodegenError {
                message: format!("native entry point '{entry}' was not found"),
            });
        };
        if function.is_dispatch {
            return Err(CodegenError {
                message: format!(
                    "native entry point '{entry}' cannot target a dispatch overload set"
                ),
            });
        }
        if !function.params.is_empty() {
            return Err(CodegenError {
                message: format!(
                    "native entry point '{entry}' currently requires a zero-argument function"
                ),
            });
        }
        let call = if matches!(
            function.return_type.as_ref(),
            Some(TypeExpr::Named { name, .. }) if name == "none" || name == "None"
        ) {
            format!("lucid_fn_{}();", function.name)
        } else {
            format!("lucid_print_val(lucid_wrap(lucid_fn_{}()));", function.name)
        };
        let marker = "return 0;\n}";
        let position = c_code.rfind(marker).ok_or_else(|| CodegenError {
            message: "native backend did not emit a process entry point".to_string(),
        })?;
        let mut with_entry = String::with_capacity(c_code.len() + call.len() + 1);
        with_entry.push_str(&c_code[..position]);
        with_entry.push_str(&call);
        with_entry.push('\n');
        with_entry.push_str(&c_code[position..]);
        with_entry
    } else {
        c_code
    };

    let temp_dir = std::env::temp_dir();
    static BUILD_ID: AtomicUsize = AtomicUsize::new(0);
    let build_id = BUILD_ID.fetch_add(1, Ordering::Relaxed);
    let temp_c_path = temp_dir.join(format!("lucid_build_{}_{}.c", std::process::id(), build_id));

    let mut file = fs::File::create(&temp_c_path).map_err(|e| CodegenError {
        message: format!("Failed to create temporary C file: {e}"),
    })?;
    file.write_all(c_code.as_bytes())
        .map_err(|e| CodegenError {
            message: format!("Failed to write temporary C file: {e}"),
        })?;

    let opt_flag = format!("-O{opt_level}");
    let mut cmd = Command::new("gcc");
    cmd.arg(&opt_flag)
        .arg("-march=native")
        .arg("-fomit-frame-pointer")
        .arg("-ffp-contract=off")
        .arg(&temp_c_path)
        .arg("-o")
        .arg(output_binary)
        .arg("-lm");

    let output = cmd.output().map_err(|e| CodegenError {
        message: format!("Failed to invoke gcc compiler: {e}"),
    })?;

    let _ = fs::remove_file(&temp_c_path);

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        return Err(CodegenError {
            message: format!("Native compilation failed:\n{err_msg}"),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CCodeGenerator, compile_to_native};
    use lucid_syntax::parse;
    use std::fs;
    use std::process::Command;

    #[test]
    fn bigint_literal_emission_normalizes_radix_spelling() {
        assert_eq!(
            super::bigint_literal_decimal("0x8000000000000000"),
            "9223372036854775808"
        );
        assert_eq!(
            super::bigint_literal_decimal("0o1000000000000000000000"),
            "9223372036854775808"
        );
        assert_eq!(
            super::bigint_literal_decimal("0b1_00000000000000000000000000000000"),
            "4294967296"
        );
        assert_eq!(
            super::bigint_literal_decimal("12345678901234567890"),
            "12345678901234567890"
        );
    }

    #[test]
    fn default_generator_matches_new_generator() {
        let module = parse("value = 1\n").expect("test program should parse");
        let mut default_generator = CCodeGenerator::default();
        let mut new_generator = CCodeGenerator::new();
        assert_eq!(
            default_generator.generate(&module).unwrap(),
            new_generator.generate(&module).unwrap()
        );
    }

    #[test]
    fn native_string_operations_match_reference_example() {
        let source = r#"
def greet(name: str) -> str:
    return "Hello, " + name + "!"

message = greet("Lucid")
words = "lucid is clean".split()
capitalized = [w.upper() for w in words]
print(message)
print(" ".join(capitalized))
"#;
        let module = parse(source).expect("test program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_string_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("test program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled test program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "Hello, Lucid!\nLUCID IS CLEAN\n"
        );
    }

    #[test]
    fn native_string_methods_match_runtime() {
        let source = "s = \"  hello world  \"\nt = s.strip().replace(\"world\", \"lucid\").upper()\nprint(t)\nprint(t.startswith(\"HELLO\"))\nprint(t.endswith(\"LUCID\"))\n";
        let module = parse(source).expect("string methods should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_string_methods_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("string methods should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled string methods should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "HELLO LUCID\ntrue\ntrue\n"
        );
    }

    #[test]
    fn native_mixed_bigint_float_arithmetic_promotes_without_truncation() {
        let source =
            "a = 9223372036854775808 + 0.5\nb = 9223372036854775808 / 3.0\nprint(a)\nprint(b)\n";
        let module = parse(source).expect("mixed numeric source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_mixed_numeric_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("mixed numeric program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        let stdout = String::from_utf8_lossy(&run.stdout);
        assert!(
            stdout.lines().all(|line| line.parse::<f64>().is_ok()),
            "unexpected output: {stdout}"
        );
        let values = stdout
            .lines()
            .map(|line| {
                line.parse::<f64>()
                    .expect("native mixed result should be numeric")
            })
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 2, "unexpected mixed numeric output: {stdout}");
        let expected_a = 9223372036854775808.0_f64;
        let expected_b = expected_a / 3.0;
        assert!(
            (values[0] - expected_a).abs() <= expected_a * 1e-8,
            "bad promoted addition: {stdout}"
        );
        assert!(
            (values[1] - expected_b).abs() <= expected_b * 1e-8,
            "bad promoted division: {stdout}"
        );
    }

    #[test]
    fn native_set_discard_is_idempotent() {
        let source = "items = {1, 2}\nitems.discard(2)\nitems.discard(9)\nprint(len(items))\n";
        let module = parse(source).expect("set discard program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_set_discard_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("set discard should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native set discard failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n");
    }

    #[test]
    fn native_set_pop_returns_and_removes_an_element() {
        let source = "items = {1, 2}\nitem = items.pop()\nprint(item)\nprint(len(items))\n";
        let module = parse(source).expect("set pop program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_set_pop_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("set pop should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native set pop failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n1\n");
    }

    #[test]
    fn native_dict_pop_returns_and_removes_value() {
        let source =
            "items = {\"a\": 7}\nvalue = items.pop(\"a\")\nprint(value)\nprint(len(items))\n";
        let module = parse(source).expect("dict pop program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_dict_pop_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dict pop should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native dict pop failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n0\n");
    }

    #[test]
    fn native_dict_pop_accepts_non_string_keys() {
        let source = "items = {1: \"one\"}\nvalue = items.pop(1)\nprint(value)\n";
        let module = parse(source).expect("integer-key dict pop should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_dict_pop_int_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("integer-key dict pop should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(
            run.status.success(),
            "native integer-key dict pop failed: {:?}",
            run
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), "one\n");
    }

    #[test]
    fn native_frozen_empty_set_pop_reports_mutation_error_first() {
        let source = "items = freeze({})\nitems.pop()\n";
        let module = parse(source).expect("frozen set pop should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_frozen_set_pop_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("frozen set pop should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success());
        assert!(String::from_utf8_lossy(&run.stderr).contains("frozen set"));
    }

    #[test]
    fn native_collection_methods_match_runtime() {
        let source = "xs = [1, 3]\nxs.insert(1, 2)\nxs.extend([4])\nprint(xs.pop())\nprint(xs[1])\nprint(xs.pop(0, 2)[1])\nd = {\"a\": 1}\nprint(d.items()[0][1])\nxs.clear()\nprint(len(xs))\n";
        let module = parse(source).expect("collection methods should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_collection_methods_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("collection methods should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled collection methods should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "4\n2\n2\n1\n0\n");
    }

    #[test]
    fn native_list_extend_accepts_ranges() {
        let source = "xs = [1]\nxs.extend(range(2, 4))\nprint(xs[2])\n";
        let module = parse(source).expect("range extend source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_extend_range_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("range extend should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n");
    }

    #[test]
    fn native_string_join_accepts_ranges() {
        let source = "print(\"-\".join(range(1, 3)))\n";
        let module = parse(source).expect("range join source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_join_range_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("range join should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1-2\n");
    }

    #[test]
    fn native_loops_and_comprehensions_accept_string_and_set_iterables() {
        let source = "total = 0\nfor x in {1, 2, 3}:\n    total = total + x\nprint(total)\nprint([x for x in \"ab\"][1])\n";
        let module = parse(source).expect("iterable loop source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_iterable_loop_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("iterable loop source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled iterable loop should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "6\nb\n");
    }

    #[test]
    fn native_for_loop_binds_tuple_patterns() {
        let source =
            "pairs = [[1, 2], [3, 4]]\nfor (left, right) in pairs:\n    print(left + right)\n";
        let module = parse(source).expect("tuple loop source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_tuple_loop_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("tuple loop should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout), "3\n7\n");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_for_loop_binds_class_patterns() {
        let source = "class Pair:\n    left: int\n    right: int\npairs = [Pair(1, 2), Pair(3, 4)]\nfor Pair(left, right) in pairs:\n    print(left + right)\n";
        let module = parse(source).expect("class pattern loop source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_class_pattern_loop_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("class pattern loop should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout), "3\n7\n");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_let_binds_destructuring_patterns() {
        let source = "class Pair:\n    left: int\n    right: int\np = Pair(5, 7)\nlet Pair(a, b) = p\nprint(a + b)\n";
        let module = parse(source).expect("let destructuring source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_let_pattern_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("let destructuring should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "12");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_function_parameters_bind_patterns() {
        let source = "class Pair:\n    left: int\n    right: int\ndef total(Pair(a, b): Pair) -> int:\n    return a + b\nprint(total(Pair(6, 9)))\n";
        let module = parse(source).expect("pattern parameter source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_pattern_param_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("pattern parameter should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "15");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_with_binds_destructuring_patterns() {
        let source = "class Pair:\n    left: int\n    right: int\ncontextmanager def provide() -> Pair:\n    yield Pair(2, 5)\nwith provide() as Pair(a, b):\n    print(a + b)\n";
        let module = parse(source).expect("with pattern source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_with_pattern_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("with pattern should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "7");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_list_comprehension_binds_tuple_patterns() {
        let source = "pairs = [[1, 2], [3, 4]]\nsums = [a + b for (a, b) in pairs]\nprint(sums[0])\nprint(sums[1])\n";
        let module = parse(source).expect("tuple comprehension source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_tuple_comp_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("tuple comprehension should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout), "3\n7\n");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_match_checks_class_patterns() {
        let source = "class Pair:\n    left: int\n    right: int\nvalue = Pair(4, 6)\nmatch value:\n    case Pair(a, b):\n        print(a + b)\n    case _:\n        print(0)\n";
        let module = parse(source).expect("class match source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_class_match_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("class match should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "10");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_match_structural_patterns_fall_through_on_wrong_shape() {
        let source = "value = 9\nmatch value:\n    case (a, b):\n        print(a + b)\n    case _:\n        print(1)\n";
        let module = parse(source).expect("structural match source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_structural_match_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("structural match should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "1");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_indexing_dispatches_to_user_getitem() {
        let source = "class Pages:\n    def __getitem__(self, index: int) -> str:\n        return \"page \" + str(index)\np = Pages()\nprint(p[2])\n";
        let module = parse(source).expect("getitem source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_getitem_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("getitem source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled getitem should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "page 2\n");
    }

    #[test]
    fn native_index_assignment_dispatches_to_user_setitem() {
        let source = "class Box:\n    value: int\n    def __setitem__(self, index: int, value: int):\n        self.value = value + index\nb = Box(0)\nb[3] = 4\nprint(b.value)\n";
        let module = parse(source).expect("setitem source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_setitem_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("setitem source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled setitem should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n");
    }

    #[test]
    fn native_for_loop_dispatches_to_user_iter() {
        let source = "class Numbers:\n    def __iter__(self):\n        return [2, 4, 6]\nn = Numbers()\ntotal = 0\nfor x in n:\n    total = total + x\nprint(total)\n";
        let module = parse(source).expect("iter method source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_iter_method_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("iter method source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled iter method should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "12\n");
    }

    #[test]
    fn native_for_loop_dispatches_custom_iter_for_each_class() {
        let source = "class First:\n    def __iter__(self):\n        return [1]\nclass Second:\n    def __iter__(self):\n        return [2, 3]\ntotal = 0\nfor x in First():\n    total = total + x\nfor x in Second():\n    total = total + x\nprint(total)\n";
        let module = parse(source).expect("iter source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_iter_methods_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("custom iter should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "6\n");
    }

    #[test]
    fn native_list_constructor_drains_user_iterator() {
        let source = "class Counter:\n    current: int\n    def next(self):\n        if self.current >= 3:\n            return iteration.done\n        self.current = self.current + 1\n        return self.current - 1\nc = Counter(0)\nprint(list(c))\n";
        let module = parse(source).expect("iterator source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_list_iterator_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("iterator constructor should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "[list len=3]\n");
    }

    #[test]
    fn native_string_join_drains_user_iterator() {
        let source = "import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 3:\n            return iteration.done\n        self.current = self.current + 1\n        return self.current - 1\nprint(\"-\".join(Counter(0)))\n";
        let module = parse(source).expect("iterator join source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_join_iterator_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("iterator join should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "0-1-2\n");
    }

    #[test]
    fn native_list_extend_drains_user_iterator() {
        let source = "import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 3:\n            return iteration.done\n        self.current = self.current + 1\n        return self.current - 1\nxs = []\nxs.extend(Counter(0))\nprint(xs[2])\n";
        let module = parse(source).expect("iterator extend source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_extend_iterator_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("iterator extend should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "2\n");
    }

    #[test]
    fn native_list_and_set_from_dict_iterate_keys() {
        let source = "d = {\"a\": 1, \"b\": 2}\nprint(list(d)[0])\nprint(len(set(d)))\n";
        let module = parse(source).expect("dict iterable source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dict_iterable_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dict iterable should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "a\n2\n");
    }

    #[test]
    fn native_collection_equality_is_structural() {
        let source = "print([1, 2] == [1, 2])\nprint(freeze({1, 2}) == freeze({2, 1}))\nprint({\"a\": 1, \"b\": 2} == {\"b\": 2, \"a\": 1})\n";
        let module = parse(source).expect("collection equality source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_collection_equality_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("collection equality should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\ntrue\ntrue\n");
    }

    #[test]
    fn native_identity_does_not_compare_string_addresses() {
        let source = "left = \"lucid\"\nright = \"lu\" + \"cid\"\nprint(left === right)\nprint(left !== right)\n";
        let module = parse(source).expect("identity source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_identity_strings_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("identity source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\nfalse\n");
    }

    #[test]
    fn native_numeric_hashes_follow_numeric_equality() {
        let source = "print(hash(1) == hash(1.0))\nprint(hash(1) == hash(1 + 0j))\n";
        let module = parse(source).expect("numeric hash source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_numeric_hash_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("numeric hash source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\ntrue\n");
    }

    #[test]
    fn native_arbitrary_precision_integer_is_hashable() {
        let source = "print(hash(123456789012345678901234567890) is int)\n";
        let module = parse(source).expect("bigint hash source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_bigint_hash_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("bigint hash source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\n");
    }

    #[test]
    fn native_frozen_set_mutation_is_rejected() {
        let source = "items = freeze({1, 2})\nitems.clear()\n";
        let module = parse(source).expect("frozen set source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_frozen_set_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("frozen set source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(
            !run.status.success(),
            "mutating a frozen set unexpectedly succeeded"
        );
    }

    #[test]
    fn native_set_remove_reports_missing_value() {
        let source = "items = {1}\nitems.remove(2)\n";
        let module = parse(source).expect("set remove source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_set_remove_missing_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("set remove source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(
            !run.status.success(),
            "removing a missing set item unexpectedly succeeded"
        );
    }

    #[test]
    fn native_dict_clear_removes_all_values() {
        let source = "items = {\"a\": 1}\nitems.clear()\nprint(len(items))\n";
        let module = parse(source).expect("dict clear source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_dict_clear_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dict clear source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "0\n");
    }

    #[test]
    fn native_predicates_iterate_dict_keys() {
        let source = "d = {\"a\": 1, \"b\": 2}\nprint(any(d))\nprint(all(d))\n";
        let module = parse(source).expect("dict predicate source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dict_predicates_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dict predicates should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\ntrue\n");
    }

    #[test]
    fn native_sum_supports_bigints() {
        let source = "print(sum([12345678901234567890, 10]))\n";
        let module = parse(source).expect("bigint sum source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_bigint_sum_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("bigint sum should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "12345678901234567900\n"
        );
    }

    #[test]
    fn native_radix_bigint_literals_execute_correctly() {
        let source = "print(0x8000000000000000 + 1)\n";
        let module = parse(source).expect("radix bigint source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_radix_bigint_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("radix bigint should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "9223372036854775809\n"
        );
    }

    #[test]
    fn native_radix_bigint_spellings_compare_equal() {
        let source = "print(0x8000000000000000 == 0o1000000000000000000000)\n";
        let module = parse(source).expect("cross-radix source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_cross_radix_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("cross-radix program should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\n");
    }

    #[test]
    fn native_sum_preserves_float_values() {
        let source = "print(sum([1.5, 2.5]))\nprint(sum([1, 2.5]))\n";
        let module = parse(source).expect("float sum source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_float_sum_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("float sum should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "4\n3.5\n");
    }

    #[test]
    fn native_sum_honors_optional_start() {
        let source = "print(sum([1, 2, 3], 10))\nprint(sum([1.5, 2.5], 1.0))\n";
        let module = parse(source).expect("sum start source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_sum_start_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("sum start should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "16\n5\n");
    }

    #[test]
    fn native_comprehension_drains_user_iterator() {
        let source = "import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 3:\n            return iteration.done\n        self.current = self.current + 1\n        return self.current - 1\nprint(sum([x * 2 for x in Counter(0)]))\n";
        let module = parse(source).expect("iterator comprehension source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_iterator_comp_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("iterator comprehension should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "6\n");
    }

    #[test]
    fn native_comprehension_dispatches_materialized_user_iter() {
        let source = "class Numbers:\n    def __iter__(self):\n        return [1, 2, 3]\nprint(sum([x * 2 for x in Numbers()]))\n";
        let module = parse(source).expect("materialized iterator source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_materialized_iter_comp_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("materialized iterator should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "12\n");
    }

    #[test]
    fn native_enumerate_drains_user_iterator() {
        let source = "import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 2:\n            return iteration.done\n        self.current = self.current + 1\n        return self.current - 1\nprint(enumerate(Counter(0)))\n";
        let module = parse(source).expect("enumerate iterator source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_enumerate_iterator_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("enumerate iterator should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "[list len=2]\n");
    }

    #[test]
    fn native_map_drains_user_iterator() {
        let source = "import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 2:\n            return iteration.done\n        self.current = self.current + 1\n        return self.current - 1\ndef double(x: int) -> int:\n    return x * 2\nprint(sum(map(double, Counter(0))))\n";
        let module = parse(source).expect("map iterator source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_map_iterator_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("map iterator should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "2\n");
    }

    #[test]
    fn native_zip_drains_user_iterators() {
        let source = "import iteration\nclass Counter:\n    current: int\n    limit: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= self.limit:\n            return iteration.done\n        self.current = self.current + 1\n        return self.current - 1\na = Counter(0, 2)\nb = Counter(0, 2)\nprint(zip(a, b))\n";
        let module = parse(source).expect("zip iterator source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_zip_iterator_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("zip iterator should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "[list len=2]\n");
    }

    #[test]
    fn native_any_drains_user_iterator() {
        let source = "import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 2:\n            return iteration.done\n        self.current = self.current + 1\n        return self.current - 1\nprint(any(Counter(0)))\nprint(all(Counter(0)))\n";
        let module = parse(source).expect("predicate iterator source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_any_iterator_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("predicate iterator should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\nfalse\n");
    }

    #[test]
    fn native_min_max_drain_user_iterator_values() {
        let source = "import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 3:\n            return iteration.done\n        self.current = self.current + 1\n        return 3 - self.current\nprint(min(Counter(0)))\nprint(max(Counter(0)))\n";
        let module = parse(source).expect("min/max iterator source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_minmax_iterator_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("min/max iterator should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "0\n2\n");
    }

    #[test]
    fn native_sorted_drains_user_iterator_values() {
        let source = "import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 3:\n            return iteration.done\n        self.current = self.current + 1\n        return 3 - self.current\nprint(sorted(Counter(0)))\n";
        let module = parse(source).expect("sorted iterator source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_sorted_iterator_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("sorted iterator should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "[list len=3]\n");
    }

    #[test]
    fn native_operator_dispatch_uses_valid_mangled_symbols() {
        let source = r#"
class Vector:
    x: int
    y: int

dispatch def +(a: Vector, b: Vector) -> Vector:
    return Vector(a.x + b.x, a.y + b.y)

a = Vector(1, 2)
b = Vector(3, 4)
c = a + b
print(c.x, c.y)
"#;
        let module = parse(source).expect("test program should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dispatch_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dispatch program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dispatch program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "4 6\n");
    }

    #[test]
    fn native_binary_dispatch_selects_both_operand_types() {
        let source = "class Left:\n    value: int\nclass Right:\n    value: int\ndispatch def +(a: Left, b: Right) -> int:\n    return a.value + b.value\ndispatch def +(a: Right, b: Left) -> int:\n    return a.value * b.value\nprint(Left(2) + Right(3))\nprint(Right(3) + Left(2))\n";
        let module = parse(source).expect("binary dispatch source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_binary_dispatch_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("binary dispatch should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "5\n6\n");
    }

    #[test]
    fn native_dispatch_prefers_exact_over_object_fallback() {
        let source = "dispatch def choose(value: object) -> str:\n    return \"object\"\ndispatch def choose(value: int) -> str:\n    return \"int\"\nprint(choose(1))\n";
        let module = parse(source).expect("dispatch source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dispatch_specificity_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dispatch should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "int\n");
    }

    #[test]
    fn native_dispatch_adapts_scalar_to_object_parameter() {
        let source =
            "dispatch def choose(value: object) -> str:\n    return \"object\"\nprint(choose(1))\n";
        let module = parse(source).expect("dispatch source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dispatch_object_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("object dispatch should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "object\n");
    }

    #[test]
    fn native_dispatch_walks_class_hierarchy() {
        let source = "class Animal:\n    pass\nclass Dog(Animal):\n    pass\ndispatch def choose(value: Animal) -> str:\n    return \"animal\"\nprint(choose(Dog()))\n";
        let module = parse(source).expect("hierarchy dispatch source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dispatch_hierarchy_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("hierarchy dispatch should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "animal\n");
    }

    #[test]
    fn native_explicit_any_uses_universal_value_abi() {
        let source = "def identity(value: Any) -> Any:\n    return value\nprint(identity(7))\n";
        let module = parse(source).expect("Any source should parse");
        let output = std::env::temp_dir().join(format!("lucid_codegen_any_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        let result = compile_to_native(&module, &output, 0);
        if result.is_ok() {
            let run = Command::new(&output).output().expect("run Any binary");
            assert!(run.status.success(), "Any binary failed: {:?}", run);
            assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n");
        }
        let _ = fs::remove_file(&output);
        assert!(
            result.is_ok(),
            "explicit Any should use LucidVal: {result:?}"
        );
    }

    #[test]
    fn native_union_return_uses_tagged_value_abi() {
        let source = "def maybe(flag: int) -> int | none:\n    if flag:\n        return 1\n    return none\nprint(maybe(1))\n";
        let module = parse(source).expect("union source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_union_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("union return should compile");
        let run = Command::new(&output).output().expect("run union binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "union binary failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n");
    }

    #[test]
    fn native_union_parameter_wraps_scalar_argument() {
        let source =
            "def describe(value: int | none) -> str:\n    return \"value\"\nprint(describe(1))\n";
        let module = parse(source).expect("union parameter source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_union_param_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("union parameter should compile");
        let run = Command::new(&output)
            .output()
            .expect("run union parameter binary");
        let _ = fs::remove_file(&output);
        assert!(
            run.status.success(),
            "union parameter binary failed: {:?}",
            run
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), "value\n");
    }

    #[test]
    fn native_len_dispatches_to_custom_len_method() {
        let source = "class Bag:\n    items: list[int]\n    def __len__(self) -> int:\n        return len(self.items)\nprint(len(Bag([1, 2, 3])))\n";
        let module = parse(source).expect("custom len source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_custom_len_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("custom len should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n");
    }

    #[test]
    fn native_len_dispatches_to_inherited_custom_len_method() {
        let source = "class Bag:\n    def __len__(self) -> int:\n        return 7\nclass Child(Bag):\n    pass\nprint(len(Child()))\n";
        let module = parse(source).expect("inherited len source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_inherited_len_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("inherited len should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n");
    }

    #[test]
    fn native_membership_dispatches_to_custom_contains_method() {
        let source = "class Bag:\n    value: int\n    def __contains__(self, item: int) -> bool:\n        return item == self.value\nif 4 in Bag(4):\n    print(1)\nif 3 not in Bag(4):\n    print(1)\n";
        let module = parse(source).expect("custom contains source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_custom_contains_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("custom contains should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n1\n");
    }

    #[test]
    fn native_hash_dispatches_to_custom_hash_method() {
        let source = "class Key:\n    value: int\n    def __hash__(self) -> int:\n        return self.value * 10\nprint(hash(Key(7)))\n";
        let module = parse(source).expect("custom hash source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_custom_hash_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("custom hash should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "70\n");
    }

    #[test]
    fn native_reversed_dispatches_to_custom_reversed_method() {
        let source = "class Bag:\n    items: list[int]\n    def __reversed__(self) -> list[int]:\n        return [3, 2, 1]\nprint(reversed(Bag([1, 2, 3])))\n";
        let module = parse(source).expect("custom reversed source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_custom_reversed_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("custom reversed should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "[list len=3]\n");
    }

    #[test]
    fn native_if_uses_custom_bool_method() {
        let source = "class Flag:\n    value: bool\n    def __bool__(self) -> bool:\n        return self.value\nif Flag(false):\n    print(1)\nelse:\n    print(0)\n";
        let module = parse(source).expect("custom bool source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_custom_bool_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("custom bool should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "0\n");
    }

    #[test]
    fn native_derived_class_inherits_custom_bool_method() {
        let source = "class Flag:\n    value: bool\n    def __bool__(self) -> bool:\n        return self.value\nclass Child(Flag):\n    pass\nc = Child(false)\nif c:\n    print(1)\nelse:\n    print(0)\n";
        let module = parse(source).expect("inherited bool source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_inherited_bool_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("inherited bool should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "0\n");
    }

    #[test]
    fn native_and_or_preserve_selected_operand() {
        let source = "print(0 and 5)\nprint(2 and 5)\nprint(0 or 5)\nprint(2 or 5)\ndef identity(value: Any) -> Any:\n    return value\nzero = identity(0)\nfive = identity(5)\nprint(zero and five)\nprint(zero or five)\n";
        let module = parse(source).expect("logical source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_logical_values_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("logical operators should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "0\n5\n5\n2\n0\n5\n");
    }

    #[test]
    fn native_logical_conditions_preserve_custom_truthiness() {
        let source = "class Flag:\n    value: bool\n    def __bool__(self) -> bool:\n        return self.value\nclass Sized:\n    items: list[int]\n    def __len__(self) -> int:\n        return len(self.items)\nf: Flag = Flag(false)\nif f and true:\n    print(1)\nelse:\n    print(0)\nif f or false:\n    print(1)\nelse:\n    print(0)\ndef identity(value: Any) -> Any:\n    return value\ndynamic = identity(Flag(false))\nif dynamic:\n    print(1)\nelse:\n    print(0)\ndynamic_sized = identity(Sized([]))\nif dynamic_sized:\n    print(1)\nelse:\n    print(0)\n";
        let module = parse(source).expect("logical condition source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_logical_truthiness_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("logical conditions should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "0\n0\n0\n0\n");
    }

    #[test]
    fn native_tagged_truthiness_handles_empty_collections_and_numeric_zero() {
        let source = "if 0:\n    print(1)\nelse:\n    print(0)\nif complex(0, 0):\n    print(1)\nelse:\n    print(0)\nif {}:\n    print(1)\nelse:\n    print(0)\nif set():\n    print(1)\nelse:\n    print(0)\nif 100000000000000000000 - 100000000000000000000:\n    print(1)\nelse:\n    print(0)\n";
        let module = parse(source).expect("tagged truthiness source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_tagged_truthiness_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("tagged truthiness should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled tagged truthiness should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {run:?}");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "0\n0\n0\n0\n0\n");
    }

    #[test]
    fn native_sorted_uses_user_less_than_for_class_values() {
        let source = "class Item:\n    value: int\n    def __lt__(self, other: Item) -> bool:\n        return self.value < other.value\nitems: list[Item] = [Item(3), Item(1), Item(2)]\nprint(sorted(items))\nprint(sorted(items)[0].value)\n";
        let module = parse(source).expect("custom sort source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_custom_sorted_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("custom sort should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "[list len=3]\n1\n");
    }

    #[test]
    fn native_min_max_use_user_less_than_for_class_values() {
        let source = "class Item:\n    value: int\n    def __lt__(self, other: Item) -> bool:\n        return self.value < other.value\nitems: list[Item] = [Item(3), Item(1), Item(2)]\nprint(min(items).value)\nprint(max(items).value)\n";
        let module = parse(source).expect("custom min/max source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_custom_minmax_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("custom min/max should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n3\n");
    }

    #[test]
    fn native_function_parameter_preserves_list_element_class() {
        let source = "class Item:\n    value: int\n    def __lt__(self, other: Item) -> bool:\n        return self.value < other.value\ndef first(items: list[Item]) -> int:\n    return sorted(items)[0].value\nprint(first([Item(2), Item(1)]))\n";
        let module = parse(source).expect("typed list parameter source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_typed_list_param_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("typed list parameter should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n");
    }

    #[test]
    fn native_any_all_use_element_bool_protocol() {
        let source = "class Flag:\n    value: bool\n    def __bool__(self) -> bool:\n        return self.value\nflags: list[Flag] = [Flag(false), Flag(true)]\nprint(any(flags))\nprint(all(flags))\n";
        let module = parse(source).expect("predicate source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_any_all_bool_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("predicate protocol should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\nfalse\n");
    }

    #[test]
    fn native_dict_and_set_comprehensions_support_lookup_and_membership() {
        let source = r#"
lookup = {str(x): x * 10 for x in range(4)}
evens = {x for x in range(6) if x % 2 == 0}
print(lookup.get("2", 0), 4 in evens, 5 not in evens)
evens.add(6)
evens.remove(0)
print(len(evens))
"#;
        let module = parse(source).expect("test program should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_collections_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("collections should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled collections program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "20 true true\n3\n");
    }

    #[test]
    fn native_records_lower_to_field_lookups() {
        let source = r#"
point = (x=10, y=20)
print(point.x + point.y)
"#;
        let module = parse(source).expect("record program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_record_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("record program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled record program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "30\n");
    }

    #[test]
    fn native_try_catches_raise_and_runs_finally() {
        let source = r#"
try:
    raise "boom"
except str as error:
    print(error)
finally:
    print("clean")
"#;
        let module = parse(source).expect("try program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_try_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("try program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled try program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "boom\nclean\n");
    }

    #[test]
    fn native_handler_raise_still_runs_finally_before_rethrowing() {
        let source = r#"
try:
    raise "first"
except str:
    raise "second"
finally:
    print("clean")
"#;
        let module = parse(source).expect("try program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_rethrow_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("rethrow program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled rethrow program should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success());
        assert_eq!(String::from_utf8_lossy(&run.stdout), "clean\n");
    }

    #[test]
    fn native_try_selects_matching_handler() {
        let source = r#"
try:
    raise "boom"
except int:
    print("wrong")
except str:
    print("right")
"#;
        let module = parse(source).expect("try program should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_typed_try_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("typed try program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "right\n");
    }

    #[test]
    fn native_getters_and_classmethods_lower_to_entry_points() {
        let source = r#"
class Point:
    x: int
    y: int
    getter total(self) -> int:
        return self.x + self.y
    classmethod label(cls) -> str:
        return "Point"

p = Point(2, 3)
print(p.total, Point.label())
print(getattr(p, "total"), hasattr(p, "total"), hasattr(p, "missing"))
"#;
        let module = parse(source).expect("class member program should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_class_members_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("class member program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "5 Point\n5 true false\n"
        );
    }

    #[test]
    fn native_setter_lowers_to_set_entry_point() {
        let source = r#"
class Box:
    value: int
    setter value(self, new_value: int):
        self.value = new_value + 1

b = Box(0)
def make() -> Box:
    print("make")
    return b
setattr(make(), "value", 4)
print(b.value)
"#;
        let module = parse(source).expect("setter program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_setter_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("setter program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "make\n5\n");
    }

    #[test]
    fn native_match_guards_use_custom_truthiness() {
        let source = "class Flag:\n    value: bool\n    def __bool__(self) -> bool:\n        return self.value\nf: Flag = Flag(false)\nmatch 1:\n    case n if f:\n        print(1)\n    case _:\n        print(0)\n";
        let module = parse(source).expect("guarded match should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_match_guard_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("guarded match should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "0\n");
    }

    #[test]
    fn native_without_capability_affects_instance_checks() {
        let source = "class Token without Eq:\n    value: int\nt = Token(1)\nprint(t is Eq)\nprint(t is not Eq)\n";
        let module = parse(source).expect("capability check source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_without_capability_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("capability checks should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "false\ntrue\n");
    }

    #[test]
    fn native_bool_builtin_uses_custom_truthiness() {
        let source = "class Flag:\n    value: bool\n    def __bool__(self) -> bool:\n        return self.value\nf: Flag = Flag(false)\nprint(bool(f))\nprint(bool(3))\n";
        let module = parse(source).expect("bool source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_bool_builtin_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("bool source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "false\ntrue\n");
    }

    #[test]
    fn native_match_executes_literal_and_binding_arms() {
        let source = r#"
value = 2
match value:
    case 1:
        print("one")
    case n:
        print(n + 3)
"#;
        let module = parse(source).expect("match program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_match_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("match program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled match program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "5\n");
    }

    #[test]
    fn native_binary_buffers_lower_to_lists() {
        let source = r#"
buffer = bytearray(b"hi")
print(buffer[0])
view = memoryview(buffer)
print(view[1])
"#;
        let module = parse(source).expect("binary program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_binary_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("binary program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "104\n105\n");
    }

    #[test]
    fn native_bytes_conversion_preserves_payload() {
        let source = r#"
data = bytes([65, 66])
print(data)
"#;
        let module = parse(source).expect("source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_bytes_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("bytes source should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "AB");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_byte_conversions_reject_invalid_inputs() {
        let source = "data = bytearray([256])\nprint(data)\n";
        let module = parse(source).expect("source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_invalid_bytearray_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("source should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        let _ = std::fs::remove_file(output);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("0..255"));
    }

    #[test]
    fn native_string_literals_escape_c_control_characters() {
        let source = "print(\"line1\\nline2\\t\\\\\\\"\")\n";
        let module = parse(source).expect("escaped string source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_string_escape_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("escaped string should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "line1\nline2\t\\\"\n");
    }

    #[test]
    fn native_help_builtin_accepts_optional_subject() {
        let module = parse("help()\nhelp(1)\nprint(3)\n").expect("help source should parse");
        let output = std::env::temp_dir().join(format!("lucid_native_help_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("help source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n");
    }

    #[test]
    fn native_slice_builtin_builds_descriptor() {
        let source = "s = slice(1, 4, 2)\nprint(s[\"start\"])\nprint(s[\"step\"])\n";
        let module = parse(source).expect("slice source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_slice_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("slice source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n2\n");
    }

    #[test]
    fn native_type_and_identity_checks_have_distinct_meanings() {
        let source = r#"
value = 1
z = 2j
print(value is int)
print(value is class)
print(value is trait)
print(value === value)
print(z is complex)
"#;
        let module = parse(source).expect("identity program should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_identity_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("identity program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "true\ntrue\nfalse\ntrue\ntrue\n"
        );
    }

    #[test]
    fn native_callable_identity_check_tracks_function_bindings() {
        let source = "def f(x: int) -> int:\n    return x\nvalue = 1\nprint(f is Callable)\nprint(len is Callable)\nprint(fields is Callable)\nprint(freeze is Callable)\nprint(value is Callable)\n";
        let module = parse(source).expect("callable identity source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_callable_identity_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("callable identity should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "true\ntrue\ntrue\ntrue\nfalse\n"
        );
    }

    #[test]
    fn native_partial_values_are_callable() {
        let source = "def add(a: int, b: int) -> int:\n    return a + b\npart = add(_, 2)\nprint(part is Callable)\n";
        let module = parse(source).expect("partial callable source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_partial_callable_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("partial callable should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\n");
    }

    #[test]
    fn native_all_positional_variadic_function_packs_arguments() {
        let source =
            "def count(*values: int) -> int:\n    return len(values)\nprint(count(1, 2, 3))\n";
        let module = parse(source).expect("variadic source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_variadic_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("variadic source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n");
    }

    #[test]
    fn native_fixed_prefix_variadic_function_packs_tail() {
        let source = "def count(start: int, *values: int) -> int:\n    return start + len(values)\nprint(count(10, 2, 3, 4))\n";
        let module = parse(source).expect("mixed variadic source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_mixed_variadic_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("mixed variadic source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "13\n");
    }

    #[test]
    fn native_keyword_variadic_function_packs_named_arguments() {
        let source =
            "def count(**values: int) -> int:\n    return len(values)\nprint(count(a=1, b=2))\n";
        let module = parse(source).expect("keyword variadic source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_keyword_variadic_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("keyword variadic source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "2\n");
    }

    #[test]
    fn native_fixed_prefix_keyword_variadic_function_packs_tail() {
        let source = "def count(start: int, **values: int) -> int:\n    return start + len(values)\nprint(count(10, a=1, b=2))\n";
        let module = parse(source).expect("mixed keyword variadic source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_mixed_keyword_variadic_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0)
            .expect("mixed keyword variadic source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "12\n");
    }

    #[test]
    fn native_identity_checks_follow_class_inheritance() {
        let source = "class Child(Base):\n    extra: int\nclass Base:\n    value: int\nc = Child(1, 2)\nprint(c is Base)\nprint(c is Child)\nprint(c is not Base)\n";
        let module = parse(source).expect("inheritance identity source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_identity_inheritance_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("inheritance identity should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(
            String::from_utf8_lossy(&result.stdout),
            "true\ntrue\nfalse\n"
        );
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_propagated_union_values_support_dynamic_attributes() {
        let source = "class NotFoundError:\n    message: str\n    getter upper(self) -> str:\n        return self.message\n    def render(self) -> str:\n        return self.message\ndef lookup(key: str) -> int | NotFoundError:\n    if key == \"missing\":\n        return NotFoundError(\"item missing\")\n    return 100\ndef get_value(key: str) -> int | NotFoundError:\n    value = lookup(key)?\n    return value + 1\nprint(get_value(\"present\"))\nprint(get_value(\"missing\").message)\nprint(get_value(\"missing\").upper)\nprint(get_value(\"missing\").render())\n";
        let module = parse(source).expect("propagated union source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_propagated_union_attr_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("propagated union should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        let _ = std::fs::remove_file(output);
        assert!(result.status.success(), "native program failed: {result:?}");
        assert_eq!(
            String::from_utf8_lossy(&result.stdout),
            "101\nitem missing\nitem missing\nitem missing\n"
        );
    }

    #[test]
    fn native_getattr_supports_propagated_union_values() {
        let source = r#"
class NotFoundError:
    message: str
    setter message(self, next: str):
        self.message = next + "!"
def get_value() -> int | NotFoundError:
    return NotFoundError("item missing")
print(getattr(get_value(), "message"))
print(hasattr(get_value(), "message"))
print(hasattr(get_value(), "missing"))
print(getattr(get_value(), "missing", "fallback"))
value = get_value()
setattr(value, "message", "updated")
print(getattr(value, "message"))
"#;
        let module = parse(source).expect("union getattr source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_union_getattr_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("union getattr should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        let _ = std::fs::remove_file(output);
        assert!(result.status.success(), "native program failed: {result:?}");
        assert_eq!(
            String::from_utf8_lossy(&result.stdout),
            "item missing\ntrue\nfalse\nfallback\nupdated!\n"
        );
    }

    #[test]
    fn native_match_class_patterns_follow_inheritance_direction() {
        let source = "class Base:\n    value: int\nclass Child(Base):\n    extra: int\nchild = Child(1, 2)\nmatch child:\n    case Base:\n        print(1)\n    case _:\n        print(0)\nbase = Base(3)\nmatch base:\n    case Child:\n        print(0)\n    case _:\n        print(1)\n";
        let module = parse(source).expect("inheritance pattern source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_pattern_inheritance_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("inheritance patterns should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout), "1\n1\n");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_literal_argument_spreads_expand_at_call_sites() {
        let source = "def add(a: int, b: int) -> int:\n    return a + b\nprint(add(*[2, 5]))\n";
        let module = parse(source).expect("argument spread source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_arg_spread_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("argument spread should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "7");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_literal_keyword_spreads_expand_at_call_sites() {
        let source = "def add(a: int, b: int) -> int:\n    return a + b\nprint(add(**{\"a\": 2, \"b\": 5}))\n";
        let module = parse(source).expect("keyword spread source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_kw_spread_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("keyword spread should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "7");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_delete_allows_checked_rebinding() {
        let source = "value = 1\ndel value\nvalue = 2\nprint(value)\n";
        let module = parse(source).expect("delete program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_delete_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("delete program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled delete program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "2\n");
    }

    #[test]
    fn native_delete_removes_name_from_locals_reflection() {
        let source =
            "value = 1\ndel value\nprint(len(locals()))\nvalue = 2\nprint(len(locals()))\n";
        let module = parse(source).expect("delete reflection program should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_delete_locals_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("delete reflection should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled delete reflection should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "0\n1\n");
    }

    #[test]
    fn native_delete_reflection_respects_runtime_branch() {
        let source = "value = 1\nif false:\n    del value\nprint(len(locals()))\n";
        let module = parse(source).expect("conditional delete program should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_delete_branch_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("conditional delete should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled conditional delete should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n");
    }

    #[test]
    fn native_class_variables_are_shared() {
        let source = r#"
class Counter:
    classvar count: int = 0
    classmethod bump(cls) -> int:
        cls.count += 1
        return cls.count

print(Counter.bump())
print(Counter.bump())
print(Counter.count)
"#;
        let module = parse(source).expect("classvar program should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_classvar_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("classvar program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled classvar program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n2\n2\n");
    }

    #[test]
    fn native_async_future_is_awaitable_once() {
        let source = r#"
async def load(x: int) -> int:
    print("body")
    return x + 1

pending = load(4)
print("after")
print(await pending)
"#;
        let module = parse(source).expect("async program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_async_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("async program should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled async program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "after\nbody\n5\n");
    }

    #[test]
    fn native_async_methods_defer_body_until_await() {
        let source = r#"
class Worker:
    value: int

    async def run(self) -> int:
        print("body")
        return self.value

w = Worker(7)
pending = w.run()
print("after")
print(await pending)
"#;
        let module = parse(source).expect("async method program should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_async_method_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("async method should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled async method should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "after\nbody\n7\n");
    }

    #[test]
    fn native_async_classmethods_defer_body_until_await() {
        let source = r#"
class Worker:
    async classmethod run(cls) -> int:
        print("body")
        return 7

pending = Worker.run()
print("after")
print(await pending)
"#;
        let module = parse(source).expect("async classmethod program should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_async_classmethod_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("async classmethod should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled async classmethod should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "after\nbody\n7\n");
    }

    #[test]
    fn native_contextmanager_runs_teardown_after_body() {
        let source = r#"
contextmanager def managed() -> int:
    print("setup")
    yield 3
    print("teardown")

with managed() as value:
    print(value)
"#;
        let module = parse(source).expect("contextmanager program should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_context_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("contextmanager should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled contextmanager should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "setup\n3\nteardown\n");
    }

    #[test]
    fn native_contextmanager_runs_teardown_on_body_raise() {
        let source = r#"
contextmanager def managed() -> int:
    print("setup")
    yield 3
    print("teardown")

with managed() as value:
    raise "boom"
"#;
        let module = parse(source).expect("contextmanager raise program should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_context_raise_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("contextmanager raise should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled contextmanager raise should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success());
        assert_eq!(String::from_utf8_lossy(&run.stdout), "setup\nteardown\n");
    }

    #[test]
    fn native_contextmanager_unwinds_prior_context_on_later_setup_failure() {
        let source = r#"
contextmanager def good() -> int:
    print("good setup")
    yield 1
    print("good teardown")

contextmanager def bad() -> int:
    print("bad setup")
    raise "boom"
    yield 2

with good() as first, bad() as second:
    print(first + second)
"#;
        let module = parse(source).expect("nested contextmanager program should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_context_setup_failure_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("nested contextmanager should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled nested contextmanager should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success());
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "good setup\nbad setup\ngood teardown\n"
        );
    }

    #[test]
    fn native_contextmanager_captures_local_teardown_state() {
        let source = r#"
contextmanager def managed() -> int:
    state: int = 1
    yield state
    state = 2
    print(state)

with managed() as value:
    print(value)
"#;
        let module = parse(source).expect("local contextmanager should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_context_local_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("local contextmanager should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled local contextmanager should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n2\n");
    }

    #[test]
    fn native_class_contextmanager_runs_teardown() {
        let source = r#"
class Session:
    contextmanager def __cm__(self):
        print("setup")
        yield self
        print("teardown")

with Session() as session:
    print("body")
"#;
        let module = parse(source).expect("class contextmanager should parse");
        let mut debug_generator = super::CCodeGenerator::new();
        let debug_c = debug_generator
            .generate(&module)
            .expect("class contextmanager should generate");
        assert!(
            debug_c.contains("Session___cm__"),
            "generated class context manager entry point missing"
        );
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_class_context_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("class contextmanager should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled class contextmanager should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "setup\nbody\nteardown\n"
        );
    }

    #[test]
    fn native_contextmanager_classmethod_runs_teardown() {
        let source = r#"
class Session:
    contextmanager classmethod open(cls) -> int:
        print("setup")
        yield 4
        print("teardown")

with Session.open() as value:
    print(value)
"#;
        let module = parse(source).expect("classmethod contextmanager should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_classmethod_context_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("classmethod contextmanager should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled classmethod contextmanager should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "setup\n4\nteardown\n");
    }

    #[test]
    fn native_contextmanager_try_finally_runs_continuation() {
        let source = r#"
contextmanager def managed() -> int:
    print("setup")
    try:
        yield 5
    finally:
        print("teardown")

with managed() as value:
    print(value)
"#;
        let module = parse(source).expect("try/finally contextmanager should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_context_try_finally_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("try/finally contextmanager should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled try/finally contextmanager should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "setup\n5\nteardown\n");
    }

    #[test]
    fn native_contextmanager_try_except_handles_body_error() {
        let source = r#"
contextmanager def managed() -> int:
    try:
        yield 5
    except:
        print("handled")

with managed() as value:
    raise "boom"
"#;
        let module = parse(source).expect("try/except contextmanager should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_context_try_except_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("try/except contextmanager should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled try/except contextmanager should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success());
        assert_eq!(String::from_utf8_lossy(&run.stdout), "handled\n");
    }

    #[test]
    fn native_if_broken_runs_only_after_break() {
        let source = r#"
found = 0
for x in [1, 2, 3]:
    if x == 2:
        break
if_broken:
    found = 9
print(found)
"#;
        let module = parse(source).expect("if_broken source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_if_broken_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("if_broken source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled if_broken program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "9\n");
    }

    #[test]
    fn native_range_supports_descending_steps() {
        let source = r#"
for x in range(3, 0, -1):
    print(x)
if range(5, 5):
    print(1)
else:
    print(0)
"#;
        let module = parse(source).expect("descending range should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_descending_range_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("descending range should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled descending range should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n2\n1\n0\n");
    }

    #[test]
    fn native_range_comprehension_checks_zero_step() {
        let source = "values = [x for x in range(3, 0, 0)]\n";
        let module = parse(source).expect("range comprehension should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_zero_step_comprehension_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "zero-step range must fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("range() step cannot be zero"));
    }

    #[test]
    fn native_for_range_reports_step_overflow() {
        let source = "for value in range(9223372036854775806, 9223372036854775807, 2):\n    pass\n";
        let module = parse(source).expect("overflow range should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_range_overflow_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("overflow range should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled overflow range should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "range overflow must fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("range step overflow"));
    }

    #[test]
    fn native_scalar_addition_reports_overflow() {
        let source = "value = 9223372036854775807\nresult = value + 1\n";
        let module = parse(source).expect("overflow source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_integer_overflow_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled program should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "overflow must fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("integer overflow in +"));
    }

    #[test]
    fn native_scalar_subtraction_and_multiplication_report_overflow() {
        for (source, marker) in [
            (
                "value = -9223372036854775807\nresult = value - 2\n",
                "integer overflow in -",
            ),
            (
                "value = 9223372036854775807\nresult = value * 2\n",
                "integer overflow in *",
            ),
            (
                "value = -9223372036854775807 - 1\nresult = -value\n",
                "integer overflow in unary -",
            ),
        ] {
            let module = parse(source).expect("overflow source should parse");
            let output = std::env::temp_dir().join(format!(
                "lucid_codegen_scalar_overflow_{}",
                std::process::id()
            ));
            let _ = fs::remove_file(&output);
            compile_to_native(&module, &output, 0).expect("source should compile");
            let run = Command::new(&output)
                .output()
                .expect("compiled program should run");
            let _ = fs::remove_file(&output);
            assert!(!run.status.success(), "overflow must fail");
            assert!(String::from_utf8_lossy(&run.stderr).contains(marker));
        }
    }

    #[test]
    fn native_reversed_and_list_range_preserve_order() {
        let source = r#"
print(reversed([1, 2, 3])[0])
print(list(range(3, 0, -1))[0])
print(reversed("abc")[0])
print(reversed({1, 2})[0])
"#;
        let module = parse(source).expect("reversed source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_reversed_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("reversed source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled reversed program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n3\nc\n2\n");
    }

    #[test]
    fn native_zip_and_enumerate_are_strict_and_indexable() {
        let source = r#"
print(zip([1, 2], [3, 4])[1][0])
print(enumerate([7, 8], 4)[1][0])
print(zip("ab", {8, 9})[1][0])
"#;
        let module = parse(source).expect("zip/enumerate source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_iter_builtins_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("zip/enumerate source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled zip/enumerate program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "2\n5\nb\n");
    }

    #[test]
    fn native_any_and_all_follow_truthiness() {
        let source = r#"
print(any([false, 0, 3]))
print(all([true, 1, 2]))
print(all([true, 0, 2]))
print(any(""))
print(all({1, 2}))
"#;
        let module = parse(source).expect("any/all source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_any_all_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("any/all source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled any/all program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "true\ntrue\nfalse\nfalse\ntrue\n"
        );
    }

    #[test]
    fn native_sorted_orders_numeric_lists() {
        let source = "print(sorted([3, 1, 2])[0])\nprint(sorted([3, 1, 2])[2])\nprint(sorted({3, 1, 2})[1])\n";
        let module = parse(source).expect("sorted source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_sorted_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("sorted source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled sorted program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n3\n2\n");
    }

    #[test]
    fn native_pow_and_iter_match_builtin_behavior() {
        let source = "print(pow(2, 3))\nprint(iter([4, 5])[1])\nprint(iter(\"ab\")[1])\nprint(iter({6, 7})[0])\n";
        let module = parse(source).expect("pow/iter source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_pow_iter_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("pow/iter source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled pow/iter program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "8\n5\nb\n6\n");
    }

    #[test]
    fn native_integer_power_promotes_without_float_rounding() {
        let source = "print(2 ** 63)\n";
        let module = parse(source).expect("integer power source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_integer_power_bigint_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("integer power should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled integer power should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "9223372036854775808\n"
        );
    }

    #[test]
    fn native_hash_matches_scalar_builtin_behavior() {
        let source = "print(hash(42))\nprint(hash(\"ab\"))\nprint(hash(none))\n";
        let module = parse(source).expect("hash source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_hash_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("hash source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled hash program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n3105\n0\n");
    }

    #[test]
    fn native_abs_and_round_preserve_integer_results() {
        let source = "print(abs(-4))\nprint(round(2.6))\nprint(round(2.6, 1))\n";
        let module = parse(source).expect("numeric builtin source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_numeric_builtins_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("numeric builtin source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled numeric builtin program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "4\n3\n2.6\n");
    }

    #[test]
    fn native_round_rejects_more_than_two_arguments() {
        let module = parse("print(round(1.25, 1, 0))\n").expect("round source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_round_arity_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        let error = compile_to_native(&module, &output, 0).expect_err("round arity should fail");
        let _ = fs::remove_file(&output);
        assert!(
            error
                .to_string()
                .contains("round() takes one or two arguments")
        );
    }

    #[test]
    fn native_round_rejects_non_integer_ndigits() {
        let source = "print(round(1.25, 1.5))\n";
        let module = parse(source).expect("round ndigits source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_round_ndigits_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("round ndigits should compile");
        let run = Command::new(&output).output().expect("run round ndigits");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "non-integer ndigits should fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("ndigits must be an int"));
    }

    #[test]
    fn native_abs_rejects_wrong_arity() {
        let module = parse("print(abs(1, 2))\n").expect("abs source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_abs_arity_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        let error = compile_to_native(&module, &output, 0).expect_err("abs arity should fail");
        let _ = fs::remove_file(&output);
        assert!(
            error
                .to_string()
                .contains("abs() takes exactly one argument")
        );
    }

    #[test]
    fn native_int_rejects_invalid_string_literals() {
        let source = "print(int(\"12x\"))\n";
        let module = parse(source).expect("invalid int source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_int_invalid_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("invalid int source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "invalid int literal should fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("invalid literal for int()"));
    }

    #[test]
    fn native_int_preserves_big_integer_literals() {
        let source = "print(int(100000000000000000001))\n";
        let module = parse(source).expect("big integer conversion source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_int_bigint_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("big integer conversion should compile");
        let run = Command::new(&output).output().expect("run big integer conversion");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "big integer conversion failed: {run:?}");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "100000000000000000001\n");
    }

    #[test]
    fn native_int_preserves_big_integer_through_any() {
        let source = "def identity(value: Any) -> Any:\n    return value\nprint(int(identity(100000000000000000001)))\n";
        let module = parse(source).expect("dynamic big integer conversion source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_int_dynamic_bigint_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0)
            .expect("dynamic big integer conversion should compile");
        let run = Command::new(&output)
            .output()
            .expect("run dynamic big integer conversion");
        let _ = fs::remove_file(&output);
        assert!(
            run.status.success(),
            "dynamic big integer conversion failed: {run:?}"
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), "100000000000000000001\n");
    }

    #[test]
    fn native_int_preserves_big_integer_from_string() {
        let source = "print(int(\"100000000000000000001\"))\n";
        let module = parse(source).expect("string big integer conversion source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_int_string_bigint_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0)
            .expect("string big integer conversion should compile");
        let run = Command::new(&output)
            .output()
            .expect("run string big integer conversion");
        let _ = fs::remove_file(&output);
        assert!(
            run.status.success(),
            "string big integer conversion failed: {run:?}"
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), "100000000000000000001\n");
    }

    #[test]
    fn native_int_float_special_values_match_interpreter_casts() {
        let source = "print(int(float.inf))\nprint(int(-float.inf))\nprint(int(float.nan))\n";
        let module = parse(source).expect("special int conversion source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_int_float_special_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0)
            .expect("special int conversion should compile");
        let run = Command::new(&output)
            .output()
            .expect("run special int conversion");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "special int conversion failed: {run:?}");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "int.inf\nint.nan\n0\n");
    }

    #[test]
    fn native_round_float_special_values_match_interpreter_casts() {
        let source = "print(round(float.inf))\nprint(round(-float.inf))\nprint(round(float.nan))\n";
        let module = parse(source).expect("special round source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_round_float_special_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("special round should compile");
        let run = Command::new(&output)
            .output()
            .expect("run special round");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "special round failed: {run:?}");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "int.inf\nint.nan\n0\n");
    }

    #[test]
    fn native_float_rejects_invalid_string_literals() {
        let source = "print(float(\"1.2x\"))\n";
        let module = parse(source).expect("invalid float source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_float_invalid_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("invalid float source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "invalid float literal should fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("invalid literal for float()"));
    }

    #[test]
    fn native_abs_supports_bigints_and_complex_numbers() {
        let source = "import math\nprint(abs(-999999999999999999999999999999))\nprint(abs(3 + 4j))\nprint(math.abs(3 + 4j))\n";
        let module = parse(source).expect("abs source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_abs_extended_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("abs source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled abs program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "999999999999999999999999999999\n5\n5\n"
        );
    }

    #[test]
    fn native_bigint_invert_uses_twos_complement_identity() {
        let source = "print(~123456789012345678901234567890)\n";
        let module = parse(source).expect("bigint invert source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_bigint_invert_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("bigint invert should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "-123456789012345678901234567891\n"
        );
    }

    #[test]
    fn native_three_argument_pow_uses_modular_exponentiation() {
        let source = "print(pow(2, 10, 1000))\n";
        let module = parse(source).expect("modular pow source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_pow_mod_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("modular pow source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "24\n");
    }

    #[test]
    fn native_complex_power() {
        let source = "print((1 + 1j) ** (1 + 1j))\n";
        let module = parse(source).expect("complex power source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_complex_pow_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("complex power source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        let output = String::from_utf8_lossy(&run.stdout);
        assert!(
            output.contains("0.273957") && output.contains("0.583701"),
            "unexpected complex result: {output}"
        );
    }

    #[test]
    fn native_iteration_done_module() {
        let source = "import iteration\nprint(iteration.done)\n";
        let module = parse(source).expect("iteration source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_iteration_done_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("iteration source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "iteration.done\n");
    }

    #[test]
    fn native_generated_replace_factory() {
        let source = "class Point:\n    x: int\n    y: int\np = Point(1, 2)\nq = Point.replace(p, y=5)\nprint(q.x + q.y)\n";
        let module = parse(source).expect("replace source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_replace_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("replace source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "6\n");
    }

    #[test]
    fn native_str_base_factories() {
        let source = "print(str.bin(255))\nprint(str.oct(8))\nprint(str.hex(255))\n";
        let module = parse(source).expect("base factory source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_str_base_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("base factory source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "0b11111111\n0o10\n0xff\n"
        );
    }

    #[test]
    fn native_str_base_factory_alias_is_callable() {
        let source = "format_base = str.hex\nprint(format_base(16))\n";
        let module = parse(source).expect("base factory alias source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_str_base_alias_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("base factory alias source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "0x10\n");
    }

    #[test]
    fn native_assert_lazy_message_is_only_used_on_failure() {
        let source = "assert(true, def: \"not evaluated\")\nprint(1)\n";
        let module = parse(source).expect("lazy assertion source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_lazy_assert_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("lazy assertion source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n");
    }

    #[test]
    fn native_for_consumes_custom_iterator_until_done() {
        let source = "import iteration\nclass Counter:\n    n: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.n >= 3:\n            return iteration.done\n        self.n += 1\n        return self.n - 1\nc = Counter(0)\nresult = 0\nfor value in c:\n    result += value\nprint(result)\n";
        let module = parse(source).expect("native iterator source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_custom_iterator_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("native iterator source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n");
    }

    #[test]
    fn native_augmented_assignment_updates_instance_fields() {
        let source = "class Counter:\n    n: int\n    def increment(self):\n        self.n += 1\nc = Counter(0)\nc.increment()\nprint(c.n)\n";
        let module = parse(source).expect("field assignment source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_field_augassign_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("field assignment source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n");
    }

    #[test]
    fn native_numeric_aggregates_accept_sets() {
        let source = "print(sum({1, 2, 3}))\nprint(min({3, 1, 2}))\nprint(max({3, 1, 2}))\n";
        let module = parse(source).expect("aggregate source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_aggregate_set_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("aggregate source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled aggregate program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "6\n1\n3\n");
    }

    #[test]
    fn native_fields_preserve_declared_order() {
        let source = "class Point:\n    x: int\n    y: int\n    def move(self):\n        pass\np = Point(1, 2)\nprint(fields(p)[0])\nprint(fields(p)[1])\nprint(fields(Point)[0])\nprint(fields(Point)[2])\n";
        let module = parse(source).expect("fields source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_fields_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("fields source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled fields program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "x\ny\nx\nmove\n");
    }

    #[test]
    fn native_env_var_returns_default_for_missing_name() {
        let source = "print(env_var(\"LUCID_CODEGEN_MISSING_ENV_VAR_9F4A\", \"fallback\"))\n";
        let module = parse(source).expect("env_var source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_env_var_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("env_var source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled env_var program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "fallback\n");
    }

    #[test]
    fn native_file_builtins_round_trip_text() {
        let path = std::env::temp_dir().join(format!(
            "lucid_codegen_file_builtin_{}.txt",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let source = format!(
            "write_file(\"{}\", \"hello\")\nprint(read_file(\"{}\"))\n",
            path.display(),
            path.display()
        );
        let module = parse(&source).expect("file builtin source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_file_builtin_bin_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("file builtin source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled file builtin program should run");
        let _ = fs::remove_file(&output);
        let _ = fs::remove_file(&path);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "hello\n");
    }

    #[test]
    fn native_time_monotonic_matches_runtime_module_api() {
        let source = "import time\nprint(time.monotonic() > 0)\n";
        let module = parse(source).expect("time module source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_time_monotonic_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("time.monotonic should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled time program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\n");
    }

    #[test]
    fn native_from_time_import_supports_monotonic() {
        let source = "from time import monotonic\nprint(monotonic() > 0)\n";
        let module = parse(source).expect("from-time source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_from_time_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("from time import should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled from-time program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\n");
    }

    #[test]
    fn native_from_math_import_supports_constants_and_functions() {
        let source = "from math import pi, sqrt\nprint(pi > 3)\nprint(sqrt(9))\n";
        let module = parse(source).expect("from-math source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_from_math_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("from math import should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled from-math program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\n3\n");
    }

    #[test]
    fn native_from_sys_import_supports_platform() {
        let source = "from sys import platform\nprint(platform)\n";
        let module = parse(source).expect("from-sys source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_from_sys_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("from sys import should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled from-sys program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            format!("{}\n", std::env::consts::OS)
        );
    }

    #[test]
    fn native_from_iteration_import_supports_done_sentinel() {
        let source = "from iteration import done\nprint(done)\n";
        let module = parse(source).expect("from-iteration source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_from_iteration_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("from iteration import should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled from-iteration program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "iteration.done\n");
    }

    #[test]
    fn native_string_membership_matches_interpreter() {
        let source = "print(\"ell\" in \"hello\")\nprint(\"z\" not in \"hello\")\n";
        let module = parse(source).expect("string membership source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_string_membership_{}_{}",
            std::process::id(),
            format!("{:?}", std::thread::current().id()).replace(['(', ')'], "")
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("string membership should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled membership program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\ntrue\n");
    }

    #[test]
    fn native_wrapped_string_length_is_unicode_aware() {
        let source = "values = {\"text\": \"café\"}\nprint(len(values[\"text\"]))\n";
        let module = parse(source).expect("wrapped string length source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_wrapped_string_len_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("wrapped string length should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled wrapped string length should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "4\n");
    }

    #[test]
    fn native_string_membership_uses_substring_semantics() {
        let source = "print(\"ell\" in \"hello\")\nprint(\"x\" not in \"hello\")\n";
        let module = parse(source).expect("string membership source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_string_membership_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("string membership should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled string membership program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\ntrue\n");
    }

    #[test]
    fn native_string_repeat_matches_runtime() {
        let source = "print(\"ha\" * 3)\nprint(\"ha\" * -1)\n";
        let module = parse(source).expect("string repeat source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_string_repeat_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("string repeat should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled string repeat program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "hahaha\n\n");
    }

    #[test]
    fn native_list_repeat_supports_arbitrary_lists_and_reflected_order() {
        let source = "a = [1, 2] * 2\nb = 2 * [1, 2]\nprint(len(a))\nprint(a[3])\nprint(len(b))\nprint(b[3])\n";
        let module = parse(source).expect("list repeat source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_list_repeat_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("list repeat should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled list repeat program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "4\n2\n4\n2\n");
    }

    #[test]
    fn native_list_add_concatenates_lists() {
        let source = "joined = [1, 2] + [3, 4]\nprint(len(joined))\nprint(joined[2])\n";
        let module = parse(source).expect("list concatenation source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_list_concat_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("list concatenation should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled list concatenation program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "4\n3\n");
    }

    #[test]
    fn native_collection_indexing_rejects_out_of_range() {
        let source = "print([1][2])\n";
        let module = parse(source).expect("invalid indexing source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_invalid_index_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("invalid indexing should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled invalid indexing program should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "out-of-range indexing should fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("out of range"));
    }

    #[test]
    fn native_list_assignment_rejects_out_of_range() {
        let source = "items = [1]\nitems[1] = 2\n";
        let module = parse(source).expect("invalid assignment source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_invalid_list_assignment_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("invalid assignment should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled invalid assignment should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "out-of-range assignment should fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("out of range"));
    }

    #[test]
    fn native_dynamic_indexing_rejects_non_container() {
        let source = "def identity(value: Any) -> Any:\n    return value\nprint(identity(7)[0])\n";
        let module = parse(source).expect("dynamic indexing source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_invalid_dynamic_index_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic indexing should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic indexing program should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "non-container indexing should fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("indexing not supported"));
    }

    #[test]
    fn native_dynamic_slicing_preserves_container_kind() {
        let source = "def identity(value: Any) -> Any:\n    return value\nprint(identity(\"hello\")[1:4])\nprint(len(identity([1, 2, 3, 4])[1:3]))\nprint(identity([1, 2, 3, 4])[1:3][0])\n";
        let module = parse(source).expect("dynamic slicing source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_slice_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic slicing should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic slicing program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "dynamic slicing failed: {run:?}");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "ell\n2\n2\n");
    }

    #[test]
    fn native_dynamic_dict_indexing_preserves_key_type() {
        let source = "def identity(value: Any) -> Any:\n    return value\nprint(identity({\"answer\": 42})[\"answer\"])\n";
        let module = parse(source).expect("dynamic dictionary indexing should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_dict_index_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic dictionary indexing should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic dictionary indexing should run");
        let _ = fs::remove_file(&output);
        assert!(
            run.status.success(),
            "dynamic dictionary indexing failed: {run:?}"
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
    }

    #[test]
    fn native_dynamic_dict_assignment_preserves_container_kind() {
        let source = "def identity(value: Any) -> Any:\n    return value\nitems = identity({\"answer\": 1})\nitems[\"answer\"] = 42\nprint(items[\"answer\"])\n";
        let module = parse(source).expect("dynamic dictionary assignment should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_dict_assignment_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0)
            .expect("dynamic dictionary assignment should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic dictionary assignment should run");
        let _ = fs::remove_file(&output);
        assert!(
            run.status.success(),
            "dynamic dictionary assignment failed: {run:?}"
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
    }

    #[test]
    fn native_dynamic_addition_preserves_container_kind() {
        let source = "def identity(value: Any) -> Any:\n    return value\nprint(identity(\"a\") + identity(\"b\"))\nleft = identity([1])\nright = identity([2])\nitems = left + right\nprint(len(items))\nprint(items[1])\nprint(identity(100000000000000000000) + identity(1))\n";
        let module = parse(source).expect("dynamic addition should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_dynamic_add_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic addition should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic addition should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "dynamic addition failed: {run:?}");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "ab\n2\n2\n100000000000000000001\n"
        );
    }

    #[test]
    fn native_dynamic_multiplication_preserves_container_kind() {
        let source = "def identity(value: Any) -> Any:\n    return value\ntext = identity(\"ab\")\ncount = identity(2)\nprint(text * count)\nitems = identity([1, 2])\nitems = items * count\nprint(len(items))\nprint(items[2])\n";
        let module = parse(source).expect("dynamic multiplication should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_dynamic_mul_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic multiplication should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic multiplication should run");
        let _ = fs::remove_file(&output);
        assert!(
            run.status.success(),
            "dynamic multiplication failed: {run:?}"
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), "abab\n4\n1\n");
    }

    #[test]
    fn native_dynamic_division_preserves_numeric_kind() {
        let source = "def identity(value: Any) -> Any:\n    return value\nleft = identity(7)\nright = identity(2)\nprint(left / right)\nprint(identity(100000000000000000001) / identity(2))\n";
        let module = parse(source).expect("dynamic division should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_division_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic division should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic division should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "dynamic division failed: {run:?}");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "3.5\n5e+19\n");
    }

    #[test]
    fn native_dynamic_complex_arithmetic_preserves_value_kind() {
        let source = "def identity(value: Any) -> Any:\n    return value\nprint(identity(1 + 2j) + identity(3 + 4j))\nprint(identity(1 + 2j) * identity(3 + 4j))\nprint(identity(1 + 2j) / identity(3 + 4j))\nprint(identity(1 + 2j) ** identity(2j))\n";
        let module = parse(source).expect("dynamic complex arithmetic should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_complex_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic complex arithmetic should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic complex arithmetic should run");
        let _ = fs::remove_file(&output);
        assert!(
            run.status.success(),
            "dynamic complex arithmetic failed: {run:?}"
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "(4+6j)\n(-5+10j)\n(0.44+0.08j)\n(-0.00421978+0.109149j)\n"
        );
    }

    #[test]
    fn native_dynamic_floor_division_and_remainder_preserve_numeric_kind() {
        let source = "def identity(value: Any) -> Any:\n    return value\nleft = identity(7)\nright = identity(2)\nprint(left // right)\nprint(left % right)\nprint(identity(100000000000000000001) // identity(2))\nprint(identity(100000000000000000001) % identity(2))\n";
        let module = parse(source).expect("dynamic floor operations should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_floor_ops_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic floor operations should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic floor operations should run");
        let _ = fs::remove_file(&output);
        assert!(
            run.status.success(),
            "dynamic floor operations failed: {run:?}"
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "3\n1\n50000000000000000000\n1\n"
        );
    }

    #[test]
    fn native_dynamic_power_preserves_numeric_kind() {
        let source = "def identity(value: Any) -> Any:\n    return value\nbase = identity(2)\nexponent = identity(10)\nprint(base ** exponent)\nprint(identity(4.0) ** identity(0.5))\nprint(identity(100000000000000000001) ** identity(2))\n";
        let module = parse(source).expect("dynamic power should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_power_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic power should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic power should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "dynamic power failed: {run:?}");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "1024\n2\n10000000000000000000200000000000000000001\n"
        );
    }

    #[test]
    fn native_dynamic_unary_operations_preserve_numeric_kind() {
        let source = "def identity(value: Any) -> Any:\n    return value\nvalue = identity(4)\nprint(-value)\nprint(+value)\nprint(~value)\nbig = identity(100000000000000000001)\nprint(-big)\nprint(+big)\nprint(~big)\n";
        let module = parse(source).expect("dynamic unary operations should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_unary_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic unary operations should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic unary operations should run");
        let _ = fs::remove_file(&output);
        assert!(
            run.status.success(),
            "dynamic unary operations failed: {run:?}"
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "-4\n4\n-5\n-100000000000000000001\n100000000000000000001\n-100000000000000000002\n"
        );
    }

    #[test]
    fn native_dynamic_shifts_preserve_numeric_kind() {
        let source = "def identity(value: Any) -> Any:\n    return value\nleft = identity(2)\nshift = identity(3)\nprint(left << shift)\nprint(left << shift >> identity(1))\n";
        let module = parse(source).expect("dynamic shifts should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_shifts_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic shifts should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic shifts should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "dynamic shifts failed: {run:?}");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "16\n8\n");
    }

    #[test]
    fn native_dynamic_comparisons_preserve_value_kind() {
        let source = "class Base:\n    pass\nclass Child(Base):\n    pass\ndef identity(value: Any) -> Any:\n    return value\nleft = identity(\"alpha\")\nright = identity(\"beta\")\nbig_left = identity(100000000000000000000)\nbig_right = identity(99999999999999999999)\nchild = identity(Child())\nprint(left < right)\nprint(big_left > big_right)\nprint(child is Base)\n";
        let module = parse(source).expect("dynamic comparisons should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_comparisons_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic comparisons should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic comparisons should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "dynamic comparisons failed: {run:?}");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\ntrue\ntrue\n");
    }

    #[test]
    fn native_dynamic_comparisons_reject_unsupported_containers() {
        let source = "def identity(value: Any) -> Any:\n    return value\nprint(identity([1]) < identity([2]))\n";
        let module = parse(source).expect("unsupported dynamic comparison should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_invalid_dynamic_comparison_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("unsupported comparison should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled unsupported comparison should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "container comparison should fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("unsupported operands for <"));
    }

    #[test]
    fn native_dynamic_membership_preserves_container_kind() {
        let source = "def identity(value: Any) -> Any:\n    return value\ntext = identity(\"hello\")\nitems = identity({\"answer\": 42})\nnumbers = identity([1, 2, 3])\nprint(\"ell\" in text)\nprint(\"answer\" in items)\nprint(2 in numbers)\nprint(9 not in numbers)\n";
        let module = parse(source).expect("dynamic membership should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_membership_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic membership should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic membership should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "dynamic membership failed: {run:?}");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "true\ntrue\ntrue\ntrue\n"
        );
    }

    #[test]
    fn native_dynamic_set_algebra_preserves_container_kind() {
        let source = "def identity(value: Any) -> Any:\n    return value\na = identity({1, 2})\nb = identity({2, 3})\nprint(len(a & b))\nprint(len(a | b))\nprint(len(a - b))\nprint(len(a ^ b))\nprint(identity(5) - identity(2))\n";
        let module = parse(source).expect("dynamic set algebra should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_set_algebra_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic set algebra should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dynamic set algebra should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "dynamic set algebra failed: {run:?}");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n3\n1\n2\n3\n");
    }

    #[test]
    fn native_dict_indexing_rejects_missing_key() {
        let source = "print({\"present\": 1}[\"missing\"])\n";
        let module = parse(source).expect("missing-key source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_missing_dict_key_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("missing-key source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled missing-key program should run");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "missing dict key should fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("key not found"));
    }

    #[test]
    fn native_dict_contains_reports_key_membership() {
        let source = "items = {\"present\": 1}\nprint(items.contains(\"present\"))\nprint(items.contains(\"missing\"))\n";
        let module = parse(source).expect("dict contains source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dict_contains_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dict contains should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled dict contains program should run");
        let _ = fs::remove_file(&output);
        assert!(
            run.status.success(),
            "native dict contains failed: {:?}",
            run
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\nfalse\n");
    }

    #[test]
    fn native_sys_module_exposes_runtime_members() {
        let source = "import sys\nprint(sys.platform)\nprint(sys.version)\nprint(sys.argv[0])\n";
        let module = parse(source).expect("sys module source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_sys_module_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("sys module should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled sys program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        let stdout = String::from_utf8_lossy(&run.stdout);
        assert!(stdout.lines().next().is_some_and(|line| !line.is_empty()));
        assert!(
            stdout.contains("0.1.0\nlucid\n"),
            "unexpected output: {stdout}"
        );
    }

    #[test]
    fn native_sys_metadata_matches_runtime_module_api() {
        let source = "import sys\nprint(sys.platform)\nprint(sys.version)\n";
        let module = parse(source).expect("sys module source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_sys_metadata_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("sys metadata should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled sys program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            format!("{}\n0.1.0\n", std::env::consts::OS)
        );
    }

    #[test]
    fn native_list_copies_sets() {
        let source = "print(len(list({1, 2, 3})))\n";
        let module = parse(source).expect("set-to-list source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_set_to_list_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("set-to-list source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled set-to-list program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n");
    }

    #[test]
    fn native_list_splits_strings_into_characters() {
        let source = "print(list(\"abc\")[1])\n";
        let module = parse(source).expect("string-to-list source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_string_to_list_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("string-to-list source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled string-to-list program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "b\n");
    }

    #[test]
    fn native_list_without_arguments_is_empty() {
        let source = "print(len(list()))\n";
        let module = parse(source).expect("empty list source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_empty_list_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("empty list source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled empty list program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "0\n");
    }

    #[test]
    fn native_next_consumes_lists_and_uses_default() {
        let source =
            "items = [4, 5]\nprint(next(items))\nprint(next(items, 9))\nprint(next(items, 9))\n";
        let module = parse(source).expect("next source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_next_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("next source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled next program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "4\n5\n9\n");
    }

    #[test]
    fn native_format_supports_numeric_specs() {
        let source =
            "print(format(255, \"x\"))\nprint(format(5, \"b\"))\nprint(format(1.5, \"f\"))\n";
        let module = parse(source).expect("format source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_format_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("format source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled format program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "ff\n101\n1.500000\n");
    }

    #[test]
    fn native_ord_and_chr_round_trip_ascii() {
        let source = "print(ord(\"A\"))\nprint(chr(66))\n";
        let module = parse(source).expect("ord/chr source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_ord_chr_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("ord/chr source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled ord/chr program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "65\nB\n");
    }

    #[test]
    fn native_ord_and_chr_handle_unicode_codepoints() {
        let source = "print(ord(\"é\"))\nprint(chr(128512))\nprint(len(\"aé😀\"))\nprint(list(\"aé😀\")[1])\n";
        let module = parse(source).expect("unicode ord/chr source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_unicode_ord_chr_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("unicode ord/chr source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled unicode ord/chr should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "233\n😀\n3\né\n");
    }

    #[test]
    fn native_complex_literals_and_arithmetic() {
        let source = "def negate(z: complex) -> complex:\n    return -z\nz = 1 + 2j\nw = z * (3 + 4j)\nprint(w)\nprint(-z)\nprint(negate(2j))\nprint(z == complex(1, 2))\n";
        let module = parse(source).expect("complex source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_complex_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("complex source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled complex program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "(-5+10j)\n(-1-2j)\n(-0-2j)\ntrue\n"
        );
    }

    #[test]
    fn native_numeric_special_class_values() {
        let source = "print(float.inf)\nprint(float.nan != float.nan)\nprint(complex.nan)\n";
        let module = parse(source).expect("special numeric source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_special_numeric_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("special numeric source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled special numeric should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "inf\ntrue\n(nan+nanj)\n"
        );
    }

    #[test]
    fn native_integer_special_values_and_zero_division() {
        let source = "print(int.inf)\nprint(int.nan)\nprint(5 // 0)\nprint(0 // 0)\nprint(5 % 0)\nprint(5 / 0)\n";
        let module = parse(source).expect("integer special source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_integer_special_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("integer special source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled integer special program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "int.inf\nint.nan\nint.inf\nint.nan\nint.nan\ninf\n"
        );
    }

    #[test]
    fn native_arbitrary_precision_integer_literals_preserve_digits() {
        let source = "x = 999999999999999999999999999999\ny = x + 2\nz = y * 3\nq = z // 3\nr = z % 7\np = x ** 2\na = -999999999999999999999999999999 // 2\nb = -999999999999999999999999999999 % 2\nprint(x)\nprint(y)\nprint(z)\nprint(q)\nprint(r)\nprint(p)\nprint(a)\nprint(b)\n";
        let module = parse(source).expect("big integer source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_big_integer_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("big integer source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled big integer program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "999999999999999999999999999999\n1000000000000000000000000000001\n3000000000000000000000000000003\n1000000000000000000000000000001\n6\n999999999999999999999999999998000000000000000000000000000001\n-500000000000000000000000000000\n1\n"
        );
    }

    #[test]
    fn native_math_imports_match_interpreter_builtins() {
        let source = "import math\nfrom math import sqrt, pi\nprint(sqrt(16))\nprint(math.abs(-42))\nprint(pi > 3.0)\nprint(math.e > 2.0)\n";
        let module = parse(source).expect("math import source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_math_import_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("math import source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled math import program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "4\n42\ntrue\ntrue\n");
    }

    #[test]
    fn native_integer_radix_builtins_match_formatting() {
        let source = "print(bin(10))\nprint(oct(10))\nprint(hex(255))\n";
        let module = parse(source).expect("radix source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_radix_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("radix source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled radix program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1010\n12\nff\n");
    }

    #[test]
    fn native_repr_handles_scalar_values() {
        let source = "print(repr(7))\nprint(repr(\"ok\"))\nprint(repr(true))\nprint(repr(2j))\n";
        let module = parse(source).expect("repr source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_repr_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("repr source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled repr program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "7\n\"ok\"\ntrue\n(0+2j)\n"
        );
    }

    #[test]
    fn native_locals_returns_current_bindings() {
        let source = "x = 1\ny = 2\nprint(len(locals()))\nprint(locals()[\"x\"])\n";
        let module = parse(source).expect("locals source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_locals_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("locals source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled locals program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "2\n1\n");
    }

    #[test]
    fn native_getattr_and_hasattr_use_declared_fields() {
        let source = r#"
class Point:
    x: int
    y: int
    def render(self) -> int:
        return self.x + self.y
p = Point(4, 9)
print(getattr(p, "x"))
setattr(p, "x", 6)
print(p.x)
name = "x"
print(getattr(p, name))
setattr(p, name, 7)
print(p.x)
def attr_name() -> str:
    print("attribute")
    return "x"
print(getattr(p, attr_name()))
print(hasattr(p, "y"))
print(hasattr(p, "z"))
print(hasattr(p, "render"))
print(getattr(p, "z", 42))
"#;
        let module = parse(source).expect("reflection source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_reflection_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("reflection source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled reflection program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "4\n6\n6\n7\nattribute\n7\ntrue\nfalse\ntrue\n42\n"
        );
    }

    #[test]
    fn native_getattr_default_evaluates_receiver_before_fallback() {
        let source = r#"
class Box:
    value: int
b = Box(1)
def make() -> Box:
    print("receiver")
    return b
print(getattr(make(), "missing", 42))
"#;
        let module = parse(source).expect("reflection default source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_reflection_default_order_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("reflection default should compile");
        let run = Command::new(&output)
            .output()
            .expect("run reflection default");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "reflection default failed: {run:?}");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "receiver\n42\n");
    }

    #[test]
    fn native_list_slices_support_negative_steps() {
        let source = "print([1, 2, 3][::-1][0])\nprint([1, 2, 3][:-1][0])\n";
        let module = parse(source).expect("negative slice should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_negative_slice_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("negative slice should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled negative slice should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n1\n");
    }

    #[test]
    fn native_map_applies_named_unary_functions() {
        let source = "def inc(x: int) -> int:\n    return x + 1\nprint(map(inc, [1, 2])[1])\n";
        let module = parse(source).expect("map source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_map_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("map source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled map program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n");
    }

    #[test]
    fn native_named_functions_can_be_bound_and_called() {
        let source = r#"
def double(x: int) -> int:
    return x * 2
g = double
print(g(21))
"#;
        let module = parse(source).expect("source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_fn_alias_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("function alias should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "42");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_user_calls_reorder_keyword_arguments() {
        let source = "def combine(left: int, right: int) -> int:\n    return left * 10 + right\nprint(combine(right=3, left=4))\n";
        let module = parse(source).expect("keyword call source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_keywords_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("keyword call should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "43");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_user_calls_fill_default_arguments() {
        let source = "def add(left: int, right: int = 2) -> int:\n    return left + right\nprint(add(40))\nprint(add(left=40))\n";
        let module = parse(source).expect("default call source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_defaults_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("default call should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n42\n");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_class_construction_uses_field_defaults() {
        let source = "class Point:\n    x: int\n    y: int = 7\np = Point(5)\nq = Point(y=9, x=2)\nprint(p.y)\nprint(q.x)\n";
        let module = parse(source).expect("default field source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_field_defaults_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("default field construction should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout), "7\n2\n");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_methods_fill_default_arguments() {
        let source = "class Counter:\n    value: int\n    def add(self, amount: int = 1) -> int:\n        return self.value + amount\nc = Counter(41)\nprint(c.add())\nprint(c.add(amount=2))\n";
        let module = parse(source).expect("method default source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_method_defaults_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("method defaults should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n43\n");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_method_resolution_uses_receiver_class() {
        let source = "class A:\n    value: int\n    def render(self) -> int:\n        return self.value + 1\nclass B:\n    value: int\n    def render(self) -> int:\n        return self.value + 2\na = A(10)\nb = B(10)\nprint(a.render())\nprint(b.render())\n";
        let module = parse(source).expect("same-name method source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_method_dispatch_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("same-name methods should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout), "11\n12\n");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_derived_classes_include_inherited_fields() {
        let source = "class Base:\n    id: int\nclass Child(Base):\n    value: int\nc = Child(3, 9)\nprint(c.id)\nprint(c.value)\n";
        let module = parse(source).expect("inheritance field source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_inherited_fields_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("inherited fields should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout), "3\n9\n");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_derived_instances_call_inherited_methods() {
        let source = "class Base:\n    id: int\n    def identify(self) -> int:\n        return self.id\nclass Child(Base):\n    value: int\nc = Child(8, 3)\nprint(c.identify())\n";
        let module = parse(source).expect("inherited method source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_inherited_method_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("inherited method should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "8");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_overridden_methods_call_super() {
        let source = "class Base:\n    value: int\n    def get(self) -> int:\n        return self.value + 1\nclass Child(Base):\n    override def get(self) -> int:\n        return super.get() + 1\nc = Child(40)\nprint(c.get())\n";
        let module = parse(source).expect("super method source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_super_method_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("super method should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "42");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_derived_instances_read_inherited_getters() {
        let source = "class Base:\n    id: int\n    getter doubled(self) -> int:\n        return self.id * 2\nclass Child(Base):\n    value: int\nc = Child(6, 1)\nprint(c.doubled)\n";
        let module = parse(source).expect("inherited getter source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_inherited_getter_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("inherited getter should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "12");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_derived_instances_use_inherited_setters() {
        let source = "class Base:\n    value: int\n    setter value(self, next: int):\n        self.value = next + 1\nclass Child(Base):\n    extra: int\nc = Child(2, 0)\nc.value = 8\nprint(c.value)\n";
        let module = parse(source).expect("inherited setter source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_inherited_setter_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("inherited setter should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "9");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_derived_classes_inherit_class_variables() {
        let source = "class Base:\n    classvar count: int = 5\nclass Child(Base):\n    value: int\nprint(Child.count)\nChild.count = 8\nprint(Base.count)\n";
        let module = parse(source).expect("inherited classvar source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_inherited_classvar_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("inherited classvar should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout), "5\n8\n");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_derived_classes_call_inherited_class_methods() {
        let source = "class Base:\n    classmethod make(cls, value: int = 4) -> int:\n        return value + 1\nclass Child(Base):\n    own: int\nprint(Child.make())\nprint(Child.make(value=8))\n";
        let module = parse(source).expect("inherited classmethod source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_inherited_classmethod_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("inherited classmethod should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout), "5\n9\n");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_class_methods_call_super() {
        let source = "class Base:\n    classmethod make(cls, value: int = 4) -> int:\n        return value + 1\nclass Child(Base):\n    classmethod make(cls) -> int:\n        return super.make() + 1\nprint(Child.make())\n";
        let module = parse(source).expect("super classmethod source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_super_classmethod_{}",
            std::process::id()
        ));
        compile_to_native(&module, &output, 0).expect("super classmethod should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "6");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_partial_application_fills_holes() {
        let source = "def combine(a: int, b: int) -> int:\n    return a * 10 + b\npart = combine(4, _)\nprint(part(3))\n";
        let module = parse(source).expect("partial call source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_partial_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("partial call should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "43");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_function_aliases_work_with_map() {
        let source = r#"
def double(x: int) -> int:
    return x * 2
g = double
result = map(g, [1, 2, 3])
print(result[1])
"#;
        let module = parse(source).expect("source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_fn_map_alias_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("function alias map should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "4");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_multi_argument_anonymous_functions_are_callable() {
        let source = "add = def(a: int, b: int): a + b\nprint(add(20, 22))\n";
        let module = parse(source).expect("multi-argument anonymous source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_fn_multi_{}", std::process::id()));
        compile_to_native(&module, &output, 0)
            .expect("multi-argument anonymous function should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "42");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_anonymous_functions_capture_locals_in_function_scope() {
        let source = "def make() -> int:\n    offset = 5\n    add = def(x: int): x + offset\n    return add(7)\nprint(make())\n";
        let module = parse(source).expect("local closure source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_native_fn_capture_{}", std::process::id()));
        compile_to_native(&module, &output, 0).expect("local closure should compile");
        let result = std::process::Command::new(&output)
            .output()
            .expect("run native binary");
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "12");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn native_map_applies_expression_bodied_anonymous_functions() {
        let source = "print(map(def(x: int) -> int: x * 2, [1, 2])[1])\n";
        let module = parse(source).expect("anonymous map source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_anonymous_map_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("anonymous map source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled anonymous map program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "4\n");
    }

    #[test]
    fn native_anonymous_functions_support_direct_and_bound_calls() {
        let source = "offset = 1\ninc = def(x: int) -> int: x + offset\nanswer = def() -> int: 42\nprint(inc(4))\nprint(answer())\n";
        let module = parse(source).expect("anonymous call source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_anonymous_call_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("anonymous call source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled anonymous call program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "5\n42\n");
    }

    #[test]
    fn native_type_expression_reifies_builtin_types() {
        let source = "print(type int)\nprint(type list[str])\n";
        let module = parse(source).expect("type expression source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_type_expression_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("type expression should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled type expression program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "__type:int\n__type:list[str]\n"
        );
    }

    #[test]
    fn native_skip_omits_print_and_collection_entries() {
        let source = "print(1, skip, 3)\nprint([1, skip, 3])\nprint({1: 2, 3: skip})\n";
        let module = parse(source).expect("skip source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_skip_test_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("skip source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled skip program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "1 3\n[list len=2]\n[dict len=1]\n"
        );
    }

    #[test]
    fn native_string_slices_support_negative_steps() {
        let source = "print(\"abcd\"[::-1])\nprint(\"abcd\"[1:3])\nprint(\"aé😀\"[1])\nprint(\"aé😀\"[::-1])\n";
        let module = parse(source).expect("string slice source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_string_slice_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("string slice source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled string slice program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "dcba\nbc\né\n😀éa\n");
    }

    #[test]
    fn native_string_slice_uses_annotated_binding_type() {
        let source = "s: str = \"abcd\"\nprint(s[1:3])\n";
        let module = parse(source).expect("annotated string slice source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_annotated_string_slice_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("annotated string slice should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "bc\n");
    }

    #[test]
    fn native_string_chars_property_is_unicode_aware() {
        let source = "print(\"aé😀\".chars)\nprint(len(\"aé😀\".chars))\n";
        let module = parse(source).expect("string chars source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_string_chars_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("string chars should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "[list len=3]\n3\n");
    }

    #[test]
    fn native_list_remove_removes_first_match() {
        let source = "items = [1, 2, 1]\nitems.remove(1)\nprint(items)\n";
        let module = parse(source).expect("list remove source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_list_remove_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("list remove should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "[list len=2]\n");
    }

    #[test]
    fn native_set_algebra_and_disjointness() {
        let source = "a = {1, 2}\nb = {2, 3}\nprint(a & b)\nprint(a | b)\nprint(a - b)\nprint(a ^ b)\nprint(a.isdisjoint({4}))\n";
        let module = parse(source).expect("set algebra source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_set_algebra_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("set algebra should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "[set len=1]\n[set len=3]\n[set len=1]\n[set len=2]\ntrue\n"
        );
    }

    #[test]
    fn native_set_isdisjoint_accepts_lists() {
        let source =
            "items = {1, 2}\nprint(items.isdisjoint([3, 4]))\nprint(items.isdisjoint([2, 4]))\n";
        let module = parse(source).expect("list isdisjoint source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_set_isdisjoint_list_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("list isdisjoint should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native isdisjoint failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\nfalse\n");
    }

    #[test]
    fn native_set_intersection_rejects_non_set_operand() {
        let source = "print({1, 2} & [2, 3])\n";
        let module = parse(source).expect("invalid set intersection source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_set_intersection_invalid_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("invalid set intersection should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "set/list intersection should fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("unsupported operands for &"));
    }

    #[test]
    fn native_set_union_rejects_non_set_operand() {
        let source = "print({1, 2} | [2, 3])\n";
        let module = parse(source).expect("invalid set union source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_set_union_invalid_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("invalid set union should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(!run.status.success(), "set/list union should fail");
        assert!(String::from_utf8_lossy(&run.stderr).contains("unsupported operands for |"));
    }

    #[test]
    fn native_set_difference_and_xor_reject_non_set_operands() {
        for (index, operation) in ["-", "^"].iter().enumerate() {
            let source = format!("print({{1, 2}} {operation} [2, 3])\n");
            let module = parse(&source).expect("invalid set operation source should parse");
            let output = std::env::temp_dir().join(format!(
                "lucid_codegen_set_invalid_{}_{}",
                std::process::id(),
                index
            ));
            let _ = fs::remove_file(&output);
            compile_to_native(&module, &output, 0).expect("invalid set operation should compile");
            let run = Command::new(&output).output().expect("run native binary");
            let _ = fs::remove_file(&output);
            assert!(!run.status.success(), "set/list {operation} should fail");
            assert!(
                String::from_utf8_lossy(&run.stderr)
                    .contains(&format!("unsupported operands for {operation}")),
                "unexpected stderr: {}",
                String::from_utf8_lossy(&run.stderr)
            );
        }
    }

    #[test]
    fn native_set_and_dict_conversion_builtins() {
        let source = "a = set([1, 2, 1])\nb = set(\"aba\")\nc = dict([[\"x\", 4], [\"y\", 5]])\nprint(len(a))\nprint(len(b))\nprint(c[\"x\"])\nprint(len(set()))\nprint(len(dict()))\n";
        let module = parse(source).expect("set/dict source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_set_dict_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("set/dict source should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled set/dict program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "2\n2\n4\n0\n0\n");
    }

    #[test]
    fn native_runtime_list_spread_calls_fixed_arity_function() {
        let source =
            "def add(a: int, b: int) -> int:\n    return a + b\nxs = [2, 3]\nprint(add(*xs))\n";
        let module = parse(source).expect("runtime spread source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_runtime_spread_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("runtime spread should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled runtime spread program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "5\n");
    }

    #[test]
    fn native_runtime_mapping_spread_calls_fixed_arity_function() {
        let source = "def add(a: int, b: int) -> int:\n    return a + b\nvalues = {\"a\": 4, \"b\": 6}\nprint(add(**values))\n";
        let module = parse(source).expect("runtime mapping spread source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_runtime_mapping_spread_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("runtime mapping spread should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled runtime mapping spread program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "10\n");
    }

    #[test]
    fn native_generator_call_shorthand_expands_values() {
        let source =
            "def add(a: int, b: int) -> int:\n    return a + b\nprint(add(x for x in [2, 3]))\n";
        let module = parse(source).expect("generator call source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_generator_call_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("generator call should compile");
        let run = Command::new(&output)
            .output()
            .expect("compiled generator call program should run");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "5\n");
    }

    #[test]
    fn native_dynamic_gather_spread_calls_fixed_arity_function() {
        let source = "class Arguments:\n    vpargs: list[int]\n    kwargs: dict[str, int]\ndef add(a: int, b: int) -> int:\n    return a + b\nargs = Arguments([2], {\"b\": 3})\nprint(add(***args))\n";
        let module = parse(source).expect("dynamic gather source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_gather_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic gather source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "5\n");
    }

    #[test]
    fn native_dynamic_parameters_spread_preserves_prefix() {
        let source = "class Parameters:\n    pargs: list[int]\n    vpargs: list[int]\n    kwargs: dict[str, int]\ndef add(a: int, b: int, c: int) -> int:\n    return a + b + c\nparams = Parameters([1], [2], {\"c\": 3})\nprint(add(***params))\n";
        let module = parse(source).expect("dynamic parameters source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_dynamic_parameters_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("dynamic parameters source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "6\n");
    }

    #[test]
    fn native_gather_call_aggregates_leftover_arguments() {
        let source = "class Arguments:\n    vpargs: list[int]\n    kwargs: dict[str, int]\ndef query(path: int, ***rest: Arguments) -> int:\n    return path + len(rest.vpargs) + len(rest.kwargs)\nprint(query(10, 1, 2, mode=3))\n";
        let module = parse(source).expect("gather call source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_gather_call_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("gather call should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "13\n");
    }

    #[test]
    fn native_method_gather_aggregates_leftover_arguments() {
        let source = "class Arguments:\n    vpargs: list[int]\n    kwargs: dict[str, int]\nclass Box:\n    value: int\n    def query(self, ***rest: Arguments) -> int:\n        return self.value + len(rest.vpargs) + len(rest.kwargs)\nbox = Box(10)\nprint(box.query(1, mode=2))\n";
        let module = parse(source).expect("method gather source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_method_gather_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("method gather should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "12\n");
    }

    #[test]
    fn native_class_gather_spread_uses_declared_field_order() {
        let source = "class Pair:\n    left: int\n    right: int\ndef add(a: int, b: int) -> int:\n    return a + b\npair = Pair(4, 7)\nprint(add(***pair))\n";
        let module = parse(source).expect("class gather source should parse");
        let output =
            std::env::temp_dir().join(format!("lucid_codegen_class_gather_{}", std::process::id()));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("class gather source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "11\n");
    }

    #[test]
    fn native_starred_assignment_binds_remainder() {
        let source =
            "first, *middle, last = [1, 2, 3, 4]\nprint(first)\nprint(middle)\nprint(last)\n";
        let module = parse(source).expect("starred assignment source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_starred_assignment_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("starred assignment should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n[list len=2]\n4\n");
    }

    #[test]
    fn native_exported_declarations_are_emitted() {
        let source = "export def answer() -> int:\n    return 42\nprint(answer())\n";
        let module = parse(source).expect("exported declaration source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_exported_declaration_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("exported declaration should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
    }

    #[test]
    fn native_freeze_prevents_container_mutation() {
        let source = "items = [1, 2]\nfreeze(items)\nitems.append(3)\n";
        let module = parse(source).expect("native freeze source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_freeze_container_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("native freeze should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(
            !run.status.success(),
            "frozen list mutation unexpectedly succeeded"
        );
        assert!(String::from_utf8_lossy(&run.stderr).contains("cannot mutate frozen list"));
    }

    #[test]
    fn native_freeze_prevents_object_mutation() {
        let source = "class Account:\n    balance: int\nacc = Account(100)\nfreeze(acc)\nsetattr(acc, \"balance\", 200)\n";
        let module = parse(source).expect("native object freeze source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_freeze_object_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("native object freeze should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(
            !run.status.success(),
            "frozen object mutation unexpectedly succeeded"
        );
        assert!(String::from_utf8_lossy(&run.stderr).contains("cannot mutate frozen object"));
    }

    #[test]
    fn native_freeze_is_deep_for_object_fields() {
        let source =
            "class Box:\n    values: list[int]\nb = Box([1])\nf = freeze(b)\nf.values[0] = 2\n";
        let module = parse(source).expect("nested freeze source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_nested_freeze_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("nested freeze source should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(
            !run.status.success(),
            "deeply frozen fields must reject mutation"
        );
    }

    #[test]
    fn native_frozen_containers_are_hashable() {
        let source = "items = freeze([1, 2])\nprint(hash(items))\n";
        let module = parse(source).expect("native frozen hash source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_codegen_frozen_hash_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("native frozen hash should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert!(!String::from_utf8_lossy(&run.stdout).trim().is_empty());
    }

    #[test]
    fn native_signed_division_and_remainder_follow_floor_semantics() {
        let source = "print(-5 // 2)\nprint(-5 % 2)\nprint(5 // -2)\nprint(5 % -2)\n";
        let module = parse(source).expect("signed arithmetic source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_signed_arithmetic_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("signed arithmetic should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "-3\n1\n-3\n-1\n");
    }

    #[test]
    fn native_float_floor_division_and_remainder_use_numeric_semantics() {
        let source = "print(-5.0 // 2.0)\nprint(-5.0 % 2.0)\nprint(5.0 // -2.0)\nprint(5.0 % -2.0)\nprint(5.0 % float.inf)\n";
        let module = parse(source).expect("float arithmetic source should parse");
        let output = std::env::temp_dir().join(format!(
            "lucid_native_float_arithmetic_{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);
        compile_to_native(&module, &output, 0).expect("float arithmetic should compile");
        let run = Command::new(&output).output().expect("run native binary");
        let _ = fs::remove_file(&output);
        assert!(run.status.success(), "native program failed: {:?}", run);
        assert_eq!(String::from_utf8_lossy(&run.stdout), "-3\n1\n-3\n-1\n5\n");
    }

    #[test]
    fn native_float_zero_floor_operations_fail_like_interpreter() {
        for (index, operation) in ["//", "%"].iter().enumerate() {
            let source = format!("print(5.0 {operation} 0.0)\n");
            let module = parse(&source).expect("float zero source should parse");
            let output = std::env::temp_dir().join(format!(
                "lucid_native_float_zero_{}_{}",
                std::process::id(),
                index
            ));
            let _ = fs::remove_file(&output);
            compile_to_native(&module, &output, 0).expect("float zero source should compile");
            let run = Command::new(&output).output().expect("run native binary");
            let _ = fs::remove_file(&output);
            assert!(
                !run.status.success(),
                "zero {operation} unexpectedly succeeded"
            );
            assert!(String::from_utf8_lossy(&run.stderr).contains("division by zero"));
        }
    }
}
