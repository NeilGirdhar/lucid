#![allow(clippy::needless_return)]

use lucid_syntax::ast::*;
use lucid_syntax::token::Span;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

pub use lucid_abi::{NativeErrorCode, NativeResult};
use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::fmt;
use std::rc::Rc;

type BuiltinFn = Rc<dyn Fn(&[Value], &mut Interpreter) -> Result<Value, RuntimeError>>;
type Teardown = (Vec<Stmt>, Option<Vec<Stmt>>, Rc<RefCell<Environment>>);

fn declaration_only_module(path: &std::path::Path) -> bool {
    let Ok(source) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(module) = lucid_syntax::parse(&source) else {
        return false;
    };
    fn is_declaration(statement: &Stmt) -> bool {
        match statement {
            Stmt::Export(inner) => is_declaration(inner),
            Stmt::ClassDef { .. }
            | Stmt::InterfaceDef { .. }
            | Stmt::TraitDef { .. }
            | Stmt::ImplementDef { .. }
            | Stmt::TypeAlias { .. }
            | Stmt::Function(_)
            | Stmt::Import { .. }
            | Stmt::FromImport { .. }
            | Stmt::Pass(_)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::VarDef { value: None, .. } => true,
            _ => false,
        }
    }
    module.statements.iter().all(is_declaration)
}

fn parse_bigint_literal(text: &str) -> Option<BigInt> {
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
        (unsigned, 10)
    };
    let mut value = BigInt::parse_bytes(digits.as_bytes(), radix)?;
    if negative {
        value = -value;
    }
    Some(value)
}

fn bigint_to_float(value: &BigInt) -> f64 {
    value.to_f64().unwrap_or_else(|| {
        if value.is_negative() {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }
    })
}

thread_local! {
    static FROZEN_CONTAINERS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

fn mark_container_frozen(identity: usize) {
    FROZEN_CONTAINERS.with(|containers| {
        containers.borrow_mut().insert(identity);
    });
}

fn container_is_frozen(identity: usize) -> bool {
    FROZEN_CONTAINERS.with(|containers| containers.borrow().contains(&identity))
}

fn dotted_path(value: &str) -> Value {
    Value::DottedPath(
        value
            .split('.')
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

fn dotted_path_string(parts: &[String]) -> String {
    parts.join(".")
}

fn hash_runtime_value(value: &Value) -> Option<i64> {
    match value {
        Value::Int(value) => Some(*value),
        Value::BigInt(value) => value.to_i64().or_else(|| {
            // Values outside i64 still satisfy Hashable.  Fold their signed
            // decimal spelling so arbitrary precision does not turn into an
            // accidental unhashable case, while in-range values retain the
            // same hash as an ordinary Int.
            Some(value.to_string().bytes().fold(17i64, |acc, byte| {
                acc.wrapping_mul(31).wrapping_add(byte as i64)
            }))
        }),
        Value::Float(value) => {
            // Numeric equality treats an exactly representable integral float
            // as equal to its integer counterpart, so use the integer hash in
            // that case.  Normalize signed zero for the same reason.
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value < i64::MAX as f64
            {
                Some(*value as i64)
            } else if *value == 0.0 {
                Some(0)
            } else {
                Some(value.to_bits() as i64)
            }
        }
        Value::Complex(real, imag) if *imag == 0.0 => hash_runtime_value(&Value::Float(*real)),
        Value::Complex(real, imag) => Some(
            (real.to_bits() as i64)
                .wrapping_mul(31)
                .wrapping_add(imag.to_bits() as i64),
        ),
        Value::Bool(value) => Some(i64::from(*value)),
        Value::Str(value) => Some(value.bytes().fold(0i64, |acc, byte| {
            acc.wrapping_mul(31).wrapping_add(byte as i64)
        })),
        Value::Bytes(value) => Some(value.iter().fold(0i64, |acc, byte| {
            acc.wrapping_mul(31).wrapping_add(*byte as i64)
        })),
        Value::None => Some(0),
        Value::List(values) if container_is_frozen(Rc::as_ptr(values) as usize) => {
            values.borrow().iter().try_fold(1i64, |acc, value| {
                hash_runtime_value(value).map(|item| acc.wrapping_mul(31).wrapping_add(item))
            })
        }
        Value::Set(values) if container_is_frozen(Rc::as_ptr(values) as usize) => {
            let mut hashes: Vec<i64> = values
                .borrow()
                .iter()
                .map(hash_runtime_value)
                .collect::<Option<_>>()?;
            hashes.sort_unstable();
            Some(
                hashes
                    .into_iter()
                    .fold(7i64, |acc, item| acc.wrapping_mul(37).wrapping_add(item)),
            )
        }
        Value::Dict(values) if container_is_frozen(Rc::as_ptr(values) as usize) => {
            // HashMap iteration order is deliberately unspecified. Sort keys
            // before folding so equal frozen dictionaries always have the
            // same hash, regardless of insertion order or allocator state.
            let mut entries: Vec<_> = values
                .borrow()
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            entries.into_iter().try_fold(13i64, |acc, (key, value)| {
                hash_runtime_value(&Value::Str(key.clone())).and_then(|k| {
                    hash_runtime_value(&value).map(|v| acc.wrapping_mul(41).wrapping_add(k ^ v))
                })
            })
        }
        _ => None,
    }
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

// Reserved representations for the unbounded integer values specified by Lucid.
// Ordinary source integers are checked against these sentinels by the parser in
// a future arbitrary-precision backend; keeping them centralized makes the
// current interpreter's behavior deterministic in the meantime.
const INT_POS_INF: i64 = i64::MAX;
const INT_NEG_INF: i64 = i64::MIN + 1;
const INT_NAN: i64 = i64::MIN;

fn materialize_range(start: i64, stop: i64, step: i64) -> Vec<Value> {
    let mut values = Vec::new();
    if step == 0 {
        return values;
    }
    let mut current = start;
    while if step > 0 {
        current < stop
    } else {
        current > stop
    } {
        values.push(Value::Int(current));
        let Some(next) = current.checked_add(step) else {
            break;
        };
        current = next;
    }
    values
}

fn range_contains(start: i64, stop: i64, step: i64, value: i64) -> bool {
    if step == 0 {
        return false;
    }
    if step > 0 {
        if value < start || value >= stop {
            return false;
        }
    } else if value > start || value <= stop {
        return false;
    }
    value
        .checked_sub(start)
        .is_some_and(|distance| distance % step == 0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub message: String,
    pub span: Span,
}

pub struct FutureTask {
    function: Value,
    args: Vec<(Option<String>, Value)>,
}

#[derive(Clone)]
pub enum Value {
    Int(i64),
    BigInt(BigInt),
    Float(f64),
    Complex(f64, f64),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
    None,
    List(Rc<RefCell<Vec<Value>>>),
    MemoryView {
        data: Rc<RefCell<Vec<Value>>>,
        start: i64,
        len: i64,
        stride: i64,
        read_only: bool,
    },
    Dict(Rc<RefCell<HashMap<String, Value>>>),
    Record(Rc<RefCell<HashMap<String, Value>>>),
    Object {
        class_name: String,
        fields: Rc<RefCell<HashMap<String, Value>>>,
        is_frozen: Rc<RefCell<bool>>,
    },
    Function {
        name: String,
        params: Vec<Param>,
        body: Vec<Stmt>,
        closure: Rc<RefCell<Environment>>,
        is_contextmanager: bool,
        is_async: bool,
    },
    Future(Rc<RefCell<Option<FutureTask>>>),
    Partial {
        func: Box<Value>,
        args: Vec<(Option<String>, Option<Value>)>,
    },
    ContextManager {
        value: Box<Value>,
        teardown: Vec<Stmt>,
        error_teardown: Option<Vec<Stmt>>,
        env: Rc<RefCell<Environment>>,
    },
    BuiltinFunction {
        name: String,
        func: BuiltinFn,
    },
    Module {
        name: String,
        path: String,
        env: Rc<RefCell<Environment>>,
    },
    ClassRef(String),
    TraitRef(String),
    DottedPath(Vec<String>),
    Set(Rc<RefCell<Vec<Value>>>),
    Range {
        start: i64,
        stop: i64,
        step: i64,
    },
    Skip,
    Sentinel(String),
    Return(Box<Value>),
}

impl Value {
    pub fn type_name(&self) -> &str {
        match self {
            Value::Int(_) => "int",
            Value::BigInt(_) => "int",
            Value::Float(_) => "float",
            Value::Complex(_, _) => "complex",
            Value::Bool(_) => "bool",
            Value::Str(_) => "str",
            Value::Bytes(_) => "Bytes",
            Value::None => "none",
            Value::List(_) => "list",
            Value::MemoryView { .. } => "MemoryView",
            Value::Dict(_) => "dict",
            Value::Set(_) => "set",
            Value::Range { .. } => "range",
            Value::Skip => "skip",
            Value::Record(_) => "record",
            Value::Object { class_name, .. } => class_name.as_str(),
            Value::Function { .. } => "function",
            Value::Future(_) => "future",
            Value::Partial { .. } => "function",
            Value::ContextManager { .. } => "contextmanager",
            Value::BuiltinFunction { .. } => "builtin_function",
            Value::Module { .. } => "module",
            Value::ClassRef(_) => "class",
            Value::TraitRef(_) => "trait",
            Value::DottedPath(_) => "DottedPath",
            Value::Sentinel(name) => name.as_str(),
            Value::Return(val) => val.type_name(),
        }
    }

    pub fn freeze(&self) {
        let mut visited = HashSet::new();
        self.freeze_with_visited(&mut visited);
    }

    fn freeze_with_visited(&self, visited: &mut HashSet<usize>) {
        match self {
            Value::Object {
                is_frozen, fields, ..
            } => {
                let identity = Rc::as_ptr(is_frozen) as usize;
                if !visited.insert(identity) {
                    return;
                }
                *is_frozen.borrow_mut() = true;
                for val in fields.borrow().values() {
                    val.freeze_with_visited(visited);
                }
            }
            Value::List(items) => {
                let identity = Rc::as_ptr(items) as usize;
                mark_container_frozen(identity);
                if !visited.insert(identity) {
                    return;
                }
                for item in items.borrow().iter() {
                    item.freeze_with_visited(visited);
                }
            }
            Value::MemoryView { data, .. } => {
                let identity = Rc::as_ptr(data) as usize;
                mark_container_frozen(identity);
            }
            Value::Set(items) => {
                let identity = Rc::as_ptr(items) as usize;
                mark_container_frozen(identity);
                if !visited.insert(identity) {
                    return;
                }
                for item in items.borrow().iter() {
                    item.freeze_with_visited(visited);
                }
            }
            Value::Dict(entries) => {
                let identity = Rc::as_ptr(entries) as usize;
                mark_container_frozen(identity);
                if !visited.insert(identity) {
                    return;
                }
                for item in entries.borrow().values() {
                    item.freeze_with_visited(visited);
                }
            }
            Value::Module { env, .. } => {
                if !visited.insert(Rc::as_ptr(env) as usize) {
                    return;
                }
                for item in env.borrow().bindings.values() {
                    item.freeze_with_visited(visited);
                }
            }
            Value::Return(val) => val.freeze_with_visited(visited),
            Value::Partial { func, args } => {
                func.freeze_with_visited(visited);
                for (_, value) in args {
                    if let Some(value) = value {
                        value.freeze_with_visited(visited);
                    }
                }
            }
            _ => {}
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Return(a), Value::Return(b)) => a == b,
            (Value::Return(a), b) | (b, Value::Return(a)) => **a == *b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::BigInt(a), Value::BigInt(b)) => a == b,
            (Value::BigInt(a), Value::Int(b)) | (Value::Int(b), Value::BigInt(a)) => {
                *a == BigInt::from(*b)
            }
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::BigInt(a), Value::Float(b)) => bigint_to_float(a) == *b,
            (Value::Float(a), Value::BigInt(b)) => *a == bigint_to_float(b),
            (Value::Complex(ar, ai), Value::Complex(br, bi)) => ar == br && ai == bi,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::None, Value::None) => true,
            (Value::Skip, Value::Skip) => true,
            (Value::Sentinel(a), Value::Sentinel(b)) => a == b,
            (
                Value::Range {
                    start: s1,
                    stop: e1,
                    step: st1,
                },
                Value::Range {
                    start: s2,
                    stop: e2,
                    step: st2,
                },
            ) => s1 == s2 && e1 == e2 && st1 == st2,
            (Value::List(a), Value::List(b)) => *a.borrow() == *b.borrow(),
            (
                Value::MemoryView {
                    data: left_data,
                    start: left_start,
                    len: left_len,
                    stride: left_stride,
                    ..
                },
                Value::MemoryView {
                    data: right_data,
                    start: right_start,
                    len: right_len,
                    stride: right_stride,
                    ..
                },
            ) => {
                if left_len != right_len {
                    return false;
                }
                let left = left_data.borrow();
                let right = right_data.borrow();
                (0..*left_len).all(|offset| {
                    left[(left_start + offset * left_stride) as usize]
                        == right[(right_start + offset * right_stride) as usize]
                })
            }
            (Value::Set(a), Value::Set(b)) => {
                let left = a.borrow();
                let right = b.borrow();
                left.len() == right.len() && left.iter().all(|value| right.contains(value))
            }
            (Value::Dict(a), Value::Dict(b)) => *a.borrow() == *b.borrow(),
            (Value::Record(a), Value::Record(b)) => *a.borrow() == *b.borrow(),
            (Value::DottedPath(a), Value::DottedPath(b)) => a == b,
            (Value::Partial { .. }, Value::Partial { .. }) => false,
            (Value::Module { path: p1, .. }, Value::Module { path: p2, .. }) => p1 == p2,
            (Value::TraitRef(a), Value::TraitRef(b)) => a == b,
            (
                Value::Object {
                    class_name: n1,
                    fields: f1,
                    ..
                },
                Value::Object {
                    class_name: n2,
                    fields: f2,
                    ..
                },
            ) => n1 == n2 && *f1.borrow() == *f2.borrow(),
            _ => false,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => match *n {
                INT_POS_INF => write!(f, "int.inf"),
                INT_NEG_INF => write!(f, "-int.inf"),
                INT_NAN => write!(f, "int.nan"),
                _ => write!(f, "{n}"),
            },
            Value::BigInt(n) => write!(f, "{n}"),
            Value::Float(n) => write!(f, "{n}"),
            Value::Complex(real, imag) => write!(f, "({real}+{imag}j)"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Str(s) => write!(f, "\"{s}\""),
            Value::Bytes(bytes) => write!(f, "b\"{}\"", String::from_utf8_lossy(bytes)),
            Value::None => write!(f, "none"),
            Value::Skip => write!(f, "skip"),
            Value::Range { start, stop, step } => {
                if *step == 1 {
                    write!(f, "range({start}, {stop})")
                } else {
                    write!(f, "range({start}, {stop}, {step})")
                }
            }
            Value::List(items) => write!(f, "{:?}", *items.borrow()),
            Value::MemoryView { len, .. } => write!(f, "memoryview(len={len})"),
            Value::Set(items) => write!(f, "{{{:?}}}", *items.borrow()),
            Value::Dict(entries) => write!(f, "{:?}", *entries.borrow()),
            Value::Record(fields) => write!(f, "record {:?}", *fields.borrow()),
            Value::Object {
                class_name,
                fields,
                is_frozen,
            } => {
                let prefix = if *is_frozen.borrow() { "!" } else { "" };
                write!(f, "{prefix}{class_name}({:?})", *fields.borrow())
            }
            Value::Function { name, .. } => write!(f, "<def {name}>"),
            Value::Future(_) => write!(f, "<future>"),
            Value::Partial { .. } => write!(f, "<partial>"),
            Value::ContextManager { value, .. } => write!(f, "<contextmanager {:?}>", value),
            Value::BuiltinFunction { name, .. } => write!(f, "<builtin {name}>"),
            Value::Module { name, .. } => write!(f, "<module '{name}'>"),
            Value::ClassRef(name) => write!(f, "<class '{name}'>"),
            Value::TraitRef(name) => write!(f, "<trait '{name}'>"),
            Value::DottedPath(parts) => write!(f, "{}", parts.join(".")),
            Value::Sentinel(s) => write!(f, "{s}"),
            Value::Return(val) => write!(f, "return {:?}", val),
        }
    }
}

fn identity_metadata_attr(value: &Value, attr: &str) -> Option<Value> {
    match value {
        Value::Function { name, .. } | Value::BuiltinFunction { name, .. } => match attr {
            "__name__" => Some(Value::Str(name.clone())),
            "__path__" => Some(dotted_path(name)),
            "__doc__" => Some(Value::None),
            _ => None,
        },
        Value::ClassRef(name) | Value::TraitRef(name) => match attr {
            "__name__" => Some(Value::Str(name.clone())),
            "__path__" => Some(dotted_path(name)),
            "__doc__" => Some(Value::None),
            _ => None,
        },
        Value::Module { name, path, .. } => match attr {
            "__name__" => Some(Value::Str(name.clone())),
            "__path__" => Some(dotted_path(path)),
            "__doc__" => Some(Value::None),
            _ => None,
        },
        _ => None,
    }
}

#[derive(Default, Clone)]
pub struct Environment {
    pub bindings: HashMap<String, Value>,
    /// Insertion order for module reflection. Local lookup remains hash-based,
    /// while `fields(module)` must expose declarations deterministically.
    pub binding_order: Vec<String>,
    pub parent: Option<Rc<RefCell<Environment>>>,
    pub final_bindings: HashSet<String>,
}

impl Environment {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_parent(parent: Rc<RefCell<Environment>>) -> Self {
        Self {
            bindings: HashMap::new(),
            binding_order: Vec::new(),
            parent: Some(parent),
            final_bindings: HashSet::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(val) = self.bindings.get(name) {
            Some(val.clone())
        } else if let Some(ref p) = self.parent {
            p.borrow().get(name)
        } else {
            None
        }
    }

    pub fn set(&mut self, name: String, value: Value) {
        if !self.bindings.contains_key(&name) {
            self.binding_order.push(name.clone());
        }
        self.bindings.insert(name, value);
    }

    pub fn mark_final(&mut self, name: String) {
        self.final_bindings.insert(name);
    }

    pub fn is_final(&self, name: &str) -> bool {
        self.final_bindings.contains(name)
            || self
                .parent
                .as_ref()
                .is_some_and(|parent| parent.borrow().is_final(name))
    }

    pub fn delete_local(&mut self, name: &str) -> bool {
        self.final_bindings.remove(name);
        let removed = self.bindings.remove(name).is_some();
        if removed {
            self.binding_order.retain(|bound| bound != name);
        }
        removed
    }

    pub fn mutate(&mut self, name: &str, value: Value) -> bool {
        if self.bindings.contains_key(name) {
            self.bindings.insert(name.to_string(), value);
            true
        } else if let Some(ref p) = self.parent {
            p.borrow_mut().mutate(name, value)
        } else {
            false
        }
    }
}

pub struct DispatchEntry {
    pub param_types: Vec<String>,
    pub func: BuiltinFn,
}

#[derive(Default)]
pub struct DispatchTable {
    pub methods: HashMap<String, Vec<DispatchEntry>>,
}

impl DispatchTable {
    pub fn register(&mut self, name: String, param_types: Vec<String>, func: BuiltinFn) {
        self.methods
            .entry(name)
            .or_default()
            .push(DispatchEntry { param_types, func });
    }

    pub fn find_match(&self, name: &str, args: &[Value]) -> Option<BuiltinFn> {
        self.find_match_by(name, args, |actual, expected| {
            if expected == "object" || expected == "LucidVal" {
                Some(1)
            } else if actual == expected {
                Some(0)
            } else {
                None
            }
        })
    }

    fn find_match_by<F>(&self, name: &str, args: &[Value], distance: F) -> Option<BuiltinFn>
    where
        F: Fn(&str, &str) -> Option<usize>,
    {
        if let Some(entries) = self.methods.get(name) {
            let arg_types: Vec<&str> = args.iter().map(|a| a.type_name()).collect();
            // Select the unique most-specific entry. Runtime values expose
            // concrete class names (but not the checker’s full hierarchy),
            // so the runtime ordering it can prove is exact type over the
            // `object` fallback. Never let registration order resolve a tie.
            let mut best: Option<(usize, BuiltinFn)> = None;
            let mut tied = false;
            for entry in entries {
                if entry.param_types.len() == args.len() {
                    let Some(specificity) = entry
                        .param_types
                        .iter()
                        .zip(&arg_types)
                        .map(|(expected, actual)| distance(actual, expected))
                        .collect::<Option<Vec<_>>>()
                        .map(|distances| distances.into_iter().fold(0usize, usize::saturating_add))
                    else {
                        continue;
                    };
                    match &best {
                        None => best = Some((specificity, entry.func.clone())),
                        Some((best_specificity, _)) if specificity < *best_specificity => {
                            best = Some((specificity, entry.func.clone()));
                            tied = false;
                        }
                        Some((best_specificity, _)) if specificity == *best_specificity => {
                            tied = true;
                        }
                        _ => {}
                    }
                }
            }
            if tied {
                return None;
            }
            return best.map(|(_, func)| func);
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct ClassDef {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub bases: Vec<TypeExpr>,
    pub without_traits: Vec<String>,
    pub body: Vec<ClassMember>,
    pub is_sealed: bool,
    pub is_final: bool,
    pub source_file: Option<std::path::PathBuf>,
    pub span: Span,
}

fn runtime_type_distance(
    classes: &HashMap<String, ClassDef>,
    actual: &str,
    expected: &str,
) -> Option<usize> {
    if expected == "object" {
        // Keep the universal fallback worse than any finite class ancestry
        // without imposing an arbitrary maximum hierarchy depth.
        return Some(usize::MAX / 4);
    }
    if actual == expected {
        return Some(0);
    }
    fn walk(
        classes: &HashMap<String, ClassDef>,
        current: &str,
        expected: &str,
        depth: usize,
        seen: &mut HashSet<String>,
    ) -> Option<usize> {
        if !seen.insert(current.to_owned()) {
            return None;
        }
        let class = classes.get(current)?;
        for base in &class.bases {
            let TypeExpr::Named { name, .. } = base else {
                continue;
            };
            if name == expected {
                return Some(depth + 1);
            }
            if let Some(distance) = walk(classes, name, expected, depth + 1, seen) {
                return Some(distance);
            }
        }
        None
    }
    walk(classes, actual, expected, 0, &mut HashSet::new())
}

fn compare_values(a: &Value, b: &Value) -> Result<std::cmp::Ordering, RuntimeError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::BigInt(x), Value::BigInt(y)) => Ok(x.cmp(y)),
        (Value::BigInt(x), Value::Int(y)) => Ok(x.cmp(&BigInt::from(*y))),
        (Value::Int(x), Value::BigInt(y)) => Ok(BigInt::from(*x).cmp(y)),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).ok_or_else(|| RuntimeError {
            message: "cannot compare NaN in min/max".into(),
            span: Span::default(),
        }),
        (Value::Int(x), Value::Float(y)) => {
            (*x as f64).partial_cmp(y).ok_or_else(|| RuntimeError {
                message: "cannot compare NaN in min/max".into(),
                span: Span::default(),
            })
        }
        (Value::Float(x), Value::Int(y)) => {
            x.partial_cmp(&(*y as f64)).ok_or_else(|| RuntimeError {
                message: "cannot compare NaN in min/max".into(),
                span: Span::default(),
            })
        }
        (Value::BigInt(x), Value::Float(y)) => {
            bigint_to_float(x)
                .partial_cmp(y)
                .ok_or_else(|| RuntimeError {
                    message: "cannot compare NaN in min/max".into(),
                    span: Span::default(),
                })
        }
        (Value::Float(x), Value::BigInt(y)) => {
            x.partial_cmp(&bigint_to_float(y))
                .ok_or_else(|| RuntimeError {
                    message: "cannot compare NaN in min/max".into(),
                    span: Span::default(),
                })
        }
        (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),
        _ => Err(RuntimeError {
            message: format!(
                "unsupported comparison between {} and {}",
                a.type_name(),
                b.type_name()
            ),
            span: Span::default(),
        }),
    }
}

fn byte_from_value(item: &Value, context: &str) -> Result<u8, RuntimeError> {
    match item {
        Value::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
        Value::BigInt(n) if n >= &BigInt::from(0) && n <= &BigInt::from(255) => {
            n.to_u8().ok_or_else(|| RuntimeError {
                message: format!("{context} items must be integers in 0..255"),
                span: Span::default(),
            })
        }
        _ => Err(RuntimeError {
            message: format!("{context} items must be integers in 0..255"),
            span: Span::default(),
        }),
    }
}

fn memoryview_items(
    data: &Rc<RefCell<Vec<Value>>>,
    start: i64,
    len: i64,
    stride: i64,
) -> Vec<Value> {
    let values = data.borrow();
    (0..len)
        .filter_map(|index| values.get((start + index * stride) as usize).cloned())
        .collect()
}

fn binary_sequence_items(value: &Value) -> Option<Vec<Value>> {
    match value {
        Value::Bytes(bytes) => Some(bytes.iter().map(|byte| Value::Int(*byte as i64)).collect()),
        Value::MemoryView {
            data,
            start,
            len,
            stride,
            ..
        } => Some(memoryview_items(data, *start, *len, *stride)),
        _ => None,
    }
}

fn repeat_bytes(bytes: &[u8], count: i64) -> Vec<u8> {
    let count = count.max(0) as usize;
    let mut repeated = Vec::with_capacity(bytes.len().saturating_mul(count));
    for _ in 0..count {
        repeated.extend_from_slice(bytes);
    }
    repeated
}

fn concat_bytes(left: &[u8], right: &[u8]) -> Vec<u8> {
    let mut joined = Vec::with_capacity(left.len().saturating_add(right.len()));
    joined.extend_from_slice(left);
    joined.extend_from_slice(right);
    joined
}

pub struct Interpreter {
    pub env: Rc<RefCell<Environment>>,
    pub classes: HashMap<String, ClassDef>,
    pub class_vars: HashMap<String, HashMap<String, Value>>,
    pub traits: HashMap<String, Vec<TraitMember>>,
    pub dispatch: DispatchTable,
    pub output: Vec<String>,
    pub current_file: Option<std::path::PathBuf>,
    pub module_cache: HashMap<std::path::PathBuf, Rc<RefCell<Environment>>>,
    /// Canonical module paths whose environments have been published but are
    /// still being initialized. This distinguishes a recursive declaration
    /// edge from a value-initialization cycle.
    module_loading: HashSet<std::path::PathBuf>,
    builtin_names: HashSet<String>,
    capture_assignment: Option<String>,
    setter_depth: usize,
    loop_depth: usize,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    fn pattern_identifier_binds(name: &str) -> bool {
        !matches!(
            name,
            "_" | "int"
                | "float"
                | "bool"
                | "str"
                | "complex"
                | "bytes"
                | "Bytes"
                | "MemoryView"
                | "list"
                | "set"
                | "dict"
                | "range"
                | "DottedPath"
                | "none"
                | "None"
        ) && !name.chars().next().is_some_and(char::is_uppercase)
    }

    pub fn new() -> Self {
        let env = Rc::new(RefCell::new(Environment::new()));
        let mut interp = Self {
            env,
            classes: HashMap::new(),
            class_vars: HashMap::new(),
            traits: HashMap::new(),
            dispatch: DispatchTable::default(),
            output: Vec::new(),
            current_file: None,
            module_cache: HashMap::new(),
            module_loading: HashSet::new(),
            builtin_names: HashSet::new(),
            capture_assignment: None,
            setter_depth: 0,
            loop_depth: 0,
        };

        interp.register_builtins();
        interp.builtin_names = interp.env.borrow().bindings.keys().cloned().collect();
        interp
    }

    pub fn set_current_file(&mut self, path: Option<std::path::PathBuf>) {
        self.current_file = path;
    }

    fn eval_loop_body(&mut self, body: &[Stmt]) -> Result<Value, RuntimeError> {
        self.loop_depth += 1;
        let result = self.eval_block(body);
        self.loop_depth -= 1;
        result
    }

    fn mark_final_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Ident(name, _) if Self::pattern_identifier_binds(name) => {
                self.env.borrow_mut().mark_final(name.clone());
            }
            Pattern::Tuple(items, _) => {
                for item in items {
                    self.mark_final_pattern(item);
                }
            }
            Pattern::ClassDestructure { fields, .. } | Pattern::RecordDestructure(fields, _) => {
                for (_, item) in fields {
                    self.mark_final_pattern(item);
                }
            }
            Pattern::Star(item, _) => self.mark_final_pattern(item),
            Pattern::Ident(_, _)
            | Pattern::Literal(_, _)
            | Pattern::Wildcard(_)
            | Pattern::Type(_, _) => {}
        }
    }

    fn field_is_final(&self, class_name: &str, field_name: &str) -> bool {
        let Some(class) = self.classes.get(class_name) else {
            return false;
        };
        if class.body.iter().any(|member| {
            matches!(member, ClassMember::Field(field) | ClassMember::ClassVar(field) if field.name == field_name && field.is_final)
        }) {
            return true;
        }
        class.bases.iter().any(|base| match base {
            TypeExpr::Named { name, .. } => self.field_is_final(name, field_name),
            _ => false,
        })
    }

    fn is_subclass(&self, class_name: &str, parent_name: &str) -> bool {
        let mut pending = vec![class_name.to_owned()];
        let mut seen = HashSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(class) = self.classes.get(&current) else {
                continue;
            };
            for base in &class.bases {
                let TypeExpr::Named { name, .. } = base else {
                    continue;
                };
                if name == parent_name {
                    return true;
                }
                pending.push(name.clone());
            }
        }
        false
    }

    fn class_var_get(&self, class_name: &str, name: &str) -> Option<Value> {
        if let Some(value) = self
            .class_vars
            .get(class_name)
            .and_then(|vars| vars.get(name))
        {
            return Some(value.clone());
        }
        self.classes.get(class_name).and_then(|class| {
            class.bases.iter().find_map(|base| match base {
                TypeExpr::Named { name: parent, .. } => self.class_var_get(parent, name),
                _ => None,
            })
        })
    }

    fn class_var_set(&mut self, class_name: &str, name: &str, value: Value) -> bool {
        if self
            .class_vars
            .get(class_name)
            .is_some_and(|vars| vars.contains_key(name))
        {
            self.class_vars
                .entry(class_name.to_string())
                .or_default()
                .insert(name.to_string(), value);
            return true;
        }
        let parent = self.classes.get(class_name).and_then(|class| {
            class.bases.iter().find_map(|base| match base {
                TypeExpr::Named { name, .. } if self.class_vars.contains_key(name) => {
                    Some(name.clone())
                }
                _ => None,
            })
        });
        if let Some(parent) = parent {
            return self.class_var_set(&parent, name, value);
        }
        false
    }

    fn private_member_owner(&self, class_name: &str, member_name: &str) -> Option<String> {
        let class = self.classes.get(class_name)?;
        let declared = class.body.iter().any(|member| match member {
            ClassMember::Field(field) | ClassMember::ClassVar(field) => field.name == member_name,
            ClassMember::Method(method) | ClassMember::ClassMethod(method) => {
                method.name == member_name
            }
            ClassMember::Factory(factory) => factory.name == member_name,
            ClassMember::Getter(getter) => getter.name == member_name,
            ClassMember::Setter(setter) => setter.name == member_name,
            ClassMember::TypeAlias { name, .. } => name == member_name,
            ClassMember::Pass(_) | ClassMember::Ellipsis(_) => false,
        });
        if declared {
            return Some(class_name.to_string());
        }
        class.bases.iter().find_map(|base| match base {
            TypeExpr::Named { name, .. } => self.private_member_owner(name, member_name),
            _ => None,
        })
    }

    fn class_members_for_lookup(&self, class_name: &str) -> Vec<ClassMember> {
        let Some(class) = self.classes.get(class_name) else {
            return Vec::new();
        };
        let mut members = class.body.clone();
        for base in &class.bases {
            if let TypeExpr::Named { name, .. } = base {
                members.extend(self.class_members_for_lookup(name));
            }
        }
        members
    }

    fn check_private_access(
        &self,
        class_name: &str,
        member_name: &str,
        span: Span,
    ) -> Result<(), RuntimeError> {
        if !member_name.starts_with('_') {
            return Ok(());
        }
        let Some(owner) = self.private_member_owner(class_name, member_name) else {
            return Ok(());
        };
        let active = self.env.borrow().get("__current_class");
        let allowed = matches!(active, Some(Value::Str(ref current)) if current == &owner);
        if !allowed {
            return Err(RuntimeError {
                message: format!("member '{member_name}' is private to class '{owner}'"),
                span,
            });
        }
        Ok(())
    }

    pub fn call_dispatch(&mut self, name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
        let classes = &self.classes;
        let func = self.dispatch.find_match_by(name, args, |actual, expected| {
            runtime_type_distance(classes, actual, expected)
        });
        if let Some(func) = func {
            func(args, self)
        } else {
            Err(RuntimeError {
                message: format!(
                    "no dispatch method found for '{name}' with argument types {:?}",
                    args.iter().map(|a| a.type_name()).collect::<Vec<_>>()
                ),
                span: Span::default(),
            })
        }
    }

    pub fn eval_binary_op(
        &mut self,
        op: &BinaryOp,
        lval: Value,
        rval: Value,
        span: &Span,
    ) -> Result<Value, RuntimeError> {
        match op {
            BinaryOp::Add => match (&lval, &rval) {
                (Value::Complex(ar, ai), Value::Complex(br, bi)) => {
                    Ok(Value::Complex(ar + br, ai + bi))
                }
                (Value::Complex(ar, ai), Value::Int(b)) => Ok(Value::Complex(ar + *b as f64, *ai)),
                (Value::Complex(ar, ai), Value::Float(b)) => Ok(Value::Complex(ar + b, *ai)),
                (Value::Int(a), Value::Complex(br, bi)) => Ok(Value::Complex(*a as f64 + br, *bi)),
                (Value::Float(a), Value::Complex(br, bi)) => Ok(Value::Complex(a + br, *bi)),
                (Value::Int(a), Value::Int(b)) => match a.checked_add(*b) {
                    Some(value) => Ok(Value::Int(value)),
                    None => Ok(Value::BigInt(BigInt::from(*a) + BigInt::from(*b))),
                },
                (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::BigInt(a + b)),
                (Value::BigInt(a), Value::Int(b)) => Ok(Value::BigInt(a + BigInt::from(*b))),
                (Value::Int(a), Value::BigInt(b)) => Ok(Value::BigInt(BigInt::from(*a) + b)),
                (Value::BigInt(a), Value::Float(b)) => Ok(Value::Float(bigint_to_float(a) + b)),
                (Value::Float(a), Value::BigInt(b)) => Ok(Value::Float(a + bigint_to_float(b))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
                (Value::Bytes(a), Value::Bytes(b)) => Ok(Value::Bytes(concat_bytes(a, b))),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{a}{b}"))),
                (Value::List(a), Value::List(b)) => {
                    let mut combined = a.borrow().clone();
                    combined.extend(b.borrow().clone());
                    Ok(Value::List(Rc::new(RefCell::new(combined))))
                }
                _ => self.call_dispatch("+", &[lval, rval]),
            },
            BinaryOp::Sub => match (&lval, &rval) {
                (Value::Complex(ar, ai), Value::Complex(br, bi)) => {
                    Ok(Value::Complex(ar - br, ai - bi))
                }
                (Value::Complex(ar, ai), Value::Int(b)) => Ok(Value::Complex(ar - *b as f64, *ai)),
                (Value::Complex(ar, ai), Value::Float(b)) => Ok(Value::Complex(ar - b, *ai)),
                (Value::Int(a), Value::Complex(br, bi)) => Ok(Value::Complex(*a as f64 - br, -*bi)),
                (Value::Float(a), Value::Complex(br, bi)) => Ok(Value::Complex(a - br, -*bi)),
                (Value::Int(a), Value::Int(b)) => match a.checked_sub(*b) {
                    Some(value) => Ok(Value::Int(value)),
                    None => Ok(Value::BigInt(BigInt::from(*a) - BigInt::from(*b))),
                },
                (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::BigInt(a - b)),
                (Value::BigInt(a), Value::Int(b)) => Ok(Value::BigInt(a - BigInt::from(*b))),
                (Value::Int(a), Value::BigInt(b)) => Ok(Value::BigInt(BigInt::from(*a) - b)),
                (Value::BigInt(a), Value::Float(b)) => Ok(Value::Float(bigint_to_float(a) - b)),
                (Value::Float(a), Value::BigInt(b)) => Ok(Value::Float(a - bigint_to_float(b))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
                (Value::Set(a), Value::Set(b)) => Ok(Value::Set(Rc::new(RefCell::new(
                    a.borrow()
                        .iter()
                        .filter(|item| !b.borrow().contains(item))
                        .cloned()
                        .collect(),
                )))),
                _ => self.call_dispatch("-", &[lval, rval]),
            },
            BinaryOp::Mul => match (&lval, &rval) {
                (Value::Complex(ar, ai), Value::Complex(br, bi)) => {
                    Ok(Value::Complex(ar * br - ai * bi, ar * bi + ai * br))
                }
                (Value::Complex(ar, ai), Value::Int(b)) => {
                    Ok(Value::Complex(ar * *b as f64, ai * *b as f64))
                }
                (Value::Complex(ar, ai), Value::Float(b)) => Ok(Value::Complex(ar * b, ai * b)),
                (Value::Int(a), Value::Complex(br, bi)) => {
                    Ok(Value::Complex(*a as f64 * br, *a as f64 * bi))
                }
                (Value::Float(a), Value::Complex(br, bi)) => Ok(Value::Complex(a * br, a * bi)),
                (Value::Int(a), Value::Int(b)) => match a.checked_mul(*b) {
                    Some(value) => Ok(Value::Int(value)),
                    None => Ok(Value::BigInt(BigInt::from(*a) * BigInt::from(*b))),
                },
                (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::BigInt(a * b)),
                (Value::BigInt(a), Value::Int(b)) => Ok(Value::BigInt(a * BigInt::from(*b))),
                (Value::Int(a), Value::BigInt(b)) => Ok(Value::BigInt(BigInt::from(*a) * b)),
                (Value::BigInt(a), Value::Float(b)) => Ok(Value::Float(bigint_to_float(a) * b)),
                (Value::Float(a), Value::BigInt(b)) => Ok(Value::Float(a * bigint_to_float(b))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * *b as f64)),
                (Value::Str(s), Value::Int(n)) => Ok(Value::Str(s.repeat((*n).max(0) as usize))),
                (Value::Bytes(bytes), Value::Int(n)) => Ok(Value::Bytes(repeat_bytes(bytes, *n))),
                (Value::Int(n), Value::Bytes(bytes)) => Ok(Value::Bytes(repeat_bytes(bytes, *n))),
                (Value::List(items), Value::Int(n)) => {
                    let count = (*n).max(0) as usize;
                    let inner = items.borrow();
                    let mut repeated = Vec::with_capacity(inner.len() * count);
                    for _ in 0..count {
                        repeated.extend(inner.clone());
                    }
                    Ok(Value::List(Rc::new(RefCell::new(repeated))))
                }
                (Value::Int(n), Value::List(items)) => {
                    let count = (*n).max(0) as usize;
                    let inner = items.borrow();
                    let mut repeated = Vec::with_capacity(inner.len() * count);
                    for _ in 0..count {
                        repeated.extend(inner.clone());
                    }
                    Ok(Value::List(Rc::new(RefCell::new(repeated))))
                }
                _ => self.call_dispatch("*", &[lval, rval]),
            },
            BinaryOp::Div => match (&lval, &rval) {
                (Value::Complex(ar, ai), Value::Complex(br, bi)) => {
                    let denom = br * br + bi * bi;
                    if denom == 0.0 {
                        Err(RuntimeError {
                            message: "division by zero".into(),
                            span: *span,
                        })
                    } else {
                        Ok(Value::Complex(
                            (ar * br + ai * bi) / denom,
                            (ai * br - ar * bi) / denom,
                        ))
                    }
                }
                (Value::Complex(ar, ai), Value::Int(b)) => {
                    if *b == 0 {
                        Err(RuntimeError {
                            message: "division by zero".into(),
                            span: *span,
                        })
                    } else {
                        Ok(Value::Complex(ar / *b as f64, ai / *b as f64))
                    }
                }
                (Value::Complex(ar, ai), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err(RuntimeError {
                            message: "division by zero".into(),
                            span: *span,
                        })
                    } else {
                        Ok(Value::Complex(ar / b, ai / b))
                    }
                }
                (Value::Int(a), Value::Complex(br, bi)) => {
                    let real = *a as f64;
                    let denom = br * br + bi * bi;
                    if denom == 0.0 {
                        Err(RuntimeError {
                            message: "division by zero".into(),
                            span: *span,
                        })
                    } else {
                        Ok(Value::Complex(real * br / denom, -real * bi / denom))
                    }
                }
                (Value::Float(a), Value::Complex(br, bi)) => {
                    let real = *a;
                    let denom = br * br + bi * bi;
                    if denom == 0.0 {
                        Err(RuntimeError {
                            message: "division by zero".into(),
                            span: *span,
                        })
                    } else {
                        Ok(Value::Complex(real * br / denom, -real * bi / denom))
                    }
                }
                (Value::Int(a), Value::Int(b)) => {
                    if *b == 0 {
                        Ok(Value::Float(if *a == 0 {
                            f64::NAN
                        } else if *a < 0 {
                            f64::NEG_INFINITY
                        } else {
                            f64::INFINITY
                        }))
                    } else {
                        Ok(Value::Float(*a as f64 / *b as f64))
                    }
                }
                (Value::Float(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Ok(Value::Float(if *a == 0.0 {
                            f64::NAN
                        } else if a.is_sign_negative() ^ b.is_sign_negative() {
                            f64::NEG_INFINITY
                        } else {
                            f64::INFINITY
                        }))
                    } else {
                        Ok(Value::Float(a / b))
                    }
                }
                (Value::Int(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Ok(Value::Float(if *a == 0 {
                            f64::NAN
                        } else if (*a < 0) ^ b.is_sign_negative() {
                            f64::NEG_INFINITY
                        } else {
                            f64::INFINITY
                        }))
                    } else {
                        Ok(Value::Float(*a as f64 / b))
                    }
                }
                (Value::Float(a), Value::Int(b)) => {
                    if *b == 0 {
                        Ok(Value::Float(if *a == 0.0 {
                            f64::NAN
                        } else if a.is_sign_negative() ^ (*b < 0) {
                            f64::NEG_INFINITY
                        } else {
                            f64::INFINITY
                        }))
                    } else {
                        Ok(Value::Float(a / *b as f64))
                    }
                }
                (Value::BigInt(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Ok(Value::Float(if a.is_zero() {
                            f64::NAN
                        } else if a.is_negative() ^ b.is_sign_negative() {
                            f64::NEG_INFINITY
                        } else {
                            f64::INFINITY
                        }))
                    } else {
                        Ok(Value::Float(bigint_to_float(a) / b))
                    }
                }
                (Value::Float(a), Value::BigInt(b)) => {
                    if b.is_zero() {
                        Ok(Value::Float(if *a == 0.0 {
                            f64::NAN
                        } else if a.is_sign_negative() ^ b.is_negative() {
                            f64::NEG_INFINITY
                        } else {
                            f64::INFINITY
                        }))
                    } else {
                        Ok(Value::Float(*a / bigint_to_float(b)))
                    }
                }
                _ => self.call_dispatch("/", &[lval, rval]),
            },
            BinaryOp::FloorDiv => match (&lval, &rval) {
                (Value::BigInt(a), Value::BigInt(b)) => {
                    if b.is_zero() {
                        Ok(Value::Int(INT_NAN))
                    } else {
                        let q = a / b;
                        let r = a % b;
                        Ok(Value::BigInt(if !r.is_zero() && (a.sign() != b.sign()) {
                            q - BigInt::one()
                        } else {
                            q
                        }))
                    }
                }
                (Value::BigInt(a), Value::Int(b)) => {
                    if *b == 0 {
                        Ok(Value::Int(INT_NAN))
                    } else {
                        let divisor = BigInt::from(*b);
                        let q = a / &divisor;
                        let r = a % &divisor;
                        Ok(Value::BigInt(
                            if !r.is_zero() && (a.sign() != divisor.sign()) {
                                q - BigInt::one()
                            } else {
                                q
                            },
                        ))
                    }
                }
                (Value::Int(a), Value::BigInt(b)) => {
                    if b.is_zero() {
                        Ok(Value::Int(INT_NAN))
                    } else {
                        let dividend = BigInt::from(*a);
                        let q = &dividend / b;
                        let r = &dividend % b;
                        Ok(Value::BigInt(
                            if !r.is_zero() && (dividend.sign() != b.sign()) {
                                q - BigInt::one()
                            } else {
                                q
                            },
                        ))
                    }
                }
                (Value::Int(a), Value::Int(b)) => {
                    if *b == 0 {
                        Ok(if *a == 0 {
                            Value::Int(INT_NAN)
                        } else if *a < 0 {
                            Value::Int(INT_NEG_INF)
                        } else {
                            Value::Int(INT_POS_INF)
                        })
                    } else {
                        match a.checked_div_euclid(*b) {
                            Some(value) => Ok(Value::Int(value)),
                            None => Err(RuntimeError {
                                message: "integer overflow in //".to_string(),
                                span: *span,
                            }),
                        }
                    }
                }
                (Value::Float(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err(RuntimeError {
                            message: "division by zero in //".to_string(),
                            span: *span,
                        })
                    } else {
                        Ok(Value::Float((a / b).floor()))
                    }
                }
                (Value::Int(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err(RuntimeError {
                            message: "division by zero in //".to_string(),
                            span: *span,
                        })
                    } else {
                        Ok(Value::Float((*a as f64 / b).floor()))
                    }
                }
                (Value::Float(a), Value::Int(b)) => {
                    if *b == 0 {
                        Err(RuntimeError {
                            message: "division by zero in //".to_string(),
                            span: *span,
                        })
                    } else {
                        Ok(Value::Float((a / *b as f64).floor()))
                    }
                }
                (Value::BigInt(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err(RuntimeError {
                            message: "division by zero in //".to_string(),
                            span: *span,
                        })
                    } else {
                        Ok(Value::Float((bigint_to_float(a) / b).floor()))
                    }
                }
                (Value::Float(a), Value::BigInt(b)) => {
                    if b.is_zero() {
                        Err(RuntimeError {
                            message: "division by zero in //".to_string(),
                            span: *span,
                        })
                    } else {
                        Ok(Value::Float((a / bigint_to_float(b)).floor()))
                    }
                }
                _ => Err(RuntimeError {
                    message: "unsupported operands for //".to_string(),
                    span: *span,
                }),
            },
            BinaryOp::Mod => match (&lval, &rval) {
                (Value::BigInt(a), Value::BigInt(b)) => {
                    if b.is_zero() {
                        Ok(Value::Int(INT_NAN))
                    } else {
                        let r = a % b;
                        Ok(Value::BigInt(if !r.is_zero() && (a.sign() != b.sign()) {
                            r + b
                        } else {
                            r
                        }))
                    }
                }
                (Value::BigInt(a), Value::Int(b)) => {
                    if *b == 0 {
                        Ok(Value::Int(INT_NAN))
                    } else {
                        let divisor = BigInt::from(*b);
                        let r = a % &divisor;
                        Ok(Value::BigInt(
                            if !r.is_zero() && (a.sign() != divisor.sign()) {
                                r + divisor
                            } else {
                                r
                            },
                        ))
                    }
                }
                (Value::Int(a), Value::BigInt(b)) => {
                    if b.is_zero() {
                        Ok(Value::Int(INT_NAN))
                    } else {
                        let dividend = BigInt::from(*a);
                        let r = &dividend % b;
                        Ok(Value::BigInt(
                            if !r.is_zero() && (dividend.sign() != b.sign()) {
                                r + b
                            } else {
                                r
                            },
                        ))
                    }
                }
                (Value::Int(a), Value::Int(b)) => {
                    if *b == 0 {
                        let _ = span;
                        Ok(Value::Int(INT_NAN))
                    } else {
                        match a.checked_rem_euclid(*b) {
                            Some(value) => Ok(Value::Int(value)),
                            None => Err(RuntimeError {
                                message: "integer overflow in %".to_string(),
                                span: *span,
                            }),
                        }
                    }
                }
                (Value::Float(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err(RuntimeError {
                            message: "division by zero in %".to_string(),
                            span: *span,
                        })
                    } else {
                        let remainder = if b.is_infinite() && a.is_finite() {
                            *a
                        } else {
                            a - (a / b).floor() * b
                        };
                        Ok(Value::Float(remainder))
                    }
                }
                (Value::Int(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err(RuntimeError {
                            message: "division by zero in %".to_string(),
                            span: *span,
                        })
                    } else {
                        let a = *a as f64;
                        Ok(Value::Float(if b.is_infinite() && a.is_finite() {
                            a
                        } else {
                            a - (a / b).floor() * b
                        }))
                    }
                }
                (Value::Float(a), Value::Int(b)) => {
                    if *b == 0 {
                        Err(RuntimeError {
                            message: "division by zero in %".to_string(),
                            span: *span,
                        })
                    } else {
                        let b = *b as f64;
                        Ok(Value::Float(if b.is_infinite() && a.is_finite() {
                            *a
                        } else {
                            a - (a / b).floor() * b
                        }))
                    }
                }
                (Value::BigInt(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err(RuntimeError {
                            message: "division by zero in %".to_string(),
                            span: *span,
                        })
                    } else {
                        let a = bigint_to_float(a);
                        Ok(Value::Float(if b.is_infinite() && a.is_finite() {
                            a
                        } else {
                            a - (a / b).floor() * b
                        }))
                    }
                }
                (Value::Float(a), Value::BigInt(b)) => {
                    if b.is_zero() {
                        Err(RuntimeError {
                            message: "division by zero in %".to_string(),
                            span: *span,
                        })
                    } else {
                        let b = bigint_to_float(b);
                        Ok(Value::Float(if b.is_infinite() && a.is_finite() {
                            *a
                        } else {
                            a - (a / b).floor() * b
                        }))
                    }
                }
                _ => Err(RuntimeError {
                    message: "unsupported operands for %".to_string(),
                    span: *span,
                }),
            },
            BinaryOp::Pow => match (&lval, &rval) {
                (Value::Complex(ar, ai), Value::Complex(br, bi)) => {
                    let radius = ar.hypot(*ai);
                    let angle = ai.atan2(*ar);
                    let scale = (br * radius.ln() - bi * angle).exp();
                    let phase = bi * radius.ln() + br * angle;
                    Ok(Value::Complex(scale * phase.cos(), scale * phase.sin()))
                }
                (Value::Complex(ar, ai), Value::Int(exp)) => {
                    let radius = ar.hypot(*ai).powi(*exp as i32);
                    let phase = ai.atan2(*ar) * *exp as f64;
                    Ok(Value::Complex(radius * phase.cos(), radius * phase.sin()))
                }
                (Value::Complex(ar, ai), Value::Float(exp)) => {
                    let radius = ar.hypot(*ai).powf(*exp);
                    let phase = ai.atan2(*ar) * *exp;
                    Ok(Value::Complex(radius * phase.cos(), radius * phase.sin()))
                }
                (Value::BigInt(base), Value::BigInt(exp))
                    if exp.sign() != num_bigint::Sign::Minus =>
                {
                    let mut power = base.clone();
                    let mut exponent = exp.clone();
                    let mut result = BigInt::one();
                    while !exponent.is_zero() {
                        if (&exponent & BigInt::one()) == BigInt::one() {
                            result *= &power;
                        }
                        exponent >>= 1;
                        if !exponent.is_zero() {
                            power = &power * &power;
                        }
                    }
                    Ok(Value::BigInt(result))
                }
                (Value::BigInt(base), Value::BigInt(exp))
                    if exp.sign() != num_bigint::Sign::Minus =>
                {
                    let mut power = base.clone();
                    let mut exponent = exp.clone();
                    let mut result = BigInt::one();
                    while !exponent.is_zero() {
                        if (&exponent & BigInt::one()) == BigInt::one() {
                            result *= &power;
                        }
                        exponent >>= 1;
                        if !exponent.is_zero() {
                            power = &power * &power;
                        }
                    }
                    Ok(Value::BigInt(result))
                }
                (Value::BigInt(base), Value::BigInt(exp)) => Ok(Value::Float(
                    bigint_to_float(base).powf(bigint_to_float(exp)),
                )),
                (Value::BigInt(base), Value::Int(exp)) if *exp >= 0 => {
                    let mut power = base.clone();
                    let mut exponent = *exp as u64;
                    let mut result = BigInt::one();
                    while exponent != 0 {
                        if exponent & 1 == 1 {
                            result *= &power;
                        }
                        exponent >>= 1;
                        if exponent != 0 {
                            power = &power * &power;
                        }
                    }
                    Ok(Value::BigInt(result))
                }
                (Value::BigInt(base), Value::Int(exp)) => {
                    Ok(Value::Float(bigint_to_float(base).powi(*exp as i32)))
                }
                (Value::Int(base), Value::BigInt(exp)) if exp.sign() != num_bigint::Sign::Minus => {
                    let mut power = BigInt::from(*base);
                    let mut exponent = exp.clone();
                    let mut result = BigInt::one();
                    while !exponent.is_zero() {
                        if (&exponent & BigInt::one()) == BigInt::one() {
                            result *= &power;
                        }
                        exponent >>= 1;
                        if !exponent.is_zero() {
                            power = &power * &power;
                        }
                    }
                    Ok(Value::BigInt(result))
                }
                (Value::Int(base), Value::BigInt(exp)) => {
                    Ok(Value::Float((*base as f64).powf(bigint_to_float(exp))))
                }
                (Value::Int(a), Value::Int(b)) => {
                    if *b >= 0 {
                        if let Ok(exp) = u32::try_from(*b) {
                            if let Some(res) = a.checked_pow(exp) {
                                return Ok(Value::Int(res));
                            }
                            return Ok(Value::BigInt(BigInt::from(*a).pow(exp)));
                        }
                        Err(RuntimeError {
                            message: "integer exponent is too large".to_string(),
                            span: *span,
                        })
                    } else {
                        Ok(Value::Float((*a as f64).powi(*b as i32)))
                    }
                }
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a.powf(*b))),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a.powi(*b as i32))),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float((*a as f64).powf(*b))),
                (Value::BigInt(a), Value::Float(b)) => {
                    Ok(Value::Float(bigint_to_float(a).powf(*b)))
                }
                (Value::Float(a), Value::BigInt(b)) => Ok(Value::Float(a.powf(bigint_to_float(b)))),
                _ => Err(RuntimeError {
                    message: "unsupported operands for **".to_string(),
                    span: *span,
                }),
            },
            BinaryOp::BitAnd => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
                (Value::Set(a), Value::Set(b)) => Ok(Value::Set(Rc::new(RefCell::new(
                    a.borrow()
                        .iter()
                        .filter(|item| b.borrow().contains(item))
                        .cloned()
                        .collect(),
                )))),
                _ => Err(RuntimeError {
                    message: "unsupported operands for &".to_string(),
                    span: *span,
                }),
            },
            BinaryOp::BitOr => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
                (Value::Set(a), Value::Set(b)) => {
                    let mut values = a.borrow().clone();
                    for item in b.borrow().iter() {
                        if !values.contains(item) {
                            values.push(item.clone());
                        }
                    }
                    Ok(Value::Set(Rc::new(RefCell::new(values))))
                }
                _ => Err(RuntimeError {
                    message: "unsupported operands for |".to_string(),
                    span: *span,
                }),
            },
            BinaryOp::BitXor => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a ^ b)),
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a ^ *b)),
                (Value::Set(a), Value::Set(b)) => {
                    let left = a.borrow();
                    let right = b.borrow();
                    let values = left
                        .iter()
                        .chain(right.iter())
                        .filter(|item| left.contains(item) != right.contains(item))
                        .cloned()
                        .collect();
                    Ok(Value::Set(Rc::new(RefCell::new(values))))
                }
                _ => Err(RuntimeError {
                    message: "unsupported operands for ^".to_string(),
                    span: *span,
                }),
            },
            BinaryOp::Shl => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => {
                    if *b < 0 {
                        Err(RuntimeError {
                            message: "negative shift count".to_string(),
                            span: *span,
                        })
                    } else if *b >= 64 {
                        Ok(Value::Int(0))
                    } else {
                        Ok(Value::Int(a << b))
                    }
                }
                _ => Err(RuntimeError {
                    message: "unsupported operands for <<".to_string(),
                    span: *span,
                }),
            },
            BinaryOp::Shr => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => {
                    if *b < 0 {
                        Err(RuntimeError {
                            message: "negative shift count".to_string(),
                            span: *span,
                        })
                    } else if *b >= 64 {
                        Ok(Value::Int(0))
                    } else {
                        Ok(Value::Int(a >> b))
                    }
                }
                _ => Err(RuntimeError {
                    message: "unsupported operands for >>".to_string(),
                    span: *span,
                }),
            },
            BinaryOp::Eq => match (&lval, &rval) {
                (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::Bool(a == b)),
                (Value::BigInt(a), Value::Int(b)) | (Value::Int(b), Value::BigInt(a)) => {
                    Ok(Value::Bool(*a == BigInt::from(*b)))
                }
                (Value::Complex(ar, ai), Value::Complex(br, bi)) => {
                    Ok(Value::Bool(ar == br && ai == bi))
                }
                (Value::Complex(ar, ai), Value::Int(b)) => {
                    Ok(Value::Bool(*ar == *b as f64 && *ai == 0.0))
                }
                (Value::Complex(ar, ai), Value::Float(b)) => {
                    Ok(Value::Bool(*ar == *b && *ai == 0.0))
                }
                (Value::Int(a), Value::Complex(br, bi)) => {
                    Ok(Value::Bool(*a as f64 == *br && *bi == 0.0))
                }
                (Value::Float(a), Value::Complex(br, bi)) => {
                    Ok(Value::Bool(*a == *br && *bi == 0.0))
                }
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) == *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a == (*b as f64))),
                (Value::BigInt(a), Value::Float(b)) => Ok(Value::Bool(bigint_to_float(a) == *b)),
                (Value::Float(a), Value::BigInt(b)) => Ok(Value::Bool(*a == bigint_to_float(b))),
                _ => Ok(Value::Bool(lval == rval)),
            },
            BinaryOp::NotEq => match (&lval, &rval) {
                (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::Bool(a != b)),
                (Value::BigInt(a), Value::Int(b)) | (Value::Int(b), Value::BigInt(a)) => {
                    Ok(Value::Bool(*a != BigInt::from(*b)))
                }
                (Value::Complex(ar, ai), Value::Complex(br, bi)) => {
                    Ok(Value::Bool(ar != br || ai != bi))
                }
                (Value::Complex(ar, ai), Value::Int(b)) => {
                    Ok(Value::Bool(*ar != *b as f64 || *ai != 0.0))
                }
                (Value::Complex(ar, ai), Value::Float(b)) => {
                    Ok(Value::Bool(*ar != *b || *ai != 0.0))
                }
                (Value::Int(a), Value::Complex(br, bi)) => {
                    Ok(Value::Bool(*a as f64 != *br || *bi != 0.0))
                }
                (Value::Float(a), Value::Complex(br, bi)) => {
                    Ok(Value::Bool(*a != *br || *bi != 0.0))
                }
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) != *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a != (*b as f64))),
                (Value::BigInt(a), Value::Float(b)) => Ok(Value::Bool(bigint_to_float(a) != *b)),
                (Value::Float(a), Value::BigInt(b)) => Ok(Value::Bool(*a != bigint_to_float(b))),
                _ => Ok(Value::Bool(lval != rval)),
            },
            BinaryOp::And => {
                if !self.is_truthy(&lval) {
                    Ok(lval)
                } else {
                    Ok(rval)
                }
            }
            BinaryOp::Or => {
                if self.is_truthy(&lval) {
                    Ok(lval)
                } else {
                    Ok(rval)
                }
            }
            BinaryOp::Lt => match (&lval, &rval) {
                (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::Bool(a < b)),
                (Value::BigInt(a), Value::Int(b)) => Ok(Value::Bool(a < &BigInt::from(*b))),
                (Value::Int(a), Value::BigInt(b)) => Ok(Value::Bool(BigInt::from(*a) < *b)),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) < *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a < (*b as f64))),
                (Value::BigInt(a), Value::Float(b)) => Ok(Value::Bool(bigint_to_float(a) < *b)),
                (Value::Float(a), Value::BigInt(b)) => Ok(Value::Bool(*a < bigint_to_float(b))),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(a < b)),
                _ => Err(RuntimeError {
                    message: "unsupported operands for <".to_string(),
                    span: *span,
                }),
            },
            BinaryOp::Gt => match (&lval, &rval) {
                (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::Bool(a > b)),
                (Value::BigInt(a), Value::Int(b)) => Ok(Value::Bool(a > &BigInt::from(*b))),
                (Value::Int(a), Value::BigInt(b)) => Ok(Value::Bool(BigInt::from(*a) > *b)),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) > *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a > (*b as f64))),
                (Value::BigInt(a), Value::Float(b)) => Ok(Value::Bool(bigint_to_float(a) > *b)),
                (Value::Float(a), Value::BigInt(b)) => Ok(Value::Bool(*a > bigint_to_float(b))),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(a > b)),
                _ => Err(RuntimeError {
                    message: "unsupported operands for >".to_string(),
                    span: *span,
                }),
            },
            BinaryOp::LtEq => match (&lval, &rval) {
                (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::Bool(a <= b)),
                (Value::BigInt(a), Value::Int(b)) => Ok(Value::Bool(a <= &BigInt::from(*b))),
                (Value::Int(a), Value::BigInt(b)) => Ok(Value::Bool(BigInt::from(*a) <= *b)),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) <= *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a <= (*b as f64))),
                (Value::BigInt(a), Value::Float(b)) => Ok(Value::Bool(bigint_to_float(a) <= *b)),
                (Value::Float(a), Value::BigInt(b)) => Ok(Value::Bool(*a <= bigint_to_float(b))),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(a <= b)),
                _ => Err(RuntimeError {
                    message: "unsupported operands for <=".to_string(),
                    span: *span,
                }),
            },
            BinaryOp::GtEq => match (&lval, &rval) {
                (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::Bool(a >= b)),
                (Value::BigInt(a), Value::Int(b)) => Ok(Value::Bool(a >= &BigInt::from(*b))),
                (Value::Int(a), Value::BigInt(b)) => Ok(Value::Bool(BigInt::from(*a) >= *b)),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) >= *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a >= (*b as f64))),
                (Value::BigInt(a), Value::Float(b)) => Ok(Value::Bool(bigint_to_float(a) >= *b)),
                (Value::Float(a), Value::BigInt(b)) => Ok(Value::Bool(*a >= bigint_to_float(b))),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(a >= b)),
                _ => Err(RuntimeError {
                    message: "unsupported operands for >=".to_string(),
                    span: *span,
                }),
            },
            BinaryOp::In => {
                if let Value::Object { fields, .. } = &rval {
                    if let Some(method) = fields.borrow().get("__contains__").cloned() {
                        return self.invoke_value(
                            method,
                            vec![(None, rval.clone()), (None, lval.clone())],
                            *span,
                        );
                    }
                }
                let contains = match &rval {
                    Value::List(l) => l.borrow().contains(&lval),
                    Value::Set(s) => s.borrow().contains(&lval),
                    Value::Dict(d) => match &lval {
                        Value::Str(s) => d.borrow().contains_key(s),
                        other => d.borrow().contains_key(&format!("{other:?}")),
                    },
                    Value::Range { start, stop, step } => match lval {
                        Value::Int(value) => range_contains(*start, *stop, *step, value),
                        _ => false,
                    },
                    Value::Bytes(bytes) => match lval {
                        Value::Int(value) => u8::try_from(value)
                            .ok()
                            .is_some_and(|byte| bytes.contains(&byte)),
                        _ => false,
                    },
                    Value::MemoryView {
                        data,
                        start,
                        len,
                        stride,
                        ..
                    } => match lval {
                        Value::Int(value) => {
                            let items = data.borrow();
                            (0..*len).any(|offset| {
                                items
                                    .get((start + offset * stride) as usize)
                                    .is_some_and(|item| item == &Value::Int(value))
                            })
                        }
                        _ => false,
                    },
                    Value::Str(s) => match &lval {
                        Value::Str(sub) => s.contains(sub),
                        _ => false,
                    },
                    _ => {
                        return Err(RuntimeError {
                            message: format!(
                                "'in' operator not supported for {}",
                                rval.type_name()
                            ),
                            span: *span,
                        })
                    }
                };
                Ok(Value::Bool(contains))
            }
            BinaryOp::NotIn => {
                if let Value::Object { fields, .. } = &rval {
                    if let Some(method) = fields.borrow().get("__contains__").cloned() {
                        let result = self.invoke_value(
                            method,
                            vec![(None, rval.clone()), (None, lval.clone())],
                            *span,
                        )?;
                        return Ok(Value::Bool(!self.is_truthy(&result)));
                    }
                }
                let contains = match &rval {
                    Value::List(l) => l.borrow().contains(&lval),
                    Value::Set(s) => s.borrow().contains(&lval),
                    Value::Dict(d) => match &lval {
                        Value::Str(s) => d.borrow().contains_key(s),
                        _ => false,
                    },
                    Value::Range { start, stop, step } => match lval {
                        Value::Int(value) => range_contains(*start, *stop, *step, value),
                        _ => false,
                    },
                    Value::Bytes(bytes) => match lval {
                        Value::Int(value) => u8::try_from(value)
                            .ok()
                            .is_some_and(|byte| bytes.contains(&byte)),
                        _ => false,
                    },
                    Value::MemoryView {
                        data,
                        start,
                        len,
                        stride,
                        ..
                    } => match lval {
                        Value::Int(value) => {
                            let items = data.borrow();
                            (0..*len).any(|offset| {
                                items
                                    .get((start + offset * stride) as usize)
                                    .is_some_and(|item| item == &Value::Int(value))
                            })
                        }
                        _ => false,
                    },
                    Value::Str(s) => match &lval {
                        Value::Str(sub) => s.contains(sub),
                        _ => false,
                    },
                    _ => {
                        return Err(RuntimeError {
                            message: format!(
                                "'not in' operator not supported for {}",
                                rval.type_name()
                            ),
                            span: *span,
                        })
                    }
                };
                Ok(Value::Bool(!contains))
            }
            BinaryOp::Identity | BinaryOp::Is => {
                if let Value::Str(marker) = &rval {
                    if let Some(kind) = marker.strip_prefix("__type:") {
                        return Ok(Value::Bool(match kind {
                            "class" => !matches!(lval, Value::None),
                            "trait" => false,
                            "interface" => false,
                            "Callable" => matches!(
                                lval,
                                Value::Function { .. }
                                    | Value::BuiltinFunction { .. }
                                    | Value::Partial { .. }
                            ),
                            name => match &lval {
                                Value::Object { class_name, .. } => {
                                    class_name == name || self.is_subclass(class_name, name)
                                }
                                _ => lval.type_name() == name,
                            },
                        }));
                    }
                }
                let same = match (&lval, &rval) {
                    (Value::None, Value::None) => true,
                    (Value::None, _) | (_, Value::None) => false,
                    (Value::Bool(a), Value::Bool(b)) => a == b,
                    (Value::Int(a), Value::Int(b)) => a == b,
                    (Value::Object { fields: f1, .. }, Value::Object { fields: f2, .. }) => {
                        Rc::ptr_eq(f1, f2)
                    }
                    (Value::List(a), Value::List(b)) => Rc::ptr_eq(a, b),
                    (Value::Dict(a), Value::Dict(b)) => Rc::ptr_eq(a, b),
                    (Value::Set(a), Value::Set(b)) => Rc::ptr_eq(a, b),
                    _ => lval == rval,
                };
                Ok(Value::Bool(same))
            }
            BinaryOp::NotIdentity | BinaryOp::IsNot => {
                if let Value::Str(marker) = &rval {
                    if let Some(kind) = marker.strip_prefix("__type:") {
                        return Ok(Value::Bool(!match kind {
                            "class" => !matches!(lval, Value::None),
                            "trait" => false,
                            "interface" => false,
                            "Callable" => matches!(
                                lval,
                                Value::Function { .. }
                                    | Value::BuiltinFunction { .. }
                                    | Value::Partial { .. }
                            ),
                            name => match &lval {
                                Value::Object { class_name, .. } => {
                                    class_name == name || self.is_subclass(class_name, name)
                                }
                                _ => lval.type_name() == name,
                            },
                        }));
                    }
                }
                let same = match (&lval, &rval) {
                    (Value::None, Value::None) => true,
                    (Value::None, _) | (_, Value::None) => false,
                    (Value::Bool(a), Value::Bool(b)) => a == b,
                    (Value::Int(a), Value::Int(b)) => a == b,
                    (Value::Object { fields: f1, .. }, Value::Object { fields: f2, .. }) => {
                        Rc::ptr_eq(f1, f2)
                    }
                    (Value::List(a), Value::List(b)) => Rc::ptr_eq(a, b),
                    (Value::Dict(a), Value::Dict(b)) => Rc::ptr_eq(a, b),
                    (Value::Set(a), Value::Set(b)) => Rc::ptr_eq(a, b),
                    _ => lval == rval,
                };
                Ok(Value::Bool(!same))
            }
        }
    }

    fn register_builtins(&mut self) {
        // print(...)
        let print_fn = Rc::new(|args: &[Value], interp: &mut Interpreter| {
            let s: Vec<String> = args
                .iter()
                .map(|a| match a {
                    Value::Str(text) => text.clone(),
                    Value::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
                    other => format!("{:?}", other),
                })
                .collect();
            let line = s.join(" ");
            interp.output.push(line);
            Ok(Value::None)
        });
        self.env.borrow_mut().set(
            "print".to_string(),
            Value::BuiltinFunction {
                name: "print".to_string(),
                func: print_fn,
            },
        );
        self.env
            .borrow_mut()
            .set("pi".to_string(), Value::Float(std::f64::consts::PI));

        // freeze(obj) -> !T
        let freeze_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if let Some(first) = args.first() {
                first.freeze();
                Ok(first.clone())
            } else {
                Ok(Value::None)
            }
        });
        self.env.borrow_mut().set(
            "freeze".to_string(),
            Value::BuiltinFunction {
                name: "freeze".to_string(),
                func: freeze_fn,
            },
        );

        // Sentinel()
        let sentinel_fn = Rc::new(|_args: &[Value], _interp: &mut Interpreter| {
            Ok(Value::Sentinel("Sentinel".to_string()))
        });
        self.env.borrow_mut().set(
            "Sentinel".to_string(),
            Value::BuiltinFunction {
                name: "Sentinel".to_string(),
                func: sentinel_fn,
            },
        );

        // Cell(val)
        let cell_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            let mut fields = HashMap::new();
            fields.insert("value".into(), args.first().cloned().unwrap_or(Value::None));
            Ok(Value::Object {
                class_name: "Cell".into(),
                fields: Rc::new(RefCell::new(fields)),
                is_frozen: Rc::new(RefCell::new(false)),
            })
        });
        self.env.borrow_mut().set(
            "Cell".to_string(),
            Value::BuiltinFunction {
                name: "Cell".to_string(),
                func: cell_fn,
            },
        );

        // range(stop) / range(start, stop, [step])
        let range_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            let (start, stop, step) = match args.len() {
                1 => match &args[0] {
                    Value::Int(stop) => (0, *stop, 1),
                    _ => {
                        return Err(RuntimeError {
                            message: "range() stop must be an int".into(),
                            span: Span::default(),
                        })
                    }
                },
                2 => match (&args[0], &args[1]) {
                    (Value::Int(start), Value::Int(stop)) => (*start, *stop, 1),
                    _ => {
                        return Err(RuntimeError {
                            message: "range() arguments must be ints".into(),
                            span: Span::default(),
                        })
                    }
                },
                3 => match (&args[0], &args[1], &args[2]) {
                    (Value::Int(start), Value::Int(stop), Value::Int(step)) => {
                        if *step == 0 {
                            return Err(RuntimeError {
                                message: "range() step cannot be zero".into(),
                                span: Span::default(),
                            });
                        }
                        (*start, *stop, *step)
                    }
                    _ => {
                        return Err(RuntimeError {
                            message: "range() arguments must be ints".into(),
                            span: Span::default(),
                        })
                    }
                },
                n => {
                    return Err(RuntimeError {
                        message: format!("range() takes 1 to 3 arguments, got {n}"),
                        span: Span::default(),
                    })
                }
            };

            Ok(Value::Range { start, stop, step })
        });
        self.env.borrow_mut().set(
            "range".to_string(),
            Value::BuiltinFunction {
                name: "range".to_string(),
                func: range_fn,
            },
        );

        // len(x)
        let len_fn = Rc::new(|args: &[Value], interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError {
                    message: format!("len() takes exactly 1 argument (got {})", args.len()),
                    span: Span::default(),
                });
            }
            match &args[0] {
                Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
                Value::Bytes(bytes) => Ok(Value::Int(bytes.len() as i64)),
                Value::List(l) => Ok(Value::Int(l.borrow().len() as i64)),
                Value::MemoryView { len, .. } => Ok(Value::Int(*len)),
                Value::Dict(d) => Ok(Value::Int(d.borrow().len() as i64)),
                Value::Set(s) => Ok(Value::Int(s.borrow().len() as i64)),
                Value::Record(r) => Ok(Value::Int(r.borrow().len() as i64)),
                Value::DottedPath(parts) => Ok(Value::Int(parts.len() as i64)),
                Value::Range { start, stop, step } => {
                    let count = if *step > 0 {
                        if *stop > *start {
                            (*stop - *start + *step - 1) / *step
                        } else {
                            0
                        }
                    } else {
                        if *start > *stop {
                            (*start - *stop + (-*step) - 1) / (-*step)
                        } else {
                            0
                        }
                    };
                    Ok(Value::Int(count))
                }
                Value::Object { fields, .. } => {
                    let method = fields.borrow().get("__len__").cloned();
                    if let Some(method) = method {
                        return interp.invoke_value(
                            method,
                            vec![(None, args[0].clone())],
                            Span::default(),
                        );
                    }
                    Err(RuntimeError {
                        message: "object has no __len__()".into(),
                        span: Span::default(),
                    })
                }
                other => Err(RuntimeError {
                    message: format!("object of type '{}' has no len()", other.type_name()),
                    span: Span::default(),
                }),
            }
        });
        self.env.borrow_mut().set(
            "len".to_string(),
            Value::BuiltinFunction {
                name: "len".to_string(),
                func: len_fn,
            },
        );

        // any(iterable) / all(iterable)
        for (name, want_any) in [("any", true), ("all", false)] {
            let builtin_name = name.to_string();
            let error_name = builtin_name.clone();
            let func = Rc::new(move |args: &[Value], interp: &mut Interpreter| {
                if args.len() != 1 {
                    return Err(RuntimeError {
                        message: format!("{error_name}() takes exactly one argument"),
                        span: Span::default(),
                    });
                }
                let values = match &args[0] {
                    Value::List(values) => values.borrow().clone(),
                    Value::Set(values) => values.borrow().clone(),
                    Value::Dict(values) => {
                        values.borrow().keys().cloned().map(Value::Str).collect()
                    }
                    Value::Range { start, stop, step } => materialize_range(*start, *stop, *step),
                    value @ (Value::Bytes(_) | Value::MemoryView { .. }) => {
                        binary_sequence_items(value).unwrap_or_default()
                    }
                    Value::Object { fields, .. } => {
                        let method = fields.borrow().get("__iter__").cloned().ok_or_else(|| {
                            RuntimeError {
                                message: format!("{error_name}() argument is not iterable: object"),
                                span: Span::default(),
                            }
                        })?;
                        let iterator = interp.invoke_value(
                            method,
                            vec![(None, args[0].clone())],
                            Span::default(),
                        )?;
                        match iterator {
                            Value::List(values) => values.borrow().clone(),
                            Value::Object {
                                fields: ref iterator_fields,
                                ..
                            } => {
                                let next =
                                    iterator_fields.borrow().get("next").cloned().ok_or_else(
                                        || RuntimeError {
                                            message: "__iter__ returned non-iterator".into(),
                                            span: Span::default(),
                                        },
                                    )?;
                                let mut out = Vec::new();
                                loop {
                                    let item = interp.invoke_value(
                                        next.clone(),
                                        vec![(None, iterator.clone())],
                                        Span::default(),
                                    )?;
                                    if matches!(item, Value::Sentinel(ref name) if name == "iteration.done")
                                    {
                                        break;
                                    }
                                    out.push(item);
                                }
                                out
                            }
                            other => {
                                return Err(RuntimeError {
                                    message: format!("__iter__ returned {}", other.type_name()),
                                    span: Span::default(),
                                })
                            }
                        }
                    }
                    other => {
                        return Err(RuntimeError {
                            message: format!(
                                "{error_name}() argument is not iterable: {}",
                                other.type_name()
                            ),
                            span: Span::default(),
                        })
                    }
                };
                let result = if want_any {
                    values.iter().any(|value| interp.is_truthy(value))
                } else {
                    values.iter().all(|value| interp.is_truthy(value))
                };
                Ok(Value::Bool(result))
            });
            self.env.borrow_mut().set(
                builtin_name.clone(),
                Value::BuiltinFunction {
                    name: builtin_name,
                    func,
                },
            );
        }

        // zip(...) is strict: all input sequences must have equal length.
        // `help` is a stable builtin hook for tooling.  The reference
        // interpreter has no interactive pager, so it deliberately returns
        // none after accepting an optional subject.
        self.env.borrow_mut().set(
            "help".into(),
            Value::BuiltinFunction {
                name: "help".into(),
                func: Rc::new(|args: &[Value], _interp: &mut Interpreter| {
                    if args.len() > 1 {
                        return Err(RuntimeError {
                            message: "help() takes zero or one argument".into(),
                            span: Span::default(),
                        });
                    }
                    Ok(Value::None)
                }),
            },
        );
        self.env.borrow_mut().set(
            "slice".into(),
            Value::BuiltinFunction {
                name: "slice".into(),
                func: Rc::new(|args: &[Value], _interp: &mut Interpreter| {
                    if !(1..=3).contains(&args.len()) {
                        return Err(RuntimeError {
                            message: "slice() takes one to three arguments".into(),
                            span: Span::default(),
                        });
                    }
                    let mut fields = HashMap::new();
                    for (name, index) in [("start", 0usize), ("stop", 1usize), ("step", 2usize)] {
                        let value = args.get(index).cloned().unwrap_or(Value::None);
                        if !matches!(value, Value::Int(_) | Value::None) {
                            return Err(RuntimeError {
                                message: format!("slice {name} must be an integer or none"),
                                span: Span::default(),
                            });
                        }
                        fields.insert(name.to_string(), value);
                    }
                    Ok(Value::Object {
                        class_name: "slice".into(),
                        fields: Rc::new(RefCell::new(fields)),
                        is_frozen: Rc::new(RefCell::new(true)),
                    })
                }),
            },
        );

        // zip(...) is strict: all input sequences must have equal length.
        self.env.borrow_mut().set(
            "zip".into(),
            Value::BuiltinFunction {
                name: "zip".into(),
                func: Rc::new(|args: &[Value], interp: &mut Interpreter| {
                    let sequences: Vec<Vec<Value>> = args
                        .iter()
                        .map(|value| match value {
                            Value::List(items) => Ok(items.borrow().clone()),
                            Value::Set(items) => Ok(items.borrow().clone()),
                            Value::Range { start, stop, step } => Ok(materialize_range(*start, *stop, *step)),
                            Value::Object { fields, .. } => {
                                let method = fields.borrow().get("__iter__").cloned().ok_or_else(|| RuntimeError { message: "object is not iterable".into(), span: Span::default() })?;
                                let iterator = interp.invoke_value(method, vec![(None, value.clone())], Span::default())?;
                                match iterator {
                                    Value::List(items) => Ok(items.borrow().clone()),
                                    Value::Object { fields: ref iterator_fields, .. } => {
                                        let next = iterator_fields.borrow().get("next").cloned().ok_or_else(|| RuntimeError { message: "__iter__ returned non-iterator".into(), span: Span::default() })?;
                                        let mut out = Vec::new();
                                        loop {
                                            let item = interp.invoke_value(next.clone(), vec![(None, iterator.clone())], Span::default())?;
                                            if matches!(item, Value::Sentinel(ref name) if name == "iteration.done") { break; }
                                            out.push(item);
                                        }
                                        Ok(out)
                                    }
                                    other => Err(RuntimeError { message: format!("__iter__ returned {}", other.type_name()), span: Span::default() }),
                                }
                            }
                            other => Err(RuntimeError {
                                message: format!("zip() argument is not iterable: {}", other.type_name()),
                                span: Span::default(),
                            }),
                        })
                        .collect::<Result<_, _>>()?;
                    let length = sequences.first().map_or(0, Vec::len);
                    if sequences.iter().any(|sequence| sequence.len() != length) {
                        return Err(RuntimeError {
                            message: "zip() arguments have different lengths".into(),
                            span: Span::default(),
                        });
                    }
                    let rows = (0..length)
                        .map(|index| {
                            Value::List(Rc::new(RefCell::new(
                                sequences.iter().map(|sequence| sequence[index].clone()).collect(),
                            )))
                        })
                        .collect();
                    Ok(Value::List(Rc::new(RefCell::new(rows))))
                }),
            },
        );

        // enumerate(iterable, start=0) and reversed(sequence)
        self.env.borrow_mut().set(
            "enumerate".into(),
            Value::BuiltinFunction {
                name: "enumerate".into(),
                func: Rc::new(|args: &[Value], interp: &mut Interpreter| {
                    if !(1..=2).contains(&args.len()) {
                        return Err(RuntimeError { message: "enumerate() takes one or two arguments".into(), span: Span::default() });
                    }
                    let mut index = if args.len() == 2 {
                        match args[1] { Value::Int(value) => value, _ => return Err(RuntimeError { message: "enumerate() start must be an int".into(), span: Span::default() }) }
                    } else { 0 };
                    let custom_iter = match &args[0] { Value::Object { fields, .. } => fields.borrow().get("__iter__").cloned(), _ => None };
                    let values = match &args[0] {
                        Value::List(values) => values.borrow().clone(),
                        Value::Set(values) => values.borrow().clone(),
                        Value::Range { start, stop, step } => materialize_range(*start, *stop, *step),
                        value @ (Value::Bytes(_) | Value::MemoryView { .. }) => {
                            binary_sequence_items(value).unwrap_or_default()
                        }
                        Value::Object { .. } if custom_iter.is_some() => {
                            let iterator_function = custom_iter.ok_or_else(|| RuntimeError {
                                message: "iterable is missing __iter__".into(),
                                span: Span::default(),
                            })?;
                            let iterator = interp.invoke_value(iterator_function, vec![(None, args[0].clone())], Span::default())?;
                            match iterator {
                                Value::List(values) => values.borrow().clone(),
                                Value::Object { fields: ref iterator_fields, .. } => {
                                    let next = iterator_fields.borrow().get("next").cloned().ok_or_else(|| RuntimeError { message: "__iter__ returned non-iterator".into(), span: Span::default() })?;
                                    let mut out = Vec::new();
                                    loop {
                                        let item = interp.invoke_value(next.clone(), vec![(None, iterator.clone())], Span::default())?;
                                        if matches!(item, Value::Sentinel(ref name) if name == "iteration.done") { break; }
                                        out.push(item);
                                    }
                                    out
                                }
                                other => return Err(RuntimeError { message: format!("__iter__ returned {}", other.type_name()), span: Span::default() }),
                            }
                        }
                        other => return Err(RuntimeError { message: format!("enumerate() argument is not iterable: {}", other.type_name()), span: Span::default() }),
                    };
                    let pairs = values.into_iter().map(|value| {
                        let pair = Value::List(Rc::new(RefCell::new(vec![Value::Int(index), value])));
                        index += 1; pair
                    }).collect();
                    Ok(Value::List(Rc::new(RefCell::new(pairs))))
                }),
            },
        );
        self.env.borrow_mut().set(
            "reversed".into(),
            Value::BuiltinFunction {
                name: "reversed".into(),
                func: Rc::new(|args: &[Value], interp: &mut Interpreter| {
                    if args.len() != 1 {
                        return Err(RuntimeError {
                            message: "reversed() takes exactly one argument".into(),
                            span: Span::default(),
                        });
                    }
                    let mut values = match &args[0] {
                        Value::List(values) => values.borrow().clone(),
                        Value::Set(values) => values.borrow().clone(),
                        Value::Range { start, stop, step } => {
                            materialize_range(*start, *stop, *step)
                        }
                        value @ (Value::Bytes(_) | Value::MemoryView { .. }) => {
                            binary_sequence_items(value).unwrap_or_default()
                        }
                        Value::Object { fields, .. } => {
                            if let Some(method) = fields.borrow().get("__reversed__").cloned() {
                                return interp.invoke_value(
                                    method,
                                    vec![(None, args[0].clone())],
                                    Span::default(),
                                );
                            }
                            return Err(RuntimeError {
                                message: "object is not reversible".into(),
                                span: Span::default(),
                            });
                        }
                        other => {
                            return Err(RuntimeError {
                                message: format!(
                                    "reversed() argument is not reversible: {}",
                                    other.type_name()
                                ),
                                span: Span::default(),
                            })
                        }
                    };
                    values.reverse();
                    Ok(Value::List(Rc::new(RefCell::new(values))))
                }),
            },
        );

        // Attribute reflection helpers.
        self.env.borrow_mut().set(
            "getattr".into(),
            Value::BuiltinFunction {
                name: "getattr".into(),
                func: Rc::new(|args: &[Value], _interp: &mut Interpreter| {
                    if !(2..=3).contains(&args.len()) {
                        return Err(RuntimeError {
                            message: "getattr() takes two or three arguments".into(),
                            span: Span::default(),
                        });
                    }
                    let name = match &args[1] {
                        Value::Str(name) => name,
                        _ => {
                            return Err(RuntimeError {
                                message: "getattr() attribute name must be a string".into(),
                                span: Span::default(),
                            })
                        }
                    };
                    if let Some(value) = identity_metadata_attr(&args[0], name) {
                        return Ok(value);
                    }
                    let value = match &args[0] {
                        Value::Object { fields, .. } => {
                            let value = fields.borrow().get(name).cloned();
                            if let Some(Value::Function {
                                name: getter_name, ..
                            }) = &value
                            {
                                if getter_name == &format!("__getter__{name}") {
                                    return _interp.invoke_value(
                                        value.expect("getter value was just matched"),
                                        vec![(None, args[0].clone())],
                                        Span::default(),
                                    );
                                }
                            }
                            value
                        }
                        Value::Module { env, .. } => env.borrow().get(name),
                        _ => None,
                    };
                    value
                        .or_else(|| args.get(2).cloned())
                        .ok_or_else(|| RuntimeError {
                            message: format!("attribute '{name}' not found"),
                            span: Span::default(),
                        })
                }),
            },
        );
        self.env.borrow_mut().set(
            "hasattr".into(),
            Value::BuiltinFunction {
                name: "hasattr".into(),
                func: Rc::new(|args: &[Value], _interp: &mut Interpreter| {
                    if args.len() != 2 {
                        return Err(RuntimeError {
                            message: "hasattr() takes exactly two arguments".into(),
                            span: Span::default(),
                        });
                    }
                    let name = match &args[1] {
                        Value::Str(name) => name,
                        _ => return Ok(Value::Bool(false)),
                    };
                    if identity_metadata_attr(&args[0], name).is_some() {
                        return Ok(Value::Bool(true));
                    }
                    let present = match &args[0] {
                        Value::Object { fields, .. } => {
                            fields.borrow().contains_key(name)
                                || fields.borrow().contains_key(&format!("__getter__{name}"))
                        }
                        Value::Module { env, .. } => env.borrow().get(name).is_some(),
                        _ => false,
                    };
                    Ok(Value::Bool(present))
                }),
            },
        );
        self.env.borrow_mut().set(
            "setattr".into(),
            Value::BuiltinFunction {
                name: "setattr".into(),
                func: Rc::new(|args: &[Value], interp: &mut Interpreter| {
                    if args.len() != 3 {
                        return Err(RuntimeError {
                            message: "setattr() takes exactly three arguments".into(),
                            span: Span::default(),
                        });
                    }
                    let name = match &args[1] {
                        Value::Str(name) => name.clone(),
                        _ => {
                            return Err(RuntimeError {
                                message: "setattr() attribute name must be a string".into(),
                                span: Span::default(),
                            })
                        }
                    };
                    match &args[0] {
                        Value::Object {
                            fields, is_frozen, ..
                        } => {
                            if *is_frozen.borrow() {
                                return Err(RuntimeError {
                                    message: "cannot mutate frozen object".into(),
                                    span: Span::default(),
                                });
                            }
                            if !fields.borrow().contains_key(&name) {
                                return Err(RuntimeError {
                                    message: format!("attribute '{name}' not declared"),
                                    span: Span::default(),
                                });
                            }
                            if let Some(setter) =
                                fields.borrow().get(&format!("__setter__{name}")).cloned()
                            {
                                let receiver = args[0].clone();
                                interp.invoke_value(
                                    setter,
                                    vec![(None, receiver), (None, args[2].clone())],
                                    Span::default(),
                                )?;
                                return Ok(Value::None);
                            }
                            fields.borrow_mut().insert(name, args[2].clone());
                            Ok(Value::None)
                        }
                        _ => Err(RuntimeError {
                            message: "setattr() target is not an object".into(),
                            span: Span::default(),
                        }),
                    }
                }),
            },
        );
        self.env.borrow_mut().set(
            "fields".into(),
            Value::BuiltinFunction {
                name: "fields".into(),
                func: Rc::new(|args: &[Value], interp: &mut Interpreter| {
                    if args.len() != 1 {
                        return Err(RuntimeError {
                            message: "fields() takes exactly one argument".into(),
                            span: Span::default(),
                        });
                    }
                    let names = match &args[0] {
                        Value::Object {
                            class_name, fields, ..
                        } => {
                            let mut declared = Vec::new();
                            if interp.classes.contains_key(class_name) {
                                let mut members = HashMap::new();
                                interp.collect_inherited_members(
                                    class_name,
                                    &mut declared,
                                    &mut members,
                                );
                            }
                            if declared.is_empty() {
                                fields.borrow().keys().cloned().collect()
                            } else {
                                declared
                            }
                        }
                        Value::Module { env, .. } => env.borrow().binding_order.clone(),
                        Value::ClassRef(class_name) => interp
                            .classes
                            .get(class_name)
                            .map(|class_def| {
                                class_def
                                    .body
                                    .iter()
                                    .filter_map(|member| match member {
                                        ClassMember::Field(field) => Some(field.name.clone()),
                                        ClassMember::Method(method) => Some(method.name.clone()),
                                        ClassMember::ClassMethod(method) => {
                                            Some(method.name.clone())
                                        }
                                        ClassMember::Factory(factory) => Some(factory.name.clone()),
                                        ClassMember::Getter(getter) => Some(getter.name.clone()),
                                        ClassMember::Setter(setter) => Some(setter.name.clone()),
                                        _ => None,
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        _ => Vec::new(),
                    };
                    Ok(Value::List(Rc::new(RefCell::new(
                        names.into_iter().map(Value::Str).collect(),
                    ))))
                }),
            },
        );

        self.env.borrow_mut().set(
            "repr".into(),
            Value::BuiltinFunction {
                name: "repr".into(),
                func: Rc::new(|args: &[Value], _interp: &mut Interpreter| {
                    if args.len() != 1 {
                        return Err(RuntimeError {
                            message: "repr() takes exactly one argument".into(),
                            span: Span::default(),
                        });
                    }
                    Ok(Value::Str(format!("{:?}", args[0])))
                }),
            },
        );
        self.env.borrow_mut().set(
            "format".into(),
            Value::BuiltinFunction {
                name: "format".into(),
                func: Rc::new(|args: &[Value], _interp: &mut Interpreter| {
                    if !(1..=2).contains(&args.len()) {
                        return Err(RuntimeError {
                            message: "format() takes one or two arguments".into(),
                            span: Span::default(),
                        });
                    }
                    let spec = if args.len() == 2 {
                        match &args[1] {
                            Value::Str(value) => value.as_str(),
                            _ => {
                                return Err(RuntimeError {
                                    message: "format spec must be a string".into(),
                                    span: Span::default(),
                                })
                            }
                        }
                    } else {
                        ""
                    };
                    let result = match (&args[0], spec) {
                        (Value::Int(value), "x") => format!("{value:x}"),
                        (Value::Int(value), "X") => format!("{value:X}"),
                        (Value::Int(value), "b") => format!("{value:b}"),
                        (Value::Int(value), "o") => format!("{value:o}"),
                        (Value::Float(value), "") => value.to_string(),
                        (Value::Float(value), "f") => format!("{value:.6}"),
                        (Value::Str(value), "") => value.clone(),
                        (value, "") => format!("{value:?}"),
                        _ => {
                            return Err(RuntimeError {
                                message: format!("unsupported format specifier '{spec}'"),
                                span: Span::default(),
                            })
                        }
                    };
                    Ok(Value::Str(result))
                }),
            },
        );
        self.env.borrow_mut().set(
            "hash".into(),
            Value::BuiltinFunction {
                name: "hash".into(),
                func: Rc::new(|args: &[Value], interp: &mut Interpreter| {
                    if args.len() != 1 {
                        return Err(RuntimeError {
                            message: "hash() takes exactly one argument".into(),
                            span: Span::default(),
                        });
                    }
                    if let Value::Object { fields, .. } = &args[0] {
                        if let Some(method) = fields.borrow().get("__hash__").cloned() {
                            return interp.invoke_value(
                                method,
                                vec![(None, args[0].clone())],
                                Span::default(),
                            );
                        }
                    }
                    let hash = hash_runtime_value(&args[0]).ok_or_else(|| RuntimeError {
                        message: "unhashable value".into(),
                        span: Span::default(),
                    })?;
                    Ok(Value::Int(hash))
                }),
            },
        );
        self.env.borrow_mut().set(
            "locals".into(),
            Value::BuiltinFunction {
                name: "locals".into(),
                func: Rc::new(|args: &[Value], interp: &mut Interpreter| {
                    if !args.is_empty() {
                        return Err(RuntimeError {
                            message: "locals() takes no arguments".into(),
                            span: Span::default(),
                        });
                    }
                    Ok(Value::Dict(Rc::new(RefCell::new(
                        interp
                            .env
                            .borrow()
                            .bindings
                            .iter()
                            .filter(|(name, value)| {
                                !matches!(
                                    value,
                                    Value::BuiltinFunction { name: builtin, .. }
                                        if interp.builtin_names.contains(*name)
                                            && builtin == *name
                                ) && !matches!(
                                    (name.as_str(), value),
                                    ("pi", Value::Float(value))
                                        if interp.builtin_names.contains(*name)
                                            && value.to_bits()
                                                == std::f64::consts::PI.to_bits()
                                )
                            })
                            .map(|(name, value)| (name.clone(), value.clone()))
                            .collect(),
                    ))))
                }),
            },
        );
        self.env.borrow_mut().set(
            "sorted".into(),
            Value::BuiltinFunction {
                name: "sorted".into(),
                func: Rc::new(|args: &[Value], interp: &mut Interpreter| {
                    if args.len() != 1 {
                        return Err(RuntimeError {
                            message: "sorted() takes exactly one argument".into(),
                            span: Span::default(),
                        });
                    }
                    let mut values = match &args[0] {
                        Value::List(values) => values.borrow().clone(),
                        Value::Set(values) => values.borrow().clone(),
                        value @ (Value::Bytes(_) | Value::MemoryView { .. }) => {
                            binary_sequence_items(value).unwrap_or_default()
                        }
                        other => {
                            return Err(RuntimeError {
                                message: format!(
                                    "sorted() argument is not iterable: {}",
                                    other.type_name()
                                ),
                                span: Span::default(),
                            })
                        }
                    };
                    for i in 1..values.len() {
                        let mut j = i;
                        while j > 0 {
                            let custom = match &values[j - 1] {
                                Value::Object { fields, .. } => {
                                    fields.borrow().get("__lt__").cloned()
                                }
                                _ => None,
                            };
                            let less = if let Some(method) = custom {
                                matches!(
                                    interp.invoke_value(
                                        method,
                                        vec![
                                            (None, values[j - 1].clone()),
                                            (None, values[j].clone())
                                        ],
                                        Span::default()
                                    ),
                                    Ok(Value::Bool(true))
                                )
                            } else {
                                compare_values(&values[j], &values[j - 1])
                                    .map(|ordering| ordering == std::cmp::Ordering::Less)
                                    .unwrap_or(false)
                            };
                            if !less {
                                break;
                            }
                            values.swap(j - 1, j);
                            j -= 1;
                        }
                    }
                    Ok(Value::List(Rc::new(RefCell::new(values))))
                }),
            },
        );
        self.env.borrow_mut().set(
            "pow".into(),
            Value::BuiltinFunction {
                name: "pow".into(),
                func: Rc::new(|args: &[Value], _interp: &mut Interpreter| {
                    if args.len() == 3 {
                        match (&args[0], &args[1], &args[2]) {
                            (Value::Int(base), Value::Int(exp), Value::Int(modulus)) if *modulus != 0 => {
                                if *exp < 0 { return Ok(Value::Int(INT_NAN)); }
                                let mut result: i128 = 1;
                                let mut factor = (*base as i128).rem_euclid(*modulus as i128);
                                let mut power = *exp as u64;
                                let modulus = *modulus as i128;
                                while power != 0 {
                                    if power & 1 == 1 { result = (result * factor).rem_euclid(modulus); }
                                    factor = (factor * factor).rem_euclid(modulus);
                                    power >>= 1;
                                }
                                Ok(Value::Int(result as i64))
                            }
                            (base, exp, modulus)
                                if matches!(base, Value::Int(_) | Value::BigInt(_))
                                    && matches!(exp, Value::Int(_) | Value::BigInt(_))
                                    && matches!(modulus, Value::Int(_) | Value::BigInt(_)) => {
                                let base = match base { Value::Int(value) => BigInt::from(*value), Value::BigInt(value) => value.clone(), _ => unreachable!() };
                                let exp = match exp { Value::Int(value) => BigInt::from(*value), Value::BigInt(value) => value.clone(), _ => unreachable!() };
                                let modulus = match modulus { Value::Int(value) => BigInt::from(*value), Value::BigInt(value) => value.clone(), _ => unreachable!() };
                                if modulus.is_zero() { return Err(RuntimeError { message: "three-argument pow() requires int arguments and a nonzero modulus".into(), span: Span::default() }); }
                                if exp.sign() == num_bigint::Sign::Minus { return Ok(Value::Int(INT_NAN)); }
                                let modulus = modulus.abs();
                                let result = base.modpow(&exp, &modulus);
                                Ok(Value::BigInt(result))
                            }
                            _ => Err(RuntimeError { message: "three-argument pow() requires int arguments and a nonzero modulus".into(), span: Span::default() }),
                        }
                    } else if args.len() != 2 {
                        Err(RuntimeError { message: "pow() takes two or three arguments".into(), span: Span::default() })
                    } else {
                        match (&args[0], &args[1]) {
                            (Value::Int(base), Value::Int(exp)) if *exp >= 0 => Ok(Value::Int(base.pow(*exp as u32))),
                            (Value::Int(base), Value::Int(exp)) => Ok(Value::Float((*base as f64).powi(*exp as i32))),
                            (Value::Float(base), Value::Int(exp)) => Ok(Value::Float(base.powi(*exp as i32))),
                            (Value::Float(base), Value::Float(exp)) => Ok(Value::Float(base.powf(*exp))),
                            _ => Err(RuntimeError { message: "pow() arguments must be numeric".into(), span: Span::default() }),
                        }
                    }
                }),
            },
        );
        self.env.borrow_mut().set(
            "iter".into(),
            Value::BuiltinFunction {
                name: "iter".into(),
                func: Rc::new(|args: &[Value], interp: &mut Interpreter| {
                    if args.len() != 1 {
                        return Err(RuntimeError {
                            message: "iter() takes exactly one argument".into(),
                            span: Span::default(),
                        });
                    }
                    match &args[0] {
                        Value::List(_) | Value::Range { .. } | Value::Set(_) => {
                            Ok(match &args[0] {
                                Value::List(_) => args[0].clone(),
                                Value::Range { start, stop, step } => Value::List(Rc::new(
                                    RefCell::new(materialize_range(*start, *stop, *step)),
                                )),
                                Value::Set(values) => {
                                    Value::List(Rc::new(RefCell::new(values.borrow().clone())))
                                }
                                _ => {
                                    return Err(RuntimeError {
                                        message: "value is not iterable".into(),
                                        span: Span::default(),
                                    })
                                }
                            })
                        }
                        Value::Object { fields, .. } => {
                            if let Some(method) = fields.borrow().get("__iter__").cloned() {
                                return interp.invoke_value(
                                    method,
                                    vec![(None, args[0].clone())],
                                    Span::default(),
                                );
                            }
                            Err(RuntimeError {
                                message: "object is not iterable".into(),
                                span: Span::default(),
                            })
                        }
                        _ => Err(RuntimeError {
                            message: "object is not iterable".into(),
                            span: Span::default(),
                        }),
                    }
                }),
            },
        );
        self.env.borrow_mut().set(
            "map".into(),
            Value::BuiltinFunction {
                name: "map".into(),
                func: Rc::new(|args: &[Value], interp: &mut Interpreter| {
                    if args.len() != 2 {
                        return Err(RuntimeError { message: "map() takes exactly two arguments".into(), span: Span::default() });
                    }
                    let values = match &args[1] {
                        Value::List(values) => values.borrow().clone(),
                        Value::Range { start, stop, step } => materialize_range(*start, *stop, *step),
                        value @ (Value::Bytes(_) | Value::MemoryView { .. }) => {
                            binary_sequence_items(value).unwrap_or_default()
                        }
                        Value::Object { fields, .. } => {
                            let method = fields.borrow().get("__iter__").cloned().ok_or_else(|| RuntimeError { message: "object is not iterable".into(), span: Span::default() })?;
                            let iterator = interp.invoke_value(method, vec![(None, args[1].clone())], Span::default())?;
                            match iterator {
                                Value::List(values) => values.borrow().clone(),
                                Value::Object { fields: ref iterator_fields, .. } => {
                                    let next = iterator_fields.borrow().get("next").cloned().ok_or_else(|| RuntimeError { message: "__iter__ returned non-iterator".into(), span: Span::default() })?;
                                    let mut out = Vec::new();
                                    loop {
                                        let item = interp.invoke_value(next.clone(), vec![(None, iterator.clone())], Span::default())?;
                                        if matches!(item, Value::Sentinel(ref name) if name == "iteration.done") { break; }
                                        out.push(item);
                                    }
                                    out
                                }
                                other => return Err(RuntimeError { message: format!("__iter__ returned {}", other.type_name()), span: Span::default() }),
                            }
                        }
                        other => return Err(RuntimeError { message: format!("map() argument is not iterable: {}", other.type_name()), span: Span::default() }),
                    };
                    let mapped = values.into_iter().map(|value| {
                        interp.invoke_value(args[0].clone(), vec![(None, value)], Span::default())
                    }).collect::<Result<Vec<_>, _>>()?;
                    Ok(Value::List(Rc::new(RefCell::new(mapped))))
                }),
            },
        );

        // min(x) / min(a, b, ...)
        let min_fn = Rc::new(|args: &[Value], interp: &mut Interpreter| {
            if args.is_empty() {
                return Err(RuntimeError {
                    message: "min() expects at least 1 argument".into(),
                    span: Span::default(),
                });
            }
            let items: Vec<Value> = if args.len() == 1 {
                match &args[0] {
                    Value::List(l) => l.borrow().clone(),
                    Value::Set(s) => s.borrow().clone(),
                    Value::Range { start, stop, step } => materialize_range(*start, *stop, *step),
                    value @ (Value::Bytes(_) | Value::MemoryView { .. }) => {
                        binary_sequence_items(value).unwrap_or_default()
                    }
                    Value::Object { .. } => {
                        interp.materialize_iterable(args[0].clone(), Span::default())?
                    }
                    other => {
                        return Err(RuntimeError {
                            message: format!(
                                "min() arg must be iterable, got {}",
                                other.type_name()
                            ),
                            span: Span::default(),
                        })
                    }
                }
            } else {
                args.to_vec()
            };
            if items.is_empty() {
                return Err(RuntimeError {
                    message: "min() arg is an empty sequence".into(),
                    span: Span::default(),
                });
            }
            let mut current_min = items[0].clone();
            for item in &items[1..] {
                let less = if let Value::Object { fields, .. } = item {
                    if let Some(method) = fields.borrow().get("__lt__").cloned() {
                        matches!(
                            interp.invoke_value(
                                method,
                                vec![(None, item.clone()), (None, current_min.clone())],
                                Span::default()
                            )?,
                            Value::Bool(true)
                        )
                    } else {
                        compare_values(item, &current_min)? == std::cmp::Ordering::Less
                    }
                } else {
                    compare_values(item, &current_min)? == std::cmp::Ordering::Less
                };
                if less {
                    current_min = item.clone();
                }
            }
            Ok(current_min)
        });
        self.env.borrow_mut().set(
            "min".to_string(),
            Value::BuiltinFunction {
                name: "min".to_string(),
                func: min_fn,
            },
        );

        // max(x) / max(a, b, ...)
        let max_fn = Rc::new(|args: &[Value], interp: &mut Interpreter| {
            if args.is_empty() {
                return Err(RuntimeError {
                    message: "max() expects at least 1 argument".into(),
                    span: Span::default(),
                });
            }
            let items: Vec<Value> = if args.len() == 1 {
                match &args[0] {
                    Value::List(l) => l.borrow().clone(),
                    Value::Set(s) => s.borrow().clone(),
                    Value::Range { start, stop, step } => materialize_range(*start, *stop, *step),
                    value @ (Value::Bytes(_) | Value::MemoryView { .. }) => {
                        binary_sequence_items(value).unwrap_or_default()
                    }
                    Value::Object { .. } => {
                        interp.materialize_iterable(args[0].clone(), Span::default())?
                    }
                    other => {
                        return Err(RuntimeError {
                            message: format!(
                                "max() arg must be iterable, got {}",
                                other.type_name()
                            ),
                            span: Span::default(),
                        })
                    }
                }
            } else {
                args.to_vec()
            };
            if items.is_empty() {
                return Err(RuntimeError {
                    message: "max() arg is an empty sequence".into(),
                    span: Span::default(),
                });
            }
            let mut current_max = items[0].clone();
            for item in &items[1..] {
                let greater = if let Value::Object { fields, .. } = &current_max {
                    if let Some(method) = fields.borrow().get("__lt__").cloned() {
                        matches!(
                            interp.invoke_value(
                                method,
                                vec![(None, current_max.clone()), (None, item.clone())],
                                Span::default()
                            )?,
                            Value::Bool(false)
                        )
                    } else {
                        compare_values(item, &current_max)? == std::cmp::Ordering::Greater
                    }
                } else {
                    compare_values(item, &current_max)? == std::cmp::Ordering::Greater
                };
                if greater {
                    current_max = item.clone();
                }
            }
            Ok(current_max)
        });
        self.env.borrow_mut().set(
            "max".to_string(),
            Value::BuiltinFunction {
                name: "max".to_string(),
                func: max_fn,
            },
        );

        // sum(iterable, [start])
        let sum_fn = Rc::new(|args: &[Value], interp: &mut Interpreter| {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError {
                    message: "sum() takes 1 or 2 arguments".into(),
                    span: Span::default(),
                });
            }
            let items: Vec<Value> = match &args[0] {
                Value::List(l) => l.borrow().clone(),
                Value::Set(s) => s.borrow().clone(),
                Value::Range { start, stop, step } => materialize_range(*start, *stop, *step),
                value @ (Value::Bytes(_) | Value::MemoryView { .. }) => {
                    binary_sequence_items(value).unwrap_or_default()
                }
                Value::Object { fields, .. } => {
                    let method =
                        fields
                            .borrow()
                            .get("__iter__")
                            .cloned()
                            .ok_or_else(|| RuntimeError {
                                message: "sum() iterable is not iterable".into(),
                                span: Span::default(),
                            })?;
                    match interp.invoke_value(
                        method,
                        vec![(None, args[0].clone())],
                        Span::default(),
                    )? {
                        Value::List(items) => items.borrow().clone(),
                        other => {
                            return Err(RuntimeError {
                                message: format!("__iter__ returned {}", other.type_name()),
                                span: Span::default(),
                            })
                        }
                    }
                }
                other => {
                    return Err(RuntimeError {
                        message: format!("sum() argument is not iterable: {}", other.type_name()),
                        span: Span::default(),
                    })
                }
            };
            let start = if args.len() == 2 {
                args[1].clone()
            } else {
                Value::Int(0)
            };
            let mut total = start;
            for item in items {
                total = match (&total, &item) {
                    (Value::BigInt(a), Value::BigInt(b)) => Value::BigInt(a + b),
                    (Value::BigInt(a), Value::Int(b)) => Value::BigInt(a + BigInt::from(*b)),
                    (Value::Int(a), Value::BigInt(b)) => Value::BigInt(BigInt::from(*a) + b),
                    (Value::Int(a), Value::Int(b)) => Value::Int(a + b),
                    (Value::BigInt(a), Value::Float(b)) => Value::Float(bigint_to_float(a) + b),
                    (Value::Float(a), Value::BigInt(b)) => Value::Float(a + bigint_to_float(b)),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a + b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 + b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a + *b as f64),
                    _ => {
                        return Err(RuntimeError {
                            message: "sum() items must be numbers".into(),
                            span: Span::default(),
                        })
                    }
                };
            }
            Ok(total)
        });
        self.env.borrow_mut().set(
            "sum".to_string(),
            Value::BuiltinFunction {
                name: "sum".to_string(),
                func: sum_fn,
            },
        );

        // read_file(path)
        let read_file_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError {
                    message: "read_file() takes exactly 1 argument (path)".into(),
                    span: Span::default(),
                });
            }
            let path = match &args[0] {
                Value::Str(p) => p,
                _ => {
                    return Err(RuntimeError {
                        message: "read_file() path must be a string".into(),
                        span: Span::default(),
                    })
                }
            };
            match std::fs::read_to_string(path) {
                Ok(contents) => Ok(Value::Str(contents)),
                Err(e) => Err(RuntimeError {
                    message: format!("read_file('{path}') failed: {e}"),
                    span: Span::default(),
                }),
            }
        });
        self.env.borrow_mut().set(
            "read_file".to_string(),
            Value::BuiltinFunction {
                name: "read_file".to_string(),
                func: read_file_fn,
            },
        );

        // write_file(path, content)
        let write_file_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 2 {
                return Err(RuntimeError {
                    message: "write_file() takes exactly 2 arguments (path, content)".into(),
                    span: Span::default(),
                });
            }
            let path = match &args[0] {
                Value::Str(p) => p,
                _ => {
                    return Err(RuntimeError {
                        message: "write_file() path must be a string".into(),
                        span: Span::default(),
                    })
                }
            };
            let content = match &args[1] {
                Value::Str(c) => c,
                _ => {
                    return Err(RuntimeError {
                        message: "write_file() content must be a string".into(),
                        span: Span::default(),
                    })
                }
            };
            match std::fs::write(path, content) {
                Ok(()) => Ok(Value::None),
                Err(e) => Err(RuntimeError {
                    message: format!("write_file('{path}') failed: {e}"),
                    span: Span::default(),
                }),
            }
        });
        self.env.borrow_mut().set(
            "write_file".to_string(),
            Value::BuiltinFunction {
                name: "write_file".to_string(),
                func: write_file_fn,
            },
        );

        // env_var(name, [default])
        let env_var_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError {
                    message: "env_var() takes 1 or 2 arguments (name, [default])".into(),
                    span: Span::default(),
                });
            }
            let name = match &args[0] {
                Value::Str(n) => n,
                _ => {
                    return Err(RuntimeError {
                        message: "env_var() name must be a string".into(),
                        span: Span::default(),
                    })
                }
            };
            match std::env::var(name) {
                Ok(v) => Ok(Value::Str(v)),
                Err(_) => {
                    if args.len() == 2 {
                        Ok(args[1].clone())
                    } else {
                        Ok(Value::None)
                    }
                }
            }
        });
        self.env.borrow_mut().set(
            "env_var".to_string(),
            Value::BuiltinFunction {
                name: "env_var".to_string(),
                func: env_var_fn,
            },
        );

        // str(x)
        let str_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError {
                    message: "str() takes exactly 1 argument".into(),
                    span: Span::default(),
                });
            }
            match &args[0] {
                Value::Str(s) => Ok(Value::Str(s.clone())),
                Value::Bytes(bytes) => Ok(Value::Str(String::from_utf8_lossy(bytes).into_owned())),
                Value::Int(n) => Ok(Value::Str(n.to_string())),
                Value::Float(f) => Ok(Value::Str(f.to_string())),
                Value::Bool(b) => Ok(Value::Str(b.to_string())),
                Value::None => Ok(Value::Str("none".to_string())),
                Value::DottedPath(parts) => Ok(Value::Str(dotted_path_string(parts))),
                other => Ok(Value::Str(format!("{other:?}"))),
            }
        });
        self.env.borrow_mut().set(
            "str".to_string(),
            Value::BuiltinFunction {
                name: "str".to_string(),
                func: str_fn,
            },
        );

        // Immutable byte conversion. Mutable bytearray/memoryview values
        // still use integer-list storage, but Bytes has its own runtime tag.
        let bytes_fn =
            Rc::new(|args: &[Value], interp: &mut Interpreter| {
                if args.len() != 1 {
                    return Err(RuntimeError {
                        message: "bytes() takes exactly one argument".into(),
                        span: Span::default(),
                    });
                }
                match &args[0] {
                    Value::Bytes(bytes) => Ok(Value::Bytes(bytes.clone())),
                    Value::Str(s) => Ok(Value::Bytes(s.bytes().collect())),
                    Value::List(items) => {
                        let mut out = Vec::with_capacity(items.borrow().len());
                        for item in items.borrow().iter() {
                            out.push(byte_from_value(item, "bytes() list")?);
                        }
                        Ok(Value::Bytes(out))
                    }
                    Value::MemoryView {
                        data,
                        start,
                        len,
                        stride,
                        ..
                    } => {
                        let items = memoryview_items(data, *start, *len, *stride);
                        let mut out = Vec::with_capacity(items.len());
                        for item in &items {
                            out.push(byte_from_value(item, "bytes() memoryview")?);
                        }
                        Ok(Value::Bytes(out))
                    }
                    Value::Object { fields, .. } => {
                        let method =
                            fields.borrow().get("__buffer__").cloned().ok_or_else(|| {
                                RuntimeError {
                                    message: "bytes() cannot convert object".into(),
                                    span: Span::default(),
                                }
                            })?;
                        match interp.invoke_value(
                            method,
                            vec![(None, args[0].clone())],
                            Span::default(),
                        )? {
                            Value::Bytes(bytes) => Ok(Value::Bytes(bytes)),
                            Value::List(items) => {
                                let mut out = Vec::with_capacity(items.borrow().len());
                                for item in items.borrow().iter() {
                                    out.push(byte_from_value(item, "bytes() buffer")?);
                                }
                                Ok(Value::Bytes(out))
                            }
                            Value::MemoryView {
                                data,
                                start,
                                len,
                                stride,
                                ..
                            } => {
                                let items = memoryview_items(&data, start, len, stride);
                                let mut out = Vec::with_capacity(items.len());
                                for item in &items {
                                    out.push(byte_from_value(item, "bytes() buffer")?);
                                }
                                Ok(Value::Bytes(out))
                            }
                            other => Err(RuntimeError {
                                message: format!("__buffer__ returned {}", other.type_name()),
                                span: Span::default(),
                            }),
                        }
                    }
                    other => Err(RuntimeError {
                        message: format!("bytes() cannot convert {}", other.type_name()),
                        span: Span::default(),
                    }),
                }
            });
        self.env.borrow_mut().set(
            "bytes".into(),
            Value::BuiltinFunction {
                name: "bytes".into(),
                func: bytes_fn,
            },
        );

        // Binary conversion builtins. The prototype stores mutable buffers
        // as integer lists and memory views as aliases to that list.
        let bytearray_fn = Rc::new(|args: &[Value], interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError {
                    message: "bytearray() takes exactly 1 argument".into(),
                    span: Span::default(),
                });
            }
            let bytes = match &args[0] {
                Value::Bytes(bytes) => bytes.iter().map(|b| Value::Int(*b as i64)).collect(),
                Value::Str(s) => s.bytes().map(|b| Value::Int(b as i64)).collect(),
                Value::List(items) => {
                    let mut bytes = Vec::with_capacity(items.borrow().len());
                    for item in items.borrow().iter() {
                        bytes.push(Value::Int(byte_from_value(item, "bytearray()")? as i64));
                    }
                    bytes
                }
                Value::MemoryView {
                    data,
                    start,
                    len,
                    stride,
                    ..
                } => memoryview_items(data, *start, *len, *stride)
                    .iter()
                    .map(|item| {
                        byte_from_value(item, "bytearray()").map(|byte| Value::Int(byte as i64))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                Value::Object { fields, .. } => {
                    let method =
                        fields
                            .borrow()
                            .get("__buffer__")
                            .cloned()
                            .ok_or_else(|| RuntimeError {
                                message: "bytearray() cannot convert object".into(),
                                span: Span::default(),
                            })?;
                    match interp.invoke_value(
                        method,
                        vec![(None, args[0].clone())],
                        Span::default(),
                    )? {
                        Value::Bytes(bytes) => {
                            bytes.iter().map(|b| Value::Int(*b as i64)).collect()
                        }
                        Value::List(items) => {
                            let mut bytes = Vec::with_capacity(items.borrow().len());
                            for item in items.borrow().iter() {
                                bytes.push(Value::Int(
                                    byte_from_value(item, "bytearray() buffer")? as i64,
                                ));
                            }
                            bytes
                        }
                        Value::MemoryView {
                            data,
                            start,
                            len,
                            stride,
                            ..
                        } => memoryview_items(&data, start, len, stride)
                            .iter()
                            .map(|item| {
                                byte_from_value(item, "bytearray() buffer")
                                    .map(|byte| Value::Int(byte as i64))
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                        other => {
                            return Err(RuntimeError {
                                message: format!("__buffer__ returned {}", other.type_name()),
                                span: Span::default(),
                            })
                        }
                    }
                }
                other => {
                    return Err(RuntimeError {
                        message: format!("bytearray() cannot convert {}", other.type_name()),
                        span: Span::default(),
                    })
                }
            };
            Ok(Value::List(Rc::new(RefCell::new(bytes))))
        });
        self.env.borrow_mut().set(
            "bytearray".into(),
            Value::BuiltinFunction {
                name: "bytearray".into(),
                func: bytearray_fn,
            },
        );
        let memoryview_fn =
            Rc::new(|args: &[Value], interp: &mut Interpreter| {
                if args.len() != 1 {
                    return Err(RuntimeError {
                        message: "memoryview() takes exactly 1 argument".into(),
                        span: Span::default(),
                    });
                }
                match &args[0] {
                    Value::MemoryView { .. } => Ok(args[0].clone()),
                    Value::List(items) => Ok(Value::MemoryView {
                        data: Rc::clone(items),
                        start: 0,
                        len: items.borrow().len() as i64,
                        stride: 1,
                        read_only: false,
                    }),
                    Value::Bytes(bytes) => Ok(Value::MemoryView {
                        data: Rc::new(RefCell::new(
                            bytes.iter().map(|byte| Value::Int(*byte as i64)).collect(),
                        )),
                        start: 0,
                        len: bytes.len() as i64,
                        stride: 1,
                        read_only: true,
                    }),
                    Value::Object { fields, .. } => {
                        let method =
                            fields.borrow().get("__buffer__").cloned().ok_or_else(|| {
                                RuntimeError {
                                    message: "memoryview() cannot convert object".into(),
                                    span: Span::default(),
                                }
                            })?;
                        match interp.invoke_value(
                            method,
                            vec![(None, args[0].clone())],
                            Span::default(),
                        )? {
                            buffer @ Value::MemoryView { .. } => Ok(buffer),
                            Value::List(items) => Ok(Value::MemoryView {
                                data: Rc::clone(&items),
                                start: 0,
                                len: items.borrow().len() as i64,
                                stride: 1,
                                read_only: false,
                            }),
                            Value::Bytes(bytes) => Ok(Value::MemoryView {
                                data: Rc::new(RefCell::new(
                                    bytes.iter().map(|byte| Value::Int(*byte as i64)).collect(),
                                )),
                                start: 0,
                                len: bytes.len() as i64,
                                stride: 1,
                                read_only: true,
                            }),
                            other => Err(RuntimeError {
                                message: format!("__buffer__ returned {}", other.type_name()),
                                span: Span::default(),
                            }),
                        }
                    }
                    other => Err(RuntimeError {
                        message: format!("memoryview() cannot convert {}", other.type_name()),
                        span: Span::default(),
                    }),
                }
            });
        self.env.borrow_mut().set(
            "memoryview".into(),
            Value::BuiltinFunction {
                name: "memoryview".into(),
                func: memoryview_fn,
            },
        );

        // int(x)
        let int_fn =
            Rc::new(|args: &[Value], _interp: &mut Interpreter| {
                if args.len() != 1 {
                    return Err(RuntimeError {
                        message: "int() takes exactly 1 argument".into(),
                        span: Span::default(),
                    });
                }
                match &args[0] {
                    Value::Int(n) => Ok(Value::Int(*n)),
                    Value::BigInt(n) => Ok(Value::BigInt(n.clone())),
                    Value::Float(f) => Ok(Value::Int(*f as i64)),
                    Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
                    Value::Str(s) => match s.trim().parse::<i64>() {
                        Ok(value) => Ok(Value::Int(value)),
                        Err(_) => s.trim().parse::<BigInt>().map(Value::BigInt).map_err(|e| {
                            RuntimeError {
                                message: format!("invalid literal for int(): '{s}' ({e})"),
                                span: Span::default(),
                            }
                        }),
                    },
                    other => Err(RuntimeError {
                        message: format!("int() cannot convert {}", other.type_name()),
                        span: Span::default(),
                    }),
                }
            });
        self.env.borrow_mut().set(
            "int".to_string(),
            Value::BuiltinFunction {
                name: "int".to_string(),
                func: int_fn,
            },
        );

        // float(x)
        let float_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError {
                    message: "float() takes exactly 1 argument".into(),
                    span: Span::default(),
                });
            }
            match &args[0] {
                Value::Float(f) => Ok(Value::Float(*f)),
                Value::Int(n) => Ok(Value::Float(*n as f64)),
                Value::BigInt(n) => Ok(Value::Float(n.to_f64().unwrap_or_else(|| {
                    if n.sign() == num_bigint::Sign::Minus {
                        f64::NEG_INFINITY
                    } else {
                        f64::INFINITY
                    }
                }))),
                Value::Bool(b) => Ok(Value::Float(if *b { 1.0 } else { 0.0 })),
                Value::Str(s) => {
                    s.trim()
                        .parse::<f64>()
                        .map(Value::Float)
                        .map_err(|e| RuntimeError {
                            message: format!("invalid literal for float(): '{s}' ({e})"),
                            span: Span::default(),
                        })
                }
                other => Err(RuntimeError {
                    message: format!("float() cannot convert {}", other.type_name()),
                    span: Span::default(),
                }),
            }
        });
        self.env.borrow_mut().set(
            "float".to_string(),
            Value::BuiltinFunction {
                name: "float".to_string(),
                func: float_fn,
            },
        );

        // complex(real=0, imag=0), represented as a pair of f64 values.
        let complex_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() > 2 {
                return Err(RuntimeError {
                    message: "complex() takes at most two arguments".into(),
                    span: Span::default(),
                });
            }
            let number = |value: &Value| match value {
                Value::Int(n) => Ok(*n as f64),
                Value::Float(n) => Ok(*n),
                _ => Err(RuntimeError {
                    message: "complex() arguments must be numeric".into(),
                    span: Span::default(),
                }),
            };
            let real = args.first().map(number).transpose()?.unwrap_or(0.0);
            let imag = args.get(1).map(number).transpose()?.unwrap_or(0.0);
            Ok(Value::Complex(real, imag))
        });
        self.env.borrow_mut().set(
            "complex".into(),
            Value::BuiltinFunction {
                name: "complex".into(),
                func: complex_fn,
            },
        );

        // bool(x)
        let bool_fn = Rc::new(|args: &[Value], interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError {
                    message: "bool() takes exactly 1 argument".into(),
                    span: Span::default(),
                });
            }
            Ok(Value::Bool(interp.is_truthy(&args[0])))
        });
        self.env.borrow_mut().set(
            "bool".to_string(),
            Value::BuiltinFunction {
                name: "bool".to_string(),
                func: bool_fn,
            },
        );

        // list(x)
        let list_fn = Rc::new(|args: &[Value], interp: &mut Interpreter| {
            if args.is_empty() {
                return Ok(Value::List(Rc::new(RefCell::new(Vec::new()))));
            }
            match &args[0] {
                Value::List(l) => Ok(Value::List(Rc::new(RefCell::new(l.borrow().clone())))),
                Value::MemoryView {
                    data,
                    start,
                    len,
                    stride,
                    ..
                } => {
                    let items = data.borrow();
                    Ok(Value::List(Rc::new(RefCell::new(
                        (0..*len)
                            .map(|offset| items[(start + offset * stride) as usize].clone())
                            .collect(),
                    ))))
                }
                Value::Set(s) => Ok(Value::List(Rc::new(RefCell::new(s.borrow().clone())))),
                Value::Range { start, stop, step } => {
                    let items = materialize_range(*start, *stop, *step);
                    Ok(Value::List(Rc::new(RefCell::new(items))))
                }
                Value::Str(s) => Ok(Value::List(Rc::new(RefCell::new(
                    s.chars().map(|c| Value::Str(c.to_string())).collect(),
                )))),
                Value::Bytes(bytes) => Ok(Value::List(Rc::new(RefCell::new(
                    bytes.iter().map(|b| Value::Int(*b as i64)).collect(),
                )))),
                Value::Object { fields, .. } => {
                    let method =
                        fields
                            .borrow()
                            .get("__iter__")
                            .cloned()
                            .ok_or_else(|| RuntimeError {
                                message: "object is not iterable".into(),
                                span: Span::default(),
                            })?;
                    let iterator = interp.invoke_value(
                        method,
                        vec![(None, args[0].clone())],
                        Span::default(),
                    )?;
                    match iterator {
                        Value::List(values) => {
                            Ok(Value::List(Rc::new(RefCell::new(values.borrow().clone()))))
                        }
                        Value::Object {
                            fields: ref iterator_fields,
                            ..
                        } => {
                            let next =
                                iterator_fields
                                    .borrow()
                                    .get("next")
                                    .cloned()
                                    .ok_or_else(|| RuntimeError {
                                        message: "__iter__ returned non-iterator".into(),
                                        span: Span::default(),
                                    })?;
                            let mut out = Vec::new();
                            loop {
                                let item = interp.invoke_value(
                                    next.clone(),
                                    vec![(None, iterator.clone())],
                                    Span::default(),
                                )?;
                                if matches!(item, Value::Sentinel(ref name) if name == "iteration.done")
                                {
                                    break;
                                }
                                out.push(item);
                            }
                            Ok(Value::List(Rc::new(RefCell::new(out))))
                        }
                        other => Err(RuntimeError {
                            message: format!("__iter__ returned {}", other.type_name()),
                            span: Span::default(),
                        }),
                    }
                }
                other => Err(RuntimeError {
                    message: format!("cannot convert {} to list", other.type_name()),
                    span: Span::default(),
                }),
            }
        });
        self.env.borrow_mut().set(
            "list".to_string(),
            Value::BuiltinFunction {
                name: "list".to_string(),
                func: list_fn,
            },
        );

        // set(x): construct a set from an iterable, or an empty set.
        let set_fn = Rc::new(|args: &[Value], interp: &mut Interpreter| {
            if args.len() > 1 {
                return Err(RuntimeError {
                    message: "set() takes zero or one argument".into(),
                    span: Span::default(),
                });
            }
            let values = match args.first() {
                None => Vec::new(),
                Some(Value::Set(items)) | Some(Value::List(items)) => items.borrow().clone(),
                Some(Value::Range { start, stop, step }) => {
                    if *step == 0 {
                        return Err(RuntimeError {
                            message: "range() arg 3 must not be zero".into(),
                            span: Span::default(),
                        });
                    }
                    materialize_range(*start, *stop, *step)
                }
                Some(Value::Str(s)) => s.chars().map(|c| Value::Str(c.to_string())).collect(),
                Some(value @ (Value::Bytes(_) | Value::MemoryView { .. })) => {
                    binary_sequence_items(value).unwrap_or_default()
                }
                Some(Value::Object { fields, .. }) => {
                    let method =
                        fields
                            .borrow()
                            .get("__iter__")
                            .cloned()
                            .ok_or_else(|| RuntimeError {
                                message: "set() argument is not iterable".into(),
                                span: Span::default(),
                            })?;
                    let iterator = interp.invoke_value(
                        method,
                        vec![(None, args[0].clone())],
                        Span::default(),
                    )?;
                    match iterator {
                        Value::List(items) => items.borrow().clone(),
                        Value::Object {
                            fields: ref iterator_fields,
                            ..
                        } => {
                            let next =
                                iterator_fields
                                    .borrow()
                                    .get("next")
                                    .cloned()
                                    .ok_or_else(|| RuntimeError {
                                        message: "__iter__ returned non-iterator".into(),
                                        span: Span::default(),
                                    })?;
                            let mut out = Vec::new();
                            loop {
                                let item = interp.invoke_value(
                                    next.clone(),
                                    vec![(None, iterator.clone())],
                                    Span::default(),
                                )?;
                                if matches!(item, Value::Sentinel(ref name) if name == "iteration.done")
                                {
                                    break;
                                }
                                out.push(item);
                            }
                            out
                        }
                        other => {
                            return Err(RuntimeError {
                                message: format!("__iter__ returned {}", other.type_name()),
                                span: Span::default(),
                            })
                        }
                    }
                }
                Some(other) => {
                    return Err(RuntimeError {
                        message: format!("set() argument is not iterable: {}", other.type_name()),
                        span: Span::default(),
                    })
                }
            };
            let mut unique = Vec::new();
            for value in values {
                if !unique.iter().any(|existing| existing == &value) {
                    unique.push(value);
                }
            }
            Ok(Value::Set(Rc::new(RefCell::new(unique))))
        });
        self.env.borrow_mut().set(
            "set".into(),
            Value::BuiltinFunction {
                name: "set".into(),
                func: set_fn,
            },
        );

        // dict(x): copy a mapping or consume a sequence of two-element pairs.
        let dict_fn = Rc::new(|args: &[Value], interp: &mut Interpreter| {
            if args.len() > 1 {
                return Err(RuntimeError {
                    message: "dict() takes zero or one argument".into(),
                    span: Span::default(),
                });
            }
            let mut out = HashMap::new();
            match args.first() {
                None => {}
                Some(Value::Dict(values)) => out.extend(values.borrow().clone()),
                Some(Value::List(pairs)) => {
                    for pair in pairs.borrow().iter() {
                        let items = match pair {
                            Value::List(items) if items.borrow().len() == 2 => {
                                items.borrow().clone()
                            }
                            _ => {
                                return Err(RuntimeError {
                                    message: "dict() sequence elements must each have length 2"
                                        .into(),
                                    span: Span::default(),
                                })
                            }
                        };
                        let key = match &items[0] {
                            Value::Str(key) => key.clone(),
                            other => format!("{other:?}"),
                        };
                        out.insert(key, items[1].clone());
                    }
                }
                Some(Value::Set(pairs)) => {
                    for pair in pairs.borrow().iter() {
                        let items = match pair {
                            Value::List(items) if items.borrow().len() == 2 => {
                                items.borrow().clone()
                            }
                            _ => {
                                return Err(RuntimeError {
                                    message: "dict() sequence elements must each have length 2"
                                        .into(),
                                    span: Span::default(),
                                })
                            }
                        };
                        let key = match &items[0] {
                            Value::Str(key) => key.clone(),
                            other => format!("{other:?}"),
                        };
                        out.insert(key, items[1].clone());
                    }
                }
                Some(Value::Object { fields, .. }) => {
                    let method =
                        fields
                            .borrow()
                            .get("__iter__")
                            .cloned()
                            .ok_or_else(|| RuntimeError {
                                message: "dict() argument is not iterable".into(),
                                span: Span::default(),
                            })?;
                    let iterator = interp.invoke_value(
                        method,
                        vec![(None, args[0].clone())],
                        Span::default(),
                    )?;
                    let pairs = match iterator {
                        Value::List(items) => items.borrow().clone(),
                        Value::Object {
                            fields: ref iterator_fields,
                            ..
                        } => {
                            let next =
                                iterator_fields
                                    .borrow()
                                    .get("next")
                                    .cloned()
                                    .ok_or_else(|| RuntimeError {
                                        message: "__iter__ returned non-iterator".into(),
                                        span: Span::default(),
                                    })?;
                            let mut out = Vec::new();
                            loop {
                                let item = interp.invoke_value(
                                    next.clone(),
                                    vec![(None, iterator.clone())],
                                    Span::default(),
                                )?;
                                if matches!(item, Value::Sentinel(ref name) if name == "iteration.done")
                                {
                                    break;
                                }
                                out.push(item);
                            }
                            out
                        }
                        other => {
                            return Err(RuntimeError {
                                message: format!("__iter__ returned {}", other.type_name()),
                                span: Span::default(),
                            })
                        }
                    };
                    for pair in pairs {
                        let items = match pair {
                            Value::List(items) if items.borrow().len() == 2 => {
                                items.borrow().clone()
                            }
                            _ => {
                                return Err(RuntimeError {
                                    message: "dict() sequence elements must each have length 2"
                                        .into(),
                                    span: Span::default(),
                                })
                            }
                        };
                        let key = match &items[0] {
                            Value::Str(key) => key.clone(),
                            other => format!("{other:?}"),
                        };
                        out.insert(key, items[1].clone());
                    }
                }
                Some(other) => {
                    return Err(RuntimeError {
                        message: format!(
                            "dict() argument is not a mapping or sequence of pairs: {}",
                            other.type_name()
                        ),
                        span: Span::default(),
                    })
                }
            }
            Ok(Value::Dict(Rc::new(RefCell::new(out))))
        });
        self.env.borrow_mut().set(
            "dict".into(),
            Value::BuiltinFunction {
                name: "dict".into(),
                func: dict_fn,
            },
        );

        // abs(x)
        let abs_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError {
                    message: "abs() takes exactly 1 argument".into(),
                    span: Span::default(),
                });
            }
            match &args[0] {
                Value::Int(n) => Ok(Value::Int(match *n {
                    INT_NAN => INT_NAN,
                    INT_NEG_INF => INT_POS_INF,
                    value => value.abs(),
                })),
                Value::BigInt(n) => Ok(Value::BigInt(n.abs())),
                Value::Float(f) => Ok(Value::Float(f.abs())),
                Value::Complex(real, imag) => Ok(Value::Float(real.hypot(*imag))),
                other => Err(RuntimeError {
                    message: format!("bad operand type for abs(): {}", other.type_name()),
                    span: Span::default(),
                }),
            }
        });
        self.env.borrow_mut().set(
            "abs".to_string(),
            Value::BuiltinFunction {
                name: "abs".to_string(),
                func: abs_fn,
            },
        );

        // round(x, [ndigits])
        let round_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            let (num, ndigits) = match args.len() {
                1 => (&args[0], 0i64),
                2 => match &args[1] {
                    Value::Int(d) => (&args[0], *d),
                    _ => {
                        return Err(RuntimeError {
                            message: "round() ndigits must be an int".into(),
                            span: Span::default(),
                        })
                    }
                },
                n => {
                    return Err(RuntimeError {
                        message: format!("round() takes 1 or 2 arguments (got {n})"),
                        span: Span::default(),
                    })
                }
            };
            match num {
                Value::Int(n) => Ok(Value::Int(*n)),
                Value::Float(f) => {
                    if args.len() == 1 {
                        Ok(Value::Int(f.round() as i64))
                    } else {
                        let factor = 10.0f64.powi(ndigits as i32);
                        Ok(Value::Float((f * factor).round() / factor))
                    }
                }
                other => Err(RuntimeError {
                    message: format!("type {} doesn't define round", other.type_name()),
                    span: Span::default(),
                }),
            }
        });
        self.env.borrow_mut().set(
            "round".to_string(),
            Value::BuiltinFunction {
                name: "round".to_string(),
                func: round_fn,
            },
        );

        // time()
        let time_now_fn = Rc::new(|_args: &[Value], _interp: &mut Interpreter| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            Ok(Value::Float(now))
        });
        self.env.borrow_mut().set(
            "time".to_string(),
            Value::BuiltinFunction {
                name: "time".to_string(),
                func: time_now_fn.clone(),
            },
        );
        self.env.borrow_mut().set(
            "now".to_string(),
            Value::BuiltinFunction {
                name: "now".to_string(),
                func: time_now_fn,
            },
        );

        // Default multiple dispatch operators (+, -, *, ==, etc.)
        let add_int =
            Rc::new(
                |args: &[Value], _interp: &mut Interpreter| match (&args[0], &args[1]) {
                    (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                    (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                    (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{a}{b}"))),
                    _ => Err(RuntimeError {
                        message: "unsupported operands for +".to_string(),
                        span: Span::default(),
                    }),
                },
            );
        self.dispatch.register(
            "+".to_string(),
            vec!["int".to_string(), "int".to_string()],
            add_int.clone(),
        );
        self.dispatch.register(
            "+".to_string(),
            vec!["float".to_string(), "float".to_string()],
            add_int.clone(),
        );
        self.dispatch.register(
            "+".to_string(),
            vec!["str".to_string(), "str".to_string()],
            add_int,
        );

        let sub_int =
            Rc::new(
                |args: &[Value], _interp: &mut Interpreter| match (&args[0], &args[1]) {
                    (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
                    (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
                    _ => Err(RuntimeError {
                        message: "unsupported operands for -".to_string(),
                        span: Span::default(),
                    }),
                },
            );
        self.dispatch.register(
            "-".to_string(),
            vec!["int".to_string(), "int".to_string()],
            sub_int.clone(),
        );
        self.dispatch.register(
            "-".to_string(),
            vec!["float".to_string(), "float".to_string()],
            sub_int,
        );

        let mul_fn =
            Rc::new(
                |args: &[Value], _interp: &mut Interpreter| match (&args[0], &args[1]) {
                    (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
                    (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
                    (Value::Str(s), Value::Int(n)) => {
                        Ok(Value::Str(s.repeat((*n).max(0) as usize)))
                    }
                    _ => Err(RuntimeError {
                        message: "unsupported operands for *".to_string(),
                        span: Span::default(),
                    }),
                },
            );
        self.dispatch.register(
            "*".to_string(),
            vec!["int".to_string(), "int".to_string()],
            mul_fn.clone(),
        );
        self.dispatch.register(
            "*".to_string(),
            vec!["float".to_string(), "float".to_string()],
            mul_fn.clone(),
        );
        self.dispatch.register(
            "*".to_string(),
            vec!["str".to_string(), "int".to_string()],
            mul_fn,
        );

        let div_fn =
            Rc::new(
                |args: &[Value], _interp: &mut Interpreter| match (&args[0], &args[1]) {
                    (Value::Int(a), Value::Int(b)) => {
                        if *b == 0 {
                            Err(RuntimeError {
                                message: "division by zero".to_string(),
                                span: Span::default(),
                            })
                        } else {
                            Ok(Value::Float(*a as f64 / *b as f64))
                        }
                    }
                    (Value::Float(a), Value::Float(b)) => {
                        if *b == 0.0 {
                            Err(RuntimeError {
                                message: "division by zero".to_string(),
                                span: Span::default(),
                            })
                        } else {
                            Ok(Value::Float(a / b))
                        }
                    }
                    _ => Err(RuntimeError {
                        message: "unsupported operands for /".to_string(),
                        span: Span::default(),
                    }),
                },
            );
        self.dispatch.register(
            "/".to_string(),
            vec!["int".to_string(), "int".to_string()],
            div_fn.clone(),
        );
        self.dispatch.register(
            "/".to_string(),
            vec!["float".to_string(), "float".to_string()],
            div_fn,
        );

        let eq_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            Ok(Value::Bool(args[0] == args[1]))
        });
        self.dispatch.register(
            "==".to_string(),
            vec!["object".to_string(), "object".to_string()],
            eq_fn,
        );
    }

    pub fn load_module(
        &mut self,
        module_name: &str,
        span: Span,
    ) -> Result<Rc<RefCell<Environment>>, RuntimeError> {
        if module_name == "math" {
            let math_env = Rc::new(RefCell::new(Environment::new()));
            math_env
                .borrow_mut()
                .set("pi".to_string(), Value::Float(std::f64::consts::PI));
            math_env
                .borrow_mut()
                .set("e".to_string(), Value::Float(std::f64::consts::E));
            math_env.borrow_mut().set(
                "sqrt".to_string(),
                Value::BuiltinFunction {
                    name: "sqrt".to_string(),
                    func: Rc::new(|args, _interp| {
                        if args.len() != 1 {
                            return Err(RuntimeError {
                                message: "sqrt() takes 1 argument".into(),
                                span: Span::default(),
                            });
                        }
                        let val = match &args[0] {
                            Value::Int(n) => *n as f64,
                            Value::Float(f) => *f,
                            _ => {
                                return Err(RuntimeError {
                                    message: "sqrt() arg must be a number".into(),
                                    span: Span::default(),
                                })
                            }
                        };
                        Ok(Value::Float(val.sqrt()))
                    }),
                },
            );
            math_env.borrow_mut().set(
                "sin".to_string(),
                Value::BuiltinFunction {
                    name: "sin".to_string(),
                    func: Rc::new(|args, _interp| {
                        if args.len() != 1 {
                            return Err(RuntimeError {
                                message: "sin() takes 1 argument".into(),
                                span: Span::default(),
                            });
                        }
                        let val = match &args[0] {
                            Value::Int(n) => *n as f64,
                            Value::Float(f) => *f,
                            _ => {
                                return Err(RuntimeError {
                                    message: "sin() arg must be a number".into(),
                                    span: Span::default(),
                                })
                            }
                        };
                        Ok(Value::Float(val.sin()))
                    }),
                },
            );
            math_env.borrow_mut().set(
                "cos".to_string(),
                Value::BuiltinFunction {
                    name: "cos".to_string(),
                    func: Rc::new(|args, _interp| {
                        if args.len() != 1 {
                            return Err(RuntimeError {
                                message: "cos() takes 1 argument".into(),
                                span: Span::default(),
                            });
                        }
                        let val = match &args[0] {
                            Value::Int(n) => *n as f64,
                            Value::Float(f) => *f,
                            _ => {
                                return Err(RuntimeError {
                                    message: "cos() arg must be a number".into(),
                                    span: Span::default(),
                                })
                            }
                        };
                        Ok(Value::Float(val.cos()))
                    }),
                },
            );
            math_env.borrow_mut().set(
                "tan".to_string(),
                Value::BuiltinFunction {
                    name: "tan".to_string(),
                    func: Rc::new(|args, _interp| {
                        if args.len() != 1 {
                            return Err(RuntimeError {
                                message: "tan() takes 1 argument".into(),
                                span: Span::default(),
                            });
                        }
                        let val = match &args[0] {
                            Value::Int(n) => *n as f64,
                            Value::Float(f) => *f,
                            _ => {
                                return Err(RuntimeError {
                                    message: "tan() arg must be a number".into(),
                                    span: Span::default(),
                                })
                            }
                        };
                        Ok(Value::Float(val.tan()))
                    }),
                },
            );
            math_env.borrow_mut().set(
                "floor".to_string(),
                Value::BuiltinFunction {
                    name: "floor".to_string(),
                    func: Rc::new(|args, _interp| {
                        if args.len() != 1 {
                            return Err(RuntimeError {
                                message: "floor() takes 1 argument".into(),
                                span: Span::default(),
                            });
                        }
                        let val = match &args[0] {
                            Value::Int(n) => *n,
                            Value::Float(f) => f.floor() as i64,
                            _ => {
                                return Err(RuntimeError {
                                    message: "floor() arg must be a number".into(),
                                    span: Span::default(),
                                })
                            }
                        };
                        Ok(Value::Int(val))
                    }),
                },
            );
            math_env.borrow_mut().set(
                "ceil".to_string(),
                Value::BuiltinFunction {
                    name: "ceil".to_string(),
                    func: Rc::new(|args, _interp| {
                        if args.len() != 1 {
                            return Err(RuntimeError {
                                message: "ceil() takes 1 argument".into(),
                                span: Span::default(),
                            });
                        }
                        let val = match &args[0] {
                            Value::Int(n) => *n,
                            Value::Float(f) => f.ceil() as i64,
                            _ => {
                                return Err(RuntimeError {
                                    message: "ceil() arg must be a number".into(),
                                    span: Span::default(),
                                })
                            }
                        };
                        Ok(Value::Int(val))
                    }),
                },
            );
            math_env.borrow_mut().set(
                "abs".to_string(),
                Value::BuiltinFunction {
                    name: "abs".to_string(),
                    func: Rc::new(|args, _interp| {
                        if args.len() != 1 {
                            return Err(RuntimeError {
                                message: "abs() takes 1 argument".into(),
                                span: Span::default(),
                            });
                        }
                        match &args[0] {
                            Value::Int(n) => Ok(Value::Int(match *n {
                                INT_NAN => INT_NAN,
                                INT_NEG_INF => INT_POS_INF,
                                value => value.abs(),
                            })),
                            Value::BigInt(n) => Ok(Value::BigInt(n.abs())),
                            Value::Float(f) => Ok(Value::Float(f.abs())),
                            Value::Complex(real, imag) => Ok(Value::Float(real.hypot(*imag))),
                            _ => Err(RuntimeError {
                                message: "abs() arg must be a number".into(),
                                span: Span::default(),
                            }),
                        }
                    }),
                },
            );
            return Ok(math_env);
        }

        if module_name == "sys" {
            let sys_env = Rc::new(RefCell::new(Environment::new()));
            sys_env.borrow_mut().set(
                "platform".to_string(),
                Value::Str(std::env::consts::OS.to_string()),
            );
            sys_env
                .borrow_mut()
                .set("version".to_string(), Value::Str("0.1.0".to_string()));
            sys_env.borrow_mut().set(
                "argv".to_string(),
                Value::List(Rc::new(RefCell::new(vec![Value::Str("lucid".to_string())]))),
            );
            return Ok(sys_env);
        }

        if module_name == "iteration" {
            let iteration_env = Rc::new(RefCell::new(Environment::new()));
            iteration_env.borrow_mut().set(
                "done".to_string(),
                Value::Sentinel("iteration.done".to_string()),
            );
            return Ok(iteration_env);
        }

        if module_name == "time" {
            let time_env = Rc::new(RefCell::new(Environment::new()));
            let time_fn = Rc::new(|_args: &[Value], _interp: &mut Interpreter| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64();
                Ok(Value::Float(now))
            });
            time_env.borrow_mut().set(
                "time".to_string(),
                Value::BuiltinFunction {
                    name: "time".to_string(),
                    func: time_fn.clone(),
                },
            );
            time_env.borrow_mut().set(
                "monotonic".to_string(),
                Value::BuiltinFunction {
                    name: "monotonic".to_string(),
                    func: time_fn.clone(),
                },
            );
            return Ok(time_env);
        }

        // Resolve relative module file
        let dot_count = module_name.chars().take_while(|&c| c == '.').count();
        let rest = &module_name[dot_count..];
        let rel_path = rest.replace('.', "/");

        let current_dir = if let Some(ref cf) = self.current_file {
            cf.parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        };

        let mut base_dir = current_dir;
        if dot_count > 1 {
            for _ in 1..dot_count {
                if let Some(parent) = base_dir.parent() {
                    base_dir = parent.to_path_buf();
                }
            }
        }

        let mut candidate = base_dir.join(format!("{rel_path}.lucid"));
        if !candidate.exists() {
            candidate = base_dir.join(format!("{rel_path}/mod.lucid"));
        }
        if !candidate.exists() {
            candidate = base_dir.join(format!("{rel_path}/__init__.lucid"));
        }
        if !candidate.exists() && dot_count == 0 {
            if let Ok(cwd) = std::env::current_dir() {
                let alt = cwd.join(format!("{rel_path}.lucid"));
                if alt.exists() {
                    candidate = alt;
                }
            }
        }

        if !candidate.exists() {
            return Err(RuntimeError {
                message: format!(
                    "cannot find module '{module_name}' (looked at {})",
                    candidate.display()
                ),
                span,
            });
        }

        let canon = std::fs::canonicalize(&candidate).unwrap_or(candidate);
        if let Some(cached) = self.module_cache.get(&canon) {
            if self.module_loading.contains(&canon) && !declaration_only_module(&canon) {
                return Err(RuntimeError {
                    message: format!(
                        "cyclic module initialization involving '{}'",
                        canon.display()
                    ),
                    span,
                });
            }
            return Ok(Rc::clone(cached));
        }

        let source = std::fs::read_to_string(&canon).map_err(|e| RuntimeError {
            message: format!("failed to read module file '{}': {e}", canon.display()),
            span,
        })?;

        let parsed = lucid_syntax::parse(&source).map_err(|e| RuntimeError {
            message: format!("syntax error in module '{}': {e}", canon.display()),
            span,
        })?;

        // Publish an activation record before evaluating imports. This makes
        // declaration-only cycles finite: a module reached recursively can
        // observe the names collected below instead of re-entering the
        // loader indefinitely. The subsequent evaluation replaces these
        // placeholders with the complete definitions.
        let mut sub_interp = Interpreter::new();
        let module_env = Rc::clone(&sub_interp.env);
        let builtin_dispatch_counts = sub_interp
            .dispatch
            .methods
            .iter()
            .map(|(name, entries)| (name.clone(), entries.len()))
            .collect::<HashMap<_, _>>();
        for statement in &parsed.statements {
            let statement = match statement {
                Stmt::Export(inner) => inner.as_ref(),
                other => other,
            };
            match statement {
                Stmt::ClassDef { name, .. } => {
                    module_env
                        .borrow_mut()
                        .set(name.clone(), Value::ClassRef(name.clone()));
                }
                Stmt::Function(function) => {
                    module_env.borrow_mut().set(
                        function.name.clone(),
                        Value::Function {
                            name: function.name.clone(),
                            params: function.params.clone(),
                            body: function.body.clone(),
                            closure: Rc::clone(&module_env),
                            is_contextmanager: false,
                            is_async: function.is_async,
                        },
                    );
                }
                _ => {}
            }
        }
        self.module_cache
            .insert(canon.clone(), Rc::clone(&module_env));
        self.module_loading.insert(canon.clone());
        sub_interp.current_file = Some(canon.clone());
        sub_interp.module_cache = self.module_cache.clone();
        sub_interp.module_loading = self.module_loading.clone();
        if let Err(error) = sub_interp.eval_module(&parsed) {
            self.module_cache.remove(&canon);
            self.module_loading.remove(&canon);
            return Err(error);
        }

        let env = sub_interp.env;
        // Module values can escape through imports and then be used by the
        // importing interpreter (for example, constructing an imported
        // class). Keep the semantic registries in sync with the environment
        // cache instead of leaving definitions stranded in the temporary
        // module interpreter.
        self.classes.extend(sub_interp.classes);
        self.class_vars.extend(sub_interp.class_vars);
        self.traits.extend(sub_interp.traits);
        for (name, entries) in sub_interp.dispatch.methods {
            let builtin_count = builtin_dispatch_counts.get(&name).copied().unwrap_or(0);
            for entry in entries.into_iter().skip(builtin_count) {
                self.dispatch
                    .register(name.clone(), entry.param_types, entry.func);
            }
        }
        self.module_cache = sub_interp.module_cache;
        self.module_loading = sub_interp.module_loading;
        self.module_loading.remove(&canon);
        self.module_cache.insert(canon, Rc::clone(&env));

        Ok(env)
    }

    pub fn eval_module(&mut self, module: &Module) -> Result<Value, RuntimeError> {
        // Entry modules are evaluated directly rather than through
        // `load_module`. Publish their environment first so a recursive
        // import observes the same declaration set and cannot re-enter the
        // module with a second activation.
        let root_path = self
            .current_file
            .as_ref()
            .filter(|path| path.is_file())
            .and_then(|path| std::fs::canonicalize(path).ok());
        let root_was_cached = root_path
            .as_ref()
            .is_some_and(|path| self.module_cache.contains_key(path));
        if let Some(path) = root_path.as_ref().filter(|_| !root_was_cached) {
            for statement in &module.statements {
                let statement = match statement {
                    Stmt::Export(inner) => inner.as_ref(),
                    other => other,
                };
                match statement {
                    Stmt::ClassDef { name, .. } => {
                        self.env
                            .borrow_mut()
                            .set(name.clone(), Value::ClassRef(name.clone()));
                    }
                    Stmt::Function(function) => {
                        self.env.borrow_mut().set(
                            function.name.clone(),
                            Value::Function {
                                name: function.name.clone(),
                                params: function.params.clone(),
                                body: function.body.clone(),
                                closure: Rc::clone(&self.env),
                                is_contextmanager: false,
                                is_async: function.is_async,
                            },
                        );
                    }
                    _ => {}
                }
            }
            self.module_cache.insert(path.clone(), Rc::clone(&self.env));
            self.module_loading.insert(path.clone());
        }
        let result = (|| {
            let mut last_val = Value::None;
            for stmt in &module.statements {
                if let Stmt::Yield { value, .. } = stmt {
                    return Err(RuntimeError {
                        message: "yield is only valid in a contextmanager definition".into(),
                        span: value.span(),
                    });
                }
                last_val = self.eval_statement(stmt)?;
                if let Value::Return(v) = last_val {
                    let _ = v;
                    return Err(RuntimeError {
                        message: "return is only valid inside a function".into(),
                        span: Span::default(),
                    });
                }
            }
            self.validate_class_hierarchy()?;
            Ok(last_val)
        })();
        if let Some(path) = root_path {
            self.module_loading.remove(&path);
        }
        result
    }

    /// Invoke a function or builtin bound in the current environment by name.
    /// Dotted names walk imported module environments, so project manifests
    /// can name public entries such as `.package.main` without exposing the
    /// interpreter's environment. A leading dot is the manifest's
    /// project-relative spelling and is ignored at this boundary.
    /// This is the runtime boundary used by project entry points and tooling;
    /// callers do not need access to the interpreter's internal environment.
    fn lookup_named_value(&self, name: &str) -> Result<Value, RuntimeError> {
        if name.trim().is_empty() || name.chars().any(char::is_whitespace) {
            return Err(RuntimeError {
                message: format!("invalid callable name '{name}'"),
                span: Span::default(),
            });
        }
        let target = name.strip_prefix('.').unwrap_or(name);
        let parts = target.split('.').collect::<Vec<_>>();
        if parts
            .iter()
            .any(|part| part.is_empty() || part.starts_with('_'))
        {
            return Err(RuntimeError {
                message: format!("name '{name}' is private or malformed and cannot be invoked"),
                span: Span::default(),
            });
        }
        let mut function = self
            .env
            .borrow()
            .get(parts[0])
            .ok_or_else(|| RuntimeError {
                message: format!("name '{name}' is not callable or is not defined"),
                span: Span::default(),
            })?;
        for part in parts.iter().skip(1) {
            function = match function {
                Value::Module { env, .. } => env.borrow().get(part),
                _ => None,
            }
            .ok_or_else(|| RuntimeError {
                message: format!("name '{name}' is not callable or is not defined"),
                span: Span::default(),
            })?;
        }
        Ok(function)
    }

    pub fn call_named(&mut self, name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
        let function = self.lookup_named_value(name)?;
        self.invoke_value(
            function,
            args.iter().cloned().map(|value| (None, value)).collect(),
            Span::default(),
        )
    }

    /// Invoke an entry point inside a configured library context manager.
    /// The synthetic `with` statement deliberately reuses the ordinary
    /// context-manager implementation, including reverse-order teardown and
    /// error cleanup, instead of introducing a second lifecycle path.
    pub fn call_named_with_context(
        &mut self,
        context_name: &str,
        entry_name: &str,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let context_factory = self.lookup_named_value(context_name)?;
        let entry = self.lookup_named_value(entry_name)?;
        let context = self.invoke_value(context_factory, Vec::new(), Span::default())?;
        let parent = Rc::clone(&self.env);
        let scoped = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&parent))));
        scoped
            .borrow_mut()
            .set("__lucid_library_context".into(), context);
        let captured_args = args.to_vec();
        scoped.borrow_mut().set(
            "__lucid_library_entry".into(),
            Value::BuiltinFunction {
                name: "__lucid_library_entry".into(),
                func: Rc::new(move |_ignored, interpreter| {
                    interpreter.invoke_value(
                        entry.clone(),
                        captured_args
                            .iter()
                            .cloned()
                            .map(|value| (None, value))
                            .collect(),
                        Span::default(),
                    )
                }),
            },
        );
        self.env = scoped;
        let ident = |name: &str| Expr::Ident {
            name: name.into(),
            span: Span::default(),
        };
        let statement = Stmt::With {
            items: vec![WithItem {
                context_expr: ident("__lucid_library_context"),
                target: None,
            }],
            body: vec![Stmt::Expr(Expr::Call {
                func: Box::new(ident("__lucid_library_entry")),
                args: Vec::new(),
                span: Span::default(),
            })],
            span: Span::default(),
        };
        let result = self.eval_statement(&statement);
        self.env = parent;
        result
    }

    fn validate_class_hierarchy(&self) -> Result<(), RuntimeError> {
        for class in self.classes.values() {
            for base in &class.bases {
                let TypeExpr::Named { name: parent, .. } = base else {
                    continue;
                };
                let Some(parent_def) = self.classes.get(parent) else {
                    continue;
                };
                if parent_def.is_final {
                    return Err(RuntimeError {
                        message: format!("class '{parent}' is final and cannot be inherited"),
                        span: class.span,
                    });
                }
                if parent_def.is_sealed && parent_def.source_file != class.source_file {
                    return Err(RuntimeError {
                        message: format!(
                            "sealed class '{parent}' may only be subclassed in its defining file"
                        ),
                        span: class.span,
                    });
                }
            }
        }
        Ok(())
    }

    pub fn eval_statement(&mut self, stmt: &Stmt) -> Result<Value, RuntimeError> {
        match stmt {
            Stmt::Export(inner) => self.eval_statement(inner),
            Stmt::ClassDef { .. } => {
                if let Stmt::ClassDef {
                    name,
                    type_params,
                    bases,
                    without_traits,
                    body,
                    is_sealed,
                    is_final,
                    span,
                    ..
                } = stmt.clone()
                {
                    for base in &bases {
                        if let TypeExpr::Named { name: parent, .. } = base {
                            let parent_def = self.classes.get(parent);
                            if parent_def.is_some_and(|class| class.is_final) {
                                return Err(RuntimeError {
                                    message: format!(
                                        "class '{parent}' is final and cannot be inherited"
                                    ),
                                    span,
                                });
                            }
                            if let Some(parent_def) = parent_def {
                                if parent_def.is_sealed
                                    && parent_def.source_file != self.current_file
                                {
                                    return Err(RuntimeError {
                                        message: format!(
                                            "sealed class '{parent}' may only be subclassed in its defining file"
                                        ),
                                        span,
                                    });
                                }
                            }
                        }
                    }
                    self.classes.insert(
                        name.clone(),
                        ClassDef {
                            name: name.clone(),
                            type_params,
                            bases,
                            without_traits,
                            body: body.clone(),
                            is_sealed,
                            is_final,
                            source_file: self.current_file.clone(),
                            span,
                        },
                    );
                    let mut vars = HashMap::new();
                    for member in &body {
                        if let ClassMember::ClassVar(field) = member {
                            let value = field
                                .default
                                .as_ref()
                                .map(|expr| self.eval_expr(expr))
                                .transpose()?
                                .unwrap_or(Value::None);
                            vars.insert(field.name.clone(), value);
                        }
                    }
                    self.class_vars.insert(name.clone(), vars);

                    // Class names are first-class references. Calling one uses
                    // its __init__ factory (or the generated field constructor),
                    // while attribute access resolves class methods/factories.
                    self.env
                        .borrow_mut()
                        .set(name.clone(), Value::ClassRef(name));
                }
                Ok(Value::None)
            }
            Stmt::TraitDef { name, body, .. } => {
                self.traits.insert(name.clone(), body.clone());
                self.env
                    .borrow_mut()
                    .set(name.clone(), Value::TraitRef(name.clone()));
                Ok(Value::None)
            }
            // Interfaces and aliases are compile-time declarations.  Keep
            // them explicit here so they are not mistaken for an
            // accidentally unhandled executable statement.
            Stmt::InterfaceDef { .. } | Stmt::TypeAlias { .. } => Ok(Value::None),
            Stmt::ImplementDef {
                interface,
                target,
                body,
                span,
            } => {
                let target_name = match target {
                    TypeExpr::Named { name, .. } => name.clone(),
                    _ => {
                        return Err(RuntimeError {
                            message: "implementation target must be a named class".into(),
                            span: *span,
                        })
                    }
                };
                let trait_name = match interface {
                    TypeExpr::Named { name, .. } => name,
                    _ => "interface",
                };
                let class = self
                    .classes
                    .get_mut(&target_name)
                    .ok_or_else(|| RuntimeError {
                        message: format!(
                            "cannot implement '{trait_name}' for unknown class '{target_name}'"
                        ),
                        span: *span,
                    })?;
                if !class
                    .bases
                    .iter()
                    .any(|base| matches!(base, TypeExpr::Named { name, .. } if name == trait_name))
                {
                    class.bases.push(TypeExpr::Named {
                        name: trait_name.to_string(),
                        args: Vec::new(),
                        span: *span,
                    });
                }
                for function in body {
                    let mut function = function.clone();
                    if !function
                        .params
                        .first()
                        .is_some_and(|param| param.name == "self")
                    {
                        function.params.insert(
                            0,
                            Param {
                                name: "self".into(),
                                pattern: None,
                                type_annotation: None,
                                default: None,
                                is_positional_only: false,
                                is_keyword_only: false,
                                is_variadic_positional: false,
                                is_variadic_keyword: false,
                                is_gather: false,
                                span: function.span,
                            },
                        );
                    }
                    class.body.push(ClassMember::Method(function));
                }
                Ok(Value::None)
            }
            Stmt::Function(func) => {
                let mut func_val = Value::Function {
                    name: func.name.clone(),
                    params: func.params.clone(),
                    body: func.body.clone(),
                    closure: Rc::clone(&self.env),
                    is_contextmanager: func.decorators.iter().any(|decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager")),
                    is_async: func.is_async,
                };

                // Decorators apply bottom-up and rebind the definition to
                // each decorator's result.
                for decorator in func.decorators.iter().rev() {
                    if matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager") {
                        continue;
                    }
                    let decorator_value = self.eval_expr(decorator)?;
                    func_val =
                        self.invoke_value(decorator_value, vec![(None, func_val)], func.span)?;
                    match &mut func_val {
                        Value::Function { name, .. } | Value::BuiltinFunction { name, .. } => {
                            *name = func.name.clone();
                        }
                        _ => {}
                    }
                }

                if func.is_dispatch {
                    let f_name = func.name.clone();
                    let param_types: Vec<String> = func
                        .params
                        .iter()
                        .map(|p| {
                            p.type_annotation
                                .as_ref()
                                .map(|t| match t {
                                    TypeExpr::Named { name, .. } => name.clone(),
                                    _ => "object".to_string(),
                                })
                                .unwrap_or_else(|| "object".to_string())
                        })
                        .collect();

                    let decorated_value = func_val.clone();
                    let dispatch_fn = Rc::new(move |args: &[Value], interp: &mut Interpreter| {
                        let call_args = args.iter().cloned().map(|value| (None, value)).collect();
                        interp.invoke_value(decorated_value.clone(), call_args, Span::default())
                    });

                    self.dispatch.register(
                        f_name.clone(),
                        param_types.clone(),
                        dispatch_fn.clone(),
                    );

                    if f_name == "__add__" {
                        self.dispatch.register(
                            "+".to_string(),
                            param_types.clone(),
                            dispatch_fn.clone(),
                        );
                    } else if f_name == "__sub__" {
                        self.dispatch.register(
                            "-".to_string(),
                            param_types.clone(),
                            dispatch_fn.clone(),
                        );
                    } else if f_name == "__mul__" {
                        self.dispatch.register(
                            "*".to_string(),
                            param_types.clone(),
                            dispatch_fn.clone(),
                        );
                    } else if f_name == "__truediv__" || f_name == "__div__" {
                        self.dispatch.register(
                            "/".to_string(),
                            param_types.clone(),
                            dispatch_fn.clone(),
                        );
                    } else if f_name == "__eq__" {
                        self.dispatch.register(
                            "==".to_string(),
                            param_types.clone(),
                            dispatch_fn.clone(),
                        );
                    }

                    let name_for_call = f_name.clone();
                    let dispatch_wrapper = Value::BuiltinFunction {
                        name: f_name.clone(),
                        func: Rc::new(move |args, interp| {
                            interp.call_dispatch(&name_for_call, args)
                        }),
                    };
                    self.env.borrow_mut().set(f_name, dispatch_wrapper);
                } else {
                    self.env.borrow_mut().set(func.name.clone(), func_val);
                }
                Ok(Value::None)
            }
            Stmt::VarDef {
                pattern,
                value,
                span,
                is_final,
                ..
            } => {
                let val = if let Some(ref e) = value {
                    let v = self.eval_expr(e)?;
                    if let Value::Return(_) = v {
                        return Ok(v);
                    }
                    v
                } else {
                    Value::None
                };

                self.bind_pattern(pattern, val.clone(), *span)?;
                if *is_final {
                    self.mark_final_pattern(pattern);
                }
                Ok(val)
            }
            Stmt::Assignment {
                target,
                value,
                span,
            } => {
                let previous_capture = self.capture_assignment.replace(match target {
                    Expr::Ident { name, .. } => name.clone(),
                    _ => String::new(),
                });
                let val_result = self.eval_expr(value);
                self.capture_assignment = previous_capture;
                let val = val_result?;
                if let Value::Return(_) = val {
                    return Ok(val);
                }
                match target {
                    Expr::Ident { name, .. } => {
                        if self.env.borrow().is_final(name) {
                            return Err(RuntimeError {
                                message: format!("cannot reassign final variable '{name}'"),
                                span: *span,
                            });
                        }
                        if !self.env.borrow_mut().mutate(name, val.clone()) {
                            self.env.borrow_mut().set(name.clone(), val.clone());
                        }
                    }
                    Expr::Record { fields, .. } => {
                        let items: Vec<Value> = match val {
                            Value::List(ref l) => l.borrow().clone(),
                            Value::Record(ref r) => {
                                let mut sorted_keys: Vec<_> = r.borrow().keys().cloned().collect();
                                sorted_keys.sort();
                                let record = r.borrow();
                                let mut values = Vec::with_capacity(sorted_keys.len());
                                for key in sorted_keys {
                                    let value = record.get(&key).ok_or_else(|| RuntimeError {
                                        message: format!(
                                            "record field '{key}' disappeared during destructuring"
                                        ),
                                        span: *span,
                                    })?;
                                    values.push(value.clone());
                                }
                                values
                            }
                            _ => Vec::new(),
                        };
                        let star = fields.iter().position(|(_, expr)| {
                            matches!(
                                expr,
                                Expr::Unary {
                                    op: UnaryOp::Spread,
                                    ..
                                }
                            )
                        });
                        if let Some(star_index) = star {
                            let fixed = fields.len() - 1;
                            if items.len() < fixed {
                                return Err(RuntimeError {
                                    message: "unpacking mismatch".into(),
                                    span: *span,
                                });
                            }
                            for (index, (_, field_expr)) in fields[..star_index].iter().enumerate()
                            {
                                if let Expr::Ident { name, .. } = field_expr {
                                    if !self.env.borrow_mut().mutate(name, items[index].clone()) {
                                        self.env
                                            .borrow_mut()
                                            .set(name.clone(), items[index].clone());
                                    }
                                }
                            }
                            let tail_count = fields.len() - star_index - 1;
                            let rest_end = items.len() - tail_count;
                            if let Expr::Unary { expr: target, .. } = &fields[star_index].1 {
                                if let Expr::Ident { name, .. } = &**target {
                                    let rest = Value::List(Rc::new(RefCell::new(
                                        items[star_index..rest_end].to_vec(),
                                    )));
                                    if !self.env.borrow_mut().mutate(name, rest.clone()) {
                                        self.env.borrow_mut().set(name.clone(), rest);
                                    }
                                }
                            }
                            for (offset, (_, field_expr)) in
                                fields[star_index + 1..].iter().enumerate()
                            {
                                if let Expr::Ident { name, .. } = field_expr {
                                    let item = items[rest_end + offset].clone();
                                    if !self.env.borrow_mut().mutate(name, item.clone()) {
                                        self.env.borrow_mut().set(name.clone(), item);
                                    }
                                }
                            }
                        } else {
                            for ((_, field_expr), item) in fields.iter().zip(items) {
                                if let Expr::Ident { name, .. } = field_expr {
                                    if !self.env.borrow_mut().mutate(name, item.clone()) {
                                        self.env.borrow_mut().set(name.clone(), item);
                                    }
                                }
                            }
                        }
                    }
                    Expr::List { elements, .. } => {
                        let items: Vec<Value> = match val {
                            Value::List(ref l) => l.borrow().clone(),
                            _ => Vec::new(),
                        };
                        for (elem_expr, item) in elements.iter().zip(items) {
                            if let Expr::Ident { name, .. } = elem_expr {
                                if !self.env.borrow_mut().mutate(name, item.clone()) {
                                    self.env.borrow_mut().set(name.clone(), item);
                                }
                            }
                        }
                    }
                    Expr::Attribute {
                        value: obj_expr,
                        attr,
                        ..
                    } => {
                        let obj = self.eval_expr(obj_expr)?;
                        match obj {
                            Value::ClassRef(class_name) => {
                                self.check_private_access(&class_name, attr, *span)?;
                                if self.field_is_final(&class_name, attr) {
                                    return Err(RuntimeError {
                                        message: format!("cannot reassign final field '{attr}'"),
                                        span: *span,
                                    });
                                }
                                if !self.class_var_set(&class_name, attr, val.clone()) {
                                    return Err(RuntimeError {
                                        message: format!(
                                            "class '{class_name}' has no class variable '{attr}'"
                                        ),
                                        span: *span,
                                    });
                                }
                            }
                            Value::Object {
                                fields,
                                is_frozen,
                                class_name,
                            } => {
                                self.check_private_access(&class_name, attr, *span)?;
                                if self.field_is_final(&class_name, attr) {
                                    return Err(RuntimeError {
                                        message: format!("cannot reassign final field '{attr}'"),
                                        span: *span,
                                    });
                                }
                                if *is_frozen.borrow() {
                                    return Err(RuntimeError {
                                        message: format!("cannot mutate attribute '{attr}' on frozen object !{class_name}"),
                                        span: *span,
                                    });
                                }
                                if self.setter_depth == 0 {
                                    let setter =
                                        fields.borrow().get(&format!("__setter__{attr}")).cloned();
                                    if let Some(Value::Function {
                                        params,
                                        body,
                                        closure,
                                        ..
                                    }) = setter
                                    {
                                        let mut call_args = vec![
                                            Value::Object {
                                                class_name: class_name.clone(),
                                                fields: Rc::clone(&fields),
                                                is_frozen: Rc::clone(&is_frozen),
                                            },
                                            val.clone(),
                                        ];
                                        let call_env = Rc::new(RefCell::new(
                                            Environment::with_parent(closure),
                                        ));
                                        for (param, arg_val) in
                                            params.iter().zip(call_args.drain(..))
                                        {
                                            call_env.borrow_mut().set(param.name.clone(), arg_val);
                                        }
                                        let previous = Rc::clone(&self.env);
                                        self.setter_depth += 1;
                                        self.env = call_env;
                                        let result = self.eval_block(&body);
                                        self.env = previous;
                                        self.setter_depth -= 1;
                                        result?;
                                    } else {
                                        fields.borrow_mut().insert(attr.clone(), val.clone());
                                    }
                                } else {
                                    fields.borrow_mut().insert(attr.clone(), val.clone());
                                }
                            }
                            _ => {
                                return Err(RuntimeError {
                                    message: "cannot set attribute on non-object".to_string(),
                                    span: *span,
                                })
                            }
                        }
                    }
                    Expr::Index {
                        value: obj_expr,
                        index: idx_expr,
                        span: idx_span,
                    } => {
                        let obj = self.eval_expr(obj_expr)?;
                        let idx = self.eval_expr(idx_expr)?;
                        if let Value::Object { fields, .. } = &obj {
                            let method = fields.borrow().get("__setitem__").cloned();
                            if let Some(method) = method {
                                self.invoke_value(
                                    method,
                                    vec![(None, obj.clone()), (None, idx), (None, val.clone())],
                                    *idx_span,
                                )?;
                                return Ok(val);
                            }
                        }
                        match obj {
                            Value::List(items) => match idx {
                                Value::Int(i) => {
                                    if container_is_frozen(Rc::as_ptr(&items) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen list".into(),
                                            span: *idx_span,
                                        });
                                    }
                                    let len = items.borrow().len() as i64;
                                    let actual_i = if i < 0 { len + i } else { i };
                                    if actual_i < 0 || actual_i >= len {
                                        return Err(RuntimeError {
                                            message: format!(
                                                "list index out of range: index {i}, len {len}"
                                            ),
                                            span: *idx_span,
                                        });
                                    }
                                    items.borrow_mut()[actual_i as usize] = val.clone();
                                }
                                _ => {
                                    return Err(RuntimeError {
                                        message: format!(
                                            "list indices must be integers, got {}",
                                            idx.type_name()
                                        ),
                                        span: *idx_span,
                                    })
                                }
                            },
                            Value::MemoryView {
                                data,
                                start,
                                len,
                                stride,
                                read_only,
                            } => match idx {
                                Value::Int(i) => {
                                    if read_only {
                                        return Err(RuntimeError {
                                            message: "cannot mutate read-only memoryview".into(),
                                            span: *idx_span,
                                        });
                                    }
                                    let actual_i = if i < 0 { len + i } else { i };
                                    if actual_i < 0 || actual_i >= len {
                                        return Err(RuntimeError {
                                            message: format!(
                                                "memoryview index out of range: index {i}, len {len}"
                                            ),
                                            span: *idx_span,
                                        });
                                    }
                                    data.borrow_mut()[(start + actual_i * stride) as usize] =
                                        val.clone();
                                }
                                _ => {
                                    return Err(RuntimeError {
                                        message: format!(
                                            "memoryview indices must be integers, got {}",
                                            idx.type_name()
                                        ),
                                        span: *idx_span,
                                    })
                                }
                            },
                            Value::Dict(d) => {
                                if container_is_frozen(Rc::as_ptr(&d) as usize) {
                                    return Err(RuntimeError {
                                        message: "cannot mutate frozen dict".into(),
                                        span: *idx_span,
                                    });
                                }
                                let key_str = match &idx {
                                    Value::Str(s) => s.clone(),
                                    other => format!("{other:?}"),
                                };
                                d.borrow_mut().insert(key_str, val.clone());
                            }
                            _ => {
                                return Err(RuntimeError {
                                    message: format!(
                                        "cannot index assign into {}",
                                        obj.type_name()
                                    ),
                                    span: *idx_span,
                                })
                            }
                        }
                    }
                    _ => {
                        return Err(RuntimeError {
                            message: "invalid assignment target".to_string(),
                            span: *span,
                        })
                    }
                }
                Ok(val)
            }
            Stmt::AugAssign {
                target,
                op,
                value,
                span,
            } => {
                let rhs = self.eval_expr(value)?;
                if let Value::Return(_) = rhs {
                    return Ok(rhs);
                }
                match target {
                    Expr::Ident {
                        name,
                        span: id_span,
                    } => {
                        if self.env.borrow().is_final(name) {
                            return Err(RuntimeError {
                                message: format!("cannot reassign final variable '{name}'"),
                                span: *id_span,
                            });
                        }
                        let cur = self.env.borrow().get(name).ok_or_else(|| RuntimeError {
                            message: format!("undefined variable '{name}'"),
                            span: *id_span,
                        })?;
                        let new_val = self.eval_binary_op(op, cur, rhs, span)?;
                        if !self.env.borrow_mut().mutate(name, new_val.clone()) {
                            self.env.borrow_mut().set(name.clone(), new_val.clone());
                        }
                        Ok(new_val)
                    }
                    Expr::Index {
                        value: obj_expr,
                        index: idx_expr,
                        span: idx_span,
                    } => {
                        let obj = self.eval_expr(obj_expr)?;
                        let idx = self.eval_expr(idx_expr)?;
                        match obj {
                            Value::List(items) => match idx {
                                Value::Int(i) => {
                                    if container_is_frozen(Rc::as_ptr(&items) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen list".into(),
                                            span: *idx_span,
                                        });
                                    }
                                    let len = items.borrow().len() as i64;
                                    let actual_i = if i < 0 { len + i } else { i };
                                    if actual_i < 0 || actual_i >= len {
                                        return Err(RuntimeError {
                                            message: format!(
                                                "list index out of range: index {i}, len {len}"
                                            ),
                                            span: *idx_span,
                                        });
                                    }
                                    let cur = items.borrow()[actual_i as usize].clone();
                                    let new_val = self.eval_binary_op(op, cur, rhs, span)?;
                                    items.borrow_mut()[actual_i as usize] = new_val.clone();
                                    Ok(new_val)
                                }
                                _ => {
                                    return Err(RuntimeError {
                                        message: format!(
                                            "list indices must be integers, got {}",
                                            idx.type_name()
                                        ),
                                        span: *idx_span,
                                    })
                                }
                            },
                            Value::MemoryView {
                                data,
                                start,
                                len,
                                stride,
                                read_only,
                            } => match idx {
                                Value::Int(i) => {
                                    if read_only {
                                        return Err(RuntimeError {
                                            message: "cannot mutate read-only memoryview".into(),
                                            span: *idx_span,
                                        });
                                    }
                                    let actual_i = if i < 0 { len + i } else { i };
                                    if actual_i < 0 || actual_i >= len {
                                        return Err(RuntimeError {
                                            message: format!(
                                                "memoryview index out of range: index {i}, len {len}"
                                            ),
                                            span: *idx_span,
                                        });
                                    }
                                    let storage_index = start + actual_i * stride;
                                    let cur = data.borrow()[storage_index as usize].clone();
                                    let new_val = self.eval_binary_op(op, cur, rhs, span)?;
                                    data.borrow_mut()[storage_index as usize] = new_val.clone();
                                    Ok(new_val)
                                }
                                _ => {
                                    return Err(RuntimeError {
                                        message: format!(
                                            "memoryview indices must be integers, got {}",
                                            idx.type_name()
                                        ),
                                        span: *idx_span,
                                    })
                                }
                            },
                            Value::Dict(d) => {
                                if container_is_frozen(Rc::as_ptr(&d) as usize) {
                                    return Err(RuntimeError {
                                        message: "cannot mutate frozen dict".into(),
                                        span: *idx_span,
                                    });
                                }
                                let key_str = match &idx {
                                    Value::Str(s) => s.clone(),
                                    other => format!("{other:?}"),
                                };
                                let cur =
                                    d.borrow().get(&key_str).cloned().unwrap_or(Value::Int(0));
                                let new_val = self.eval_binary_op(op, cur, rhs, span)?;
                                d.borrow_mut().insert(key_str, new_val.clone());
                                Ok(new_val)
                            }
                            _ => {
                                return Err(RuntimeError {
                                    message: format!("cannot index mutate {}", obj.type_name()),
                                    span: *idx_span,
                                })
                            }
                        }
                    }
                    Expr::Attribute {
                        value: obj_expr,
                        attr,
                        span: attr_span,
                    } => {
                        let obj = self.eval_expr(obj_expr)?;
                        match obj {
                            Value::Object {
                                fields,
                                is_frozen,
                                class_name,
                            } => {
                                self.check_private_access(&class_name, attr, *attr_span)?;
                                if self.field_is_final(&class_name, attr) {
                                    return Err(RuntimeError {
                                        message: format!("cannot reassign final field '{attr}'"),
                                        span: *attr_span,
                                    });
                                }
                                if *is_frozen.borrow() {
                                    return Err(RuntimeError {
                                        message: format!("cannot mutate attribute '{attr}' on frozen object !{class_name}"),
                                        span: *attr_span,
                                    });
                                }
                                let cur = fields.borrow().get(attr).cloned().unwrap_or(Value::None);
                                let new_val = self.eval_binary_op(op, cur, rhs, span)?;
                                fields.borrow_mut().insert(attr.clone(), new_val.clone());
                                Ok(new_val)
                            }
                            _ => {
                                return Err(RuntimeError {
                                    message: "cannot set attribute on non-object".to_string(),
                                    span: *attr_span,
                                })
                            }
                        }
                    }
                    _ => Err(RuntimeError {
                        message: "invalid augmented assignment target".to_string(),
                        span: *span,
                    }),
                }
            }
            Stmt::If {
                condition,
                then_branch,
                elif_branches,
                else_branch,
                ..
            } => {
                let cond_val = self.eval_expr(condition)?;
                if self.is_truthy(&cond_val) {
                    return self.eval_block(then_branch);
                }
                for (elif_cond, elif_body) in elif_branches {
                    let elif_val = self.eval_expr(elif_cond)?;
                    if self.is_truthy(&elif_val) {
                        return self.eval_block(elif_body);
                    }
                }
                if let Some(ref eb) = else_branch {
                    return self.eval_block(eb);
                }
                Ok(Value::None)
            }
            Stmt::For {
                target,
                iterable,
                body,
                if_broken,
                span,
            } => {
                let mut iter_val = self.eval_expr(iterable)?;
                if let Value::Object { fields, .. } = &iter_val {
                    let iterator = fields.borrow().get("__iter__").cloned();
                    if let Some(iterator) = iterator {
                        iter_val =
                            self.invoke_value(iterator, vec![(None, iter_val.clone())], *span)?;
                    }
                }
                let mut broken = false;

                if let Value::Range { start, stop, step } = iter_val {
                    let mut cur = start;
                    while (step > 0 && cur < stop) || (step < 0 && cur > stop) {
                        let iter_env =
                            Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&self.env))));
                        let prev_env = Rc::clone(&self.env);
                        self.env = iter_env;

                        let res = (|| {
                            self.bind_pattern(target, Value::Int(cur), *span)?;
                            self.eval_loop_body(body)
                        })();
                        self.env = prev_env;

                        match res {
                            Ok(Value::Sentinel(s)) if s == "__break__" => {
                                broken = true;
                                break;
                            }
                            Ok(Value::Sentinel(s)) if s == "__continue__" => {}
                            Ok(Value::Return(val)) => return Ok(Value::Return(val)),
                            Ok(v) => {
                                let _ = v;
                            }
                            Err(e) => return Err(e),
                        }
                        let Some(next) = cur.checked_add(step) else {
                            return Err(RuntimeError {
                                message: "range step overflow".into(),
                                span: *span,
                            });
                        };
                        cur = next;
                    }
                } else {
                    if let Value::Object { fields, .. } = &iter_val {
                        let next_member = { fields.borrow().get("next").cloned() };
                        if let Some(next_member) = next_member {
                            loop {
                                let result = match next_member.clone() {
                                    Value::Function { .. } => self.invoke_value(
                                        next_member.clone(),
                                        vec![(None, iter_val.clone())],
                                        *span,
                                    )?,
                                    _ => {
                                        self.invoke_value(next_member.clone(), Vec::new(), *span)?
                                    }
                                };
                                if matches!(result, Value::Sentinel(ref name) if name == "iteration.done")
                                {
                                    break;
                                }
                                let iter_env = Rc::new(RefCell::new(Environment::with_parent(
                                    Rc::clone(&self.env),
                                )));
                                let prev_env = Rc::clone(&self.env);
                                self.env = iter_env;
                                let res = (|| {
                                    self.bind_pattern(target, result, *span)?;
                                    self.eval_loop_body(body)
                                })();
                                self.env = prev_env;
                                match res {
                                    Ok(Value::Sentinel(s)) if s == "__break__" => {
                                        broken = true;
                                        break;
                                    }
                                    Ok(Value::Sentinel(s)) if s == "__continue__" => continue,
                                    Ok(Value::Return(value)) => return Ok(Value::Return(value)),
                                    Ok(_) => {}
                                    Err(error) => return Err(error),
                                }
                            }
                            if broken {
                                if let Some(ref ib) = if_broken {
                                    return self.eval_block(ib);
                                }
                            }
                            return Ok(Value::None);
                        }
                    }
                    let items = match iter_val {
                        Value::List(items) => items.borrow().clone(),
                        Value::Set(items) => items.borrow().clone(),
                        // Dictionaries are iterable over their keys, matching
                        // the collection protocol and the other iterable
                        // materialization paths.
                        Value::Dict(items) => {
                            items.borrow().keys().cloned().map(Value::Str).collect()
                        }
                        value @ (Value::Bytes(_) | Value::MemoryView { .. }) => {
                            binary_sequence_items(&value).unwrap_or_default()
                        }
                        _ => {
                            return Err(RuntimeError {
                                message: "value is not iterable".to_string(),
                                span: *span,
                            })
                        }
                    };

                    for item in items {
                        // Fresh iteration binding (basedpython semantics)
                        let iter_env =
                            Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&self.env))));
                        let prev_env = Rc::clone(&self.env);
                        self.env = iter_env;

                        let res = (|| {
                            self.bind_pattern(target, item, *span)?;
                            self.eval_loop_body(body)
                        })();
                        self.env = prev_env;

                        match res {
                            Ok(Value::Sentinel(s)) if s == "__break__" => {
                                broken = true;
                                break;
                            }
                            Ok(Value::Sentinel(s)) if s == "__continue__" => continue,
                            Ok(Value::Return(val)) => return Ok(Value::Return(val)),
                            Ok(v) => {
                                let _ = v;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                }

                if broken {
                    if let Some(ref ib) = if_broken {
                        return self.eval_block(ib);
                    }
                }

                Ok(Value::None)
            }
            Stmt::While {
                condition,
                body,
                if_broken,
                ..
            } => {
                let mut broken = false;
                loop {
                    let cond = self.eval_expr(condition)?;
                    if !self.is_truthy(&cond) {
                        break;
                    }
                    let res = self.eval_loop_body(body);
                    match res {
                        Ok(Value::Sentinel(s)) if s == "__break__" => {
                            broken = true;
                            break;
                        }
                        Ok(Value::Sentinel(s)) if s == "__continue__" => continue,
                        Ok(Value::Return(val)) => return Ok(Value::Return(val)),
                        Ok(v) => {
                            let _ = v;
                        }
                        Err(e) => return Err(e),
                    }
                }

                if broken {
                    if let Some(ref ib) = if_broken {
                        return self.eval_block(ib);
                    }
                }

                Ok(Value::None)
            }
            Stmt::Match {
                subject,
                subject_alias,
                arms,
                span,
            } => {
                let subj_val = self.eval_expr(subject)?;

                for arm in arms {
                    if self.matches_pattern(&arm.pattern, &subj_val) {
                        let saved_bindings = self.env.borrow().bindings.clone();
                        let saved_binding_order = self.env.borrow().binding_order.clone();
                        let saved_final_bindings = self.env.borrow().final_bindings.clone();
                        if let Some(ref alias) = subject_alias {
                            self.env.borrow_mut().set(alias.clone(), subj_val.clone());
                        }
                        self.bind_match_pattern(&arm.pattern, subj_val.clone(), *span)?;
                        if let Some(guard) = &arm.guard {
                            let guard_value = self.eval_expr(guard)?;
                            if !self.is_truthy(&guard_value) {
                                let mut env = self.env.borrow_mut();
                                env.bindings = saved_bindings;
                                env.binding_order = saved_binding_order;
                                env.final_bindings = saved_final_bindings;
                                continue;
                            }
                        }
                        return self.eval_block(&arm.body);
                    }
                }

                Err(RuntimeError {
                    message: format!("non-exhaustive match: unhandled value {:?}", subj_val),
                    span: *span,
                })
            }
            Stmt::Return { value, .. } => {
                let val = if let Some(ref e) = value {
                    self.eval_expr(e)?
                } else {
                    Value::None
                };
                Ok(Value::Return(Box::new(val)))
            }
            Stmt::Assert {
                condition,
                message,
                span,
            } => {
                let condition_value = self.eval_expr(condition)?;
                if !self.is_truthy(&condition_value) {
                    let detail = if let Some(message) = message {
                        let message_value = self.eval_expr(message)?;
                        let message_value = if matches!(
                            message_value,
                            Value::Function { .. } | Value::BuiltinFunction { .. }
                        ) {
                            self.invoke_value(message_value, Vec::new(), *span)?
                        } else {
                            message_value
                        };
                        match message_value {
                            Value::Str(text) => text,
                            other => format!("{other:?}"),
                        }
                    } else {
                        "assertion failed".to_string()
                    };
                    return Err(RuntimeError {
                        message: detail,
                        span: *span,
                    });
                }
                Ok(Value::None)
            }
            Stmt::Delete { names, span } => {
                for name in names {
                    if !self.env.borrow_mut().delete_local(name) {
                        return Err(RuntimeError {
                            message: format!("cannot delete undefined variable '{name}'"),
                            span: *span,
                        });
                    }
                }
                Ok(Value::None)
            }
            Stmt::Raise { exception, span } => {
                let value = self.eval_expr(exception)?;
                Err(RuntimeError {
                    message: format!("raised invariant ({}): {value:?}", value.type_name()),
                    span: *span,
                })
            }
            Stmt::Yield { value, .. } => {
                // A complete contextmanager scheduler will suspend and resume
                // this frame. The tree-walk backend has no scheduler yet, but it
                // must still evaluate yield expressions for their side effects.
                let _ = self.eval_expr(value)?;
                Ok(Value::None)
            }
            Stmt::Break(span) => {
                if self.loop_depth == 0 {
                    return Err(RuntimeError {
                        message: "break is only valid inside a loop".into(),
                        span: *span,
                    });
                }
                Ok(Value::Sentinel("__break__".to_string()))
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    return Err(RuntimeError {
                        message: "continue is only valid inside a loop".into(),
                        span: *span,
                    });
                }
                Ok(Value::Sentinel("__continue__".to_string()))
            }
            Stmt::With { items, body, .. } => {
                let mut teardowns: Vec<Teardown> = Vec::new();
                for item in items {
                    let ctx_val = match self.eval_expr(&item.context_expr) {
                        Ok(value) => value,
                        Err(error) => {
                            // Contexts acquired before a later setup failure still
                            // unwind in reverse order.
                            let mut result = Err(error);
                            for (teardown, _, teardown_env) in teardowns.into_iter().rev() {
                                let previous = Rc::clone(&self.env);
                                self.env = teardown_env;
                                if let Err(cleanup_error) = self.eval_block(&teardown) {
                                    result = Err(cleanup_error);
                                }
                                self.env = previous;
                            }
                            return result;
                        }
                    };
                    let managed_value = match ctx_val {
                        Value::ContextManager {
                            value,
                            teardown,
                            error_teardown,
                            env,
                        } => {
                            teardowns.push((teardown, error_teardown, env));
                            *value
                        }
                        object @ Value::Object { .. } => {
                            let cm = match &object {
                                Value::Object { fields, .. } => {
                                    fields.borrow().get("__cm__").cloned()
                                }
                                _ => None,
                            };
                            if let Some(cm) = cm {
                                let managed_result = self.invoke_value(
                                    cm,
                                    vec![(None, object.clone())],
                                    item.context_expr.span(),
                                );
                                let managed = match managed_result {
                                    Ok(value) => value,
                                    Err(error) => {
                                        let mut result = Err(error);
                                        for (teardown, _, teardown_env) in
                                            teardowns.into_iter().rev()
                                        {
                                            let previous = Rc::clone(&self.env);
                                            self.env = teardown_env;
                                            if let Err(cleanup_error) = self.eval_block(&teardown) {
                                                result = Err(cleanup_error);
                                            }
                                            self.env = previous;
                                        }
                                        return result;
                                    }
                                };
                                match managed {
                                    Value::ContextManager {
                                        value,
                                        teardown,
                                        error_teardown,
                                        env,
                                    } => {
                                        teardowns.push((teardown, error_teardown, env));
                                        *value
                                    }
                                    other => other,
                                }
                            } else {
                                object
                            }
                        }
                        other => other,
                    };
                    if let Some(ref target) = item.target {
                        self.bind_pattern(target, managed_value, item.context_expr.span())?;
                    }
                }
                let body_result = self.eval_block(body);
                let body_failed = body_result.is_err();
                let mut result = body_result;
                for (teardown, error_teardown, teardown_env) in teardowns.into_iter().rev() {
                    let previous = Rc::clone(&self.env);
                    self.env = teardown_env;
                    let cleanup = if body_failed {
                        error_teardown.as_ref().unwrap_or(&teardown)
                    } else {
                        &teardown
                    };
                    let cleanup_result = self.eval_block(cleanup);
                    self.env = previous;
                    if cleanup_result.is_err() {
                        result = cleanup_result;
                    } else if body_failed && error_teardown.is_some() {
                        result = Ok(Value::None);
                    }
                }
                result
            }
            Stmt::Try {
                body,
                handlers,
                finally_body,
                ..
            } => {
                let body_result = self.eval_block(body);
                let mut result = match body_result {
                    Ok(value) => Ok(value),
                    Err(error) => {
                        if let Some(handler) = handlers.iter().find(|handler| {
                            self.error_matches_handler(&error, &handler.exception_type)
                        }) {
                            if let Some(name) = &handler.name {
                                self.env
                                    .borrow_mut()
                                    .set(name.clone(), Value::Str(error.message.clone()));
                            }
                            self.eval_block(&handler.body)
                        } else {
                            Err(error)
                        }
                    }
                };
                if let Some(finally_body) = finally_body {
                    let cleanup = self.eval_block(finally_body);
                    if cleanup.is_err() || matches!(cleanup, Ok(Value::Return(_))) {
                        result = cleanup;
                    }
                }
                result
            }
            Stmt::Import {
                module,
                alias,
                span,
            } => {
                let mod_env = self.load_module(module, *span)?;
                let bound_name = alias
                    .clone()
                    .unwrap_or_else(|| module.rsplit('.').next().unwrap_or(module).to_string());
                let mod_val = Value::Module {
                    name: bound_name.clone(),
                    path: module.clone(),
                    env: mod_env,
                };
                self.env.borrow_mut().set(bound_name, mod_val);
                Ok(Value::None)
            }
            Stmt::FromImport {
                module,
                names,
                span,
                ..
            } => {
                let mod_env = self.load_module(module, *span)?;
                for (name, alias) in names {
                    if name.starts_with('_') {
                        return Err(RuntimeError {
                            message: format!(
                                "cannot import private name '{name}' from module '{module}'"
                            ),
                            span: *span,
                        });
                    }
                    let val = mod_env.borrow().get(name).ok_or_else(|| RuntimeError {
                        message: format!("cannot import name '{name}' from module '{module}'"),
                        span: *span,
                    })?;
                    let bound_name = alias.as_ref().unwrap_or(name).clone();
                    self.env.borrow_mut().set(bound_name, val);
                }
                Ok(Value::None)
            }
            Stmt::Pass(_) => Ok(Value::None),
            Stmt::Expr(expr) => self.eval_expr(expr),
        }
    }

    fn eval_block(&mut self, stmts: &[Stmt]) -> Result<Value, RuntimeError> {
        let mut last_val = Value::None;
        for s in stmts {
            last_val = self.eval_statement(s)?;
            if matches!(last_val, Value::Sentinel(ref s) if s == "__break__" || s == "__continue__")
            {
                return Ok(last_val);
            }
            if let Value::Return(_) = last_val {
                return Ok(last_val);
            }
        }
        Ok(last_val)
    }

    fn materialize_iterable(
        &mut self,
        value: Value,
        span: Span,
    ) -> Result<Vec<Value>, RuntimeError> {
        match value {
            Value::List(values) => Ok(values.borrow().clone()),
            Value::MemoryView {
                data,
                start,
                len,
                stride,
                ..
            } => {
                let items = data.borrow();
                Ok((0..len)
                    .map(|offset| items[(start + offset * stride) as usize].clone())
                    .collect())
            }
            Value::Set(values) => Ok(values.borrow().clone()),
            Value::Dict(values) => Ok(values.borrow().keys().cloned().map(Value::Str).collect()),
            Value::Bytes(bytes) => Ok(bytes
                .into_iter()
                .map(|byte| Value::Int(byte as i64))
                .collect()),
            Value::Range { start, stop, step } => Ok(materialize_range(start, stop, step)),
            Value::Object {
                class_name,
                fields,
                is_frozen,
            } => {
                let object = Value::Object {
                    class_name,
                    fields: fields.clone(),
                    is_frozen,
                };
                let iterator = fields.borrow().get("__iter__").cloned();
                let iter_value = if let Some(iterator) = iterator {
                    self.invoke_value(iterator, vec![(None, object.clone())], span)?
                } else {
                    object
                };
                let receiver = iter_value.clone();
                match iter_value {
                    Value::Object { fields, .. } => {
                        let next =
                            fields
                                .borrow()
                                .get("next")
                                .cloned()
                                .ok_or_else(|| RuntimeError {
                                    message: "__iter__ returned non-iterator".into(),
                                    span,
                                })?;
                        let mut values = Vec::new();
                        loop {
                            let item = self.invoke_value(
                                next.clone(),
                                vec![(None, receiver.clone())],
                                span,
                            )?;
                            if matches!(item, Value::Sentinel(ref name) if name == "iteration.done")
                            {
                                break;
                            }
                            values.push(item);
                        }
                        Ok(values)
                    }
                    other => self.materialize_iterable(other, span),
                }
            }
            other => Err(RuntimeError {
                message: format!("{} is not iterable", other.type_name()),
                span,
            }),
        }
    }

    #[allow(clippy::needless_return)]
    pub fn eval_expr(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
        match expr {
            Expr::Literal { value, .. } => Ok(match value {
                LiteralValue::Int(n) => Value::Int(*n),
                LiteralValue::BigInt(n) => {
                    Value::BigInt(parse_bigint_literal(n).unwrap_or_else(BigInt::zero))
                }
                LiteralValue::Float(f) => Value::Float(*f),
                LiteralValue::Complex(imag) => Value::Complex(0.0, *imag),
                LiteralValue::Bool(b) => Value::Bool(*b),
                LiteralValue::Str(s) => Value::Str(s.clone()),
                LiteralValue::Bytes(bytes) => Value::Bytes(bytes.clone()),
                LiteralValue::None => Value::None,
                LiteralValue::Sentinel(s) => Value::Sentinel(s.clone()),
                LiteralValue::Ellipsis => Value::None,
            }),
            Expr::Ident { name, span } => self.env.borrow().get(name).ok_or_else(|| RuntimeError {
                message: format!("undefined variable '{name}'"),
                span: *span,
            }),
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => {
                let lval = self.eval_expr(left)?;
                if matches!(op, BinaryOp::Is | BinaryOp::IsNot) {
                    let type_name = match &**right {
                        Expr::Type(TypeExpr::Named { name, .. }) | Expr::Ident { name, .. } => {
                            Some(name.clone())
                        }
                        _ => None,
                    };
                    if let Some(name) = type_name {
                        let is_type_operand = matches!(
                            name.as_str(),
                            "int"
                                | "float"
                                | "complex"
                                | "bool"
                                | "str"
                                | "none"
                                | "None"
                                | "class"
                                | "trait"
                                | "interface"
                                | "Callable"
                                | "Eq"
                                | "Ord"
                                | "Hashable"
                                | "Sized"
                                | "Iterable"
                                | "Iterator"
                                | "Reversible"
                                | "Set"
                                | "Container"
                                | "Collection"
                                | "Sequence"
                                | "Buffer"
                                | "Shape"
                        ) || self.classes.contains_key(&name);
                        if !is_type_operand {
                            let rval = self.eval_expr(right)?;
                            return Ok(Value::Bool(if matches!(op, BinaryOp::Is) {
                                lval == rval
                            } else {
                                lval != rval
                            }));
                        }
                        let matched = match name.as_str() {
                            "class" => !matches!(lval, Value::None),
                            "trait" | "interface" => false,
                            "Callable" => matches!(
                                lval,
                                Value::Function { .. }
                                    | Value::BuiltinFunction { .. }
                                    | Value::Partial { .. }
                            ),
                            "Sized" => {
                                matches!(
                                    lval,
                                    Value::Str(_)
                                        | Value::Bytes(_)
                                        | Value::DottedPath(_)
                                        | Value::List(_)
                                        | Value::Set(_)
                                        | Value::Dict(_)
                                        | Value::Range { .. }
                                ) || matches!(&lval, Value::Object { class_name, .. } if self.class_has_capability(class_name, "Sized"))
                            }
                            "Container" => {
                                matches!(
                                    lval,
                                    Value::Str(_)
                                        | Value::Bytes(_)
                                        | Value::DottedPath(_)
                                        | Value::List(_)
                                        | Value::Set(_)
                                        | Value::Dict(_)
                                        | Value::Range { .. }
                                ) || matches!(&lval, Value::Object { class_name, .. } if self.class_has_capability(class_name, "Container"))
                            }
                            "Iterable" | "Collection" => {
                                matches!(
                                    lval,
                                    Value::Bytes(_)
                                        | Value::MemoryView { .. }
                                        | Value::List(_)
                                        | Value::Set(_)
                                        | Value::Dict(_)
                                        | Value::Range { .. }
                                ) || matches!(&lval, Value::Object { class_name, .. } if self.class_has_capability(class_name, &name))
                            }
                            "Sequence" | "Reversible" => {
                                matches!(
                                    lval,
                                    Value::Bytes(_)
                                        | Value::MemoryView { .. }
                                        | Value::List(_)
                                        | Value::Range { .. }
                                ) || matches!(&lval, Value::Object { class_name, .. } if self.class_has_capability(class_name, &name))
                            }
                            "Set" => {
                                matches!(lval, Value::Set(_))
                                    || matches!(&lval, Value::Object { class_name, .. } if self.class_has_capability(class_name, "Set"))
                            }
                            "Buffer" => match &lval {
                                Value::Bytes(_) | Value::MemoryView { .. } | Value::List(_) => true,
                                Value::Object { fields, .. } => {
                                    fields.borrow().contains_key("__buffer__")
                                }
                                _ => false,
                            },
                            "Shape" => {
                                matches!(lval, Value::List(_))
                                    || matches!(&lval, Value::Object { class_name, .. } if self.class_has_capability(class_name, "Shape"))
                            }
                            "Eq" | "Ord" | "Hashable" => match &lval {
                                Value::None => false,
                                Value::Object { class_name, .. } => {
                                    self.class_has_capability(class_name, &name)
                                }
                                _ => true,
                            },
                            _ => self.matches_pattern(
                                &Pattern::Type(
                                    TypeExpr::Named {
                                        name,
                                        args: Vec::new(),
                                        span: *span,
                                    },
                                    *span,
                                ),
                                &lval,
                            ),
                        };
                        return Ok(Value::Bool(if matches!(op, BinaryOp::Is) {
                            matched
                        } else {
                            !matched
                        }));
                    }
                }
                // `and` and `or` return an operand, and must not evaluate
                // their right-hand side when the left-hand truth value already
                // determines the result.
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    let left_truthy = self.is_truthy(&lval);
                    if matches!(op, BinaryOp::And) {
                        return if left_truthy {
                            self.eval_expr(right)
                        } else {
                            Ok(lval)
                        };
                    }
                    return if left_truthy {
                        Ok(lval)
                    } else {
                        self.eval_expr(right)
                    };
                }
                let rval = self.eval_expr(right)?;
                self.eval_binary_op(op, lval, rval, span)
            }
            Expr::Unary { op, expr, span } => {
                let val = self.eval_expr(expr)?;
                match op {
                    UnaryOp::Neg => match val {
                        Value::Int(n) => Ok(Value::Int(match n {
                            INT_POS_INF => INT_NEG_INF,
                            INT_NEG_INF => INT_POS_INF,
                            value => -value,
                        })),
                        Value::BigInt(n) => Ok(Value::BigInt(-n)),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        Value::Complex(real, imag) => Ok(Value::Complex(-real, -imag)),
                        _ => Err(RuntimeError {
                            message: "unsupported operand for unary -".to_string(),
                            span: *span,
                        }),
                    },
                    UnaryOp::Pos => match val {
                        Value::Int(n) => Ok(Value::Int(n)),
                        Value::BigInt(n) => Ok(Value::BigInt(n)),
                        Value::Float(f) => Ok(Value::Float(f)),
                        Value::Complex(_, _) => Ok(val),
                        _ => Err(RuntimeError {
                            message: "unsupported operand for unary +".to_string(),
                            span: *span,
                        }),
                    },
                    UnaryOp::Not => Ok(Value::Bool(!self.is_truthy(&val))),
                    UnaryOp::Invert => match val {
                        Value::Int(n) => Ok(Value::Int(!n)),
                        // Two's-complement inversion is defined for arbitrary
                        // precision integers by the identity `~n == -n - 1`.
                        Value::BigInt(n) => Ok(Value::BigInt(-n - BigInt::one())),
                        _ => Err(RuntimeError {
                            message: "unsupported operand for ~".to_string(),
                            span: *span,
                        }),
                    },
                    UnaryOp::Spread | UnaryOp::GatherSpread => Ok(val),
                }
            }
            Expr::Call { func, args, span } => {
                // `Class.replace(instance, field=value, ...)` is a generated
                // factory whose keyword names must survive the call boundary.
                if let Expr::Attribute { value, attr, .. } = &**func {
                    if let Expr::Ident { name, .. } = &**value {
                        if name == "str" && matches!(attr.as_str(), "bin" | "oct" | "hex") {
                            if args.len() != 1 {
                                return Err(RuntimeError {
                                    message: format!("str.{attr}() takes exactly one argument"),
                                    span: *span,
                                });
                            }
                            let value = self.eval_expr(&args[0].value)?;
                            let Value::Int(value) = value else {
                                return Err(RuntimeError {
                                    message: format!("str.{attr}() argument must be int"),
                                    span: args[0].span,
                                });
                            };
                            let rendered = match attr.as_str() {
                                "bin" => format!("0b{value:b}"),
                                "oct" => format!("0o{value:o}"),
                                _ => format!("0x{value:x}"),
                            };
                            return Ok(Value::Str(rendered));
                        }
                    }
                    if attr == "replace" {
                        if let Expr::Ident {
                            name: class_name, ..
                        } = &**value
                        {
                            if self.classes.contains_key(class_name) {
                                let Some(first) = args.first() else {
                                    return Err(RuntimeError {
                                        message: "replace() requires an instance".into(),
                                        span: *span,
                                    });
                                };
                                let instance = self.eval_expr(&first.value)?;
                                let Value::Object {
                                    class_name: actual,
                                    fields,
                                    ..
                                } = instance
                                else {
                                    return Err(RuntimeError {
                                        message: "replace() requires an instance".into(),
                                        span: *span,
                                    });
                                };
                                if actual != *class_name {
                                    return Err(RuntimeError {
                                        message: format!(
                                            "replace() requires an exact {class_name} instance"
                                        ),
                                        span: *span,
                                    });
                                }
                                let mut names = Vec::new();
                                let mut methods = HashMap::new();
                                self.collect_inherited_members(
                                    class_name,
                                    &mut names,
                                    &mut methods,
                                );
                                let mut updated = fields.borrow().clone();
                                for arg in args.iter().skip(1) {
                                    let Some(name) = &arg.name else {
                                        return Err(RuntimeError {
                                            message: "replace() fields must be passed by keyword"
                                                .into(),
                                            span: arg.span,
                                        });
                                    };
                                    if !names.iter().any(|field| field == name) {
                                        return Err(RuntimeError {
                                            message: format!(
                                                "replace() got unknown field '{name}'"
                                            ),
                                            span: arg.span,
                                        });
                                    }
                                    updated.insert(name.clone(), self.eval_expr(&arg.value)?);
                                }
                                return Ok(Value::Object {
                                    class_name: class_name.clone(),
                                    fields: Rc::new(RefCell::new(updated)),
                                    is_frozen: Rc::new(RefCell::new(false)),
                                });
                            }
                        }
                    }
                }
                // These two calls are compiler intrinsics. Resolve them at
                // the call expression so nested calls still see their own
                // source location and assignment target.
                if let Expr::Attribute { value, attr, .. } = &**func {
                    if let Expr::Ident { name, .. } = &**value {
                        if name == "SourceLocation" && attr == "caller" {
                            let mut fields = HashMap::new();
                            fields.insert(
                                "file".into(),
                                Value::Str(
                                    self.current_file
                                        .as_ref()
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_default(),
                                ),
                            );
                            fields.insert("line".into(), Value::Int(span.line as i64));
                            fields.insert("column".into(), Value::Int(span.column as i64));
                            return Ok(Value::Object {
                                class_name: "SourceLocation".into(),
                                fields: Rc::new(RefCell::new(fields)),
                                is_frozen: Rc::new(RefCell::new(true)),
                            });
                        }
                        if name == "VarName" && attr == "from_assignment" {
                            let captured = self.capture_assignment.clone().filter(|s| !s.is_empty()).ok_or_else(|| RuntimeError {
                                message: "VarName.from_assignment() requires a simple assignment target".into(),
                                span: *span,
                            })?;
                            return Ok(Value::Str(captured));
                        }
                    }
                }
                if args.iter().any(|arg| {
                    !arg.is_spread
                        && !arg.is_dict_spread
                        && matches!(arg.value, Expr::Ident { ref name, .. } if name == "_")
                }) {
                    let partial_func = self.eval_expr(func)?;
                    let mut partial_args = Vec::new();
                    for arg in args {
                        if matches!(&arg.value, Expr::Ident { name, .. } if name == "_") {
                            partial_args.push((arg.name.clone(), None));
                        } else {
                            partial_args
                                .push((arg.name.clone(), Some(self.eval_expr(&arg.value)?)));
                        }
                    }
                    return Ok(Value::Partial {
                        func: Box::new(partial_func),
                        args: partial_args,
                    });
                }
                let func_val = self.eval_expr(func)?;
                let mut evaluated_args: Vec<(Option<String>, Value)> = Vec::new();
                for arg in args {
                    if arg.is_gather_spread {
                        let val = self.eval_expr(&arg.value)?;
                        match val {
                            Value::Object {
                                class_name, fields, ..
                            } => {
                                let fields = fields.borrow();
                                let is_bundle =
                                    fields.contains_key("vpargs") || fields.contains_key("kwargs");
                                if let Some(Value::List(values)) = fields.get("pargs") {
                                    evaluated_args.extend(
                                        values.borrow().iter().cloned().map(|value| (None, value)),
                                    );
                                }
                                if let Some(Value::List(values)) = fields.get("vpargs") {
                                    evaluated_args.extend(
                                        values.borrow().iter().cloned().map(|value| (None, value)),
                                    );
                                }
                                if let Some(Value::Dict(values)) = fields.get("kwargs") {
                                    evaluated_args.extend(
                                        values
                                            .borrow()
                                            .iter()
                                            .map(|(key, value)| (Some(key.clone()), value.clone())),
                                    );
                                }
                                if !is_bundle {
                                    // Ordinary classes use their generated
                                    // __spread__: one argument per declared field.
                                    let mut names = Vec::new();
                                    let mut methods = HashMap::new();
                                    self.collect_inherited_members(
                                        &class_name,
                                        &mut names,
                                        &mut methods,
                                    );
                                    for name in names {
                                        if let Some(value) = fields.get(&name) {
                                            evaluated_args.push((None, value.clone()));
                                        }
                                    }
                                }
                            }
                            other => {
                                return Err(RuntimeError {
                                    message: format!("*** spread requires an Arguments or Parameters value, got {}", other.type_name()),
                                    span: arg.span,
                                });
                            }
                        }
                    } else if arg.is_spread {
                        let val = self.eval_expr(&arg.value)?;
                        if arg.is_dict_spread {
                            match val {
                                Value::Dict(d) => {
                                    for (key, value) in d.borrow().iter() {
                                        evaluated_args.push((Some(key.clone()), value.clone()));
                                    }
                                }
                                other => {
                                    return Err(RuntimeError {
                                        message: format!(
                                            "** spread requires a dict, got {}",
                                            other.type_name()
                                        ),
                                        span: arg.span,
                                    })
                                }
                            }
                        } else {
                            match val {
                                Value::List(l) => evaluated_args
                                    .extend(l.borrow().iter().cloned().map(|value| (None, value))),
                                other => {
                                    return Err(RuntimeError {
                                        message: format!(
                                            "* spread requires a list, got {}",
                                            other.type_name()
                                        ),
                                        span: arg.span,
                                    })
                                }
                            }
                        }
                    } else {
                        evaluated_args.push((arg.name.clone(), self.eval_expr(&arg.value)?));
                    }
                }
                let positional_args: Vec<Value> = evaluated_args
                    .iter()
                    .map(|(_, value)| value.clone())
                    .collect();

                match func_val {
                    Value::Partial {
                        func,
                        args: partial_args,
                    } => {
                        let mut supplied = evaluated_args;
                        let mut next = supplied.drain(..);
                        let mut merged: Vec<(Option<String>, Value)> = Vec::new();
                        let mut partial_slots: Vec<(Option<String>, Option<Value>)> = Vec::new();
                        let mut has_holes = false;
                        for (name, value) in partial_args {
                            if let Some(value) = value {
                                partial_slots.push((name.clone(), Some(value.clone())));
                                merged.push((name, value));
                            } else if let Some(value) = next.next() {
                                partial_slots.push((value.0.clone(), Some(value.1.clone())));
                                merged.push(value);
                            } else {
                                has_holes = true;
                                partial_slots.push((name, None));
                            }
                        }
                        if has_holes {
                            return Ok(Value::Partial {
                                func,
                                args: partial_slots
                                    .into_iter()
                                    .chain(next.map(|(name, value)| (name, Some(value))))
                                    .collect(),
                            });
                        }
                        merged.extend(next);
                        self.invoke_value(*func, merged, *span)
                    }
                    Value::ClassRef(class_name) => {
                        self.call_class(&class_name, &positional_args, *span)
                    }
                    Value::BuiltinFunction { func, .. } => (func)(&positional_args, self),
                    Value::Function {
                        name,
                        params,
                        body,
                        closure,
                        is_contextmanager,
                        is_async,
                        ..
                    } => {
                        if is_contextmanager {
                            return self.call_contextmanager_body(
                                &params,
                                &body,
                                &positional_args,
                                closure,
                            );
                        }
                        if is_async {
                            let function = Value::Function {
                                name,
                                params,
                                body,
                                closure,
                                is_contextmanager,
                                is_async: false,
                            };
                            return Ok(Value::Future(Rc::new(RefCell::new(Some(FutureTask {
                                function,
                                args: evaluated_args,
                            })))));
                        }
                        let bound_args = self.bind_arguments(&params, &evaluated_args)?;
                        let call_env = Rc::new(RefCell::new(Environment::with_parent(closure)));
                        for (param, val) in params.iter().zip(bound_args) {
                            call_env.borrow_mut().set(param.name.clone(), val);
                        }
                        let prev_env = Rc::clone(&self.env);
                        let prev_loop_depth = self.loop_depth;
                        self.env = call_env;
                        self.loop_depth = 0;
                        let res = self.eval_block(&body);
                        self.env = prev_env;
                        self.loop_depth = prev_loop_depth;
                        match res {
                            Ok(Value::Return(v)) => Ok(*v),
                            other => other,
                        }
                    }
                    _ => Err(RuntimeError {
                        message: "value is not callable".to_string(),
                        span: *span,
                    }),
                }
            }
            Expr::Propagate { expr, .. } => {
                let val = self.eval_expr(expr)?;
                if let Value::Return(_) = val {
                    return Ok(val);
                }
                // If value is an Error object or error string, return early out of enclosing function; else unwrap
                if let Value::Object { ref class_name, .. } = val {
                    if class_name.ends_with("Error") {
                        return Ok(Value::Return(Box::new(val)));
                    }
                }
                if let Value::Str(ref s) = val {
                    if s.starts_with("error:") {
                        return Ok(Value::Return(Box::new(val)));
                    }
                }
                Ok(val)
            }
            Expr::Await { expr, span } => {
                let value = self.eval_expr(expr)?;
                match value {
                    Value::Future(task) => {
                        let task = task.borrow_mut().take().ok_or_else(|| RuntimeError {
                            message: "future has already been awaited".into(),
                            span: *span,
                        })?;
                        self.invoke_value(task.function, task.args, *span)
                    }
                    other => Ok(other),
                }
            }
            Expr::Attribute { value, attr, span } => {
                if let Expr::Ident { name, .. } = &**value {
                    match (name.as_str(), attr.as_str()) {
                        ("float", "inf") => return Ok(Value::Float(f64::INFINITY)),
                        ("float", "nan") => return Ok(Value::Float(f64::NAN)),
                        ("int", "inf") => return Ok(Value::Int(INT_POS_INF)),
                        ("int", "nan") => return Ok(Value::Int(INT_NAN)),
                        ("complex", "nan") => return Ok(Value::Complex(f64::NAN, f64::NAN)),
                        _ => {}
                    }
                }
                if matches!(&**value, Expr::Ident { name, .. } if name == "super") {
                    let current = match self.env.borrow().get("__current_class") {
                        Some(Value::Str(name)) => name,
                        _ => {
                            return Err(RuntimeError {
                                message: "super used outside a class method".into(),
                                span: *span,
                            })
                        }
                    };
                    let receiver = self
                        .env
                        .borrow()
                        .get("self")
                        .or_else(|| self.env.borrow().get("cls"))
                        .ok_or_else(|| RuntimeError {
                            message: "super used without self".into(),
                            span: *span,
                        })?;
                    let parent = self
                        .classes
                        .get(&current)
                        .and_then(|class_def| {
                            class_def.bases.iter().find_map(|base| match base {
                                TypeExpr::Named { name, .. } if self.classes.contains_key(name) => {
                                    Some(name.clone())
                                }
                                _ => None,
                            })
                        })
                        .ok_or_else(|| RuntimeError {
                            message: format!("class '{current}' has no parent"),
                            span: *span,
                        })?;
                    let parent_member = self
                        .classes
                        .get(&parent)
                        .and_then(|class_def| {
                            class_def.body.iter().find_map(|member| match member {
                                ClassMember::Method(method) if method.name == *attr => {
                                    Some((false, method.clone()))
                                }
                                ClassMember::ClassMethod(method) if method.name == *attr => {
                                    Some((true, method.clone()))
                                }
                                _ => None,
                            })
                        })
                        .ok_or_else(|| RuntimeError {
                            message: format!("parent class '{parent}' has no method '{attr}'"),
                            span: *span,
                        })?;
                    let (is_classmethod, method) = parent_member;
                    return Ok(Value::BuiltinFunction {
                        name: format!("super.{attr}"),
                        func: Rc::new(move |args, interp| {
                            let mut call_args = vec![receiver.clone()];
                            call_args.extend_from_slice(args);
                            if is_classmethod {
                                interp.call_class_method(&parent, &method, args)
                            } else {
                                interp.call_function(&method, &call_args, Rc::clone(&interp.env))
                            }
                        }),
                    });
                }
                let obj = self.eval_expr(value)?;
                match obj {
                    Value::Function { name, .. } => match attr.as_str() {
                        "__name__" => Ok(Value::Str(name)),
                        "__doc__" => Ok(Value::None),
                        "__path__" => Ok(dotted_path(&name)),
                        _ => Err(RuntimeError {
                            message: format!("function has no attribute '{attr}'"),
                            span: *span,
                        }),
                    },
                    Value::ClassRef(class_name) => {
                        match attr.as_str() {
                            "__name__" => return Ok(Value::Str(class_name)),
                            "__doc__" => return Ok(Value::None),
                            "__path__" => return Ok(dotted_path(&class_name)),
                            _ => {}
                        }
                        self.check_private_access(&class_name, attr, *span)?;
                        if let Some(value) = self.class_var_get(&class_name, attr) {
                            return Ok(value);
                        }
                        if !self.classes.contains_key(&class_name) {
                            return Err(RuntimeError {
                                message: format!("class '{class_name}' not found"),
                                span: *span,
                            });
                        }
                        if attr == "replace" {
                            let target_class = class_name.clone();
                            return Ok(Value::BuiltinFunction {
                                name: format!("{target_class}.replace"),
                                func: Rc::new(move |args, interp| {
                                    let Some(Value::Object {
                                        class_name: actual,
                                        fields,
                                        ..
                                    }) = args.first()
                                    else {
                                        return Err(RuntimeError { message: "replace() requires an instance as its first argument".into(), span: Span::default() });
                                    };
                                    if actual != &target_class {
                                        return Err(RuntimeError { message: format!("replace() requires an exact {target_class} instance"), span: Span::default() });
                                    }
                                    let mut names = Vec::new();
                                    let mut methods = HashMap::new();
                                    interp.collect_inherited_members(
                                        &target_class,
                                        &mut names,
                                        &mut methods,
                                    );
                                    let mut updated = fields.borrow().clone();
                                    for (index, value) in args.iter().skip(1).enumerate() {
                                        let Some(name) = names.get(index) else {
                                            return Err(RuntimeError {
                                                message: "replace() got too many field values"
                                                    .into(),
                                                span: Span::default(),
                                            });
                                        };
                                        if !names.iter().any(|field| field == name) {
                                            return Err(RuntimeError {
                                                message: format!(
                                                    "replace() got unknown field '{name}'"
                                                ),
                                                span: Span::default(),
                                            });
                                        }
                                        updated.insert(name.clone(), value.clone());
                                    }
                                    Ok(Value::Object {
                                        class_name: target_class.clone(),
                                        fields: Rc::new(RefCell::new(updated)),
                                        is_frozen: Rc::new(RefCell::new(false)),
                                    })
                                }),
                            });
                        }
                        for member in self.class_members_for_lookup(&class_name) {
                            match member {
                                ClassMember::Factory(factory) if factory.name == *attr => {
                                    return Ok(Value::BuiltinFunction {
                                        name: format!("{class_name}.{attr}"),
                                        func: Rc::new(move |args, interp| {
                                            interp.call_factory(&class_name, &factory, args)
                                        }),
                                    });
                                }
                                ClassMember::ClassMethod(method) if method.name == *attr => {
                                    return Ok(Value::BuiltinFunction {
                                        name: format!("{class_name}.{attr}"),
                                        func: Rc::new(move |args, interp| {
                                            interp.call_class_method(&class_name, &method, args)
                                        }),
                                    });
                                }
                                _ => {}
                            }
                        }
                        Err(RuntimeError {
                            message: format!("class '{class_name}' has no attribute '{attr}'"),
                            span: *span,
                        })
                    }
                    Value::TraitRef(trait_name) => match attr.as_str() {
                        "__name__" => Ok(Value::Str(trait_name)),
                        "__doc__" => Ok(Value::None),
                        "__path__" => Ok(dotted_path(&trait_name)),
                        _ => Err(RuntimeError {
                            message: format!("trait '{trait_name}' has no attribute '{attr}'"),
                            span: *span,
                        }),
                    },
                    Value::Object {
                        fields,
                        is_frozen,
                        class_name,
                    } => {
                        self.check_private_access(&class_name, attr, *span)?;
                        if let Some(val) = fields.borrow().get(attr).cloned() {
                            let fallback = val.clone();
                            if let Value::Function {
                                name,
                                params,
                                body,
                                closure,
                                is_contextmanager,
                                ..
                            } = val
                            {
                                if !params.is_empty() && params[0].name == "self" {
                                    let bound_self = Value::Object {
                                        class_name: class_name.clone(),
                                        fields: Rc::clone(&fields),
                                        is_frozen: Rc::clone(&is_frozen),
                                    };
                                    let p = params.clone();
                                    let b = body.clone();
                                    let c = Rc::clone(&closure);
                                    if name.starts_with("__getter__") {
                                        let mut call_args = vec![bound_self];
                                        let call_env =
                                            Rc::new(RefCell::new(Environment::with_parent(c)));
                                        for (param, arg_val) in
                                            params.iter().zip(call_args.drain(..))
                                        {
                                            call_env.borrow_mut().set(param.name.clone(), arg_val);
                                        }
                                        let previous = Rc::clone(&self.env);
                                        self.env = call_env;
                                        let result = self.eval_block(&body);
                                        self.env = previous;
                                        return match result {
                                            Ok(Value::Return(value)) => Ok(*value),
                                            Ok(value) => Ok(value),
                                            Err(error) => Err(error),
                                        };
                                    }
                                    return Ok(Value::BuiltinFunction {
                                        name: name.clone(),
                                        func: Rc::new(move |args, interp| {
                                            let mut call_args = vec![bound_self.clone()];
                                            call_args.extend_from_slice(args);
                                            if is_contextmanager {
                                                return interp.call_contextmanager_body(
                                                    &p,
                                                    &b,
                                                    &call_args,
                                                    Rc::clone(&c),
                                                );
                                            }
                                            let call_env = Rc::new(RefCell::new(
                                                Environment::with_parent(Rc::clone(&c)),
                                            ));
                                            call_env.borrow_mut().set(
                                                "__current_class".into(),
                                                Value::Str(class_name.clone()),
                                            );
                                            for (param, arg_val) in p.iter().zip(call_args) {
                                                call_env
                                                    .borrow_mut()
                                                    .set(param.name.clone(), arg_val);
                                            }
                                            let prev = Rc::clone(&interp.env);
                                            interp.env = call_env;
                                            let res = interp.eval_block(&b);
                                            interp.env = prev;
                                            match res {
                                                Ok(Value::Return(v)) => Ok(*v),
                                                other => other,
                                            }
                                        }),
                                    });
                                }
                            }
                            Ok(fallback)
                        } else if let Some(value) = self.class_var_get(&class_name, attr) {
                            Ok(value)
                        } else {
                            Err(RuntimeError {
                                message: format!("object has no attribute '{attr}'"),
                                span: *span,
                            })
                        }
                    }
                    Value::Record(fields) => {
                        if let Some(val) = fields.borrow().get(attr) {
                            Ok(val.clone())
                        } else {
                            Err(RuntimeError {
                                message: format!("record has no field '{attr}'"),
                                span: *span,
                            })
                        }
                    }
                    Value::Module {
                        name: mod_name,
                        path: mod_path,
                        env,
                    } => {
                        match attr.as_str() {
                            "__name__" => return Ok(Value::Str(mod_name)),
                            "__doc__" => return Ok(Value::None),
                            "__path__" => return Ok(dotted_path(&mod_path)),
                            _ => {}
                        }
                        if attr.starts_with('_') {
                            return Err(RuntimeError {
                                message: format!(
                                    "module '{mod_name}' has private attribute '{attr}'"
                                ),
                                span: *span,
                            });
                        }
                        if let Some(val) = env.borrow().get(attr) {
                            Ok(val)
                        } else {
                            Err(RuntimeError {
                                message: format!("module '{mod_name}' has no attribute '{attr}'"),
                                span: *span,
                            })
                        }
                    }
                    Value::BuiltinFunction {
                        name: builtin_name, ..
                    } if builtin_name == "str"
                        && matches!(attr.as_str(), "bin" | "oct" | "hex") =>
                    {
                        let factory_name = attr.clone();
                        Ok(Value::BuiltinFunction {
                            name: format!("str.{factory_name}"),
                            func: Rc::new(move |args, _interp| {
                                if args.len() != 1 {
                                    return Err(RuntimeError {
                                        message: format!(
                                            "str.{factory_name}() takes exactly one argument"
                                        ),
                                        span: Span::default(),
                                    });
                                }
                                let Value::Int(value) = args[0] else {
                                    return Err(RuntimeError {
                                        message: format!(
                                            "str.{factory_name}() argument must be int"
                                        ),
                                        span: Span::default(),
                                    });
                                };
                                let rendered = match factory_name.as_str() {
                                    "bin" => format!("0b{value:b}"),
                                    "oct" => format!("0o{value:o}"),
                                    _ => format!("0x{value:x}"),
                                };
                                Ok(Value::Str(rendered))
                            }),
                        })
                    }
                    Value::BuiltinFunction { name, .. } => match attr.as_str() {
                        "__name__" => Ok(Value::Str(name)),
                        "__doc__" => Ok(Value::None),
                        "__path__" => Ok(dotted_path(&name)),
                        _ => Err(RuntimeError {
                            message: format!("function has no attribute '{attr}'"),
                            span: *span,
                        }),
                    },
                    Value::Complex(real, _) if attr == "real" => Ok(Value::Float(real)),
                    Value::Complex(_, imag) if attr == "imag" => Ok(Value::Float(imag)),
                    Value::Str(s) => {
                        if attr == "chars" {
                            let chars = Value::List(Rc::new(RefCell::new(
                                s.chars().map(|ch| Value::Str(ch.to_string())).collect(),
                            )));
                            chars.freeze();
                            return Ok(chars);
                        }
                        if attr == "split" {
                            let s = s.clone();
                            return Ok(Value::BuiltinFunction {
                                name: "split".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() > 1 {
                                        return Err(RuntimeError {
                                            message: "split() takes zero or one argument".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let pieces: Vec<Value> = if args.is_empty() {
                                        s.split_whitespace()
                                            .map(|p| Value::Str(p.to_string()))
                                            .collect()
                                    } else if let Value::Str(sep) = &args[0] {
                                        s.split(sep.as_str())
                                            .map(|p| Value::Str(p.to_string()))
                                            .collect()
                                    } else {
                                        return Err(RuntimeError {
                                            message: "split separator must be a string".into(),
                                            span: Span::default(),
                                        });
                                    };
                                    Ok(Value::List(Rc::new(RefCell::new(pieces))))
                                }),
                            });
                        }
                        if attr == "join" {
                            let sep = s.clone();
                            return Ok(Value::BuiltinFunction {
                                name: "join".to_string(),
                                func: Rc::new(move |args, interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message: "join() takes exactly 1 argument (iterable)"
                                                .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let values = interp
                                        .materialize_iterable(args[0].clone(), Span::default())?;
                                    let items: Vec<String> = values
                                        .iter()
                                        .map(|v| match v {
                                            Value::Str(s) => s.clone(),
                                            other => format!("{other:?}"),
                                        })
                                        .collect();
                                    Ok(Value::Str(items.join(&sep)))
                                }),
                            });
                        }
                        if attr == "strip" || attr == "trim" {
                            let s = s.clone();
                            let method_name = attr.clone();
                            return Ok(Value::BuiltinFunction {
                                name: method_name.clone(),
                                func: Rc::new(move |args, _interp| {
                                    if !args.is_empty() {
                                        return Err(RuntimeError {
                                            message: format!("{method_name}() takes no arguments"),
                                            span: Span::default(),
                                        });
                                    }
                                    Ok(Value::Str(s.trim().to_string()))
                                }),
                            });
                        }
                        if attr == "replace" {
                            let s = s.clone();
                            return Ok(Value::BuiltinFunction {
                                name: "replace".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 2 {
                                        return Err(RuntimeError {
                                            message:
                                                "replace() takes exactly 2 arguments (from, to)"
                                                    .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    match (&args[0], &args[1]) {
                                        (Value::Str(from), Value::Str(to)) => {
                                            Ok(Value::Str(s.replace(from, to)))
                                        }
                                        _ => Err(RuntimeError {
                                            message: "replace() arguments must be strings".into(),
                                            span: Span::default(),
                                        }),
                                    }
                                }),
                            });
                        }
                        if attr == "startswith" {
                            let s = s.clone();
                            return Ok(Value::BuiltinFunction {
                                name: "startswith".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message:
                                                "startswith() takes exactly 1 argument (prefix)"
                                                    .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    match &args[0] {
                                        Value::Str(prefix) => {
                                            Ok(Value::Bool(s.starts_with(prefix)))
                                        }
                                        _ => Err(RuntimeError {
                                            message: "startswith() prefix must be a string".into(),
                                            span: Span::default(),
                                        }),
                                    }
                                }),
                            });
                        }
                        if attr == "endswith" {
                            let s = s.clone();
                            return Ok(Value::BuiltinFunction {
                                name: "endswith".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message: "endswith() takes exactly 1 argument (suffix)"
                                                .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    match &args[0] {
                                        Value::Str(suffix) => Ok(Value::Bool(s.ends_with(suffix))),
                                        _ => Err(RuntimeError {
                                            message: "endswith() suffix must be a string".into(),
                                            span: Span::default(),
                                        }),
                                    }
                                }),
                            });
                        }
                        if attr == "lower" {
                            let s = s.clone();
                            return Ok(Value::BuiltinFunction {
                                name: "lower".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if !args.is_empty() {
                                        return Err(RuntimeError {
                                            message: "lower() takes no arguments".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    Ok(Value::Str(s.to_lowercase()))
                                }),
                            });
                        }
                        if attr == "upper" {
                            let s = s.clone();
                            return Ok(Value::BuiltinFunction {
                                name: "upper".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if !args.is_empty() {
                                        return Err(RuntimeError {
                                            message: "upper() takes no arguments".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    Ok(Value::Str(s.to_uppercase()))
                                }),
                            });
                        }
                        Err(RuntimeError {
                            message: format!("str has no attribute '{attr}'"),
                            span: *span,
                        })
                    }
                    Value::List(l) => {
                        if attr == "remove" {
                            let l_clone = Rc::clone(&l);
                            return Ok(Value::BuiltinFunction {
                                name: "remove".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message: "remove() takes exactly 1 argument (item)"
                                                .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if container_is_frozen(Rc::as_ptr(&l_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen list".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let mut values = l_clone.borrow_mut();
                                    if let Some(index) =
                                        values.iter().position(|value| value == &args[0])
                                    {
                                        values.remove(index);
                                        Ok(Value::None)
                                    } else {
                                        Err(RuntimeError {
                                            message: "list.remove(x): x not in list".into(),
                                            span: Span::default(),
                                        })
                                    }
                                }),
                            });
                        }
                        if attr == "append" {
                            let l_clone = Rc::clone(&l);
                            return Ok(Value::BuiltinFunction {
                                name: "append".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message: "append() takes exactly 1 argument (item)"
                                                .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if container_is_frozen(Rc::as_ptr(&l_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen list".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    l_clone.borrow_mut().push(args[0].clone());
                                    Ok(Value::None)
                                }),
                            });
                        }
                        if attr == "pop" {
                            let l_clone = Rc::clone(&l);
                            return Ok(Value::BuiltinFunction {
                                name: "pop".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if container_is_frozen(Rc::as_ptr(&l_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen list".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if args.len() == 2 {
                                        let (start, stop) = match (&args[0], &args[1]) {
                                            (Value::Int(start), Value::Int(stop)) => {
                                                (*start, *stop)
                                            }
                                            _ => {
                                                return Err(RuntimeError {
                                                    message: "pop range bounds must be ints".into(),
                                                    span: Span::default(),
                                                })
                                            }
                                        };
                                        let mut vec = l_clone.borrow_mut();
                                        let len = vec.len() as i64;
                                        let start = if start < 0 {
                                            (len + start).max(0)
                                        } else {
                                            start.min(len)
                                        };
                                        let stop = if stop < 0 {
                                            (len + stop).max(0)
                                        } else {
                                            stop.min(len)
                                        };
                                        if start > stop {
                                            return Ok(Value::List(Rc::new(RefCell::new(
                                                Vec::new(),
                                            ))));
                                        }
                                        let removed: Vec<Value> =
                                            vec.drain(start as usize..stop as usize).collect();
                                        return Ok(Value::List(Rc::new(RefCell::new(removed))));
                                    }
                                    if args.len() > 2 {
                                        return Err(RuntimeError {
                                            message: "pop() takes zero, one, or two arguments"
                                                .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let mut vec = l_clone.borrow_mut();
                                    if vec.is_empty() {
                                        return Err(RuntimeError {
                                            message: "pop from empty list".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if args.is_empty() {
                                        vec.pop().ok_or_else(|| RuntimeError {
                                            message: "pop from empty list".into(),
                                            span: Span::default(),
                                        })
                                    } else if let Value::Int(idx) = &args[0] {
                                        let i = if *idx < 0 {
                                            vec.len() as i64 + *idx
                                        } else {
                                            *idx
                                        };
                                        if i < 0 || i as usize >= vec.len() {
                                            return Err(RuntimeError {
                                                message: format!("pop index {idx} out of range"),
                                                span: Span::default(),
                                            });
                                        }
                                        Ok(vec.remove(i as usize))
                                    } else {
                                        Err(RuntimeError {
                                            message: "pop index must be an int".into(),
                                            span: Span::default(),
                                        })
                                    }
                                }),
                            });
                        }
                        if attr == "insert" {
                            let l_clone = Rc::clone(&l);
                            return Ok(Value::BuiltinFunction {
                                name: "insert".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if container_is_frozen(Rc::as_ptr(&l_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen list".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if args.len() != 2 {
                                        return Err(RuntimeError {
                                            message:
                                                "insert() takes exactly 2 arguments (index, item)"
                                                    .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if let Value::Int(idx) = &args[0] {
                                        let mut vec = l_clone.borrow_mut();
                                        let len = vec.len() as i64;
                                        let i = if *idx < 0 {
                                            (len + *idx).max(0) as usize
                                        } else {
                                            (*idx).min(len) as usize
                                        };
                                        vec.insert(i, args[1].clone());
                                        Ok(Value::None)
                                    } else {
                                        Err(RuntimeError {
                                            message: "insert index must be an int".into(),
                                            span: Span::default(),
                                        })
                                    }
                                }),
                            });
                        }
                        if attr == "extend" {
                            let l_clone = Rc::clone(&l);
                            return Ok(Value::BuiltinFunction {
                                name: "extend".to_string(),
                                func: Rc::new(move |args, interp| {
                                    if container_is_frozen(Rc::as_ptr(&l_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen list".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message: "extend() takes exactly 1 argument (iterable)"
                                                .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let items = interp
                                        .materialize_iterable(args[0].clone(), Span::default())?;
                                    l_clone.borrow_mut().extend(items);
                                    Ok(Value::None)
                                }),
                            });
                        }
                        if attr == "clear" {
                            let l_clone = Rc::clone(&l);
                            return Ok(Value::BuiltinFunction {
                                name: "clear".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if !args.is_empty() {
                                        return Err(RuntimeError {
                                            message: "clear() takes no arguments".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if container_is_frozen(Rc::as_ptr(&l_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen list".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    l_clone.borrow_mut().clear();
                                    Ok(Value::None)
                                }),
                            });
                        }
                        Err(RuntimeError {
                            message: format!("list has no attribute '{attr}'"),
                            span: *span,
                        })
                    }
                    Value::Dict(d) => {
                        if attr == "get" {
                            let d_clone = Rc::clone(&d);
                            return Ok(Value::BuiltinFunction {
                                name: "get".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.is_empty() || args.len() > 2 {
                                        return Err(RuntimeError {
                                            message:
                                                "get() takes 1 or 2 arguments (key, [default])"
                                                    .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let key_str = match &args[0] {
                                        Value::Str(s) => s.clone(),
                                        other => format!("{other:?}"),
                                    };
                                    if let Some(val) = d_clone.borrow().get(&key_str) {
                                        Ok(val.clone())
                                    } else if args.len() == 2 {
                                        Ok(args[1].clone())
                                    } else {
                                        Ok(Value::None)
                                    }
                                }),
                            });
                        }
                        if attr == "keys" {
                            let d_clone = Rc::clone(&d);
                            return Ok(Value::BuiltinFunction {
                                name: "keys".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if !args.is_empty() {
                                        return Err(RuntimeError {
                                            message: "keys() takes no arguments".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let keys: Vec<Value> =
                                        d_clone.borrow().keys().cloned().map(Value::Str).collect();
                                    Ok(Value::List(Rc::new(RefCell::new(keys))))
                                }),
                            });
                        }
                        if attr == "values" {
                            let d_clone = Rc::clone(&d);
                            return Ok(Value::BuiltinFunction {
                                name: "values".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if !args.is_empty() {
                                        return Err(RuntimeError {
                                            message: "values() takes no arguments".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let vals: Vec<Value> =
                                        d_clone.borrow().values().cloned().collect();
                                    Ok(Value::List(Rc::new(RefCell::new(vals))))
                                }),
                            });
                        }
                        if attr == "items" {
                            let d_clone = Rc::clone(&d);
                            return Ok(Value::BuiltinFunction {
                                name: "items".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if !args.is_empty() {
                                        return Err(RuntimeError {
                                            message: "items() takes no arguments".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let items: Vec<Value> = d_clone
                                        .borrow()
                                        .iter()
                                        .map(|(k, v)| {
                                            Value::List(Rc::new(RefCell::new(vec![
                                                Value::Str(k.clone()),
                                                v.clone(),
                                            ])))
                                        })
                                        .collect();
                                    Ok(Value::List(Rc::new(RefCell::new(items))))
                                }),
                            });
                        }
                        if attr == "clear" {
                            let d_clone = Rc::clone(&d);
                            return Ok(Value::BuiltinFunction {
                                name: "clear".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if !args.is_empty() {
                                        return Err(RuntimeError {
                                            message: "clear() takes no arguments".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if container_is_frozen(Rc::as_ptr(&d_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen dict".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    d_clone.borrow_mut().clear();
                                    Ok(Value::None)
                                }),
                            });
                        }
                        if attr == "pop" {
                            let d_clone = Rc::clone(&d);
                            return Ok(Value::BuiltinFunction {
                                name: "pop".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message: "dict.pop() takes exactly 1 argument (key)"
                                                .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if container_is_frozen(Rc::as_ptr(&d_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen dict".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let key = match &args[0] {
                                        Value::Str(value) => value.clone(),
                                        other => format!("{other:?}"),
                                    };
                                    d_clone
                                        .borrow_mut()
                                        .remove(&key)
                                        .ok_or_else(|| RuntimeError {
                                            message: "dict.pop(key): key not found".into(),
                                            span: Span::default(),
                                        })
                                }),
                            });
                        }
                        if attr == "contains" {
                            let d_clone = Rc::clone(&d);
                            return Ok(Value::BuiltinFunction {
                                name: "contains".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message: "contains() takes exactly 1 argument (key)"
                                                .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let key_str = match &args[0] {
                                        Value::Str(s) => s.clone(),
                                        other => format!("{other:?}"),
                                    };
                                    Ok(Value::Bool(d_clone.borrow().contains_key(&key_str)))
                                }),
                            });
                        }
                        Err(RuntimeError {
                            message: format!("dict has no attribute '{attr}'"),
                            span: *span,
                        })
                    }
                    Value::Set(s) => {
                        if attr == "isdisjoint" {
                            let s_clone = Rc::clone(&s);
                            return Ok(Value::BuiltinFunction {
                                name: "isdisjoint".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message: "isdisjoint() takes exactly one argument"
                                                .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let other = match &args[0] {
                                        Value::Set(values) => values.borrow().clone(),
                                        Value::List(values) => values.borrow().clone(),
                                        _ => {
                                            return Err(RuntimeError {
                                                message: "isdisjoint() argument must be iterable"
                                                    .into(),
                                                span: Span::default(),
                                            })
                                        }
                                    };
                                    Ok(Value::Bool(
                                        !s_clone.borrow().iter().any(|item| other.contains(item)),
                                    ))
                                }),
                            });
                        }
                        if attr == "add" {
                            let s_clone = Rc::clone(&s);
                            return Ok(Value::BuiltinFunction {
                                name: "add".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message: "add() takes exactly 1 argument (item)".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if container_is_frozen(Rc::as_ptr(&s_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen set".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let mut vec = s_clone.borrow_mut();
                                    if !vec.contains(&args[0]) {
                                        vec.push(args[0].clone());
                                    }
                                    Ok(Value::None)
                                }),
                            });
                        }
                        if attr == "remove" {
                            let s_clone = Rc::clone(&s);
                            return Ok(Value::BuiltinFunction {
                                name: "remove".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message: "remove() takes exactly 1 argument (item)"
                                                .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if container_is_frozen(Rc::as_ptr(&s_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen set".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let mut vec = s_clone.borrow_mut();
                                    if let Some(pos) = vec.iter().position(|x| x == &args[0]) {
                                        vec.remove(pos);
                                        return Ok(Value::None);
                                    }
                                    Err(RuntimeError {
                                        message: "set.remove(x): x not in set".into(),
                                        span: Span::default(),
                                    })
                                }),
                            });
                        }
                        if attr == "discard" {
                            let s_clone = Rc::clone(&s);
                            return Ok(Value::BuiltinFunction {
                                name: "discard".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message: "discard() takes exactly 1 argument (item)"
                                                .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if container_is_frozen(Rc::as_ptr(&s_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen set".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let mut values = s_clone.borrow_mut();
                                    if let Some(pos) =
                                        values.iter().position(|item| item == &args[0])
                                    {
                                        values.remove(pos);
                                    }
                                    Ok(Value::None)
                                }),
                            });
                        }
                        if attr == "pop" {
                            let s_clone = Rc::clone(&s);
                            return Ok(Value::BuiltinFunction {
                                name: "pop".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if !args.is_empty() {
                                        return Err(RuntimeError {
                                            message: "set.pop() takes no arguments".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if container_is_frozen(Rc::as_ptr(&s_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen set".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let mut values = s_clone.borrow_mut();
                                    if values.is_empty() {
                                        return Err(RuntimeError {
                                            message: "set.pop(): pop from an empty set".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    Ok(values.remove(0))
                                }),
                            });
                        }
                        if attr == "clear" {
                            let s_clone = Rc::clone(&s);
                            return Ok(Value::BuiltinFunction {
                                name: "clear".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if !args.is_empty() {
                                        return Err(RuntimeError {
                                            message: "clear() takes no arguments".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    if container_is_frozen(Rc::as_ptr(&s_clone) as usize) {
                                        return Err(RuntimeError {
                                            message: "cannot mutate frozen set".into(),
                                            span: Span::default(),
                                        });
                                    }
                                    s_clone.borrow_mut().clear();
                                    Ok(Value::None)
                                }),
                            });
                        }
                        if attr == "contains" {
                            let s_clone = Rc::clone(&s);
                            return Ok(Value::BuiltinFunction {
                                name: "contains".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError {
                                            message: "contains() takes exactly 1 argument (item)"
                                                .into(),
                                            span: Span::default(),
                                        });
                                    }
                                    let vec = s_clone.borrow();
                                    Ok(Value::Bool(vec.contains(&args[0])))
                                }),
                            });
                        }
                        Err(RuntimeError {
                            message: format!("set has no attribute '{attr}'"),
                            span: *span,
                        })
                    }
                    _ => Err(RuntimeError {
                        message: "attribute access on non-object".to_string(),
                        span: *span,
                    }),
                }
            }
            Expr::Index { value, index, span } => {
                let obj = self.eval_expr(value)?;
                if let Expr::Slice {
                    ref start,
                    ref stop,
                    ref step,
                    span: slice_span,
                } = **index
                {
                    let start_val = if let Some(ref s) = start {
                        match self.eval_expr(s)? {
                            Value::Int(i) => Some(i),
                            _ => {
                                return Err(RuntimeError {
                                    message: "slice start must be an integer".into(),
                                    span: slice_span,
                                })
                            }
                        }
                    } else {
                        None
                    };
                    let stop_val = if let Some(ref s) = stop {
                        match self.eval_expr(s)? {
                            Value::Int(i) => Some(i),
                            _ => {
                                return Err(RuntimeError {
                                    message: "slice stop must be an integer".into(),
                                    span: slice_span,
                                })
                            }
                        }
                    } else {
                        None
                    };
                    let step_val = if let Some(ref s) = step {
                        match self.eval_expr(s)? {
                            Value::Int(i) => {
                                if i == 0 {
                                    return Err(RuntimeError {
                                        message: "slice step cannot be zero".into(),
                                        span: slice_span,
                                    });
                                }
                                i
                            }
                            _ => {
                                return Err(RuntimeError {
                                    message: "slice step must be an integer".into(),
                                    span: slice_span,
                                })
                            }
                        }
                    } else {
                        1
                    };

                    match obj {
                        Value::List(l) => {
                            let items = l.borrow();
                            let len = items.len() as i64;
                            let mut cur = start_val
                                .map(|s| if s < 0 { (len + s).max(0) } else { s.min(len) })
                                .unwrap_or(if step_val > 0 { 0 } else { len - 1 });
                            let end = stop_val
                                .map(|s| if s < 0 { (len + s).max(0) } else { s.min(len) })
                                .unwrap_or(if step_val > 0 { len } else { -1 });
                            let mut res = Vec::new();
                            if step_val > 0 {
                                while cur < end && cur < len {
                                    res.push(items[cur as usize].clone());
                                    let Some(next) = cur.checked_add(step_val) else {
                                        return Err(RuntimeError {
                                            message: "slice step overflow".into(),
                                            span: slice_span,
                                        });
                                    };
                                    cur = next;
                                }
                            } else {
                                while cur > end && cur >= 0 {
                                    res.push(items[cur as usize].clone());
                                    let Some(next) = cur.checked_add(step_val) else {
                                        return Err(RuntimeError {
                                            message: "slice step overflow".into(),
                                            span: slice_span,
                                        });
                                    };
                                    cur = next;
                                }
                            }
                            return Ok(Value::List(Rc::new(RefCell::new(res))));
                        }
                        Value::Range { start, stop, step } => {
                            let items = materialize_range(start, stop, step);
                            let len = items.len() as i64;
                            let mut cur = start_val
                                .map(|s| if s < 0 { (len + s).max(0) } else { s.min(len) })
                                .unwrap_or(if step_val > 0 { 0 } else { len - 1 });
                            let end = stop_val
                                .map(|s| if s < 0 { (len + s).max(0) } else { s.min(len) })
                                .unwrap_or(if step_val > 0 { len } else { -1 });
                            let mut res = Vec::new();
                            if step_val > 0 {
                                while cur < end && cur < len {
                                    res.push(items[cur as usize].clone());
                                    let Some(next) = cur.checked_add(step_val) else {
                                        return Err(RuntimeError {
                                            message: "slice step overflow".into(),
                                            span: slice_span,
                                        });
                                    };
                                    cur = next;
                                }
                            } else {
                                while cur > end && cur >= 0 {
                                    res.push(items[cur as usize].clone());
                                    let Some(next) = cur.checked_add(step_val) else {
                                        return Err(RuntimeError {
                                            message: "slice step overflow".into(),
                                            span: slice_span,
                                        });
                                    };
                                    cur = next;
                                }
                            }
                            return Ok(Value::List(Rc::new(RefCell::new(res))));
                        }
                        Value::MemoryView {
                            data,
                            start,
                            len,
                            stride,
                            read_only,
                        } => {
                            let mut slice_start =
                                start_val.unwrap_or(if step_val > 0 { 0 } else { len - 1 });
                            let mut slice_stop =
                                stop_val.unwrap_or(if step_val > 0 { len } else { -1 });
                            if slice_start < 0 {
                                slice_start += len;
                            }
                            if slice_stop < 0 && !(stop_val.is_none() && step_val < 0) {
                                slice_stop += len;
                            }
                            if step_val > 0 {
                                slice_start = slice_start.clamp(0, len);
                                slice_stop = slice_stop.clamp(0, len);
                            } else {
                                slice_start = slice_start.clamp(-1, len - 1);
                                slice_stop = slice_stop.min(len - 1);
                            }
                            let mut count = 0;
                            let mut cur = slice_start;
                            while if step_val > 0 {
                                cur < slice_stop
                            } else {
                                cur > slice_stop
                            } {
                                count += 1;
                                let Some(next) = cur.checked_add(step_val) else {
                                    return Err(RuntimeError {
                                        message: "slice step overflow".into(),
                                        span: slice_span,
                                    });
                                };
                                cur = next;
                            }
                            return Ok(Value::MemoryView {
                                data,
                                start: start + slice_start * stride,
                                len: count,
                                stride: stride * step_val,
                                read_only,
                            });
                        }
                        Value::Str(s) => {
                            let chars: Vec<char> = s.chars().collect();
                            let len = chars.len() as i64;
                            let mut cur = start_val
                                .map(|st| {
                                    if st < 0 {
                                        (len + st).max(0)
                                    } else {
                                        st.min(len)
                                    }
                                })
                                .unwrap_or(if step_val > 0 { 0 } else { len - 1 });
                            let end = stop_val
                                .map(|st| {
                                    if st < 0 {
                                        (len + st).max(0)
                                    } else {
                                        st.min(len)
                                    }
                                })
                                .unwrap_or(if step_val > 0 { len } else { -1 });
                            let mut res = String::new();
                            if step_val > 0 {
                                while cur < end && cur < len {
                                    res.push(chars[cur as usize]);
                                    let Some(next) = cur.checked_add(step_val) else {
                                        return Err(RuntimeError {
                                            message: "slice step overflow".into(),
                                            span: slice_span,
                                        });
                                    };
                                    cur = next;
                                }
                            } else {
                                while cur > end && cur >= 0 {
                                    res.push(chars[cur as usize]);
                                    let Some(next) = cur.checked_add(step_val) else {
                                        return Err(RuntimeError {
                                            message: "slice step overflow".into(),
                                            span: slice_span,
                                        });
                                    };
                                    cur = next;
                                }
                            }
                            return Ok(Value::Str(res));
                        }
                        Value::Bytes(bytes) => {
                            let len = bytes.len() as i64;
                            let mut cur = start_val
                                .map(|st| {
                                    if st < 0 {
                                        (len + st).max(0)
                                    } else {
                                        st.min(len)
                                    }
                                })
                                .unwrap_or(if step_val > 0 { 0 } else { len - 1 });
                            let end = stop_val
                                .map(|st| {
                                    if st < 0 {
                                        (len + st).max(0)
                                    } else {
                                        st.min(len)
                                    }
                                })
                                .unwrap_or(if step_val > 0 { len } else { -1 });
                            let mut res = Vec::new();
                            if step_val > 0 {
                                while cur < end && cur < len {
                                    res.push(bytes[cur as usize]);
                                    let Some(next) = cur.checked_add(step_val) else {
                                        return Err(RuntimeError {
                                            message: "slice step overflow".into(),
                                            span: slice_span,
                                        });
                                    };
                                    cur = next;
                                }
                            } else {
                                while cur > end && cur >= 0 {
                                    res.push(bytes[cur as usize]);
                                    let Some(next) = cur.checked_add(step_val) else {
                                        return Err(RuntimeError {
                                            message: "slice step overflow".into(),
                                            span: slice_span,
                                        });
                                    };
                                    cur = next;
                                }
                            }
                            return Ok(Value::Bytes(res));
                        }
                        _ => {
                            return Err(RuntimeError {
                                message: format!("slice not supported on {}", obj.type_name()),
                                span: *span,
                            })
                        }
                    }
                }

                let idx = self.eval_expr(index)?;
                if let Value::Object {
                    class_name, fields, ..
                } = &obj
                {
                    if class_name == "slice" {
                        if let Value::Str(name) = &idx {
                            if let Some(value) = fields.borrow().get(name).cloned() {
                                return Ok(value);
                            }
                        }
                    }
                    if let Some(method) = fields.borrow().get("__getitem__").cloned() {
                        return self.invoke_value(
                            method,
                            vec![(None, obj.clone()), (None, idx)],
                            *span,
                        );
                    }
                }
                match (obj, idx) {
                    (Value::List(list), Value::Int(i)) => {
                        let vec = list.borrow();
                        let actual_idx = if i < 0 { vec.len() as i64 + i } else { i };
                        if actual_idx < 0 || actual_idx as usize >= vec.len() {
                            return Err(RuntimeError {
                                message: format!("index {i} out of range"),
                                span: *span,
                            });
                        }
                        Ok(vec[actual_idx as usize].clone())
                    }
                    (
                        Value::MemoryView {
                            data,
                            start,
                            len,
                            stride,
                            ..
                        },
                        Value::Int(i),
                    ) => {
                        let actual_idx = if i < 0 { len + i } else { i };
                        if actual_idx < 0 || actual_idx >= len {
                            return Err(RuntimeError {
                                message: format!("index {i} out of range"),
                                span: *span,
                            });
                        }
                        Ok(data.borrow()[(start + actual_idx * stride) as usize].clone())
                    }
                    (Value::Str(s), Value::Int(i)) => {
                        let chars: Vec<char> = s.chars().collect();
                        let actual_idx = if i < 0 { chars.len() as i64 + i } else { i };
                        if actual_idx < 0 || actual_idx as usize >= chars.len() {
                            return Err(RuntimeError {
                                message: format!("index {i} out of range"),
                                span: *span,
                            });
                        }
                        Ok(Value::Str(chars[actual_idx as usize].to_string()))
                    }
                    (Value::DottedPath(parts), Value::Int(i)) => {
                        let actual_idx = if i < 0 { parts.len() as i64 + i } else { i };
                        if actual_idx < 0 || actual_idx as usize >= parts.len() {
                            return Err(RuntimeError {
                                message: format!("index {i} out of range"),
                                span: *span,
                            });
                        }
                        Ok(Value::Str(parts[actual_idx as usize].clone()))
                    }
                    (Value::Bytes(bytes), Value::Int(i)) => {
                        let actual_idx = if i < 0 { bytes.len() as i64 + i } else { i };
                        if actual_idx < 0 || actual_idx as usize >= bytes.len() {
                            return Err(RuntimeError {
                                message: format!("index {i} out of range"),
                                span: *span,
                            });
                        }
                        Ok(Value::Int(bytes[actual_idx as usize] as i64))
                    }
                    (Value::Range { start, stop, step }, Value::Int(i)) => {
                        let values = materialize_range(start, stop, step);
                        let actual_idx = if i < 0 { values.len() as i64 + i } else { i };
                        if actual_idx < 0 || actual_idx as usize >= values.len() {
                            return Err(RuntimeError {
                                message: format!("index {i} out of range"),
                                span: *span,
                            });
                        }
                        Ok(values[actual_idx as usize].clone())
                    }
                    (Value::Dict(dict), Value::Str(k)) => {
                        dict.borrow().get(&k).cloned().ok_or_else(|| RuntimeError {
                            message: format!("key '{k}' not found"),
                            span: *span,
                        })
                    }
                    (Value::Record(fields), Value::Int(i)) => {
                        let key = format!("{i}");
                        fields
                            .borrow()
                            .get(&key)
                            .cloned()
                            .ok_or_else(|| RuntimeError {
                                message: format!("record field index {i} not found"),
                                span: *span,
                            })
                    }
                    (Value::Record(fields), Value::Str(k)) => fields
                        .borrow()
                        .get(&k)
                        .cloned()
                        .ok_or_else(|| RuntimeError {
                            message: format!("record field '{k}' not found"),
                            span: *span,
                        }),
                    (Value::Dict(dict), other) => {
                        let k = format!("{other:?}");
                        dict.borrow().get(&k).cloned().ok_or_else(|| RuntimeError {
                            message: format!("key '{k}' not found"),
                            span: *span,
                        })
                    }
                    (other_obj, _) => Err(RuntimeError {
                        message: format!("indexing not supported on {}", other_obj.type_name()),
                        span: *span,
                    }),
                }
            }
            Expr::Record { fields, .. } => {
                let mut map = HashMap::new();
                for (idx, (opt_name, expr)) in fields.iter().enumerate() {
                    let val = self.eval_expr(expr)?;
                    let name = opt_name.clone().unwrap_or_else(|| format!("{idx}"));
                    map.insert(name, val);
                }
                Ok(Value::Record(Rc::new(RefCell::new(map))))
            }
            Expr::List { elements, .. } => {
                let mut vals = Vec::new();
                for e in elements {
                    let val = self.eval_expr(e)?;
                    if !matches!(val, Value::Skip) {
                        vals.push(val);
                    }
                }
                Ok(Value::List(Rc::new(RefCell::new(vals))))
            }
            Expr::Dict { entries, .. } => {
                let mut map = HashMap::new();
                for (k, v) in entries {
                    let k_val = self.eval_expr(k)?;
                    if matches!(k_val, Value::Skip) {
                        continue;
                    }
                    let v_val = self.eval_expr(v)?;
                    if matches!(v_val, Value::Skip) {
                        continue;
                    }
                    let k_str = match k_val {
                        Value::Str(s) => s,
                        other => format!("{:?}", other),
                    };
                    map.insert(k_str, v_val);
                }
                Ok(Value::Dict(Rc::new(RefCell::new(map))))
            }
            Expr::Set { elements, .. } => {
                let mut set_vals = Vec::new();
                for e in elements {
                    let val = self.eval_expr(e)?;
                    if !matches!(val, Value::Skip) && !set_vals.contains(&val) {
                        set_vals.push(val);
                    }
                }
                Ok(Value::Set(Rc::new(RefCell::new(set_vals))))
            }
            Expr::Construct { args, span: _ } => {
                let mut field_values = HashMap::new();
                for (idx, arg) in args.iter().enumerate() {
                    let val = self.eval_expr(&arg.value)?;
                    let name = arg.name.clone().unwrap_or_else(|| format!("field_{idx}"));
                    field_values.insert(name, val);
                }

                Ok(Value::Object {
                    class_name: "Constructed".to_string(),
                    fields: Rc::new(RefCell::new(field_values)),
                    is_frozen: Rc::new(RefCell::new(false)),
                })
            }
            Expr::Freeze { expr, .. } => {
                let val = self.eval_expr(expr)?;
                val.freeze();
                Ok(val)
            }
            Expr::Trust { expr, .. } => self.eval_expr(expr),
            Expr::Skip(_) => Ok(Value::Skip),
            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let cond_val = self.eval_expr(condition)?;
                if self.is_truthy(&cond_val) {
                    self.eval_expr(then_branch)
                } else {
                    self.eval_expr(else_branch)
                }
            }
            Expr::ListComp {
                element,
                target,
                iter,
                condition,
                ..
            } => {
                let iter_val = self.eval_expr(iter)?;
                let items = self.materialize_iterable(iter_val, expr.span())?;
                let mut results = Vec::new();
                for item in items {
                    // Comprehension targets get a fresh environment per
                    // iteration, just like statement-level `for` targets.
                    // This keeps closures created by the element expression
                    // bound to that iteration's value.
                    let iter_env =
                        Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&self.env))));
                    let prev_env = Rc::clone(&self.env);
                    self.env = iter_env;
                    let value = (|| {
                        self.bind_pattern(target, item, Span::default())?;
                        let keep = if let Some(cond) = condition {
                            let c = self.eval_expr(cond)?;
                            self.is_truthy(&c)
                        } else {
                            true
                        };
                        if keep {
                            Ok(Some(self.eval_expr(element)?))
                        } else {
                            Ok(None)
                        }
                    })();
                    self.env = prev_env;
                    if let Some(val) = value? {
                        if !matches!(val, Value::Skip) {
                            results.push(val);
                        }
                    }
                }
                Ok(Value::List(Rc::new(RefCell::new(results))))
            }
            Expr::SetComp {
                element,
                target,
                iter,
                condition,
                ..
            } => {
                let iter_val = self.eval_expr(iter)?;
                let items = self.materialize_iterable(iter_val, expr.span())?;
                let mut results = Vec::new();
                for item in items {
                    let iter_env =
                        Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&self.env))));
                    let prev_env = Rc::clone(&self.env);
                    self.env = iter_env;
                    let value = (|| {
                        self.bind_pattern(target, item, Span::default())?;
                        let keep = if let Some(cond) = condition {
                            let c = self.eval_expr(cond)?;
                            self.is_truthy(&c)
                        } else {
                            true
                        };
                        if keep {
                            Ok(Some(self.eval_expr(element)?))
                        } else {
                            Ok(None)
                        }
                    })();
                    self.env = prev_env;
                    if let Some(val) = value? {
                        if !matches!(val, Value::Skip) && !results.contains(&val) {
                            results.push(val);
                        }
                    }
                }
                Ok(Value::Set(Rc::new(RefCell::new(results))))
            }
            Expr::DictComp {
                key,
                value,
                target,
                iter,
                condition,
                ..
            } => {
                let iter_val = self.eval_expr(iter)?;
                let items = self.materialize_iterable(iter_val, expr.span())?;
                let mut results = HashMap::new();
                for item in items {
                    let iter_env =
                        Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&self.env))));
                    let prev_env = Rc::clone(&self.env);
                    self.env = iter_env;
                    let pair = (|| {
                        self.bind_pattern(target, item, Span::default())?;
                        let keep = if let Some(cond) = condition {
                            let c = self.eval_expr(cond)?;
                            self.is_truthy(&c)
                        } else {
                            true
                        };
                        if keep {
                            let k = match self.eval_expr(key)? {
                                Value::Str(s) => s,
                                other => format!("{other:?}"),
                            };
                            Ok(Some((k, self.eval_expr(value)?)))
                        } else {
                            Ok(None)
                        }
                    })();
                    self.env = prev_env;
                    if let Some((k, v)) = pair? {
                        results.insert(k, v);
                    }
                }
                Ok(Value::Dict(Rc::new(RefCell::new(results))))
            }
            Expr::Type(type_expr) => {
                let marker = format!("__type:{}", type_form_name(type_expr));
                Ok(Value::Str(marker))
            }
            Expr::AnonymousDef { params, body, .. } => Ok(Value::Function {
                name: "<def>".to_string(),
                params: params.clone(),
                body: body.clone(),
                closure: Rc::clone(&self.env),
                is_contextmanager: false,
                is_async: false,
            }),
            _ => Err(RuntimeError {
                message: "unsupported expression form".into(),
                span: expr.span(),
            }),
        }
    }

    fn construct_class(&mut self, class_name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
        let class_def = self
            .classes
            .get(class_name)
            .cloned()
            .ok_or_else(|| RuntimeError {
                message: format!("class '{class_name}' not found"),
                span: Span::default(),
            })?;

        let mut fields = HashMap::new();
        let mut field_names = Vec::new();

        for base in &class_def.bases {
            if let TypeExpr::Named {
                name: parent_name, ..
            } = base
            {
                if self.classes.contains_key(parent_name) {
                    self.collect_inherited_members(parent_name, &mut field_names, &mut fields);
                }
            }
        }

        for member in &class_def.body {
            if let ClassMember::Field(f) = member {
                field_names.push(f.name.clone());
            }
        }

        // Inherited trait methods
        for base in &class_def.bases {
            if let TypeExpr::Named {
                name: trait_name, ..
            } = base
            {
                if !class_def.without_traits.contains(trait_name) {
                    if let Some(trait_body) = self.traits.get(trait_name) {
                        for member in trait_body {
                            if let TraitMember::Method(m) = member {
                                fields.insert(
                                    m.name.clone(),
                                    Value::Function {
                                        name: m.name.clone(),
                                        params: m.params.clone(),
                                        body: m.body.clone(),
                                        closure: Rc::clone(&self.env),
                                        is_contextmanager: m.decorators.iter().any(|decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager")),
                                        is_async: m.is_async,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }

        // Declared class methods
        for member in &class_def.body {
            if let ClassMember::Method(m) = member {
                fields.insert(
                    m.name.clone(),
                    Value::Function {
                        name: m.name.clone(),
                        params: m.params.clone(),
                        body: m.body.clone(),
                        closure: Rc::clone(&self.env),
                        is_contextmanager: m.decorators.iter().any(|decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager")),
                        is_async: m.is_async,
                    },
                );
            }
            if let ClassMember::Getter(g) = member {
                fields.insert(
                    g.name.clone(),
                    Value::Function {
                        name: format!("__getter__{}", g.name),
                        params: vec![Param {
                            name: "self".into(),
                            pattern: None,
                            type_annotation: None,
                            default: None,
                            is_positional_only: false,
                            is_keyword_only: false,
                            is_variadic_positional: false,
                            is_variadic_keyword: false,
                            is_gather: false,
                            span: g.span,
                        }],
                        body: g.body.clone(),
                        closure: Rc::clone(&self.env),
                        is_contextmanager: false,
                        is_async: false,
                    },
                );
            }
            if let ClassMember::Setter(s) = member {
                fields.insert(
                    format!("__setter__{}", s.name),
                    Value::Function {
                        name: format!("__setter__{}", s.name),
                        params: vec![
                            Param {
                                name: "self".into(),
                                pattern: None,
                                type_annotation: None,
                                default: None,
                                is_positional_only: false,
                                is_keyword_only: false,
                                is_variadic_positional: false,
                                is_variadic_keyword: false,
                                is_gather: false,
                                span: s.span,
                            },
                            s.param.clone(),
                        ],
                        body: s.body.clone(),
                        closure: Rc::clone(&self.env),
                        is_contextmanager: false,
                        is_async: false,
                    },
                );
            }
        }

        for (name, val) in field_names.iter().zip(args) {
            fields.insert(name.clone(), val.clone());
        }

        Ok(Value::Object {
            class_name: class_name.to_string(),
            fields: Rc::new(RefCell::new(fields)),
            is_frozen: Rc::new(RefCell::new(false)),
        })
    }

    fn class_has_capability(&self, class_name: &str, capability: &str) -> bool {
        let generated = matches!(capability, "Eq" | "Ord" | "Hashable");
        let mut current = Some(class_name.to_string());
        while let Some(name) = current {
            let Some(class) = self.classes.get(&name) else {
                break;
            };
            if class
                .without_traits
                .iter()
                .any(|trait_name| trait_name == capability)
            {
                return false;
            }
            if class
                .bases
                .iter()
                .any(|base| matches!(base, TypeExpr::Named { name, .. } if name == capability))
            {
                return true;
            }
            current = class.bases.iter().find_map(|base| {
                if let TypeExpr::Named { name, .. } = base {
                    if self.classes.contains_key(name) {
                        return Some(name.clone());
                    }
                }
                None
            });
        }
        generated
    }

    fn collect_inherited_members(
        &self,
        class_name: &str,
        field_names: &mut Vec<String>,
        fields: &mut HashMap<String, Value>,
    ) {
        let Some(class_def) = self.classes.get(class_name) else {
            return;
        };
        for base in &class_def.bases {
            if let TypeExpr::Named {
                name: parent_name, ..
            } = base
            {
                if self.classes.contains_key(parent_name) {
                    self.collect_inherited_members(parent_name, field_names, fields);
                }
            }
        }
        for member in &class_def.body {
            match member {
                ClassMember::Field(field) => field_names.push(field.name.clone()),
                ClassMember::Method(method) => {
                    fields.entry(method.name.clone()).or_insert_with(|| Value::Function {
                        name: method.name.clone(),
                        params: method.params.clone(),
                        body: method.body.clone(),
                        closure: Rc::clone(&self.env),
                        is_contextmanager: method.decorators.iter().any(|decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager")),
                        is_async: method.is_async,
                    });
                }
                ClassMember::Getter(getter) => {
                    fields
                        .entry(getter.name.clone())
                        .or_insert_with(|| Value::Function {
                            name: format!("__getter__{}", getter.name),
                            params: vec![Param {
                                name: "self".into(),
                                pattern: None,
                                type_annotation: None,
                                default: None,
                                is_positional_only: false,
                                is_keyword_only: false,
                                is_variadic_positional: false,
                                is_variadic_keyword: false,
                                is_gather: false,
                                span: getter.span,
                            }],
                            body: getter.body.clone(),
                            closure: Rc::clone(&self.env),
                            is_contextmanager: false,
                            is_async: false,
                        });
                }
                ClassMember::Setter(setter) => {
                    let key = format!("__setter__{}", setter.name);
                    fields
                        .entry(key.clone())
                        .or_insert_with(|| Value::Function {
                            name: key,
                            params: vec![
                                Param {
                                    name: "self".into(),
                                    pattern: None,
                                    type_annotation: None,
                                    default: None,
                                    is_positional_only: false,
                                    is_keyword_only: false,
                                    is_variadic_positional: false,
                                    is_variadic_keyword: false,
                                    is_gather: false,
                                    span: setter.span,
                                },
                                setter.param.clone(),
                            ],
                            body: setter.body.clone(),
                            closure: Rc::clone(&self.env),
                            is_contextmanager: false,
                            is_async: false,
                        });
                }
                _ => {}
            }
        }
    }

    fn call_class(
        &mut self,
        class_name: &str,
        args: &[Value],
        span: Span,
    ) -> Result<Value, RuntimeError> {
        let factory = self.classes.get(class_name).and_then(|class_def| {
            class_def.body.iter().find_map(|member| match member {
                ClassMember::Factory(factory) if factory.name == "__init__" => {
                    Some(factory.clone())
                }
                _ => None,
            })
        });
        if let Some(factory) = factory {
            return self.call_factory(class_name, &factory, args);
        }
        self.construct_class(class_name, args).map_err(|mut error| {
            error.span = span;
            error
        })
    }

    fn call_factory(
        &mut self,
        class_name: &str,
        factory: &FactoryDef,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let mut call_args = vec![Value::ClassRef(class_name.to_string())];
        call_args.extend_from_slice(args);
        let closure = Rc::clone(&self.env);
        let call_env = Rc::new(RefCell::new(Environment::with_parent(closure)));
        for (param, val) in factory.params.iter().zip(call_args) {
            call_env.borrow_mut().set(param.name.clone(), val);
        }
        let previous = Rc::clone(&self.env);
        self.env = call_env;
        let result = self.eval_block(&factory.body);
        self.env = previous;
        match result {
            Ok(Value::Return(value)) => Ok(self.normalize_constructed(class_name, *value)),
            Ok(value) => Ok(self.normalize_constructed(class_name, value)),
            Err(error) => Err(error),
        }
    }

    fn call_class_method(
        &mut self,
        class_name: &str,
        method: &FunctionDef,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let mut call_args = vec![Value::ClassRef(class_name.to_string())];
        call_args.extend_from_slice(args);
        let method_closure = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&self.env))));
        method_closure
            .borrow_mut()
            .set("__current_class".into(), Value::Str(class_name.to_string()));
        if method.decorators.iter().any(
            |decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager"),
        ) {
            let named_args: Vec<(Option<String>, Value)> = call_args
                .iter()
                .cloned()
                .map(|value| (None, value))
                .collect();
            let bound = self.bind_arguments(&method.params, &named_args)?;
            return self.call_contextmanager_body(
                &method.params,
                &method.body,
                &bound,
                method_closure,
            );
        }
        self.call_function(method, &call_args, method_closure)
    }

    fn normalize_constructed(&self, class_name: &str, value: Value) -> Value {
        let Value::Object {
            class_name: constructed,
            fields,
            is_frozen,
        } = value
        else {
            return value;
        };
        if constructed != "Constructed" {
            return Value::Object {
                class_name: constructed,
                fields,
                is_frozen,
            };
        }
        let Some(class_def) = self.classes.get(class_name) else {
            return Value::Object {
                class_name: constructed,
                fields,
                is_frozen,
            };
        };
        let source = fields.borrow();
        // Preserve methods, getters, setters, and any explicitly named fields
        // installed during class construction; only rewrite positional
        // `field_N` slots to their declared names.
        let mut normalized = source.clone();
        fn declared_fields(
            classes: &HashMap<String, ClassDef>,
            class_def: &ClassDef,
            output: &mut Vec<String>,
        ) {
            for base in &class_def.bases {
                if let TypeExpr::Named { name, .. } = base {
                    if let Some(parent) = classes.get(name) {
                        declared_fields(classes, parent, output);
                    }
                }
            }
            output.extend(class_def.body.iter().filter_map(|member| match member {
                ClassMember::Field(field) => Some(field.name.clone()),
                _ => None,
            }));
        }
        let mut declared = Vec::new();
        declared_fields(&self.classes, class_def, &mut declared);
        for (index, name) in declared.iter().enumerate() {
            if let Some(value) = source.get(&format!("field_{index}")) {
                normalized.insert(name.clone(), value.clone());
            } else if let Some(value) = source.get(name) {
                normalized.insert(name.clone(), value.clone());
            }
        }
        for base in &class_def.bases {
            if let TypeExpr::Named {
                name: trait_name, ..
            } = base
            {
                if !class_def.without_traits.contains(trait_name) {
                    if let Some(trait_body) = self.traits.get(trait_name) {
                        for member in trait_body {
                            if let TraitMember::Method(method) = member {
                                normalized.insert(method.name.clone(), Value::Function {
                                    name: method.name.clone(),
                                    params: method.params.clone(),
                                    body: method.body.clone(),
                                    closure: Rc::clone(&self.env),
                                    is_contextmanager: method.decorators.iter().any(|decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager")),
                                    is_async: method.is_async,
                                });
                            }
                        }
                    }
                }
            }
        }
        for member in &class_def.body {
            if let ClassMember::Method(method) = member {
                normalized.insert(method.name.clone(), Value::Function {
                    name: method.name.clone(),
                    params: method.params.clone(),
                    body: method.body.clone(),
                    closure: Rc::clone(&self.env),
                    is_contextmanager: method.decorators.iter().any(|decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager")),
                    is_async: method.is_async,
                });
            }
        }
        Value::Object {
            class_name: class_name.to_string(),
            fields: Rc::new(RefCell::new(normalized)),
            is_frozen,
        }
    }

    fn call_function(
        &mut self,
        func: &FunctionDef,
        args: &[Value],
        closure: Rc<RefCell<Environment>>,
    ) -> Result<Value, RuntimeError> {
        let call_env = Rc::new(RefCell::new(Environment::with_parent(closure)));
        for (param, val) in func.params.iter().zip(args) {
            call_env.borrow_mut().set(param.name.clone(), val.clone());
        }
        let prev_env = Rc::clone(&self.env);
        let prev_loop_depth = self.loop_depth;
        self.env = call_env;
        self.loop_depth = 0;
        let res = self.eval_block(&func.body);
        self.env = prev_env;
        self.loop_depth = prev_loop_depth;
        match res {
            Ok(Value::Return(v)) => Ok(*v),
            other => other,
        }
    }

    fn invoke_value(
        &mut self,
        value: Value,
        args: Vec<(Option<String>, Value)>,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        match value {
            Value::BuiltinFunction { func, .. } => (func)(
                &args.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
                self,
            ),
            Value::Function {
                name,
                params,
                body,
                closure,
                is_contextmanager,
                is_async,
                ..
            } => {
                if is_contextmanager {
                    let bound = self.bind_arguments(&params, &args)?;
                    return self.call_contextmanager_body(&params, &body, &bound, closure);
                }
                if is_async {
                    let function = Value::Function {
                        name,
                        params,
                        body,
                        closure,
                        is_contextmanager,
                        is_async: false,
                    };
                    return Ok(Value::Future(Rc::new(RefCell::new(Some(FutureTask {
                        function,
                        args,
                    })))));
                }
                let bound = self.bind_arguments(&params, &args)?;
                let call_env = Rc::new(RefCell::new(Environment::with_parent(closure)));
                for (param, val) in params.iter().zip(bound) {
                    call_env.borrow_mut().set(param.name.clone(), val);
                }
                let previous = Rc::clone(&self.env);
                let previous_loop_depth = self.loop_depth;
                self.env = call_env;
                self.loop_depth = 0;
                let result = self.eval_block(&body);
                self.env = previous;
                self.loop_depth = previous_loop_depth;
                match result {
                    Ok(Value::Return(value)) => Ok(*value),
                    Ok(value) => Ok(value),
                    Err(error) => Err(error),
                }
            }
            _ => Err(RuntimeError {
                message: "decorator is not callable".into(),
                span,
            }),
        }
    }

    fn bind_arguments(
        &mut self,
        params: &[Param],
        args: &[(Option<String>, Value)],
    ) -> Result<Vec<Value>, RuntimeError> {
        let mut positional = Vec::new();
        let mut keywords = HashMap::new();
        for (name, value) in args {
            if let Some(name) = name {
                if keywords.insert(name.clone(), value.clone()).is_some() {
                    return Err(RuntimeError {
                        message: format!("multiple values for argument '{name}'"),
                        span: Span::default(),
                    });
                }
            } else {
                positional.push(value.clone());
            }
        }
        let mut position = 0usize;
        let mut bound = Vec::with_capacity(params.len());
        for param in params {
            if param.is_gather {
                let gathered =
                    self.gather_argument_value(param, &positional[position..], &keywords)?;
                position = positional.len();
                keywords.clear();
                bound.push(gathered);
                continue;
            }
            if param.is_variadic_positional {
                bound.push(Value::List(Rc::new(RefCell::new(
                    positional[position..].to_vec(),
                ))));
                position = positional.len();
                continue;
            }
            if param.is_variadic_keyword {
                let rest = keywords.clone();
                keywords.clear();
                bound.push(Value::Dict(Rc::new(RefCell::new(rest))));
                continue;
            }
            let value = if let Some(value) = keywords.remove(&param.name) {
                value
            } else if position < positional.len() {
                let value = positional[position].clone();
                position += 1;
                value
            } else if let Some(default) = &param.default {
                self.eval_expr(default)?
            } else {
                return Err(RuntimeError {
                    message: format!("missing required argument '{}'", param.name),
                    span: param.span,
                });
            };
            bound.push(value);
        }
        if position < positional.len() || !keywords.is_empty() {
            return Err(RuntimeError {
                message: "too many arguments for function".into(),
                span: Span::default(),
            });
        }
        Ok(bound)
    }

    fn gather_argument_value(
        &self,
        param: &Param,
        remaining_positional: &[Value],
        remaining_keywords: &HashMap<String, Value>,
    ) -> Result<Value, RuntimeError> {
        let class_name = match param.type_annotation.as_ref() {
            Some(TypeExpr::Named { name, .. }) => name.as_str(),
            Some(TypeExpr::View { inner, .. }) => match inner.as_ref() {
                TypeExpr::Named { name, .. } => name.as_str(),
                _ => "Arguments",
            },
            _ => "Arguments",
        };
        let is_bundle = matches!(class_name, "Arguments" | "Parameters")
            || class_name.ends_with("Arguments")
            || class_name.ends_with("Parameters");
        let mut fields = HashMap::new();
        if is_bundle {
            fields.insert(
                "vpargs".to_string(),
                Value::List(Rc::new(RefCell::new(remaining_positional.to_vec()))),
            );
            fields.insert(
                "kwargs".to_string(),
                Value::Dict(Rc::new(RefCell::new(
                    remaining_keywords
                        .iter()
                        .map(|(name, value)| (name.clone(), value.clone()))
                        .collect(),
                ))),
            );
            if class_name == "Parameters" || class_name.ends_with("Parameters") {
                fields.insert(
                    "pargs".to_string(),
                    Value::List(Rc::new(RefCell::new(Vec::new()))),
                );
            }
        } else {
            let Some(_class_def) = self.classes.get(class_name) else {
                return Err(RuntimeError {
                    message: format!("unknown gather target class '{class_name}'"),
                    span: param.span,
                });
            };
            let mut names = Vec::new();
            self.collect_inherited_members(class_name, &mut names, &mut fields);
            let mut index = 0;
            let field_count = names.len();
            for name in names {
                let value = remaining_keywords
                    .get(&name)
                    .cloned()
                    .or_else(|| remaining_positional.get(index).cloned())
                    .ok_or_else(|| RuntimeError {
                        message: format!("missing gathered argument '{name}'"),
                        span: param.span,
                    })?;
                if !remaining_keywords.contains_key(&name) {
                    index += 1;
                }
                fields.insert(name, value);
            }
            if index != remaining_positional.len() || fields.len() < field_count {
                return Err(RuntimeError {
                    message: "too many arguments for gathered class".into(),
                    span: param.span,
                });
            }
        }
        Ok(Value::Object {
            class_name: class_name.to_string(),
            fields: Rc::new(RefCell::new(fields)),
            is_frozen: Rc::new(RefCell::new(false)),
        })
    }

    fn call_contextmanager_body(
        &mut self,
        params: &[Param],
        body: &[Stmt],
        args: &[Value],
        closure: Rc<RefCell<Environment>>,
    ) -> Result<Value, RuntimeError> {
        let mut flattened_body = body.to_vec();
        let mut special_error_teardown: Option<Vec<Stmt>> = None;
        if !flattened_body
            .iter()
            .any(|statement| matches!(statement, Stmt::Yield { .. }))
        {
            if let Some((try_index, try_body, handlers, finally_body)) = flattened_body
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
                        Some((index, body, handlers, finally_body))
                    }
                    _ => None,
                })
            {
                let try_body_owned = try_body.to_vec();
                let handlers_owned = handlers.to_vec();
                let finally_owned = finally_body.clone();
                let mut replacement = flattened_body[..try_index].to_vec();
                replacement.extend_from_slice(&try_body_owned);
                if let Some(finally_body) = &finally_owned {
                    replacement.extend_from_slice(finally_body);
                }
                flattened_body = replacement;
                if !handlers_owned.is_empty() {
                    let mut error_teardown = Vec::new();
                    for handler in &handlers_owned {
                        error_teardown.extend_from_slice(&handler.body);
                    }
                    if let Some(finally_body) = &finally_owned {
                        error_teardown.extend_from_slice(finally_body);
                    }
                    special_error_teardown = Some(error_teardown);
                }
            }
        }
        let call_env = Rc::new(RefCell::new(Environment::with_parent(closure)));
        for (param, val) in params.iter().zip(args) {
            call_env.borrow_mut().set(param.name.clone(), val.clone());
        }
        let previous = Rc::clone(&self.env);
        self.env = Rc::clone(&call_env);
        for (index, statement) in flattened_body.iter().enumerate() {
            if let Stmt::Yield { value, .. } = statement {
                let yielded = self.eval_expr(value)?;
                let teardown = flattened_body[index + 1..].to_vec();
                self.env = previous;
                return Ok(Value::ContextManager {
                    value: Box::new(yielded),
                    teardown,
                    error_teardown: special_error_teardown,
                    env: call_env,
                });
            }
            if let Err(error) = self.eval_statement(statement) {
                self.env = previous;
                return Err(error);
            }
        }
        self.env = previous;
        Err(RuntimeError {
            message: "contextmanager must yield exactly once".into(),
            span: Span::default(),
        })
    }

    fn bind_pattern(
        &mut self,
        pattern: &Pattern,
        value: Value,
        span: Span,
    ) -> Result<(), RuntimeError> {
        match pattern {
            Pattern::Ident(name, _) if Self::pattern_identifier_binds(name) => {
                self.env.borrow_mut().set(name.clone(), value);
                Ok(())
            }
            Pattern::Tuple(elements, _) => match value {
                Value::List(items) => {
                    let items_ref = items.borrow();
                    let star_index = elements
                        .iter()
                        .position(|p| matches!(p, Pattern::Star(_, _)));
                    let fixed_count = elements
                        .len()
                        .saturating_sub(usize::from(star_index.is_some()));
                    if (star_index.is_none() && items_ref.len() != elements.len())
                        || (star_index.is_some() && items_ref.len() < fixed_count)
                    {
                        return Err(RuntimeError {
                            message: format!(
                                "unpacking mismatch: expected {} items, got {}",
                                fixed_count,
                                items_ref.len()
                            ),
                            span,
                        });
                    }
                    if let Some(star) = star_index {
                        for (index, pat) in elements[..star].iter().enumerate() {
                            self.bind_pattern(pat, items_ref[index].clone(), span)?;
                        }
                        let tail_count = elements.len() - star - 1;
                        let rest_end = items_ref.len() - tail_count;
                        if let Pattern::Star(nested, _) = &elements[star] {
                            let rest = Value::List(Rc::new(RefCell::new(
                                items_ref[star..rest_end].to_vec(),
                            )));
                            self.bind_pattern(nested, rest, span)?;
                        }
                        for (offset, pat) in elements[star + 1..].iter().enumerate() {
                            self.bind_pattern(pat, items_ref[rest_end + offset].clone(), span)?;
                        }
                    } else {
                        for (pat, val) in elements.iter().zip(items_ref.iter()) {
                            self.bind_pattern(pat, val.clone(), span)?;
                        }
                    }
                    Ok(())
                }
                _ => Err(RuntimeError {
                    message: "cannot unpack non-sequence".to_string(),
                    span,
                }),
            },
            Pattern::RecordDestructure(fields, _) => {
                let values = match value {
                    Value::Record(values) => values.borrow().clone(),
                    Value::Object { fields: values, .. } => values.borrow().clone(),
                    Value::Dict(values) => values
                        .borrow()
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                    _ => {
                        return Err(RuntimeError {
                            message: "cannot destructure non-record value".into(),
                            span,
                        });
                    }
                };
                for (field_name, nested) in fields {
                    let name = field_name.clone().ok_or_else(|| RuntimeError {
                        message: "record destructuring requires field names".into(),
                        span,
                    })?;
                    let field_value = values.get(&name).cloned().ok_or_else(|| RuntimeError {
                        message: format!("missing field '{name}' during destructuring"),
                        span,
                    })?;
                    self.bind_pattern(nested, field_value, span)?;
                }
                Ok(())
            }
            Pattern::Star(nested, _) => self.bind_pattern(nested, value, span),
            Pattern::ClassDestructure {
                class_name, fields, ..
            } => {
                let (actual_class, values) = match value {
                    Value::Object {
                        class_name, fields, ..
                    } => (class_name, fields.borrow().clone()),
                    _ => {
                        return Err(RuntimeError {
                            message: format!("cannot destructure non-{class_name} value"),
                            span,
                        })
                    }
                };
                if actual_class != *class_name && !self.is_subclass(&actual_class, class_name) {
                    return Err(RuntimeError {
                        message: format!("expected {class_name}, got {actual_class}"),
                        span,
                    });
                }
                let declared = self
                    .classes
                    .get(class_name)
                    .map(|class| {
                        class
                            .body
                            .iter()
                            .filter_map(|member| match member {
                                ClassMember::Field(field) => Some(field.name.clone()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                for (index, (field_name, nested)) in fields.iter().enumerate() {
                    let name = field_name
                        .clone()
                        .or_else(|| declared.get(index).cloned())
                        .ok_or_else(|| RuntimeError {
                            message: "class destructuring field is missing".into(),
                            span,
                        })?;
                    let field_value = values.get(&name).cloned().ok_or_else(|| RuntimeError {
                        message: format!("missing field '{name}' during destructuring"),
                        span,
                    })?;
                    self.bind_pattern(nested, field_value, span)?;
                }
                Ok(())
            }
            Pattern::Ident(_, _)
            | Pattern::Literal(_, _)
            | Pattern::Wildcard(_)
            | Pattern::Type(_, _) => Ok(()),
        }
    }

    fn bind_match_pattern(
        &mut self,
        pattern: &Pattern,
        value: Value,
        span: Span,
    ) -> Result<(), RuntimeError> {
        match pattern {
            // In a match, an otherwise-unqualified identifier is a binding;
            // builtin type names and class names are narrowing patterns.
            Pattern::Ident(name, _) if Self::pattern_identifier_binds(name) => {
                self.env.borrow_mut().set(name.clone(), value);
                Ok(())
            }
            Pattern::Tuple(elements, _) => {
                if let Value::List(items) = value {
                    let values = items.borrow().clone();
                    let star_index = elements
                        .iter()
                        .position(|p| matches!(p, Pattern::Star(_, _)));
                    let fixed_count = elements
                        .len()
                        .saturating_sub(usize::from(star_index.is_some()));
                    if (star_index.is_none() && values.len() != elements.len())
                        || (star_index.is_some() && values.len() < fixed_count)
                    {
                        return Err(RuntimeError {
                            message: "match tuple destructuring length mismatch".into(),
                            span,
                        });
                    }
                    if let Some(star) = star_index {
                        for (index, element) in elements[..star].iter().enumerate() {
                            self.bind_match_pattern(element, values[index].clone(), span)?;
                        }
                        let tail_count = elements.len() - star - 1;
                        let rest_end = values.len() - tail_count;
                        if let Pattern::Star(nested, _) = &elements[star] {
                            self.bind_match_pattern(
                                nested,
                                Value::List(Rc::new(RefCell::new(values[star..rest_end].to_vec()))),
                                span,
                            )?;
                        }
                        for (offset, element) in elements[star + 1..].iter().enumerate() {
                            self.bind_match_pattern(
                                element,
                                values[rest_end + offset].clone(),
                                span,
                            )?;
                        }
                    } else {
                        for (element, item) in elements.iter().zip(values) {
                            self.bind_match_pattern(element, item, span)?;
                        }
                    }
                }
                Ok(())
            }
            Pattern::RecordDestructure(fields, _) | Pattern::ClassDestructure { fields, .. } => {
                for (field_name, nested) in fields {
                    let key = field_name.clone().unwrap_or_default();
                    let field_value = match &value {
                        Value::Record(values) => values.borrow().get(&key).cloned(),
                        Value::Object { fields: values, .. } => values.borrow().get(&key).cloned(),
                        _ => None,
                    };
                    if let Some(field_value) = field_value {
                        self.bind_match_pattern(nested, field_value, span)?;
                    }
                }
                Ok(())
            }
            Pattern::Star(nested, _) => self.bind_match_pattern(nested, value, span),
            Pattern::Ident(_, _)
            | Pattern::Literal(_, _)
            | Pattern::Wildcard(_)
            | Pattern::Type(_, _) => Ok(()),
        }
    }

    fn matches_pattern(&self, pattern: &Pattern, value: &Value) -> bool {
        match pattern {
            Pattern::Wildcard(_) => true,
            Pattern::Ident(name, _) => match value {
                Value::Int(_) if name == "int" => true,
                Value::BigInt(_) if name == "int" => true,
                Value::Float(_) if name == "float" => true,
                Value::Complex(_, _) if name == "complex" => true,
                Value::Bool(_) if name == "bool" => true,
                Value::Str(_) if name == "str" => true,
                Value::Bytes(_) if matches!(name.as_str(), "bytes" | "Bytes") => true,
                Value::MemoryView { .. } if name == "MemoryView" => true,
                Value::List(_) if name == "list" => true,
                Value::Set(_) if name == "set" => true,
                Value::Dict(_) if name == "dict" => true,
                Value::Range { .. } if name == "range" => true,
                Value::DottedPath(_) if name == "DottedPath" => true,
                Value::None if matches!(name.as_str(), "none" | "None") => true,
                Value::Object { class_name, .. }
                    if class_name == name || self.is_subclass(class_name, name) =>
                {
                    true
                }
                _ if Self::pattern_identifier_binds(name) => true,
                _ => false,
            },
            Pattern::Literal(lit, _) => match (lit, value) {
                (LiteralValue::Int(a), Value::Int(b)) => a == b,
                (LiteralValue::BigInt(a), Value::BigInt(b)) => {
                    parse_bigint_literal(a).as_ref() == Some(b)
                }
                (LiteralValue::Bool(a), Value::Bool(b)) => a == b,
                (LiteralValue::Str(a), Value::Str(b)) => a == b,
                (LiteralValue::None, Value::None) => true,
                _ => false,
            },
            Pattern::Type(te, _) => match te {
                TypeExpr::Named { name, .. } => match value {
                    Value::List(_) if name == "list" => true,
                    Value::Set(_) if name == "set" => true,
                    Value::Dict(_) if name == "dict" => true,
                    Value::Range { .. } if name == "range" => true,
                    Value::Bytes(_) if matches!(name.as_str(), "bytes" | "Bytes") => true,
                    Value::MemoryView { .. } if name == "MemoryView" => true,
                    Value::DottedPath(_) if name == "DottedPath" => true,
                    Value::Object { class_name, .. }
                        if class_name == name || self.is_subclass(class_name, name) =>
                    {
                        true
                    }
                    Value::Int(_) if name == "int" => true,
                    Value::BigInt(_) if name == "int" => true,
                    Value::Float(_) if name == "float" => true,
                    Value::Complex(_, _) if name == "complex" => true,
                    Value::Str(_) if name == "str" => true,
                    Value::Bool(_) if name == "bool" => true,
                    Value::None if matches!(name.as_str(), "none" | "None") => true,
                    _ => false,
                },
                TypeExpr::Union { types, .. } => types
                    .iter()
                    .any(|ty| self.matches_pattern(&Pattern::Type(ty.clone(), ty.span()), value)),
                TypeExpr::View { inner, .. }
                | TypeExpr::Existential {
                    interface: inner, ..
                }
                | TypeExpr::Reification { inner, .. } => {
                    self.matches_pattern(&Pattern::Type((**inner).clone(), inner.span()), value)
                }
                TypeExpr::Wildcard(_) => true,
                TypeExpr::Never(_) => false,
                TypeExpr::Literal { value: literal, .. } => {
                    self.matches_pattern(&Pattern::Literal(literal.clone(), te.span()), value)
                }
                TypeExpr::Function { .. } | TypeExpr::Record { .. } | TypeExpr::Match { .. } => {
                    false
                }
            },
            Pattern::Tuple(elements, _) => {
                let Value::List(items) = value else {
                    return false;
                };
                let values = items.borrow();
                let star_index = elements
                    .iter()
                    .position(|p| matches!(p, Pattern::Star(_, _)));
                let fixed_count = elements
                    .len()
                    .saturating_sub(usize::from(star_index.is_some()));
                if (star_index.is_none() && values.len() != elements.len())
                    || (star_index.is_some() && values.len() < fixed_count)
                {
                    return false;
                }
                if let Some(star) = star_index {
                    let tail_count = elements.len() - star - 1;
                    let rest_end = values.len() - tail_count;
                    elements[..star]
                        .iter()
                        .enumerate()
                        .all(|(i, p)| self.matches_pattern(p, &values[i]))
                        && self.matches_pattern(
                            &elements[star],
                            &Value::List(Rc::new(RefCell::new(values[star..rest_end].to_vec()))),
                        )
                        && elements[star + 1..]
                            .iter()
                            .enumerate()
                            .all(|(i, p)| self.matches_pattern(p, &values[rest_end + i]))
                } else {
                    elements
                        .iter()
                        .zip(values.iter())
                        .all(|(p, v)| self.matches_pattern(p, v))
                }
            }
            Pattern::Star(nested, _) => self.matches_pattern(nested, value),
            Pattern::RecordDestructure(fields, _) => fields.iter().all(|(name, nested)| {
                let Some(name) = name else {
                    return true;
                };
                let field = match value {
                    Value::Record(values) => values.borrow().get(name).cloned(),
                    Value::Object { fields: values, .. } => values.borrow().get(name).cloned(),
                    Value::Dict(values) => values.borrow().get(name).cloned(),
                    _ => None,
                };
                field.is_some_and(|field| self.matches_pattern(nested, &field))
            }),
            Pattern::ClassDestructure {
                class_name, fields, ..
            } => {
                let Value::Object {
                    class_name: actual, ..
                } = value
                else {
                    return false;
                };
                if actual != class_name && !self.is_subclass(actual, class_name) {
                    return false;
                }
                fields.iter().all(|(name, nested)| {
                    let Some(name) = name else {
                        return true;
                    };
                    let field = match value {
                        Value::Object { fields, .. } => fields.borrow().get(name).cloned(),
                        _ => None,
                    };
                    field.is_some_and(|field| self.matches_pattern(nested, &field))
                })
            }
        }
    }

    fn is_truthy(&mut self, val: &Value) -> bool {
        match val {
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::BigInt(n) => !n.is_zero(),
            Value::Float(f) => *f != 0.0,
            Value::Complex(real, imag) => *real != 0.0 || *imag != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::None => false,
            Value::List(l) => !l.borrow().is_empty(),
            Value::MemoryView { len, .. } => *len > 0,
            Value::Dict(d) => !d.borrow().is_empty(),
            Value::Set(s) => !s.borrow().is_empty(),
            Value::Range { start, stop, step } => {
                if *step > 0 {
                    start < stop
                } else if *step < 0 {
                    start > stop
                } else {
                    false
                }
            }
            Value::Skip => false,
            Value::Object { fields, .. } => {
                if let Some(method) = fields.borrow().get("__bool__").cloned() {
                    return match self.invoke_value(
                        method,
                        vec![(None, val.clone())],
                        Span::default(),
                    ) {
                        Ok(Value::Bool(value)) => value,
                        Ok(Value::Int(value)) => value != 0,
                        Ok(other) => self.is_truthy(&other),
                        Err(_) => false,
                    };
                }
                if let Some(method) = fields.borrow().get("__len__").cloned() {
                    return match self.invoke_value(
                        method,
                        vec![(None, val.clone())],
                        Span::default(),
                    ) {
                        Ok(Value::Int(value)) => value != 0,
                        Ok(other) => self.is_truthy(&other),
                        Err(_) => false,
                    };
                }
                true
            }
            _ => true,
        }
    }

    fn error_matches_handler(&self, error: &RuntimeError, exception_type: &TypeExpr) -> bool {
        match exception_type {
            TypeExpr::Named { name, .. } => {
                if matches!(name.as_str(), "Any" | "Exception" | "BaseException") {
                    return true;
                }
                error
                    .message
                    .starts_with(&format!("raised invariant ({name}):"))
            }
            TypeExpr::Union { types, .. } => types
                .iter()
                .any(|exception_type| self.error_matches_handler(error, exception_type)),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_pattern_binding_skips_type_like_identifiers() {
        assert!(!Interpreter::pattern_identifier_binds("Pair"));
        assert!(!Interpreter::pattern_identifier_binds("int"));
        assert!(!Interpreter::pattern_identifier_binds("complex"));
        assert!(!Interpreter::pattern_identifier_binds("bytes"));
        assert!(!Interpreter::pattern_identifier_binds("MemoryView"));
        assert!(!Interpreter::pattern_identifier_binds("list"));
        assert!(!Interpreter::pattern_identifier_binds("set"));
        assert!(!Interpreter::pattern_identifier_binds("dict"));
        assert!(!Interpreter::pattern_identifier_binds("range"));
        assert!(!Interpreter::pattern_identifier_binds("DottedPath"));
        assert!(!Interpreter::pattern_identifier_binds("_"));
        assert!(Interpreter::pattern_identifier_binds("value"));
    }

    #[test]
    fn runtime_reexports_shared_native_abi() {
        assert_eq!(NativeResult::ok(7).into_result(), Ok(7));
        assert_eq!(
            NativeErrorCode::from_raw(1),
            NativeErrorCode::DivisionByZero
        );
    }
    use lucid_syntax::parse;

    #[test]
    fn test_eval_classes_and_objects() {
        let src = r#"
class Point:
    x: int
    y: int

p = Point(10, 20)
res = p.x + p.y
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let _ = interp.eval_module(&module).unwrap();

        let res = interp.env.borrow().get("res").unwrap();
        assert_eq!(res, Value::Int(30));
    }

    #[test]
    fn test_freeze_prevents_mutation() {
        let src = r#"
class Account:
    balance: int

acc = Account(100)
frozen = freeze(acc)
acc.balance = 200
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let err = interp.eval_module(&module);
        assert!(err.is_err());
        assert!(err
            .unwrap_err()
            .message
            .contains("cannot mutate attribute 'balance' on frozen object !Account"));
    }

    #[test]
    fn test_freeze_handles_cyclic_aliases() {
        let items = Rc::new(RefCell::new(Vec::new()));
        let cycle = Value::List(Rc::clone(&items));
        items.borrow_mut().push(cycle.clone());

        cycle.freeze();
        assert_eq!(items.borrow().len(), 1);
    }

    #[test]
    fn test_freeze_prevents_mutating_containers() {
        let src = r#"
items = [1, 2]
freeze(items)
items.append(3)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let err = interp.eval_module(&module).unwrap_err();
        assert!(err.message.contains("cannot mutate frozen list"));
    }

    #[test]
    fn test_frozen_containers_are_hashable() {
        let module = parse("items = freeze([1, 2])\nh = hash(items)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(matches!(interp.env.borrow().get("h"), Some(Value::Int(_))));
    }

    #[test]
    fn test_final_variable_cannot_be_reassigned_at_runtime() {
        let module = parse("final answer: int = 1\nanswer = 2\n").unwrap();
        let mut interp = Interpreter::new();
        let err = interp.eval_module(&module).unwrap_err();
        assert!(err.message.contains("final variable 'answer'"));
    }

    #[test]
    fn test_final_field_cannot_be_reassigned_at_runtime() {
        let module = parse("class User:\n    final id: int\n\nu = User(1)\nu.id = 2\n").unwrap();
        let mut interp = Interpreter::new();
        let err = interp.eval_module(&module).unwrap_err();
        assert!(err.message.contains("final field 'id'"));
    }

    #[test]
    fn test_final_class_cannot_be_inherited_at_runtime() {
        let module =
            parse("final class Base:\n    pass\n\nclass Child(Base):\n    pass\n").unwrap();
        let mut interp = Interpreter::new();
        let err = interp.eval_module(&module).unwrap_err();
        assert!(err.message.contains("final and cannot be inherited"));
    }

    #[test]
    fn test_final_class_rule_is_order_independent_at_runtime() {
        let module =
            parse("class Child(Base):\n    pass\n\nfinal class Base:\n    pass\n").unwrap();
        let mut interp = Interpreter::new();
        let err = interp.eval_module(&module).unwrap_err();
        assert!(err.message.contains("final and cannot be inherited"));
    }

    #[test]
    fn test_sealed_class_rejects_subclass_from_another_file() {
        let mut interp = Interpreter::new();
        interp.set_current_file(Some(std::path::PathBuf::from("base.lucid")));
        interp
            .eval_module(&parse("sealed class Base:\n    pass\n").unwrap())
            .unwrap();
        interp.set_current_file(Some(std::path::PathBuf::from("other.lucid")));
        let err = interp
            .eval_module(&parse("class Child(Base):\n    pass\n").unwrap())
            .unwrap_err();
        assert!(err.message.contains("sealed class 'Base'"));
    }

    #[test]
    fn test_private_members_are_visible_only_to_declaring_class() {
        let module = parse(
            "class Secret:\n    _value: int\n    def get(self) -> int:\n        return self._value\n\ns = Secret(7)\ninside = s.get()\noutside = s._value\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        let err = interp.eval_module(&module).unwrap_err();
        assert!(err.message.contains("member '_value' is private"));

        let module = parse(
            "class Secret:\n    _value: int\n    def get(self) -> int:\n        return self._value\n\ns = Secret(7)\ninside = s.get()\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("inside"), Some(Value::Int(7)));
    }

    #[test]
    fn test_class_variables_are_shared_and_inherited() {
        let module = parse(
            "class Counter:\n    classvar count: int = 0\n    classmethod next(cls) -> int:\n        cls.count = cls.count + 1\n        return cls.count\n\nclass Child(Counter):\n    pass\na = Counter.next()\nb = Child.next()\nvalue = Counter.count\nchild_value = Child.count\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Int(1)));
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Int(2)));
        assert_eq!(interp.env.borrow().get("value"), Some(Value::Int(2)));
        assert_eq!(interp.env.borrow().get("child_value"), Some(Value::Int(2)));
    }

    #[test]
    fn test_retroactive_trait_implementation_adds_methods() {
        let module = parse(
            "class Widget:\n    value: int\n\ntrait Describable:\n    def describe(self) -> str:\n        return \"widget\"\n\nimplement Describable for Widget:\n    def describe(self) -> str:\n        return \"widget\"\n\nw = Widget(3)\nresult = w.describe()\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("widget".into()))
        );
    }

    #[test]
    fn test_delete_ends_name_lifetime_at_runtime() {
        let module = parse("value: int = 1\ndel value\nvalue = 2\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("value"), Some(Value::Int(2)));

        let bad = parse("value: int = 1\ndel value\nresult = value\n").unwrap();
        let mut interp = Interpreter::new();
        let err = interp.eval_module(&bad).unwrap_err();
        assert!(err.message.contains("undefined variable 'value'"));
    }

    #[test]
    fn test_fresh_loop_bindings() {
        let src = r#"
total = 0
for i in [1, 2, 3]:
    total = total + i
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let _ = interp.eval_module(&module).unwrap();

        let total = interp.env.borrow().get("total").unwrap();
        assert_eq!(total, Value::Int(6));
    }

    #[test]
    fn loop_closures_capture_each_iteration_binding() {
        let module = parse(
            "fns = []\nfor i in [1, 2, 3]:\n    fns.append(def(): i)\nfirst = fns[0]()\nsecond = fns[1]()\nthird = fns[2]()\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("first"), Some(Value::Int(1)));
        assert_eq!(interp.env.borrow().get("second"), Some(Value::Int(2)));
        assert_eq!(interp.env.borrow().get("third"), Some(Value::Int(3)));
    }

    #[test]
    fn comprehension_closures_capture_each_iteration_binding() {
        let module = parse(
            "fns = [def(): i for i in [1, 2, 3]]\nfirst = fns[0]()\nsecond = fns[1]()\nthird = fns[2]()\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("first"), Some(Value::Int(1)));
        assert_eq!(interp.env.borrow().get("second"), Some(Value::Int(2)));
        assert_eq!(interp.env.borrow().get("third"), Some(Value::Int(3)));
    }

    #[test]
    fn recursive_closure_escapes_defining_function() {
        let module = parse(
            "def make() -> (int) -> int:\n    fact = def(n: int) -> int: 1 if n == 0 else n * fact(n - 1)\n    return fact\nrun = make()\nresult = run(5)\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(120)));
    }

    #[test]
    fn comprehension_target_does_not_rebind_outer_name() {
        let module = parse("i = 99\nvalues = [i for i in [1, 2]]\nresult = i\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(99)));
        assert_eq!(
            interp.env.borrow().get("values"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Int(1),
                Value::Int(2),
            ]))))
        );
    }

    #[test]
    fn test_break_and_continue_outside_loops_are_runtime_errors() {
        for (source, message) in [
            ("break\n", "break is only valid inside a loop"),
            ("continue\n", "continue is only valid inside a loop"),
        ] {
            let module = parse(source).unwrap();
            let mut interp = Interpreter::new();
            let error = interp.eval_module(&module).unwrap_err();
            assert!(error.message.contains(message));
        }
    }

    #[test]
    fn failed_loop_binding_restores_outer_environment_for_handlers() {
        let module = parse(
            "handled = 0\ntry:\n    for (left, right) in [1]:\n        pass\nexcept:\n    handled = handled + 1\nresult = handled\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(1)));
    }

    #[test]
    fn test_called_functions_do_not_inherit_loop_control_context() {
        let module = parse("def invalid():\n    break\nfor item in [1]:\n    invalid()\n").unwrap();
        let mut interp = Interpreter::new();
        let error = interp.eval_module(&module).unwrap_err();
        assert!(error.message.contains("break is only valid inside a loop"));
    }

    #[test]
    fn test_top_level_return_is_runtime_error() {
        let module = parse("return 1\n").unwrap();
        let mut interp = Interpreter::new();
        let error = interp.eval_module(&module).unwrap_err();
        assert!(error
            .message
            .contains("return is only valid inside a function"));
        let module = parse("yield 1\n").unwrap();
        let mut interp = Interpreter::new();
        let error = interp.eval_module(&module).unwrap_err();
        assert!(error
            .message
            .contains("yield is only valid in a contextmanager definition"));
    }

    #[test]
    fn test_if_broken_clause() {
        let src = r#"
found = 0
for x in [1, 2, 3, 4]:
    if x == 3:
        found = x
        break
if_broken:
    found = 99
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let _ = interp.eval_module(&module).unwrap();

        let found = interp.env.borrow().get("found").unwrap();
        assert_eq!(found, Value::Int(99));
    }

    #[test]
    fn test_truthiness_uses_custom_bool_and_len() {
        let source = "class Flag:\n    value: bool\n    def __bool__(self) -> bool:\n        return self.value\nclass Sized:\n    items: list[int]\n    def __len__(self) -> int:\n        return len(self.items)\nif Flag(false):\n    print(1)\nelse:\n    print(0)\nif Sized([]):\n    print(1)\nelse:\n    print(0)\n";
        let module = parse(source).expect("custom truthiness source should parse");
        let mut interp = Interpreter::new();
        interp
            .eval_module(&module)
            .expect("custom truthiness should run");
    }

    #[test]
    fn test_builtin_truthiness_handles_empty_collections_and_numeric_zero() {
        let source = "if 0:\n    result = 1\nelse:\n    result = 0\nif complex(0, 0):\n    complex_result = 1\nelse:\n    complex_result = 0\nif {}:\n    dict_result = 1\nelse:\n    dict_result = 0\nif set():\n    set_result = 1\nelse:\n    set_result = 0\nif 100000000000000000000 - 100000000000000000000:\n    bigint_result = 1\nelse:\n    bigint_result = 0\n";
        let module = parse(source).expect("builtin truthiness source should parse");
        let mut interp = Interpreter::new();
        interp
            .eval_module(&module)
            .expect("builtin truthiness should run");
        let env = interp.env.borrow();
        assert_eq!(env.get("result"), Some(Value::Int(0)));
        assert_eq!(env.get("complex_result"), Some(Value::Int(0)));
        assert_eq!(env.get("dict_result"), Some(Value::Int(0)));
        assert_eq!(env.get("set_result"), Some(Value::Int(0)));
        assert_eq!(env.get("bigint_result"), Some(Value::Int(0)));
    }

    #[test]
    fn test_membership_uses_custom_contains_method() {
        let source = "class Bag:\n    value: int\n    def __contains__(self, item: int) -> bool:\n        return item == self.value\nif 4 in Bag(4):\n    print(1)\nif 3 not in Bag(4):\n    print(1)\n";
        let module = parse(source).expect("custom contains source should parse");
        let mut interp = Interpreter::new();
        interp
            .eval_module(&module)
            .expect("custom contains should run");
    }

    #[test]
    fn test_multiple_dispatch_functions() {
        let src = r#"
class Circle:
    radius: int

class Square:
    side: int

dispatch def area(c: Circle) -> int:
    return c.radius + c.radius

dispatch def area(s: Square) -> int:
    return s.side + s.side

c = Circle(10)
s = Square(20)
a1 = area(c)
a2 = area(s)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let _ = interp.eval_module(&module).unwrap();

        let a1 = interp.env.borrow().get("a1").unwrap();
        let a2 = interp.env.borrow().get("a2").unwrap();
        assert_eq!(a1, Value::Int(20));
        assert_eq!(a2, Value::Int(40));
    }

    #[test]
    fn test_dispatch_prefers_exact_type_over_object_fallback() {
        let src = r#"
dispatch def choose(value: object) -> str:
    return "object"

dispatch def choose(value: object) -> str:
    return "duplicate-object"

dispatch def choose(value: int) -> str:
    return "int"

result = choose(1)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("int".into()))
        );
    }

    #[test]
    fn test_dispatch_walks_class_hierarchy() {
        let src = r#"
class Animal:
    pass
class Dog(Animal):
    pass
class Puppy(Dog):
    pass
dispatch def choose(value: object) -> str:
    return "object"
dispatch def choose(value: Animal) -> str:
    return "animal"
result = choose(Puppy())
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("animal".into()))
        );
    }

    #[test]
    fn test_dispatch_rejects_equal_distance_matches() {
        let src = r#"
dispatch def choose(value: int) -> str:
    return "first"
dispatch def choose(value: int) -> str:
    return "second"
result = choose(1)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let error = interp.eval_module(&module).unwrap_err();
        assert!(error.message.contains("no dispatch method found"));
    }

    #[test]
    fn public_dispatch_table_accepts_lucidval_fallback() {
        let mut table = DispatchTable::default();
        table.register(
            "choose".into(),
            vec!["LucidVal".into()],
            Rc::new(|_, _| Ok(Value::Str("ok".into()))),
        );
        let selected = table.find_match("choose", &[Value::Int(1)]);
        assert!(selected.is_some());
    }

    #[test]
    fn test_error_propagation_question_mark() {
        let src = r#"
class NotFoundError:
    message: str

def lookup(key: str):
    if key == "missing":
        return NotFoundError("item missing")
    return 100

def get_doubled(key: str):
    val = lookup(key)?
    return val + 1

r1 = get_doubled("present")
r2 = get_doubled("missing")
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let _ = interp.eval_module(&module).unwrap();

        let r1 = interp.env.borrow().get("r1").unwrap();
        assert_eq!(r1, Value::Int(101));

        let r2 = interp.env.borrow().get("r2").unwrap();
        match r2 {
            Value::Object { class_name, .. } => assert_eq!(class_name, "NotFoundError"),
            other => panic!("expected NotFoundError, got {:?}", other),
        }
    }

    #[test]
    fn test_match_binds_identifier_patterns() {
        let src = r#"
value = 2
match value:
    case 1:
        result = 0
    case n:
        result = n + 3
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(5)));
    }

    #[test]
    fn test_match_bytes_and_memoryview_patterns() {
        let src = r#"
data = b"ab"
match data:
    case bytes:
        bytes_result = 1
    case _:
        bytes_result = 0

view = memoryview(data)
match view:
    case MemoryView:
        view_result = 1
    case _:
        view_result = 0

other = 3
match other:
    case bytes:
        other_result = 1
    case _:
        other_result = 0

items = [1, 2]
match items:
    case list:
        list_result = 1
    case _:
        list_result = 0

unique = {1, 2}
match unique:
    case set:
        set_result = 1
    case _:
        set_result = 0

mapping = {"a": 1}
match mapping:
    case dict:
        dict_result = 1
    case _:
        dict_result = 0

span = range(3)
match span:
    case range:
        range_result = 1
    case _:
        range_result = 0

def sample() -> int:
    return 1
path = sample.__path__
match path:
    case DottedPath:
        path_result = 1
    case _:
        path_result = 0

doc = sample.__doc__
match doc:
    case None:
        none_result = 1
    case _:
        none_result = 0
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        let env = interp.env.borrow();
        assert_eq!(env.get("bytes_result"), Some(Value::Int(1)));
        assert_eq!(env.get("view_result"), Some(Value::Int(1)));
        assert_eq!(env.get("other_result"), Some(Value::Int(0)));
        assert_eq!(env.get("list_result"), Some(Value::Int(1)));
        assert_eq!(env.get("set_result"), Some(Value::Int(1)));
        assert_eq!(env.get("dict_result"), Some(Value::Int(1)));
        assert_eq!(env.get("range_result"), Some(Value::Int(1)));
        assert_eq!(env.get("path_result"), Some(Value::Int(1)));
        assert_eq!(env.get("none_result"), Some(Value::Int(1)));
    }

    #[test]
    fn test_match_generic_type_patterns_check_value_kind() {
        let src = r#"
items = [1, 2]
match items:
    case list[int]:
        list_result = 1
    case _:
        list_result = 0

unique = {1, 2}
match unique:
    case set[int]:
        set_result = 1
    case _:
        set_result = 0

mapping = {"a": 1}
match mapping:
    case dict[str, int]:
        dict_result = 1
    case _:
        dict_result = 0

other = 3
match other:
    case list[int]:
        other_result = 1
    case _:
        other_result = 0
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        let env = interp.env.borrow();
        assert_eq!(env.get("list_result"), Some(Value::Int(1)));
        assert_eq!(env.get("set_result"), Some(Value::Int(1)));
        assert_eq!(env.get("dict_result"), Some(Value::Int(1)));
        assert_eq!(env.get("other_result"), Some(Value::Int(0)));
    }

    #[test]
    fn test_non_name_type_patterns_do_not_match_everything() {
        let interp = Interpreter::new();
        let span = Span::default();
        let function_pattern = Pattern::Type(
            TypeExpr::Function {
                params: vec![TypeExpr::Named {
                    name: "int".to_string(),
                    args: Vec::new(),
                    span,
                }],
                return_type: Box::new(TypeExpr::Named {
                    name: "int".to_string(),
                    args: Vec::new(),
                    span,
                }),
                span,
            },
            span,
        );
        assert!(!interp.matches_pattern(&function_pattern, &Value::Int(1)));

        let union_pattern = Pattern::Type(
            TypeExpr::Union {
                types: vec![
                    TypeExpr::Named {
                        name: "int".to_string(),
                        args: Vec::new(),
                        span,
                    },
                    TypeExpr::Named {
                        name: "str".to_string(),
                        args: Vec::new(),
                        span,
                    },
                ],
                span,
            },
            span,
        );
        assert!(interp.matches_pattern(&union_pattern, &Value::Str("ok".to_string())));
        assert!(!interp.matches_pattern(&union_pattern, &Value::Bool(true)));
    }

    #[test]
    fn test_match_guard_false_falls_through() {
        let src = r#"
value = 2
match value:
    case n if n > 10:
        result = n
    case _:
        result = 99
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(99)));
    }

    #[test]
    fn test_match_guard_can_use_subject_alias_and_pattern_binding() {
        let src = r#"
value = 4
match value as subject:
    case n if subject == n:
        result = n + subject
    case _:
        result = 0
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(8)));
    }

    #[test]
    fn test_tuple_patterns_support_starred_remainder() {
        let src = r#"
value = [1, 2, 3, 4]
first, *middle, last = value
match value:
    case (1, *rest, 4):
        matched = rest
    case _:
        matched = []
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("first"), Some(Value::Int(1)));
        assert_eq!(interp.env.borrow().get("last"), Some(Value::Int(4)));
        assert_eq!(
            interp.env.borrow().get("middle"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Int(2),
                Value::Int(3)
            ]))))
        );
        assert_eq!(
            interp.env.borrow().get("matched"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Int(2),
                Value::Int(3)
            ]))))
        );
    }

    #[test]
    fn test_contextmanager_runs_setup_and_teardown() {
        let src = r#"
events = []
contextmanager def managed():
    yield 7
    events.append("done")

with managed() as value:
    result = value
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(7)));
        let events = interp.env.borrow().get("events").unwrap();
        assert_eq!(
            events,
            Value::List(Rc::new(RefCell::new(vec![Value::Str("done".into())])))
        );
    }

    #[test]
    fn test_contextmanager_cleanup_preserves_body_error() {
        let src = r#"
events = []
contextmanager def managed():
    yield none
    events.append("cleanup")

with managed():
    raise "body failed"
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let error = interp
            .eval_module(&module)
            .expect_err("body error should propagate");
        assert!(error.message.contains("body failed"));
        assert_eq!(
            interp.env.borrow().get("events"),
            Some(Value::List(Rc::new(RefCell::new(vec![Value::Str(
                "cleanup".into()
            )]))))
        );
    }

    #[test]
    fn test_try_selects_matching_exception_handler() {
        let src = r#"
try:
    raise "boom"
except int:
    result = "wrong"
except str:
    result = "right"
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("right".into()))
        );
    }

    #[test]
    fn test_try_union_handler_matches_only_union_members() {
        let src = r#"
first = ""
try:
    raise "boom"
except int | bool:
    first = "wrong"
except str:
    first = "right"

second = ""
try:
    raise true
except int | str:
    second = "wrong"
except bool:
    second = "right"
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        let env = interp.env.borrow();
        assert_eq!(env.get("first"), Some(Value::Str("right".into())));
        assert_eq!(env.get("second"), Some(Value::Str("right".into())));
    }

    #[test]
    fn test_getter_is_evaluated_as_attribute() {
        let src = r#"
class Point:
    x: int
    y: int
    getter total(self) -> int:
        return self.x + self.y

p = Point(2, 3)
result = p.total
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(5)));
    }

    #[test]
    fn test_setter_runs_on_attribute_assignment() {
        let src = r#"
class Box:
    value: int
    setter value(self, new_value: int):
        self.value = new_value + 1

b = Box(0)
b.value = 4
result = b.value
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(5)));
    }

    #[test]
    fn test_named_factory_and_classmethod_calls() {
        let src = r#"
class Point:
    x: int
    y: int
    factory origin(cls) -> Point:
        return construct(0, 0)
    classmethod label(cls) -> str:
        return "Point"

p = Point.origin()
result = p.x + p.y
name = Point.label()
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(0)));
        assert_eq!(
            interp.env.borrow().get("name"),
            Some(Value::Str("Point".into()))
        );
    }

    #[test]
    fn test_class_inheritance_preserves_parent_layout() {
        let src = r#"
class Base:
    x: int
    def double(self) -> int:
        return self.x * 2

class Child(Base):
    y: int

c = Child(3, 4)
result = c.x + c.y + c.double()

class ChildWithFactory(Base):
    y: int
    factory __init__(cls, x: int, y: int):
        return construct(x, y)

cf = ChildWithFactory(7, 8)
factory_result = cf.x + cf.y
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(13)));
        assert_eq!(
            interp.env.borrow().get("factory_result"),
            Some(Value::Int(15))
        );
    }

    #[test]
    fn test_named_default_and_variadic_arguments_bind_by_parameter_zone() {
        let src = r#"
def summarize(a: int, b: int = 2, *rest: int) -> int:
    total = a + b
    for item in rest:
        total += item
    return total

result = summarize(1, 4, 5, 6)
named = summarize(b=3, a=2)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(16)));
        assert_eq!(interp.env.borrow().get("named"), Some(Value::Int(5)));
    }

    #[test]
    fn test_variadic_keyword_arguments_are_collected() {
        let src = r#"
def count_named(**kwargs: int) -> int:
    return len(kwargs)

result = count_named(alpha=1, beta=2)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(2)));
    }

    #[test]
    fn test_async_function_returns_single_use_future() {
        let src = r#"
async def load(x: int) -> int:
    return x + 1

pending = load(4)
result = await pending
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(5)));

        let second =
            parse("async def load() -> int:\n    return 1\nf = load()\na = await f\nb = await f\n")
                .unwrap();
        let mut interp = Interpreter::new();
        let err = interp.eval_module(&second).unwrap_err();
        assert!(err.message.contains("already been awaited"));
    }

    #[test]
    fn test_async_methods_and_classmethods_return_futures() {
        let src = r#"
class Worker:
    value: int
    async def run(self) -> int:
        return self.value
    async classmethod answer(cls) -> int:
        return 42

w = Worker(7)
a = await w.run()
b = await Worker.answer()
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Int(7)));
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Int(42)));
    }

    #[test]
    fn test_contextmanager_try_finally_teardown() {
        let src = r#"
contextmanager def managed() -> int:
    events = []
    try:
        yield 5
    finally:
        print("teardown")

with managed() as value:
    print(value)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
    }

    #[test]
    fn test_contextmanager_try_except_handles_body_error() {
        let src = r#"
contextmanager def managed() -> int:
    try:
        yield 5
    except:
        handled = true

with managed() as value:
    raise "boom"
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        assert!(interp.eval_module(&module).is_ok());
    }

    #[test]
    fn test_builtins_range_len_min_max_sum() {
        let src = r#"
r = range(5)
empty = range(5, 5)
l = len(r)
m1 = min(r)
m2 = max(r)
s = sum(r)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let _ = interp.eval_module(&module).unwrap();

        assert_eq!(interp.env.borrow().get("l").unwrap(), Value::Int(5));
        assert_eq!(interp.env.borrow().get("m1").unwrap(), Value::Int(0));
        assert_eq!(interp.env.borrow().get("m2").unwrap(), Value::Int(4));
        assert_eq!(interp.env.borrow().get("s").unwrap(), Value::Int(10));
        let empty = interp.env.borrow().get("empty").unwrap().clone();
        assert!(!interp.is_truthy(&empty));
    }

    #[test]
    fn test_complex_literals_and_arithmetic() {
        let module =
            parse("z = 1 + 2j\nw = z * (3 + 4j)\nneg = -z\nresult = z == complex(1, 2)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("z"), Some(Value::Complex(1.0, 2.0)));
        assert_eq!(
            interp.env.borrow().get("w"),
            Some(Value::Complex(-5.0, 10.0))
        );
        assert_eq!(
            interp.env.borrow().get("neg"),
            Some(Value::Complex(-1.0, -2.0))
        );
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Bool(true)));
    }

    #[test]
    fn complex_real_and_imag_attributes_return_float_components() {
        let module = parse("z = 1 + 2j\nreal = z.real\nimag = z.imag\n").unwrap();
        let mut interp = Interpreter::default();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("real"), Some(Value::Float(1.0)));
        assert_eq!(interp.env.borrow().get("imag"), Some(Value::Float(2.0)));
    }

    #[test]
    fn test_numeric_special_class_values() {
        let module = parse("a = float.inf\nb = float.nan\nc = complex.nan\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("a"),
            Some(Value::Float(f64::INFINITY))
        );
        assert!(matches!(interp.env.borrow().get("b"), Some(Value::Float(v)) if v.is_nan()));
        assert!(
            matches!(interp.env.borrow().get("c"), Some(Value::Complex(r, i)) if r.is_nan() && i.is_nan())
        );
    }

    #[test]
    fn test_float_remainder_matches_floor_division() {
        let module = parse(
            "a = -5.0 % 2.0\nb = 5.0 % -2.0\nc = 5.0 % float.inf\nd = -5 % 2.0\ne = 5.0 % -2\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Float(1.0)));
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Float(-1.0)));
        assert_eq!(interp.env.borrow().get("c"), Some(Value::Float(5.0)));
        assert_eq!(interp.env.borrow().get("d"), Some(Value::Float(1.0)));
        assert_eq!(interp.env.borrow().get("e"), Some(Value::Float(-1.0)));
    }

    #[test]
    fn test_integer_special_values_and_zero_division() {
        let module =
            parse("a = int.inf\nb = int.nan\nc = 5 // 0\nd = 0 // 0\ne = 5 % 0\nf = 5 / 0\n")
                .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Int(INT_POS_INF)));
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Int(INT_NAN)));
        assert_eq!(interp.env.borrow().get("c"), Some(Value::Int(INT_POS_INF)));
        assert_eq!(interp.env.borrow().get("d"), Some(Value::Int(INT_NAN)));
        assert_eq!(interp.env.borrow().get("e"), Some(Value::Int(INT_NAN)));
        assert!(matches!(interp.env.borrow().get("f"), Some(Value::Float(v)) if v.is_infinite()));
    }

    #[test]
    fn range_materialization_stops_at_integer_overflow() {
        let values = materialize_range(i64::MAX - 1, i64::MAX, 2);
        assert_eq!(values, vec![Value::Int(i64::MAX - 1)]);
    }

    #[test]
    fn range_consuming_builtins_share_checked_materialization() {
        let module = parse(
            "low = min(range(9223372036854775806, 9223372036854775807, 2))\nmaximal = max(range(-9223372036854775806, (-9223372036854775807 - 1), -2))\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("low"),
            Some(Value::Int(i64::MAX - 1))
        );
        assert_eq!(
            interp.env.borrow().get("maximal"),
            Some(Value::Int(i64::MIN + 2))
        );
    }

    #[test]
    fn range_indexing_uses_sequence_positions() {
        let module = parse("a = range(1, 6, 2)[1]\nb = range(5, 0, -2)[-1]\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Int(3)));
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Int(1)));
    }

    #[test]
    fn range_slicing_materializes_sequence_slice() {
        let module = parse("a = range(0, 6)[1:5:2]\nb = range(5, 0, -1)[::-2]\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("a"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Int(1),
                Value::Int(3)
            ]))))
        );
        assert_eq!(
            interp.env.borrow().get("b"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Int(1),
                Value::Int(3),
                Value::Int(5)
            ]))))
        );
    }

    #[test]
    fn range_membership_uses_arithmetic_progression() {
        let module = parse("a = 4 in range(0, 10, 2)\nb = 5 in range(0, 10, 2)\nc = 3 in range(5, 0, -2)\nd = 4 not in range(5, 0, -2)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Bool(true)));
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Bool(false)));
        assert_eq!(interp.env.borrow().get("c"), Some(Value::Bool(true)));
        assert_eq!(interp.env.borrow().get("d"), Some(Value::Bool(true)));
    }

    #[test]
    fn integer_floor_operations_report_min_over_neg_one_without_panicking() {
        for source in [
            "value = (-9223372036854775807 - 1) // -1\n",
            "value = (-9223372036854775807 - 1) % -1\n",
        ] {
            let module = parse(source).unwrap();
            let mut interp = Interpreter::new();
            let error = interp
                .eval_module(&module)
                .expect_err("unrepresentable result must error");
            assert!(error.message.contains("integer overflow"));
        }
    }

    #[test]
    fn test_arbitrary_precision_integer_literals_and_arithmetic() {
        let module = parse("x = 999999999999999999999999999999\ny = x + 2\nz = y * 3\nq = z // 3\nr = z % 7\np = x ** 2\na = -5 // 2\nb = -5 % 2\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("x"),
            Some(Value::BigInt(
                "999999999999999999999999999999".parse().unwrap()
            ))
        );
        assert_eq!(
            interp.env.borrow().get("y"),
            Some(Value::BigInt(
                "1000000000000000000000000000001".parse().unwrap()
            ))
        );
        assert_eq!(
            interp.env.borrow().get("z"),
            Some(Value::BigInt(
                "3000000000000000000000000000003".parse().unwrap()
            ))
        );
        assert_eq!(
            interp.env.borrow().get("q"),
            Some(Value::BigInt(
                "1000000000000000000000000000001".parse().unwrap()
            ))
        );
        assert_eq!(
            interp.env.borrow().get("r"),
            Some(Value::BigInt("6".parse().unwrap()))
        );
        assert_eq!(
            interp.env.borrow().get("p"),
            Some(Value::BigInt(
                "999999999999999999999999999998000000000000000000000000000001"
                    .parse()
                    .unwrap()
            ))
        );
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Int(-3)));
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Int(1)));
    }

    #[test]
    fn test_radix_bigint_literals_round_trip_through_runtime() {
        let module = parse("x = 0x8000000000000000\ny = 0o1000000000000000000000\nz = 0b1_00000000000000000000000000000000\nw = -0x8000000000000000\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("x"),
            Some(Value::BigInt(BigInt::from(1u8) << 63))
        );
        assert_eq!(
            interp.env.borrow().get("y"),
            Some(Value::BigInt(BigInt::from(1u8) << 63))
        );
        assert_eq!(
            interp.env.borrow().get("z"),
            Some(Value::BigInt(BigInt::from(1u8) << 32))
        );
        let expected_w = -(BigInt::from(1u8) << 63usize);
        assert_eq!(
            interp.env.borrow().get("w"),
            Some(Value::BigInt(expected_w))
        );
    }

    #[test]
    fn test_abs_supports_bigints_and_complex_numbers() {
        let module = parse("a = abs(-999999999999999999999999999999)\nb = abs(3 + 4j)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("a"),
            Some(Value::BigInt(
                "999999999999999999999999999999".parse().unwrap()
            ))
        );
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Float(5.0)));
    }

    #[test]
    fn test_three_argument_pow_uses_modular_exponentiation() {
        let module = parse("result = pow(2, 10, 1000)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(24)));
    }

    #[test]
    fn test_three_argument_pow_supports_bigints() {
        let module = parse("result = pow(100000000000000000000, 2, 1000)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::BigInt(BigInt::from(0)))
        );
    }

    #[test]
    fn test_bigint_exponent_keeps_exact_integer_result() {
        let module = parse("result = 1 ** 9223372036854775808\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::BigInt(BigInt::from(1)))
        );
    }

    #[test]
    fn test_integer_special_negation_preserves_sentinels() {
        let module = parse("a = -int.inf\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Int(INT_NEG_INF)));
    }

    #[test]
    fn test_complex_power() {
        let module = parse("result = (1 + 1j) ** (1 + 1j)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(
            matches!(interp.env.borrow().get("result"), Some(Value::Complex(real, imag)) if (real - 0.273957).abs() < 1e-5 && (imag - 0.583701).abs() < 1e-5)
        );
    }

    #[test]
    fn test_builtins_any_and_all() {
        let module = parse("a = any([false, true])\nb = all([true, true])\nc = all([])\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Bool(true)));
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Bool(true)));
        assert_eq!(interp.env.borrow().get("c"), Some(Value::Bool(true)));
    }

    #[test]
    fn test_iterable_builtins_accept_strings_and_sets() {
        let module = parse("a = reversed(\"abc\".chars)\nb = enumerate({4, 5}, 7)\nc = zip(\"ab\".chars, {8, 9})\nd = any(\"\".chars)\ne = all({1, 2})\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("a").and_then(|v| match v {
                Value::List(xs) => xs.borrow().first().cloned(),
                _ => None,
            }),
            Some(Value::Str("c".into()))
        );
        assert_eq!(
            interp
                .env
                .borrow()
                .get("b")
                .and_then(|v| match v {
                    Value::List(xs) => xs.borrow().first().cloned(),
                    _ => None,
                })
                .and_then(|v| match v {
                    Value::List(pair) => pair.borrow().first().cloned(),
                    _ => None,
                }),
            Some(Value::Int(7))
        );
        assert!(matches!(interp.env.borrow().get("c"), Some(Value::List(_))));
        assert_eq!(interp.env.borrow().get("d"), Some(Value::Bool(false)));
        assert_eq!(interp.env.borrow().get("e"), Some(Value::Bool(true)));
    }

    #[test]
    fn non_iterable_string_builtins_reject_raw_strings() {
        for source in [
            "any(\"abc\")\n",
            "all(\"abc\")\n",
            "iter(\"abc\")\n",
            "enumerate(\"abc\")\n",
            "reversed(\"abc\")\n",
            "sum(\"abc\")\n",
        ] {
            let mut interp = Interpreter::new();
            let error = interp
                .eval_module(&parse(source).unwrap())
                .expect_err("raw strings are not Iterable/Reversible");
            assert!(
                error.message.contains("not iterable") || error.message.contains("not reversible"),
                "{source}: {}",
                error.message
            );
        }
    }

    #[test]
    fn test_iteration_done_sentinel_module() {
        let module = parse("import iteration\nresult = iteration.done\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Sentinel("iteration.done".into()))
        );
    }

    #[test]
    fn test_generated_replace_factory_copies_and_updates_fields() {
        let module = parse("class Point:\n    x: int\n    y: int\np = Point(1, 2)\nq = Point.replace(p, y=5)\nresult = q.x + q.y\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(6)));
    }

    #[test]
    fn test_str_base_factories() {
        let module = parse("a = str.bin(255)\nb = str.oct(8)\nc = str.hex(255)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("a"),
            Some(Value::Str("0b11111111".into()))
        );
        assert_eq!(
            interp.env.borrow().get("b"),
            Some(Value::Str("0o10".into()))
        );
        assert_eq!(
            interp.env.borrow().get("c"),
            Some(Value::Str("0xff".into()))
        );
    }

    #[test]
    fn test_str_base_factories_are_first_class() {
        let module = parse("format_base = str.hex\nresult = format_base(16)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("0x10".into()))
        );
    }

    #[test]
    fn test_for_consumes_custom_iterator_until_done() {
        let source = "import iteration\nclass Counter:\n    n: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.n >= 3:\n            return iteration.done\n        value = self.n\n        self.n += 1\n        return value\nc = Counter(0)\nresult = []\nfor value in c:\n    result.append(value)\n";
        let module = parse(source).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Int(0),
                Value::Int(1),
                Value::Int(2)
            ]))))
        );
    }

    #[test]
    fn test_assert_invokes_lazy_message_callable_only_on_failure() {
        let module = parse("assert(true, def: \"not evaluated\")\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        let module = parse("assert(false, def: \"failure detail\")\n").unwrap();
        let error = interp.eval_module(&module).unwrap_err();
        assert_eq!(error.message, "failure detail");
    }

    #[test]
    fn test_for_loop_rejects_string_iterable() {
        let module = parse("for c in \"aé\":\n    pass\n").unwrap();
        let mut interp = Interpreter::new();
        let error = interp.eval_module(&module).unwrap_err();
        assert!(error.message.contains("not iterable"));

        let module =
            parse("chars = []\nfor c in \"aé\".chars:\n    chars.append(c)\nresult = chars[1]\n")
                .unwrap();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("é".into()))
        );
    }

    #[test]
    fn test_list_pop_supports_range_form() {
        let module = parse(
            "xs = [1, 2, 3, 4]\nremoved = xs.pop(1, 3)\nresult = removed[0] + removed[1] + xs[1]\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(9)));
    }

    #[test]
    fn test_list_pop_empty_is_recoverable() {
        let module = parse("xs = []\nxs.pop()\n").unwrap();
        let mut interp = Interpreter::new();
        let error = interp.eval_module(&module).unwrap_err();
        assert_eq!(error.message, "pop from empty list");
    }

    #[test]
    fn test_indexing_dispatches_to_user_getitem() {
        let module = parse("class Pages:\n    def __getitem__(self, index: int) -> str:\n        return \"page \" + str(index)\np = Pages()\nresult = p[2]\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("page 2".into()))
        );
    }

    #[test]
    fn test_index_assignment_dispatches_to_user_setitem() {
        let module = parse("class Box:\n    value: int\n    def __setitem__(self, index: int, value: int):\n        self.value = value + index\nb = Box(0)\nb[3] = 4\nresult = b.value\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(7)));
    }

    #[test]
    fn test_for_loop_dispatches_to_user_iter() {
        let module = parse("class Numbers:\n    def __iter__(self):\n        return [2, 4, 6]\nn = Numbers()\ntotal = 0\nfor x in n:\n    total = total + x\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("total"), Some(Value::Int(12)));
    }

    #[test]
    fn test_reflection_and_mapping_builtins() {
        let module = parse(
            "class Pair:\n    left: int\n    right: int\np = Pair(1, 2)\nsetattr(p, \"left\", 4)\nnames = fields(p)\nvalues = map(def(x: int) -> int: x * 2, [1, 2])\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("p").and_then(|value| match value {
                Value::Object { fields, .. } => fields.borrow().get("left").cloned(),
                _ => None,
            }),
            Some(Value::Int(4))
        );
        assert!(matches!(
            interp.env.borrow().get("names"),
            Some(Value::List(_))
        ));
        assert!(matches!(
            interp.env.borrow().get("values"),
            Some(Value::List(_))
        ));
        let ordered = parse(
            "class Ordered:\n    zeta: int\n    alpha: int\no = Ordered(1, 2)\nnames = fields(o)\n",
        )
        .unwrap();
        let mut ordered_interp = Interpreter::new();
        ordered_interp.eval_module(&ordered).unwrap();
        assert_eq!(
            ordered_interp.env.borrow().get("names"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Str("zeta".into()),
                Value::Str("alpha".into()),
            ]))))
        );
        let mut module_env = Environment::new();
        module_env.set("zeta".into(), Value::Int(1));
        module_env.set("alpha".into(), Value::Int(2));
        let module_fields = ordered_interp
            .call_named(
                "fields",
                &[Value::Module {
                    name: "ordered".into(),
                    path: "ordered".into(),
                    env: Rc::new(RefCell::new(module_env)),
                }],
            )
            .expect("module fields should be reflectable");
        assert_eq!(
            module_fields,
            Value::List(Rc::new(RefCell::new(vec![
                Value::Str("zeta".into()),
                Value::Str("alpha".into()),
            ])))
        );
        let mut module_env = Environment::new();
        module_env.set("zeta".into(), Value::Int(1));
        module_env.set("alpha".into(), Value::Int(2));
        module_env.delete_local("zeta");
        module_env.set("zeta".into(), Value::Int(3));
        let rebound_fields = ordered_interp
            .call_named(
                "fields",
                &[Value::Module {
                    name: "rebound".into(),
                    path: "rebound".into(),
                    env: Rc::new(RefCell::new(module_env)),
                }],
            )
            .expect("rebound module fields should be reflectable");
        assert_eq!(
            rebound_fields,
            Value::List(Rc::new(RefCell::new(vec![
                Value::Str("alpha".into()),
                Value::Str("zeta".into()),
            ])))
        );
    }

    #[test]
    fn test_super_calls_single_class_parent_method() {
        let src = r#"
class Base:
    def value(self) -> int:
        return 2

class Child(Base):
    def value(self) -> int:
        return super.value() + 1

result = Child().value()
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(3)));
    }

    #[test]
    fn test_super_works_inside_classmethod() {
        let src = r#"
class Base:
    classmethod value(cls) -> int:
        return 5

class Child(Base):
    classmethod value(cls) -> int:
        return super.value() + 2

result = Child.value()
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(7)));
    }

    #[test]
    fn test_strict_zip_rejects_mismatched_lengths() {
        let module = parse("pairs = zip([1, 2], [3, 4])\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(interp
            .eval_module(&parse("zip([1], [2, 3])\n").unwrap())
            .is_err());
    }

    #[test]
    fn test_string_chars_property_is_unicode_aware() {
        let module = parse("chars = \"aé😀\".chars\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(
            matches!(interp.env.borrow().get("chars"), Some(Value::List(values)) if values.borrow().len() == 3)
        );
    }

    #[test]
    fn test_comprehensions_consume_user_iterators() {
        let source = "import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 3:\n            return iteration.done\n        self.current = self.current + 1\n        return self.current - 1\nvalues = [x for x in Counter(0)]\n";
        let module = parse(source).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(
            matches!(interp.env.borrow().get("values"), Some(Value::List(values)) if values.borrow().len() == 3)
        );
    }

    #[test]
    fn test_dict_comprehension_consumes_user_iterator() {
        let source = "import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 2:\n            return iteration.done\n        self.current = self.current + 1\n        return self.current - 1\nvalues = {str(x): x * 10 for x in Counter(0)}\n";
        let module = parse(source).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(
            matches!(interp.env.borrow().get("values"), Some(Value::Dict(values)) if values.borrow().len() == 2 && values.borrow().get("1") == Some(&Value::Int(10)))
        );
    }

    #[test]
    fn test_for_loop_iterates_dictionary_keys() {
        let module =
            parse("seen = []\nfor key in {\"a\": 1, \"b\": 2}:\n    seen.append(key)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(
            matches!(interp.env.borrow().get("seen"), Some(Value::List(values)) if values.borrow().len() == 2 && values.borrow().iter().all(|value| matches!(value, Value::Str(key) if key == "a" || key == "b")))
        );
    }

    #[test]
    fn test_for_loop_does_not_exhaust_user_iterator_before_body() {
        let source = r#"
import iteration
class Counter:
    current: int
    def __iter__(self):
        return self
    def next(self):
        self.current = self.current + 1
        if self.current > 3:
            return iteration.done
        return self.current
c = Counter(0)
for value in c:
    break
after = c.current
"#;
        let module = parse(source).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("after"), Some(Value::Int(1)));
    }

    #[test]
    fn test_for_loop_reports_range_step_overflow() {
        let module =
            parse("for value in range(9223372036854775806, 9223372036854775807, 2):\n    pass\n")
                .unwrap();
        let mut interp = Interpreter::new();
        let error = interp
            .eval_module(&module)
            .expect_err("overflow must be reported");
        assert!(error.message.contains("range step overflow"));
    }

    #[test]
    fn test_equal_frozen_dictionaries_have_equal_hashes() {
        let module = parse("left = freeze({\"a\": 1, \"b\": 2})\nright = freeze({\"b\": 2, \"a\": 1})\nh_left = hash(left)\nh_right = hash(right)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("h_left"),
            interp.env.borrow().get("h_right")
        );
    }

    #[test]
    fn test_sets_compare_and_hash_without_regard_to_order() {
        let module = parse("left = freeze({1, 2})\nright = freeze({2, 1})\nequal = left == right\nh_left = hash(left)\nh_right = hash(right)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("equal"), Some(Value::Bool(true)));
        assert_eq!(
            interp.env.borrow().get("h_left"),
            interp.env.borrow().get("h_right")
        );
    }

    #[test]
    fn test_dictionary_membership_accepts_non_string_keys() {
        let module =
            parse("values = {1: \"one\"}\npresent = 1 in values\nmissing = 2 in values\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("present"), Some(Value::Bool(true)));
        assert_eq!(interp.env.borrow().get("missing"), Some(Value::Bool(false)));
    }

    #[test]
    fn test_dictionary_integer_key_assignment_matches_lookup() {
        let module =
            parse("values = {1: \"one\"}\nvalues[1] = \"updated\"\nresult = values[1]\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("updated".into()))
        );
    }

    #[test]
    fn test_dict_constructor_accepts_non_string_pair_keys() {
        let module = parse("values = dict([[1, \"one\"]])\nresult = values[1]\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("one".into()))
        );
    }

    #[test]
    fn test_dict_pop_accepts_non_string_keys() {
        let module = parse("values = {1: \"one\"}\nresult = values.pop(1)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("one".into()))
        );
    }

    #[test]
    fn test_list_append_requires_exactly_one_argument() {
        let mut interp = Interpreter::new();
        let module = parse("items = []\nitems.append()\n").unwrap();
        assert!(interp.eval_module(&module).is_err());
        let module = parse("items = []\nitems.append(1, 2)\n").unwrap();
        assert!(interp.eval_module(&module).is_err());
    }

    #[test]
    fn test_string_methods_validate_argument_counts() {
        let mut interp = Interpreter::new();
        for source in [
            "value = \"x\".split(\",\", \";\")\n",
            "value = \"x\".strip(1)\n",
            "value = \"x\".lower(1)\n",
            "value = \"x\".upper(1)\n",
        ] {
            assert!(
                interp.eval_module(&parse(source).unwrap()).is_err(),
                "expected argument-count error for {source}"
            );
        }
    }

    #[test]
    fn test_collection_zero_argument_methods_validate_counts() {
        let mut interp = Interpreter::new();
        for source in [
            "items = []\nitems.clear(1)\n",
            "items = {1}\nitems.clear(1)\n",
            "items = {1: 2}\nitems.clear(1)\n",
            "values = {:}\nvalues.keys(1)\n",
            "values = {:}\nvalues.values(1)\n",
            "values = {:}\nvalues.items(1)\n",
        ] {
            assert!(
                interp.eval_module(&parse(source).unwrap()).is_err(),
                "expected argument-count error for {source}"
            );
        }
    }

    #[test]
    fn test_string_chars_view_is_immutable() {
        let mut interp = Interpreter::new();
        let module = parse("chars = \"abc\".chars\nchars.append(\"d\")\n").unwrap();
        assert!(interp.eval_module(&module).is_err());
    }

    #[test]
    fn test_frozen_set_mutations_are_rejected() {
        let mut interp = Interpreter::new();
        for source in [
            "items = freeze({1, 2})\nitems.clear()\n",
            "items = freeze({1, 2})\nitems.remove(1)\n",
        ] {
            assert!(
                interp.eval_module(&parse(source).unwrap()).is_err(),
                "expected frozen-set mutation error"
            );
        }
    }

    #[test]
    fn test_set_remove_reports_missing_value() {
        let mut interp = Interpreter::new();
        let module = parse("items = {1}\nitems.remove(2)\n").unwrap();
        assert!(interp.eval_module(&module).is_err());
    }

    #[test]
    fn test_set_clear_removes_all_values() {
        let module = parse("items = {1, 2}\nitems.clear()\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(
            matches!(interp.env.borrow().get("items"), Some(Value::Set(values)) if values.borrow().is_empty())
        );
    }

    #[test]
    fn test_dict_clear_removes_all_values() {
        let module = parse("items = {\"a\": 1}\nitems.clear()\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(
            matches!(interp.env.borrow().get("items"), Some(Value::Dict(values)) if values.borrow().is_empty())
        );
    }

    #[test]
    fn test_numeric_hashes_follow_numeric_equality() {
        let module =
            parse("int_hash = hash(1)\nfloat_hash = hash(1.0)\ncomplex_hash = hash(1 + 0j)\n")
                .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("int_hash"),
            interp.env.borrow().get("float_hash")
        );
        assert_eq!(
            interp.env.borrow().get("int_hash"),
            interp.env.borrow().get("complex_hash")
        );
    }

    #[test]
    fn test_arbitrary_precision_integer_is_hashable() {
        let module = parse("value = 123456789012345678901234567890\nh = hash(value)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(matches!(interp.env.borrow().get("h"), Some(Value::Int(_))));
    }

    #[test]
    fn test_set_comprehension_consumes_user_iterator() {
        let source = "import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 3:\n            return iteration.done\n        self.current = self.current + 1\n        return self.current - 1\nvalues = {x * 2 for x in Counter(0)}\n";
        let module = parse(source).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(
            matches!(interp.env.borrow().get("values"), Some(Value::Set(values)) if values.borrow().len() == 3 && values.borrow().contains(&Value::Int(4)))
        );
    }

    #[test]
    fn test_min_max_consume_user_iterator() {
        let source = "import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 3:\n            return iteration.done\n        self.current = self.current + 1\n        return 3 - self.current\nlo = min(Counter(0))\nhi = max(Counter(0))\n";
        let module = parse(source).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("lo"), Some(Value::Int(0)));
        assert_eq!(interp.env.borrow().get("hi"), Some(Value::Int(2)));
    }

    #[test]
    fn test_list_remove_removes_first_match() {
        let module = parse("items = [1, 2, 1]\nitems.remove(1)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(
            matches!(interp.env.borrow().get("items"), Some(Value::List(values)) if values.borrow().len() == 2 && values.borrow()[0] == Value::Int(2))
        );
    }

    #[test]
    fn test_set_algebra_and_disjointness() {
        let module = parse("a = {1, 2}\nb = {2, 3}\ni = a & b\nu = a | b\nd = a - b\nx = a ^ b\nok = a.isdisjoint({4})\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(
            matches!(interp.env.borrow().get("i"), Some(Value::Set(values)) if values.borrow().len() == 1)
        );
        assert!(
            matches!(interp.env.borrow().get("u"), Some(Value::Set(values)) if values.borrow().len() == 3)
        );
        assert!(
            matches!(interp.env.borrow().get("d"), Some(Value::Set(values)) if values.borrow().len() == 1)
        );
        assert!(
            matches!(interp.env.borrow().get("x"), Some(Value::Set(values)) if values.borrow().len() == 2)
        );
        assert_eq!(interp.env.borrow().get("ok"), Some(Value::Bool(true)));
    }

    #[test]
    fn test_set_discard_is_idempotent() {
        let module = parse("items = {1, 2}\nitems.discard(2)\nitems.discard(9)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(
            matches!(interp.env.borrow().get("items"), Some(Value::Set(values)) if values.borrow().as_slice() == [Value::Int(1)])
        );
    }

    #[test]
    fn test_set_pop_returns_and_removes_an_element() {
        let module = parse("items = {1, 2}\nitem = items.pop()\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("item"), Some(Value::Int(1)));
        assert!(
            matches!(interp.env.borrow().get("items"), Some(Value::Set(values)) if values.borrow().as_slice() == [Value::Int(2)])
        );
    }

    #[test]
    fn test_frozen_empty_set_pop_reports_mutation_error_first() {
        let module = parse("items = freeze({})\nitems.pop()\n").unwrap();
        let mut interp = Interpreter::new();
        let error = interp
            .eval_module(&module)
            .expect_err("frozen set mutation must fail");
        assert!(error.message.contains("frozen set"));
    }

    #[test]
    fn test_dict_pop_returns_and_removes_value() {
        let module = parse("items = {\"a\": 7}\nvalue = items.pop(\"a\")\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("value"), Some(Value::Int(7)));
        assert!(
            matches!(interp.env.borrow().get("items"), Some(Value::Dict(values)) if values.borrow().is_empty())
        );
    }

    #[test]
    fn test_string_methods() {
        let src = r#"
s = "  hello world  "
stripped = s.strip()
parts = stripped.split(" ")
joined = "-".join(parts)
up = joined.upper()
sw = up.startswith("HEL")
ew = up.endswith("RLD")
rep = up.replace("WORLD", "LUCID")
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let _ = interp.eval_module(&module).unwrap();

        assert_eq!(
            interp.env.borrow().get("stripped").unwrap(),
            Value::Str("hello world".into())
        );
        assert_eq!(
            interp.env.borrow().get("joined").unwrap(),
            Value::Str("hello-world".into())
        );
        assert_eq!(
            interp.env.borrow().get("up").unwrap(),
            Value::Str("HELLO-WORLD".into())
        );
        assert_eq!(interp.env.borrow().get("sw").unwrap(), Value::Bool(true));
        assert_eq!(interp.env.borrow().get("ew").unwrap(), Value::Bool(true));
        assert_eq!(
            interp.env.borrow().get("rep").unwrap(),
            Value::Str("HELLO-LUCID".into())
        );
    }

    #[test]
    fn test_list_dict_set_methods() {
        let src = r#"
# List methods
items = [1, 2]
items.append(3)
items.extend([4, 5])
items.insert(0, 0)
popped = items.pop()

# Dict methods
d = {"a": 10, "b": 20}
has_a = d.contains("a")
val_b = d.get("b", 0)
val_c = d.get("c", 99)

# Set methods
s = {1, 2}
s.add(3)
s.remove(1)
has_3 = s.contains(3)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let _ = interp.eval_module(&module).unwrap();

        assert_eq!(interp.env.borrow().get("popped").unwrap(), Value::Int(5));
        assert_eq!(interp.env.borrow().get("has_a").unwrap(), Value::Bool(true));
        assert_eq!(interp.env.borrow().get("val_b").unwrap(), Value::Int(20));
        assert_eq!(interp.env.borrow().get("val_c").unwrap(), Value::Int(99));
        assert_eq!(interp.env.borrow().get("has_3").unwrap(), Value::Bool(true));
    }

    #[test]
    fn test_membership_and_comparisons() {
        let src = r#"
in_list = 2 in [1, 2, 3]
not_in_list = 5 not in [1, 2, 3]
in_str = "ell" in "hello"
le = 5 <= 10
ge = 10 >= 5
rem = 17 % 5
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let _ = interp.eval_module(&module).unwrap();

        assert_eq!(
            interp.env.borrow().get("in_list").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            interp.env.borrow().get("not_in_list").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            interp.env.borrow().get("in_str").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(interp.env.borrow().get("le").unwrap(), Value::Bool(true));
        assert_eq!(interp.env.borrow().get("ge").unwrap(), Value::Bool(true));
        assert_eq!(interp.env.borrow().get("rem").unwrap(), Value::Int(2));
    }

    #[test]
    fn test_module_loading_and_math() {
        let src = r#"
global_p = pi > 3.0
import math
from math import sqrt, pi

sq = sqrt(16)
p = pi > 3.0
timestamp = now() > 0.0
abs_val = math.abs(-42)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let _ = interp.eval_module(&module).unwrap();

        assert_eq!(interp.env.borrow().get("sq").unwrap(), Value::Float(4.0));
        assert_eq!(interp.env.borrow().get("p").unwrap(), Value::Bool(true));
        assert_eq!(
            interp.env.borrow().get("global_p").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            interp.env.borrow().get("timestamp").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(interp.env.borrow().get("abs_val").unwrap(), Value::Int(42));
    }

    #[test]
    fn imported_class_definitions_are_available_to_parent_interpreter() {
        let root = std::env::temp_dir().join(format!(
            "lucid_runtime_imported_class_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("child.lucid"), "class Box:\n    value: int\n").unwrap();
        let entry = root.join("entry.lucid");
        let source = "import .child\nitem = child.Box(7)\n";
        std::fs::write(&entry, source).unwrap();
        let module = parse(source).unwrap();
        let mut interp = Interpreter::default();
        interp.set_current_file(Some(entry));
        interp
            .eval_module(&module)
            .expect("imported class should construct");
        assert!(matches!(
            interp.env.borrow().get("item"),
            Some(Value::Object { class_name, .. }) if class_name == "Box"
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn imported_dispatch_overloads_are_registered_in_parent_interpreter() {
        let root = std::env::temp_dir().join(format!(
            "lucid_runtime_imported_dispatch_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("child.lucid"),
            "dispatch def choose(a: int, b: int):\n    return 1\ndispatch def choose(a: str, b: str):\n    return 2\n",
        )
        .unwrap();
        let entry = root.join("entry.lucid");
        let source = "import .child\n";
        std::fs::write(&entry, source).unwrap();
        let module = parse(source).unwrap();
        let mut interp = Interpreter::default();
        interp.set_current_file(Some(entry));
        interp
            .eval_module(&module)
            .expect("dispatch module should load");
        assert_eq!(
            interp.call_dispatch("choose", &[Value::Int(2), Value::Int(3)]),
            Ok(Value::Int(1))
        );
        assert_eq!(
            interp.call_dispatch("choose", &[Value::Str("a".into()), Value::Str("b".into())]),
            Ok(Value::Int(2))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn declaration_only_module_cycles_are_cached_before_execution() {
        let root =
            std::env::temp_dir().join(format!("lucid_runtime_decl_cycle_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let a = root.join("a.lucid");
        let b = root.join("b.lucid");
        std::fs::write(&a, "from .b import B\nclass A:\n    pass\n").unwrap();
        std::fs::write(&b, "from .a import A\nclass B:\n    pass\n").unwrap();
        let source = std::fs::read_to_string(&a).unwrap();
        let module = parse(&source).unwrap();
        let mut interp = Interpreter::default();
        interp.set_current_file(Some(a.clone()));
        interp
            .eval_module(&module)
            .expect("declaration-only cycle should terminate");
        assert!(interp.env.borrow().get("A").is_some());
        assert!(interp.env.borrow().get("B").is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn value_module_cycles_report_initialization_error() {
        let root =
            std::env::temp_dir().join(format!("lucid_runtime_value_cycle_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.lucid"), "import .b\nvalue = 1\n").unwrap();
        std::fs::write(root.join("b.lucid"), "import .a\nvalue = 2\n").unwrap();
        let mut interp = Interpreter::default();
        interp.set_current_file(Some(root.join("entry.lucid")));
        let error = match interp.load_module(".a", Span::default()) {
            Ok(_) => panic!("value cycle should be rejected"),
            Err(error) => error,
        };
        assert!(error.message.contains("cyclic module initialization"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_from_import_rejects_private_names() {
        let module = parse("from math import _internal\n").unwrap();
        let mut interp = Interpreter::new();
        let error = interp
            .eval_module(&module)
            .expect_err("private imports must be rejected");
        assert!(error.message.contains("private name '_internal'"));
    }

    #[test]
    fn test_module_attribute_rejects_private_names() {
        let module = parse("import math\nvalue = math._internal\n").unwrap();
        let mut interp = Interpreter::new();
        let error = interp
            .eval_module(&module)
            .expect_err("private module attributes must be rejected");
        assert!(error.message.contains("private attribute '_internal'"));
    }

    #[test]
    fn test_call_site_capture_intrinsics() {
        let src = r#"
name = VarName.from_assignment()
location = SourceLocation.caller()
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("name"),
            Some(Value::Str("name".into()))
        );
        let location = interp.env.borrow().get("location");
        match location {
            Some(Value::Object {
                class_name, fields, ..
            }) => {
                assert_eq!(class_name, "SourceLocation");
                assert!(matches!(fields.borrow().get("line"), Some(Value::Int(n)) if *n > 0));
            }
            other => panic!("expected source location object, got {other:?}"),
        }
    }

    #[test]
    fn test_declaration_kind_identity_checks() {
        let src = r#"
class Box:
    value: int
class Base:
    pass
class Child(Base):
    pass

value = 1
z = 2j
child = Child()
is_class = value is class
is_trait = value is trait
is_int = value is int
identity = value === value
is_complex = z is complex
is_parent = child is Base
is_not_other = child is not Box
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("is_class"), Some(Value::Bool(true)));
        assert_eq!(
            interp.env.borrow().get("is_trait"),
            Some(Value::Bool(false))
        );
        assert_eq!(interp.env.borrow().get("is_int"), Some(Value::Bool(true)));
        assert_eq!(interp.env.borrow().get("identity"), Some(Value::Bool(true)));
        assert_eq!(
            interp.env.borrow().get("is_complex"),
            Some(Value::Bool(true))
        );
        assert_eq!(
            interp.env.borrow().get("is_parent"),
            Some(Value::Bool(true))
        );
        assert_eq!(
            interp.env.borrow().get("is_not_other"),
            Some(Value::Bool(true))
        );
    }

    #[test]
    fn test_callable_identity_check() {
        let module = parse(
            "def f(x: int) -> int:\n    return x\ncheck = f is Callable\nnot_value = 1 is Callable",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("check"), Some(Value::Bool(true)));
        assert_eq!(
            interp.env.borrow().get("not_value"),
            Some(Value::Bool(false))
        );
    }

    #[test]
    fn test_binary_buffer_conversions_and_aliasing() {
        let src = r#"
buffer = bytearray(b"hi")
view = memoryview(buffer)
window = view[0:2]
buffer[0] = 72
first = view[0]
second = window[1]
view[1] = 73
mutated = buffer[1]
readonly = memoryview(bytes([0, 65]))
readonly_len = len(readonly)
readonly_first = readonly[0]
readonly_tail_first = readonly[1:][0]
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("first"), Some(Value::Int(72)));
        assert_eq!(interp.env.borrow().get("second"), Some(Value::Int(105)));
        assert_eq!(interp.env.borrow().get("mutated"), Some(Value::Int(73)));
        assert_eq!(interp.env.borrow().get("readonly_len"), Some(Value::Int(2)));
        assert_eq!(
            interp.env.borrow().get("readonly_first"),
            Some(Value::Int(0))
        );
        assert_eq!(
            interp.env.borrow().get("readonly_tail_first"),
            Some(Value::Int(65))
        );
        assert!(matches!(
            interp.env.borrow().get("window"),
            Some(Value::MemoryView { len: 2, .. })
        ));

        let module = parse("view = memoryview(bytes([65]))\nview[0] = 66\n").unwrap();
        let mut interp = Interpreter::new();
        let error = interp
            .eval_module(&module)
            .expect_err("read-only memoryview mutation should fail");
        assert!(error.message.contains("read-only memoryview"));
    }

    #[test]
    fn test_binary_buffer_membership_uses_byte_values() {
        let module = parse(
            "data = b\"AB\"\nhas_a = 65 in data\nmissing = 67 in data\nview = memoryview(data)\nhas_b = 66 in view\nnot_zero = 0 not in view\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("has_a"), Some(Value::Bool(true)));
        assert_eq!(interp.env.borrow().get("missing"), Some(Value::Bool(false)));
        assert_eq!(interp.env.borrow().get("has_b"), Some(Value::Bool(true)));
        assert_eq!(interp.env.borrow().get("not_zero"), Some(Value::Bool(true)));
    }

    #[test]
    fn test_binary_buffers_iterate_as_integer_sequences() {
        let module = parse(
            r#"data = b"AB"
buffer = bytearray(data)
view = memoryview(data)
total = 0
for byte in data:
    total = total + byte
buffer_total = 0
for byte in buffer:
    buffer_total = buffer_total + byte
view_total = 0
for byte in view:
    view_total = view_total + byte
copied = list(data)
mutable_copy = list(buffer)
view_copy = list(view)
rev = reversed(data)
buffer_rev = reversed(buffer)
view_rev = reversed(view)
summed = sum(data)
view_sum = sum(view)
largest = max(data)
first_enumerated = enumerate(data)[0][1]
sorted_first = sorted(bytes([66, 65]))[0]
repeated = data * 2
reflected = 2 * data
empty_repeat = data * -1
joined = data + b"CD"
is_iterable = data is Iterable
is_collection = data is Collection
is_sequence = data is Sequence
is_reversible = data is Reversible
buffer_is_sequence = buffer is Sequence
view_is_sequence = view is Sequence
"#,
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("total"), Some(Value::Int(131)));
        assert_eq!(
            interp.env.borrow().get("buffer_total"),
            Some(Value::Int(131))
        );
        assert_eq!(interp.env.borrow().get("view_total"), Some(Value::Int(131)));
        assert!(
            matches!(interp.env.borrow().get("copied"), Some(Value::List(values)) if *values.borrow() == vec![Value::Int(65), Value::Int(66)])
        );
        assert!(
            matches!(interp.env.borrow().get("mutable_copy"), Some(Value::List(values)) if *values.borrow() == vec![Value::Int(65), Value::Int(66)])
        );
        assert!(
            matches!(interp.env.borrow().get("view_copy"), Some(Value::List(values)) if *values.borrow() == vec![Value::Int(65), Value::Int(66)])
        );
        assert!(
            matches!(interp.env.borrow().get("rev"), Some(Value::List(values)) if *values.borrow() == vec![Value::Int(66), Value::Int(65)])
        );
        assert!(
            matches!(interp.env.borrow().get("buffer_rev"), Some(Value::List(values)) if *values.borrow() == vec![Value::Int(66), Value::Int(65)])
        );
        assert!(
            matches!(interp.env.borrow().get("view_rev"), Some(Value::List(values)) if *values.borrow() == vec![Value::Int(66), Value::Int(65)])
        );
        assert_eq!(interp.env.borrow().get("summed"), Some(Value::Int(131)));
        assert_eq!(interp.env.borrow().get("view_sum"), Some(Value::Int(131)));
        assert_eq!(interp.env.borrow().get("largest"), Some(Value::Int(66)));
        assert_eq!(
            interp.env.borrow().get("first_enumerated"),
            Some(Value::Int(65))
        );
        assert_eq!(
            interp.env.borrow().get("sorted_first"),
            Some(Value::Int(65))
        );
        assert_eq!(
            interp.env.borrow().get("repeated"),
            Some(Value::Bytes(vec![65, 66, 65, 66]))
        );
        assert_eq!(
            interp.env.borrow().get("reflected"),
            Some(Value::Bytes(vec![65, 66, 65, 66]))
        );
        assert_eq!(
            interp.env.borrow().get("empty_repeat"),
            Some(Value::Bytes(Vec::new()))
        );
        assert_eq!(
            interp.env.borrow().get("joined"),
            Some(Value::Bytes(vec![65, 66, 67, 68]))
        );
        assert_eq!(
            interp.env.borrow().get("is_iterable"),
            Some(Value::Bool(true))
        );
        assert_eq!(
            interp.env.borrow().get("is_collection"),
            Some(Value::Bool(true))
        );
        assert_eq!(
            interp.env.borrow().get("is_sequence"),
            Some(Value::Bool(true))
        );
        assert_eq!(
            interp.env.borrow().get("is_reversible"),
            Some(Value::Bool(true))
        );
        assert_eq!(
            interp.env.borrow().get("buffer_is_sequence"),
            Some(Value::Bool(true))
        );
        assert_eq!(
            interp.env.borrow().get("view_is_sequence"),
            Some(Value::Bool(true))
        );
    }

    #[test]
    fn test_user_defined_buffer_protocol_feeds_memoryview() {
        let module = parse(
            r#"
class Packet:
    storage: ByteArray
    def __buffer__(self) -> MemoryView:
        return memoryview(self.storage)

packet = Packet(bytearray(b"hi"))
view = memoryview(packet)
view[0] = 72
first = packet.storage[0]
second = view[1]
packet_is_buffer = packet is Buffer
str_is_buffer = "hi" is Buffer
copied = bytes(packet)
mutable = bytearray(packet)
mutable[1] = 73
copied_first = copied[0]
mutable_second = mutable[1]
storage_second = packet.storage[1]
"#,
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("first"), Some(Value::Int(72)));
        assert_eq!(interp.env.borrow().get("second"), Some(Value::Int(105)));
        assert_eq!(
            interp.env.borrow().get("packet_is_buffer"),
            Some(Value::Bool(true))
        );
        assert_eq!(
            interp.env.borrow().get("str_is_buffer"),
            Some(Value::Bool(false))
        );
        assert_eq!(
            interp.env.borrow().get("copied_first"),
            Some(Value::Int(72))
        );
        assert_eq!(
            interp.env.borrow().get("mutable_second"),
            Some(Value::Int(73))
        );
        assert_eq!(
            interp.env.borrow().get("storage_second"),
            Some(Value::Int(105))
        );
    }

    #[test]
    fn test_class_destructuring_binds_fields() {
        let module =
            parse("class Point:\n    x: int\n    y: int\np = Point(4, 9)\nlet Point(a, b) = p")
                .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Int(4)));
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Int(9)));
    }

    #[test]
    fn test_bytes_conversion_is_immutable_payload() {
        let module = parse(
            "data = bytes([65, 66])\n\
             print(data)\n\
             first = data[0]\n\
             tail = data[1:]\n\
             values = list(data)\n\
             mutable = bytearray(data)\n\
             zero = bytes([0, 65])\n\
             zero_len = len(zero)\n\
             zero_first = zero[0]\n\
             zero_tail = zero[1:]\n\
             zero_values = list(zero)\n\
             zero_mutable = bytearray(zero)\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        let value = interp
            .eval_module(&module)
            .expect("bytes conversion should evaluate");
        assert_eq!(
            value,
            Value::List(Rc::new(RefCell::new(vec![Value::Int(0), Value::Int(65)])))
        );
        assert_eq!(
            interp.env.borrow().get("data"),
            Some(Value::Bytes(vec![65, 66]))
        );
        assert_eq!(interp.output, vec!["AB"]);
        assert_eq!(interp.env.borrow().get("first"), Some(Value::Int(65)));
        assert_eq!(
            interp.env.borrow().get("tail"),
            Some(Value::Bytes(vec![66]))
        );
        assert_eq!(
            interp.env.borrow().get("values"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Int(65),
                Value::Int(66)
            ]))))
        );
        assert_eq!(
            interp.env.borrow().get("mutable"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Int(65),
                Value::Int(66)
            ]))))
        );
        assert_eq!(
            interp.env.borrow().get("zero"),
            Some(Value::Bytes(vec![0, 65]))
        );
        assert_eq!(interp.env.borrow().get("zero_len"), Some(Value::Int(2)));
        assert_eq!(interp.env.borrow().get("zero_first"), Some(Value::Int(0)));
        assert_eq!(
            interp.env.borrow().get("zero_tail"),
            Some(Value::Bytes(vec![65]))
        );
        assert_eq!(
            interp.env.borrow().get("zero_values"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Int(0),
                Value::Int(65)
            ]))))
        );
        assert_eq!(
            interp.env.borrow().get("zero_mutable"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Int(0),
                Value::Int(65)
            ]))))
        );
    }

    #[test]
    fn byte_conversions_accept_in_range_bigints_and_reject_invalid_inputs() {
        let module = parse(
            "ok = bytes([256 - 1, 2 ** 200])\n\
             bad = bytearray([256])\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        let error = interp
            .eval_module(&module)
            .expect_err("out-of-range bytearray item must fail");
        assert!(error.message.contains("0..255"));

        let module = parse("ok = bytes([2 ** 200])\n").unwrap();
        let mut interp = Interpreter::new();
        let error = interp
            .eval_module(&module)
            .expect_err("oversized bigint must not be accepted as a byte");
        assert!(error.message.contains("0..255"));

        let module = parse("bad = bytes(1)\n").unwrap();
        let mut interp = Interpreter::new();
        let error = interp
            .eval_module(&module)
            .expect_err("unsupported bytes input must fail");
        assert!(error.message.contains("cannot convert int"));
    }

    #[test]
    fn locals_excludes_intrinsic_builtins_but_includes_rebindings() {
        let module = parse(
            "x = 1\nfirst = len(locals())\ncount = len\nlen = 99\nsecond = count(locals())\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp
            .eval_module(&module)
            .expect("locals program should run");
        assert_eq!(interp.env.borrow().get("first"), Some(Value::Int(1)));
        assert_eq!(interp.env.borrow().get("second"), Some(Value::Int(4)));
    }

    #[test]
    fn test_decorators_rebind_bottom_up() {
        let src = r#"
def mark(f):
    return f

@mark
def greet() -> str:
    return "hello"

result = greet()
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("hello".into()))
        );
    }

    #[test]
    fn decorators_preserve_replaced_function_identity_metadata() {
        let src = r#"
def wrap(f):
    return def() -> int: 2

@wrap
def original() -> int:
    return 1

plain = wrap(original)
decorated_name = original.__name__
decorated_path = original.__path__
decorated_path_text = str(original.__path__)
decorated_path_last = original.__path__[-1]
plain_name = plain.__name__
result = original()
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        let env = interp.env.borrow();
        assert_eq!(env.get("result"), Some(Value::Int(2)));
        assert_eq!(
            env.get("decorated_name"),
            Some(Value::Str("original".into()))
        );
        assert_eq!(
            env.get("decorated_path"),
            Some(Value::DottedPath(vec!["original".into()]))
        );
        assert_eq!(
            env.get("decorated_path_text"),
            Some(Value::Str("original".into()))
        );
        assert_eq!(
            env.get("decorated_path_last"),
            Some(Value::Str("original".into()))
        );
        assert_eq!(env.get("plain_name"), Some(Value::Str("<def>".into())));
    }

    #[test]
    fn test_partial_application_fills_holes_left_to_right() {
        let src = r#"
def add(a: int, b: int) -> int:
    return a + b

inc = add(1, _)
result = inc(4)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(5)));
    }

    #[test]
    fn test_gather_parameter_collects_remaining_arguments() {
        let src = r#"
def query(path: str, ***rest: Arguments):
    return rest.vpargs

result = query("/items", 1, 2, 3)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
            ]))))
        );
    }

    #[test]
    fn test_gather_spread_replays_bundle_arguments() {
        let src = r#"
class Arguments:
    vpargs: list[int]
    kwargs: dict[str, int]

def add(a: int, b: int) -> int:
    return a + b

args = Arguments([2], {"b": 3})
result = add(***args)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(5)));
    }

    #[test]
    fn test_ordinary_class_gather_spread_uses_field_order() {
        let src = r#"
class Pair:
    left: int
    right: int

def add(a: int, b: int) -> int:
    return a + b

pair = Pair(4, 5)
result = add(***pair)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(9)));
    }

    #[test]
    fn test_class_contextmanager_dunder_is_used_by_with() {
        let src = r#"
class Session:
    value: int

    contextmanager def __cm__(self):
        self.value = self.value + 1
        yield self
        self.value = self.value + 10

s = Session(0)
with s as active:
    active.value = active.value + 1
result = s.value
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(12)));
    }

    #[test]
    fn test_contextmanager_classmethod_returns_managed_context() {
        let src = r#"
class Session:
    contextmanager classmethod open(cls) -> int:
        yield 7

with Session.open() as value:
    result = value + 1
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(8)));
    }

    #[test]
    fn test_with_unwinds_prior_context_when_later_setup_fails() {
        let src = r#"
cleaned = 0

contextmanager def first():
    yield none
    cleaned = cleaned + 1

contextmanager def second():
    raise "setup failed"
    yield none

with first():
    with second():
        pass
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        assert!(interp.eval_module(&module).is_err());
        assert_eq!(interp.env.borrow().get("cleaned"), Some(Value::Int(1)));
    }

    #[test]
    fn test_set_and_dict_conversion_builtins() {
        let src = r#"
a = set([1, 2, 1])
b = set("aba")
c = dict([["x", 4], ["y", 5]])
empty_s = set()
empty_d = dict()
result = len(a) + len(b) + c["x"] + len(empty_s) + len(empty_d)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(8)));
    }

    #[test]
    fn test_list_extend_accepts_any_iterable() {
        let module = parse("items = [1]\nitems.extend(range(2, 4))\nresult = items[2]\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(3)));
    }

    #[test]
    fn test_string_join_accepts_any_iterable() {
        let module = parse("result = \"-\".join(range(1, 3))\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("1-2".into()))
        );
    }

    #[test]
    fn test_string_join_drains_user_iterator() {
        let module = parse("import iteration\nclass Counter:\n    current: int\n    def __iter__(self):\n        return self\n    def next(self):\n        if self.current >= 3:\n            return iteration.done\n        self.current = self.current + 1\n        return self.current - 1\nresult = \"-\".join(Counter(0))\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::Str("0-1-2".into()))
        );
    }

    #[test]
    fn test_predicates_iterate_dict_keys() {
        let module = parse("d = {\"a\": 1, \"b\": 2}\na = any(d)\nb = all(d)\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Bool(true)));
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Bool(true)));
    }

    #[test]
    fn test_sum_supports_bigints() {
        let module = parse("result = sum([12345678901234567890, 10])\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::BigInt(
                BigInt::parse_bytes(b"12345678901234567900", 10).unwrap()
            ))
        );
    }

    #[test]
    fn test_bigint_float_arithmetic_promotes_to_float() {
        let module = parse(
            "a = 12345678901234567890 + 0.5\nb = 12345678901234567890 - 0.5\nc = 2.0 * 12345678901234567890\nd = sum([12345678901234567890, 0.5])\ne = 12345678901234567890 / 2.0\nf = 12345678901234567890 // 2.0\ng = 12345678901234567890 % 2.0\nh = 12345678901234567890 > 0.5\ni = 12345678901234567890 == 12345678901234567890.0\nj = 12345678901234567890 ** 0.5\nk = 12345678901234567890 ** -1\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert!(
            matches!(interp.env.borrow().get("a"), Some(Value::Float(value)) if value.is_finite())
        );
        assert!(
            matches!(interp.env.borrow().get("b"), Some(Value::Float(value)) if value.is_finite())
        );
        assert!(
            matches!(interp.env.borrow().get("c"), Some(Value::Float(value)) if value.is_finite())
        );
        assert!(
            matches!(interp.env.borrow().get("d"), Some(Value::Float(value)) if value.is_finite())
        );
        assert!(
            matches!(interp.env.borrow().get("e"), Some(Value::Float(value)) if value.is_finite())
        );
        assert!(
            matches!(interp.env.borrow().get("f"), Some(Value::Float(value)) if value.is_finite())
        );
        assert!(
            matches!(interp.env.borrow().get("g"), Some(Value::Float(value)) if value.is_finite())
        );
        assert_eq!(interp.env.borrow().get("h"), Some(Value::Bool(true)));
        assert_eq!(interp.env.borrow().get("i"), Some(Value::Bool(true)));
        assert!(
            matches!(interp.env.borrow().get("j"), Some(Value::Float(value)) if value.is_finite())
        );
        assert!(
            matches!(interp.env.borrow().get("k"), Some(Value::Float(value)) if value.is_finite())
        );
    }

    #[test]
    fn test_sum_preserves_float_values() {
        let module = parse("a = sum([1.5, 2.5])\nb = sum([1, 2.5])\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Float(4.0)));
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Float(3.5)));
    }

    #[test]
    fn test_slice_builtin_returns_descriptor() {
        let module = parse("s = slice(1, 4, 2)\nresult = s[\"step\"]\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Int(2)));
    }

    #[test]
    fn test_logical_operators_short_circuit_rhs() {
        let module = parse("a = false and missing\nb = true or missing\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("a"), Some(Value::Bool(false)));
        assert_eq!(interp.env.borrow().get("b"), Some(Value::Bool(true)));
    }

    #[test]
    fn test_bigint_invert_uses_twos_complement_identity() {
        let module = parse("result = ~123456789012345678901234567890\n").unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("result"),
            Some(Value::BigInt(
                BigInt::parse_bytes(b"-123456789012345678901234567891", 10).unwrap()
            ))
        );
    }

    #[test]
    fn test_machine_integer_overflow_promotes_to_bigint() {
        let module = parse(
            "a = 9223372036854775807 + 1\nb = -9223372036854775807 - 2\nc = 3037000500 * 3037000500\nf = 2 ** 63\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.env.borrow().get("a"),
            Some(Value::BigInt(BigInt::from(i64::MAX) + BigInt::one()))
        );
        assert_eq!(
            interp.env.borrow().get("b"),
            Some(Value::BigInt(BigInt::from(i64::MIN) - BigInt::one()))
        );
        assert_eq!(
            interp.env.borrow().get("c"),
            Some(Value::BigInt(
                BigInt::from(3037000500i64) * BigInt::from(3037000500i64)
            ))
        );
        assert_eq!(
            interp.env.borrow().get("f"),
            Some(Value::BigInt(BigInt::from(2).pow(63)))
        );
    }

    #[test]
    fn test_without_capability_affects_instance_checks() {
        let module = parse(
            "class Token without Eq:\n    value: int\n\
             class Nominal:\n    def __len__(self) -> int:\n        return 1\n\
             class Declared(Sized):\n    def __len__(self) -> int:\n        return 1\n\
             class Retro:\n    pass\n\
             implement Sized for Retro:\n    def __len__(self) -> int:\n        return 1\n\
             t = Token(1)\nresult = t is Eq\nnominal = Nominal() is Sized\ndeclared = Declared() is Sized\nretro = Retro() is Sized\n",
        )
        .unwrap();
        let mut interp = Interpreter::new();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.env.borrow().get("result"), Some(Value::Bool(false)));
        assert_eq!(interp.env.borrow().get("nominal"), Some(Value::Bool(false)));
        assert_eq!(interp.env.borrow().get("declared"), Some(Value::Bool(true)));
        assert_eq!(interp.env.borrow().get("retro"), Some(Value::Bool(true)));
    }

    #[test]
    fn default_interpreter_registers_builtins() {
        let interp = Interpreter::default();
        assert!(interp.env.borrow().get("len").is_some());
    }

    #[test]
    fn call_named_invokes_module_function_and_reports_missing_names() {
        let module =
            parse("def answer(value: int):\n    return value + 1\ndef _private():\n    return 0\n")
                .unwrap();
        let mut interp = Interpreter::default();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.call_named("answer", &[Value::Int(41)]),
            Ok(Value::Int(42))
        );
        let error = interp.call_named("missing", &[]).unwrap_err();
        assert!(error.message.contains("not callable or is not defined"));
        let error = interp.call_named("_private", &[]).unwrap_err();
        assert!(error.message.contains("private"));
        let error = interp.call_named("bad name", &[]).unwrap_err();
        assert!(error.message.contains("invalid callable name"));
    }

    #[test]
    fn function_values_expose_identity_metadata() {
        let module = parse("def answer(value: int):\n    return value + 1\nname = answer.__name__\npath = answer.__path__\npath_sized = path is Sized\npath_container = path is Container\n")
            .unwrap();
        let mut interp = Interpreter::default();
        interp.eval_module(&module).unwrap();
        let env = interp.env.borrow();
        assert_eq!(env.get("name"), Some(Value::Str("answer".into())));
        assert_eq!(env.get("path_sized"), Some(Value::Bool(true)));
        assert_eq!(env.get("path_container"), Some(Value::Bool(true)));
    }

    #[test]
    fn reflection_builtins_expose_function_identity_metadata() {
        let module = parse(
            "def answer() -> int:\n    return 1\nitems = [answer]\nname = getattr(items[0], \"__name__\")\nhas_name = hasattr(items[0], \"__name__\")\nhas_missing = hasattr(items[0], \"missing\")\n",
        )
        .unwrap();
        let mut interp = Interpreter::default();
        interp.eval_module(&module).unwrap();
        let env = interp.env.borrow();
        assert_eq!(env.get("name"), Some(Value::Str("answer".into())));
        assert_eq!(env.get("has_name"), Some(Value::Bool(true)));
        assert_eq!(env.get("has_missing"), Some(Value::Bool(false)));
    }

    #[test]
    fn class_objects_expose_identity_metadata() {
        let module = parse(
            "class User:\n    name: str\nname = User.__name__\npath = User.__path__\ndoc = User.__doc__\n",
        )
        .unwrap();
        let mut interp = Interpreter::default();
        interp.eval_module(&module).unwrap();
        let env = interp.env.borrow();
        assert_eq!(env.get("name"), Some(Value::Str("User".into())));
        assert_eq!(
            env.get("path"),
            Some(Value::DottedPath(vec!["User".into()]))
        );
        assert_eq!(env.get("doc"), Some(Value::None));
    }

    #[test]
    fn traits_expose_identity_metadata() {
        let module = parse(
            "trait Named:\n    def name(self) -> str\nname = Named.__name__\npath = Named.__path__\ndoc = Named.__doc__\n",
        )
        .unwrap();
        let mut interp = Interpreter::default();
        interp.eval_module(&module).unwrap();
        let env = interp.env.borrow();
        assert_eq!(env.get("name"), Some(Value::Str("Named".into())));
        assert_eq!(
            env.get("path"),
            Some(Value::DottedPath(vec!["Named".into()]))
        );
        assert_eq!(env.get("doc"), Some(Value::None));
    }

    #[test]
    fn modules_expose_identity_metadata_without_opening_private_members() {
        let module = parse(
            "name = helpers.__name__\npath = helpers.__path__\ndoc = helpers.__doc__\nprivate = helpers._internal\n",
        )
        .unwrap();
        let mut interp = Interpreter::default();
        let mut module_env = Environment::new();
        module_env.set("_internal".into(), Value::Int(1));
        interp.env.borrow_mut().set(
            "helpers".into(),
            Value::Module {
                name: "util".into(),
                path: "pkg.helpers".into(),
                env: Rc::new(RefCell::new(module_env)),
            },
        );
        let error = interp.eval_module(&module).unwrap_err();
        assert!(error.message.contains("private attribute '_internal'"));
        let env = interp.env.borrow();
        assert_eq!(env.get("name"), Some(Value::Str("util".into())));
        assert_eq!(
            env.get("path"),
            Some(Value::DottedPath(vec!["pkg".into(), "helpers".into()]))
        );
        assert_eq!(env.get("doc"), Some(Value::None));
    }

    #[test]
    fn call_named_resolves_public_entries_through_imported_modules() {
        let root =
            std::env::temp_dir().join(format!("lucid_call_named_module_{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("main.lucid");
        let child = root.join("child.lucid");
        std::fs::write(&child, "def answer(value: int):\n    return value + 1\n").unwrap();
        std::fs::write(&entry, "import child\n").unwrap();
        let source = std::fs::read_to_string(&entry).unwrap();
        let module = parse(&source).unwrap();
        let mut interp = Interpreter::default();
        interp.set_current_file(Some(entry.clone()));
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.call_named(".child.answer", &[Value::Int(41)]),
            Ok(Value::Int(42))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn call_named_with_context_reuses_contextmanager_teardown() {
        let module = parse(
            "events = []\ncontextmanager def managed():\n    events.append(\"setup\")\n    yield none\n    events.append(\"teardown\")\ndef answer():\n    events.append(\"entry\")\n    return 42\n",
        )
        .unwrap();
        let mut interp = Interpreter::default();
        interp.eval_module(&module).unwrap();
        assert_eq!(
            interp.call_named_with_context("managed", "answer", &[]),
            Ok(Value::Int(42))
        );
        assert_eq!(
            interp.env.borrow().get("events"),
            Some(Value::List(Rc::new(RefCell::new(vec![
                Value::Str("setup".into()),
                Value::Str("entry".into()),
                Value::Str("teardown".into()),
            ]))))
        );
    }

    #[test]
    fn reflection_invokes_inherited_getters() {
        let module = parse(
            "class Base:\n    value: int\n    getter doubled(self) -> int:\n        return self.value * 2\nclass Child(Base):\n    pass\nchild = Child(4)\nprint(getattr(child, \"doubled\"))\nprint(hasattr(child, \"doubled\"))\n",
        )
        .unwrap();
        let mut interp = Interpreter::default();
        interp.eval_module(&module).unwrap();
        assert_eq!(interp.output, vec!["8", "true"]);
    }
}
