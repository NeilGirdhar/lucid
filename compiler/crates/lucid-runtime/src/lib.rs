use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use lucid_syntax::ast::*;
use lucid_syntax::token::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub message: String,
    pub span: Span,
}

#[derive(Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    None,
    List(Rc<RefCell<Vec<Value>>>),
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
    },
    BuiltinFunction {
        name: String,
        func: Rc<dyn Fn(&[Value], &mut Interpreter) -> Result<Value, RuntimeError>>,
    },
    Module {
        name: String,
        env: Rc<RefCell<Environment>>,
    },
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
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::Str(_) => "str",
            Value::None => "none",
            Value::List(_) => "list",
            Value::Dict(_) => "dict",
            Value::Set(_) => "set",
            Value::Range { .. } => "range",
            Value::Skip => "skip",
            Value::Record(_) => "record",
            Value::Object { class_name, .. } => class_name.as_str(),
            Value::Function { .. } => "function",
            Value::BuiltinFunction { .. } => "builtin_function",
            Value::Module { .. } => "module",
            Value::Sentinel(name) => name.as_str(),
            Value::Return(val) => val.type_name(),
        }
    }

    pub fn freeze(&self) {
        match self {
            Value::Object { is_frozen, fields, .. } => {
                *is_frozen.borrow_mut() = true;
                for val in fields.borrow().values() {
                    val.freeze();
                }
            }
            Value::List(items) => {
                for item in items.borrow().iter() {
                    item.freeze();
                }
            }
            Value::Set(items) => {
                for item in items.borrow().iter() {
                    item.freeze();
                }
            }
            Value::Dict(entries) => {
                for item in entries.borrow().values() {
                    item.freeze();
                }
            }
            Value::Module { env, .. } => {
                for item in env.borrow().bindings.values() {
                    item.freeze();
                }
            }
            Value::Return(val) => val.freeze(),
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
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::None, Value::None) => true,
            (Value::Skip, Value::Skip) => true,
            (Value::Sentinel(a), Value::Sentinel(b)) => a == b,
            (Value::Range { start: s1, stop: e1, step: st1 }, Value::Range { start: s2, stop: e2, step: st2 }) => {
                s1 == s2 && e1 == e2 && st1 == st2
            }
            (Value::List(a), Value::List(b)) => *a.borrow() == *b.borrow(),
            (Value::Set(a), Value::Set(b)) => *a.borrow() == *b.borrow(),
            (Value::Dict(a), Value::Dict(b)) => *a.borrow() == *b.borrow(),
            (Value::Record(a), Value::Record(b)) => *a.borrow() == *b.borrow(),
            (Value::Module { name: n1, .. }, Value::Module { name: n2, .. }) => n1 == n2,
            (Value::Object { class_name: n1, fields: f1, .. }, Value::Object { class_name: n2, fields: f2, .. }) => {
                n1 == n2 && *f1.borrow() == *f2.borrow()
            }
            _ => false,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(n) => write!(f, "{n}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Str(s) => write!(f, "\"{s}\""),
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
            Value::Set(items) => write!(f, "{{{:?}}}", *items.borrow()),
            Value::Dict(entries) => write!(f, "{:?}", *entries.borrow()),
            Value::Record(fields) => write!(f, "record {:?}", *fields.borrow()),
            Value::Object { class_name, fields, is_frozen } => {
                let prefix = if *is_frozen.borrow() { "!" } else { "" };
                write!(f, "{prefix}{class_name}({:?})", *fields.borrow())
            }
            Value::Function { name, .. } => write!(f, "<def {name}>"),
            Value::BuiltinFunction { name, .. } => write!(f, "<builtin {name}>"),
            Value::Module { name, .. } => write!(f, "<module '{name}'>"),
            Value::Sentinel(s) => write!(f, "{s}"),
            Value::Return(val) => write!(f, "return {:?}", val),
        }
    }
}

#[derive(Default, Clone)]
pub struct Environment {
    pub bindings: HashMap<String, Value>,
    pub parent: Option<Rc<RefCell<Environment>>>,
}

impl Environment {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_parent(parent: Rc<RefCell<Environment>>) -> Self {
        Self {
            bindings: HashMap::new(),
            parent: Some(parent),
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
        self.bindings.insert(name, value);
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
    pub func: Rc<dyn Fn(&[Value], &mut Interpreter) -> Result<Value, RuntimeError>>,
}

#[derive(Default)]
pub struct DispatchTable {
    pub methods: HashMap<String, Vec<DispatchEntry>>,
}

impl DispatchTable {
    pub fn register(&mut self, name: String, param_types: Vec<String>, func: Rc<dyn Fn(&[Value], &mut Interpreter) -> Result<Value, RuntimeError>>) {
        self.methods.entry(name).or_default().push(DispatchEntry { param_types, func });
    }

    pub fn find_match(&self, name: &str, args: &[Value]) -> Option<Rc<dyn Fn(&[Value], &mut Interpreter) -> Result<Value, RuntimeError>>> {
        if let Some(entries) = self.methods.get(name) {
            let arg_types: Vec<&str> = args.iter().map(|a| a.type_name()).collect();
            // Find best matching entry
            for entry in entries {
                if entry.param_types.len() == args.len() {
                    let mut matches = true;
                    for (expected, actual) in entry.param_types.iter().zip(&arg_types) {
                        if expected != "object" && expected != *actual {
                            matches = false;
                            break;
                        }
                    }
                    if matches {
                        return Some(entry.func.clone());
                    }
                }
            }
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
    pub span: Span,
}

fn compare_values(a: &Value, b: &Value) -> Result<std::cmp::Ordering, RuntimeError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).ok_or_else(|| RuntimeError {
            message: "cannot compare NaN in min/max".into(),
            span: Span::default(),
        }),
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y).ok_or_else(|| RuntimeError {
            message: "cannot compare NaN in min/max".into(),
            span: Span::default(),
        }),
        (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)).ok_or_else(|| RuntimeError {
            message: "cannot compare NaN in min/max".into(),
            span: Span::default(),
        }),
        (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),
        _ => Err(RuntimeError {
            message: format!("unsupported comparison between {} and {}", a.type_name(), b.type_name()),
            span: Span::default(),
        }),
    }
}

pub struct Interpreter {
    pub env: Rc<RefCell<Environment>>,
    pub classes: HashMap<String, ClassDef>,
    pub traits: HashMap<String, Vec<TraitMember>>,
    pub dispatch: DispatchTable,
    pub output: Vec<String>,
    pub current_file: Option<std::path::PathBuf>,
    pub module_cache: HashMap<std::path::PathBuf, Rc<RefCell<Environment>>>,
}

impl Interpreter {
    pub fn new() -> Self {
        let env = Rc::new(RefCell::new(Environment::new()));
        let mut interp = Self {
            env,
            classes: HashMap::new(),
            traits: HashMap::new(),
            dispatch: DispatchTable::default(),
            output: Vec::new(),
            current_file: None,
            module_cache: HashMap::new(),
        };

        interp.register_builtins();
        interp
    }

    pub fn set_current_file(&mut self, path: Option<std::path::PathBuf>) {
        self.current_file = path;
    }

    pub fn call_dispatch(&mut self, name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
        let func = self.dispatch.find_match(name, args);
        if let Some(func) = func {
            func(args, self)
        } else {
            Err(RuntimeError {
                message: format!("no dispatch method found for '{name}' with argument types {:?}", args.iter().map(|a| a.type_name()).collect::<Vec<_>>()),
                span: Span::default(),
            })
        }
    }

    pub fn eval_binary_op(&mut self, op: &BinaryOp, lval: Value, rval: Value, span: &Span) -> Result<Value, RuntimeError> {
        match op {
            BinaryOp::Add => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{a}{b}"))),
                (Value::List(a), Value::List(b)) => {
                    let mut combined = a.borrow().clone();
                    combined.extend(b.borrow().clone());
                    Ok(Value::List(Rc::new(RefCell::new(combined))))
                }
                _ => self.call_dispatch("+", &[lval, rval]),
            },
            BinaryOp::Sub => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
                _ => self.call_dispatch("-", &[lval, rval]),
            },
            BinaryOp::Mul => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * *b as f64)),
                (Value::Str(s), Value::Int(n)) => Ok(Value::Str(s.repeat((*n).max(0) as usize))),
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
                (Value::Int(a), Value::Int(b)) => {
                    if *b == 0 {
                        Err(RuntimeError { message: "division by zero".to_string(), span: *span })
                    } else {
                        Ok(Value::Float(*a as f64 / *b as f64))
                    }
                }
                (Value::Float(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err(RuntimeError { message: "division by zero".to_string(), span: *span })
                    } else {
                        Ok(Value::Float(a / b))
                    }
                }
                (Value::Int(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err(RuntimeError { message: "division by zero".to_string(), span: *span })
                    } else {
                        Ok(Value::Float(*a as f64 / b))
                    }
                }
                (Value::Float(a), Value::Int(b)) => {
                    if *b == 0 {
                        Err(RuntimeError { message: "division by zero".to_string(), span: *span })
                    } else {
                        Ok(Value::Float(a / *b as f64))
                    }
                }
                _ => self.call_dispatch("/", &[lval, rval]),
            },
            BinaryOp::FloorDiv => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => {
                    if *b == 0 {
                        Err(RuntimeError { message: "division by zero in //".to_string(), span: *span })
                    } else {
                        Ok(Value::Int(a / b))
                    }
                }
                (Value::Float(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err(RuntimeError { message: "division by zero in //".to_string(), span: *span })
                    } else {
                        Ok(Value::Float((a / b).floor()))
                    }
                }
                (Value::Int(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err(RuntimeError { message: "division by zero in //".to_string(), span: *span })
                    } else {
                        Ok(Value::Float((*a as f64 / b).floor()))
                    }
                }
                (Value::Float(a), Value::Int(b)) => {
                    if *b == 0 {
                        Err(RuntimeError { message: "division by zero in //".to_string(), span: *span })
                    } else {
                        Ok(Value::Float((a / *b as f64).floor()))
                    }
                }
                _ => Err(RuntimeError { message: "unsupported operands for //".to_string(), span: *span }),
            },
            BinaryOp::Mod => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => {
                    if *b == 0 {
                        Err(RuntimeError { message: "division by zero in %".to_string(), span: *span })
                    } else {
                        Ok(Value::Int(a % b))
                    }
                }
                (Value::Float(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err(RuntimeError { message: "division by zero in %".to_string(), span: *span })
                    } else {
                        Ok(Value::Float(a % b))
                    }
                }
                _ => Err(RuntimeError { message: "unsupported operands for %".to_string(), span: *span }),
            },
            BinaryOp::Pow => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => {
                    if *b >= 0 {
                        if let Ok(exp) = u32::try_from(*b) {
                            if let Some(res) = a.checked_pow(exp) {
                                return Ok(Value::Int(res));
                            }
                        }
                        Ok(Value::Float((*a as f64).powi(*b as i32)))
                    } else {
                        Ok(Value::Float((*a as f64).powi(*b as i32)))
                    }
                }
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a.powf(*b))),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a.powi(*b as i32))),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float((*a as f64).powf(*b))),
                _ => Err(RuntimeError { message: "unsupported operands for **".to_string(), span: *span }),
            },
            BinaryOp::BitAnd => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
                _ => Err(RuntimeError { message: "unsupported operands for &".to_string(), span: *span }),
            },
            BinaryOp::BitOr => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
                _ => Err(RuntimeError { message: "unsupported operands for |".to_string(), span: *span }),
            },
            BinaryOp::BitXor => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a ^ b)),
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a ^ *b)),
                _ => Err(RuntimeError { message: "unsupported operands for ^".to_string(), span: *span }),
            },
            BinaryOp::Shl => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => {
                    if *b < 0 {
                        Err(RuntimeError { message: "negative shift count".to_string(), span: *span })
                    } else if *b >= 64 {
                        Ok(Value::Int(0))
                    } else {
                        Ok(Value::Int(a << b))
                    }
                }
                _ => Err(RuntimeError { message: "unsupported operands for <<".to_string(), span: *span }),
            },
            BinaryOp::Shr => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => {
                    if *b < 0 {
                        Err(RuntimeError { message: "negative shift count".to_string(), span: *span })
                    } else if *b >= 64 {
                        Ok(Value::Int(0))
                    } else {
                        Ok(Value::Int(a >> b))
                    }
                }
                _ => Err(RuntimeError { message: "unsupported operands for >>".to_string(), span: *span }),
            },
            BinaryOp::Eq => match (&lval, &rval) {
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) == *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a == (*b as f64))),
                _ => Ok(Value::Bool(lval == rval)),
            },
            BinaryOp::NotEq => match (&lval, &rval) {
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) != *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a != (*b as f64))),
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
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) < *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a < (*b as f64))),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(a < b)),
                _ => Err(RuntimeError { message: "unsupported operands for <".to_string(), span: *span }),
            },
            BinaryOp::Gt => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) > *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a > (*b as f64))),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(a > b)),
                _ => Err(RuntimeError { message: "unsupported operands for >".to_string(), span: *span }),
            },
            BinaryOp::LtEq => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) <= *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a <= (*b as f64))),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(a <= b)),
                _ => Err(RuntimeError { message: "unsupported operands for <=".to_string(), span: *span }),
            },
            BinaryOp::GtEq => match (&lval, &rval) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) >= *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a >= (*b as f64))),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(a >= b)),
                _ => Err(RuntimeError { message: "unsupported operands for >=".to_string(), span: *span }),
            },
            BinaryOp::In => {
                let contains = match &rval {
                    Value::List(l) => l.borrow().contains(&lval),
                    Value::Set(s) => s.borrow().contains(&lval),
                    Value::Dict(d) => match &lval {
                        Value::Str(s) => d.borrow().contains_key(s),
                        _ => false,
                    },
                    Value::Str(s) => match &lval {
                        Value::Str(sub) => s.contains(sub),
                        _ => false,
                    },
                    _ => return Err(RuntimeError { message: format!("'in' operator not supported for {}", rval.type_name()), span: *span }),
                };
                Ok(Value::Bool(contains))
            }
            BinaryOp::NotIn => {
                let contains = match &rval {
                    Value::List(l) => l.borrow().contains(&lval),
                    Value::Set(s) => s.borrow().contains(&lval),
                    Value::Dict(d) => match &lval {
                        Value::Str(s) => d.borrow().contains_key(s),
                        _ => false,
                    },
                    Value::Str(s) => match &lval {
                        Value::Str(sub) => s.contains(sub),
                        _ => false,
                    },
                    _ => return Err(RuntimeError { message: format!("'not in' operator not supported for {}", rval.type_name()), span: *span }),
                };
                Ok(Value::Bool(!contains))
            }
            BinaryOp::Is => {
                let same = match (&lval, &rval) {
                    (Value::None, Value::None) => true,
                    (Value::None, _) | (_, Value::None) => false,
                    (Value::Bool(a), Value::Bool(b)) => a == b,
                    (Value::Int(a), Value::Int(b)) => a == b,
                    (Value::Object { fields: f1, .. }, Value::Object { fields: f2, .. }) => Rc::ptr_eq(f1, f2),
                    (Value::List(a), Value::List(b)) => Rc::ptr_eq(a, b),
                    (Value::Dict(a), Value::Dict(b)) => Rc::ptr_eq(a, b),
                    (Value::Set(a), Value::Set(b)) => Rc::ptr_eq(a, b),
                    _ => lval == rval,
                };
                Ok(Value::Bool(same))
            }
            BinaryOp::IsNot => {
                let same = match (&lval, &rval) {
                    (Value::None, Value::None) => true,
                    (Value::None, _) | (_, Value::None) => false,
                    (Value::Bool(a), Value::Bool(b)) => a == b,
                    (Value::Int(a), Value::Int(b)) => a == b,
                    (Value::Object { fields: f1, .. }, Value::Object { fields: f2, .. }) => Rc::ptr_eq(f1, f2),
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
            let s: Vec<String> = args.iter().map(|a| match a {
                Value::Str(text) => text.clone(),
                other => format!("{:?}", other),
            }).collect();
            let line = s.join(" ");
            interp.output.push(line);
            Ok(Value::None)
        });
        self.env.borrow_mut().set("print".to_string(), Value::BuiltinFunction { name: "print".to_string(), func: print_fn });

        // freeze(obj) -> !T
        let freeze_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if let Some(first) = args.first() {
                first.freeze();
                Ok(first.clone())
            } else {
                Ok(Value::None)
            }
        });
        self.env.borrow_mut().set("freeze".to_string(), Value::BuiltinFunction { name: "freeze".to_string(), func: freeze_fn });

        // Sentinel()
        let sentinel_fn = Rc::new(|_args: &[Value], _interp: &mut Interpreter| {
            Ok(Value::Sentinel("Sentinel".to_string()))
        });
        self.env.borrow_mut().set("Sentinel".to_string(), Value::BuiltinFunction { name: "Sentinel".to_string(), func: sentinel_fn });

        // Cell(val)
        let cell_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if let Some(first) = args.first() {
                Ok(first.clone())
            } else {
                Ok(Value::None)
            }
        });
        self.env.borrow_mut().set("Cell".to_string(), Value::BuiltinFunction { name: "Cell".to_string(), func: cell_fn });

        // range(stop) / range(start, stop, [step])
        let range_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            let (start, stop, step) = match args.len() {
                1 => match &args[0] {
                    Value::Int(stop) => (0, *stop, 1),
                    _ => return Err(RuntimeError { message: "range() stop must be an int".into(), span: Span::default() }),
                },
                2 => match (&args[0], &args[1]) {
                    (Value::Int(start), Value::Int(stop)) => (*start, *stop, 1),
                    _ => return Err(RuntimeError { message: "range() arguments must be ints".into(), span: Span::default() }),
                },
                3 => match (&args[0], &args[1], &args[2]) {
                    (Value::Int(start), Value::Int(stop), Value::Int(step)) => {
                        if *step == 0 {
                            return Err(RuntimeError { message: "range() step cannot be zero".into(), span: Span::default() });
                        }
                        (*start, *stop, *step)
                    }
                    _ => return Err(RuntimeError { message: "range() arguments must be ints".into(), span: Span::default() }),
                },
                n => return Err(RuntimeError { message: format!("range() takes 1 to 3 arguments, got {n}"), span: Span::default() }),
            };

            Ok(Value::Range { start, stop, step })
        });
        self.env.borrow_mut().set("range".to_string(), Value::BuiltinFunction { name: "range".to_string(), func: range_fn });

        // len(x)
        let len_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError { message: format!("len() takes exactly 1 argument (got {})", args.len()), span: Span::default() });
            }
            match &args[0] {
                Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
                Value::List(l) => Ok(Value::Int(l.borrow().len() as i64)),
                Value::Dict(d) => Ok(Value::Int(d.borrow().len() as i64)),
                Value::Set(s) => Ok(Value::Int(s.borrow().len() as i64)),
                Value::Record(r) => Ok(Value::Int(r.borrow().len() as i64)),
                Value::Range { start, stop, step } => {
                    let count = if *step > 0 {
                        if *stop > *start { (*stop - *start + *step - 1) / *step } else { 0 }
                    } else {
                        if *start > *stop { (*start - *stop + (-*step) - 1) / (-*step) } else { 0 }
                    };
                    Ok(Value::Int(count))
                }
                other => Err(RuntimeError {
                    message: format!("object of type '{}' has no len()", other.type_name()),
                    span: Span::default(),
                }),
            }
        });
        self.env.borrow_mut().set("len".to_string(), Value::BuiltinFunction { name: "len".to_string(), func: len_fn });

        // min(x) / min(a, b, ...)
        let min_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.is_empty() {
                return Err(RuntimeError { message: "min() expects at least 1 argument".into(), span: Span::default() });
            }
            let items: Vec<Value> = if args.len() == 1 {
                match &args[0] {
                    Value::List(l) => l.borrow().clone(),
                    Value::Set(s) => s.borrow().clone(),
                    Value::Range { start, stop, step } => {
                        let mut r = Vec::new();
                        let mut cur = *start;
                        if *step > 0 {
                            while cur < *stop { r.push(Value::Int(cur)); cur += *step; }
                        } else {
                            while cur > *stop { r.push(Value::Int(cur)); cur += *step; }
                        }
                        r
                    }
                    other => return Err(RuntimeError { message: format!("min() arg must be iterable, got {}", other.type_name()), span: Span::default() }),
                }
            } else {
                args.to_vec()
            };
            if items.is_empty() {
                return Err(RuntimeError { message: "min() arg is an empty sequence".into(), span: Span::default() });
            }
            let mut current_min = items[0].clone();
            for item in &items[1..] {
                if compare_values(item, &current_min)? == std::cmp::Ordering::Less {
                    current_min = item.clone();
                }
            }
            Ok(current_min)
        });
        self.env.borrow_mut().set("min".to_string(), Value::BuiltinFunction { name: "min".to_string(), func: min_fn });

        // max(x) / max(a, b, ...)
        let max_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.is_empty() {
                return Err(RuntimeError { message: "max() expects at least 1 argument".into(), span: Span::default() });
            }
            let items: Vec<Value> = if args.len() == 1 {
                match &args[0] {
                    Value::List(l) => l.borrow().clone(),
                    Value::Set(s) => s.borrow().clone(),
                    Value::Range { start, stop, step } => {
                        let mut r = Vec::new();
                        let mut cur = *start;
                        if *step > 0 {
                            while cur < *stop { r.push(Value::Int(cur)); cur += *step; }
                        } else {
                            while cur > *stop { r.push(Value::Int(cur)); cur += *step; }
                        }
                        r
                    }
                    other => return Err(RuntimeError { message: format!("max() arg must be iterable, got {}", other.type_name()), span: Span::default() }),
                }
            } else {
                args.to_vec()
            };
            if items.is_empty() {
                return Err(RuntimeError { message: "max() arg is an empty sequence".into(), span: Span::default() });
            }
            let mut current_max = items[0].clone();
            for item in &items[1..] {
                if compare_values(item, &current_max)? == std::cmp::Ordering::Greater {
                    current_max = item.clone();
                }
            }
            Ok(current_max)
        });
        self.env.borrow_mut().set("max".to_string(), Value::BuiltinFunction { name: "max".to_string(), func: max_fn });

        // sum(iterable, [start])
        let sum_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError { message: "sum() takes 1 or 2 arguments".into(), span: Span::default() });
            }
            let items: Vec<Value> = match &args[0] {
                Value::List(l) => l.borrow().clone(),
                Value::Set(s) => s.borrow().clone(),
                Value::Range { start, stop, step } => {
                    let mut r = Vec::new();
                    let mut cur = *start;
                    if *step > 0 {
                        while cur < *stop { r.push(Value::Int(cur)); cur += *step; }
                    } else {
                        while cur > *stop { r.push(Value::Int(cur)); cur += *step; }
                    }
                    r
                }
                other => return Err(RuntimeError { message: format!("sum() iterable must be list or set, got {}", other.type_name()), span: Span::default() }),
            };
            let start = if args.len() == 2 { args[1].clone() } else { Value::Int(0) };
            let mut total = start;
            for item in items {
                total = match (&total, &item) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(a + b),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a + b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 + b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a + *b as f64),
                    _ => return Err(RuntimeError { message: "sum() items must be numbers".into(), span: Span::default() }),
                };
            }
            Ok(total)
        });
        self.env.borrow_mut().set("sum".to_string(), Value::BuiltinFunction { name: "sum".to_string(), func: sum_fn });

        // read_file(path)
        let read_file_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError { message: "read_file() takes exactly 1 argument (path)".into(), span: Span::default() });
            }
            let path = match &args[0] {
                Value::Str(p) => p,
                _ => return Err(RuntimeError { message: "read_file() path must be a string".into(), span: Span::default() }),
            };
            match std::fs::read_to_string(path) {
                Ok(contents) => Ok(Value::Str(contents)),
                Err(e) => Err(RuntimeError { message: format!("read_file('{path}') failed: {e}"), span: Span::default() }),
            }
        });
        self.env.borrow_mut().set("read_file".to_string(), Value::BuiltinFunction { name: "read_file".to_string(), func: read_file_fn });

        // write_file(path, content)
        let write_file_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 2 {
                return Err(RuntimeError { message: "write_file() takes exactly 2 arguments (path, content)".into(), span: Span::default() });
            }
            let path = match &args[0] {
                Value::Str(p) => p,
                _ => return Err(RuntimeError { message: "write_file() path must be a string".into(), span: Span::default() }),
            };
            let content = match &args[1] {
                Value::Str(c) => c,
                _ => return Err(RuntimeError { message: "write_file() content must be a string".into(), span: Span::default() }),
            };
            match std::fs::write(path, content) {
                Ok(()) => Ok(Value::None),
                Err(e) => Err(RuntimeError { message: format!("write_file('{path}') failed: {e}"), span: Span::default() }),
            }
        });
        self.env.borrow_mut().set("write_file".to_string(), Value::BuiltinFunction { name: "write_file".to_string(), func: write_file_fn });

        // env_var(name, [default])
        let env_var_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError { message: "env_var() takes 1 or 2 arguments (name, [default])".into(), span: Span::default() });
            }
            let name = match &args[0] {
                Value::Str(n) => n,
                _ => return Err(RuntimeError { message: "env_var() name must be a string".into(), span: Span::default() }),
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
        self.env.borrow_mut().set("env_var".to_string(), Value::BuiltinFunction { name: "env_var".to_string(), func: env_var_fn });

        // str(x)
        let str_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError { message: "str() takes exactly 1 argument".into(), span: Span::default() });
            }
            match &args[0] {
                Value::Str(s) => Ok(Value::Str(s.clone())),
                Value::Int(n) => Ok(Value::Str(n.to_string())),
                Value::Float(f) => Ok(Value::Str(f.to_string())),
                Value::Bool(b) => Ok(Value::Str(b.to_string())),
                Value::None => Ok(Value::Str("none".to_string())),
                other => Ok(Value::Str(format!("{other:?}"))),
            }
        });
        self.env.borrow_mut().set("str".to_string(), Value::BuiltinFunction { name: "str".to_string(), func: str_fn });

        // int(x)
        let int_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError { message: "int() takes exactly 1 argument".into(), span: Span::default() });
            }
            match &args[0] {
                Value::Int(n) => Ok(Value::Int(*n)),
                Value::Float(f) => Ok(Value::Int(*f as i64)),
                Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
                Value::Str(s) => s.trim().parse::<i64>().map(Value::Int).map_err(|e| RuntimeError {
                    message: format!("invalid literal for int(): '{s}' ({e})"),
                    span: Span::default(),
                }),
                other => Err(RuntimeError { message: format!("int() cannot convert {}", other.type_name()), span: Span::default() }),
            }
        });
        self.env.borrow_mut().set("int".to_string(), Value::BuiltinFunction { name: "int".to_string(), func: int_fn });

        // float(x)
        let float_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError { message: "float() takes exactly 1 argument".into(), span: Span::default() });
            }
            match &args[0] {
                Value::Float(f) => Ok(Value::Float(*f)),
                Value::Int(n) => Ok(Value::Float(*n as f64)),
                Value::Bool(b) => Ok(Value::Float(if *b { 1.0 } else { 0.0 })),
                Value::Str(s) => s.trim().parse::<f64>().map(Value::Float).map_err(|e| RuntimeError {
                    message: format!("invalid literal for float(): '{s}' ({e})"),
                    span: Span::default(),
                }),
                other => Err(RuntimeError { message: format!("float() cannot convert {}", other.type_name()), span: Span::default() }),
            }
        });
        self.env.borrow_mut().set("float".to_string(), Value::BuiltinFunction { name: "float".to_string(), func: float_fn });

        // bool(x)
        let bool_fn = Rc::new(|args: &[Value], interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError { message: "bool() takes exactly 1 argument".into(), span: Span::default() });
            }
            Ok(Value::Bool(interp.is_truthy(&args[0])))
        });
        self.env.borrow_mut().set("bool".to_string(), Value::BuiltinFunction { name: "bool".to_string(), func: bool_fn });

        // list(x)
        let list_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.is_empty() {
                return Ok(Value::List(Rc::new(RefCell::new(Vec::new()))));
            }
            match &args[0] {
                Value::List(l) => Ok(Value::List(Rc::new(RefCell::new(l.borrow().clone())))),
                Value::Set(s) => Ok(Value::List(Rc::new(RefCell::new(s.borrow().clone())))),
                Value::Range { start, stop, step } => {
                    let mut items = Vec::new();
                    let mut cur = *start;
                    if *step > 0 {
                        while cur < *stop {
                            items.push(Value::Int(cur));
                            cur += *step;
                        }
                    } else {
                        while cur > *stop {
                            items.push(Value::Int(cur));
                            cur += *step;
                        }
                    }
                    Ok(Value::List(Rc::new(RefCell::new(items))))
                }
                Value::Str(s) => Ok(Value::List(Rc::new(RefCell::new(s.chars().map(|c| Value::Str(c.to_string())).collect())))),
                other => Err(RuntimeError { message: format!("cannot convert {} to list", other.type_name()), span: Span::default() }),
            }
        });
        self.env.borrow_mut().set("list".to_string(), Value::BuiltinFunction { name: "list".to_string(), func: list_fn });

        // abs(x)
        let abs_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError { message: "abs() takes exactly 1 argument".into(), span: Span::default() });
            }
            match &args[0] {
                Value::Int(n) => Ok(Value::Int(n.abs())),
                Value::Float(f) => Ok(Value::Float(f.abs())),
                other => Err(RuntimeError { message: format!("bad operand type for abs(): {}", other.type_name()), span: Span::default() }),
            }
        });
        self.env.borrow_mut().set("abs".to_string(), Value::BuiltinFunction { name: "abs".to_string(), func: abs_fn });

        // round(x, [ndigits])
        let round_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            let (num, ndigits) = match args.len() {
                1 => (&args[0], 0i64),
                2 => match &args[1] {
                    Value::Int(d) => (&args[0], *d),
                    _ => return Err(RuntimeError { message: "round() ndigits must be an int".into(), span: Span::default() }),
                },
                n => return Err(RuntimeError { message: format!("round() takes 1 or 2 arguments (got {n})"), span: Span::default() }),
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
                other => Err(RuntimeError { message: format!("type {} doesn't define round", other.type_name()), span: Span::default() }),
            }
        });
        self.env.borrow_mut().set("round".to_string(), Value::BuiltinFunction { name: "round".to_string(), func: round_fn });

        // ord(c)
        let ord_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError { message: "ord() takes exactly 1 argument".into(), span: Span::default() });
            }
            match &args[0] {
                Value::Str(s) => {
                    let mut chars = s.chars();
                    if let (Some(c), None) = (chars.next(), chars.next()) {
                        Ok(Value::Int(c as i64))
                    } else {
                        Err(RuntimeError { message: format!("ord() expected a character, but string of length {} found", s.len()), span: Span::default() })
                    }
                }
                other => Err(RuntimeError { message: format!("ord() expected string of length 1, but {} found", other.type_name()), span: Span::default() }),
            }
        });
        self.env.borrow_mut().set("ord".to_string(), Value::BuiltinFunction { name: "ord".to_string(), func: ord_fn });

        // chr(i)
        let chr_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            if args.len() != 1 {
                return Err(RuntimeError { message: "chr() takes exactly 1 argument".into(), span: Span::default() });
            }
            match &args[0] {
                Value::Int(n) => {
                    if let Some(c) = char::from_u32(*n as u32) {
                        Ok(Value::Str(c.to_string()))
                    } else {
                        Err(RuntimeError { message: format!("chr() arg not in range: {n}"), span: Span::default() })
                    }
                }
                other => Err(RuntimeError { message: format!("an integer is required (got type {})", other.type_name()), span: Span::default() }),
            }
        });
        self.env.borrow_mut().set("chr".to_string(), Value::BuiltinFunction { name: "chr".to_string(), func: chr_fn });

        // time()
        let time_now_fn = Rc::new(|_args: &[Value], _interp: &mut Interpreter| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            Ok(Value::Float(now))
        });
        self.env.borrow_mut().set("time".to_string(), Value::BuiltinFunction { name: "time".to_string(), func: time_now_fn });

        // Default multiple dispatch operators (+, -, *, ==, etc.)
        let add_int = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            match (&args[0], &args[1]) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{a}{b}"))),
                _ => Err(RuntimeError { message: "unsupported operands for +".to_string(), span: Span::default() }),
            }
        });
        self.dispatch.register("+".to_string(), vec!["int".to_string(), "int".to_string()], add_int.clone());
        self.dispatch.register("+".to_string(), vec!["float".to_string(), "float".to_string()], add_int.clone());
        self.dispatch.register("+".to_string(), vec!["str".to_string(), "str".to_string()], add_int);

        let sub_int = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            match (&args[0], &args[1]) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
                _ => Err(RuntimeError { message: "unsupported operands for -".to_string(), span: Span::default() }),
            }
        });
        self.dispatch.register("-".to_string(), vec!["int".to_string(), "int".to_string()], sub_int.clone());
        self.dispatch.register("-".to_string(), vec!["float".to_string(), "float".to_string()], sub_int);

        let mul_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            match (&args[0], &args[1]) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
                (Value::Str(s), Value::Int(n)) => Ok(Value::Str(s.repeat((*n).max(0) as usize))),
                _ => Err(RuntimeError { message: "unsupported operands for *".to_string(), span: Span::default() }),
            }
        });
        self.dispatch.register("*".to_string(), vec!["int".to_string(), "int".to_string()], mul_fn.clone());
        self.dispatch.register("*".to_string(), vec!["float".to_string(), "float".to_string()], mul_fn.clone());
        self.dispatch.register("*".to_string(), vec!["str".to_string(), "int".to_string()], mul_fn);

        let div_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            match (&args[0], &args[1]) {
                (Value::Int(a), Value::Int(b)) => {
                    if *b == 0 { Err(RuntimeError { message: "division by zero".to_string(), span: Span::default() }) }
                    else { Ok(Value::Float(*a as f64 / *b as f64)) }
                }
                (Value::Float(a), Value::Float(b)) => {
                    if *b == 0.0 { Err(RuntimeError { message: "division by zero".to_string(), span: Span::default() }) }
                    else { Ok(Value::Float(a / b)) }
                }
                _ => Err(RuntimeError { message: "unsupported operands for /".to_string(), span: Span::default() }),
            }
        });
        self.dispatch.register("/".to_string(), vec!["int".to_string(), "int".to_string()], div_fn.clone());
        self.dispatch.register("/".to_string(), vec!["float".to_string(), "float".to_string()], div_fn);

        let eq_fn = Rc::new(|args: &[Value], _interp: &mut Interpreter| {
            Ok(Value::Bool(args[0] == args[1]))
        });
        self.dispatch.register("==".to_string(), vec!["object".to_string(), "object".to_string()], eq_fn);
    }

    pub fn load_module(&mut self, module_name: &str, span: Span) -> Result<Rc<RefCell<Environment>>, RuntimeError> {
        if module_name == "math" {
            let math_env = Rc::new(RefCell::new(Environment::new()));
            math_env.borrow_mut().set("pi".to_string(), Value::Float(std::f64::consts::PI));
            math_env.borrow_mut().set("e".to_string(), Value::Float(std::f64::consts::E));
            math_env.borrow_mut().set("sqrt".to_string(), Value::BuiltinFunction {
                name: "sqrt".to_string(),
                func: Rc::new(|args, _interp| {
                    if args.len() != 1 { return Err(RuntimeError { message: "sqrt() takes 1 argument".into(), span: Span::default() }); }
                    let val = match &args[0] {
                        Value::Int(n) => *n as f64,
                        Value::Float(f) => *f,
                        _ => return Err(RuntimeError { message: "sqrt() arg must be a number".into(), span: Span::default() }),
                    };
                    Ok(Value::Float(val.sqrt()))
                }),
            });
            math_env.borrow_mut().set("sin".to_string(), Value::BuiltinFunction {
                name: "sin".to_string(),
                func: Rc::new(|args, _interp| {
                    if args.len() != 1 { return Err(RuntimeError { message: "sin() takes 1 argument".into(), span: Span::default() }); }
                    let val = match &args[0] { Value::Int(n) => *n as f64, Value::Float(f) => *f, _ => return Err(RuntimeError { message: "sin() arg must be a number".into(), span: Span::default() }) };
                    Ok(Value::Float(val.sin()))
                }),
            });
            math_env.borrow_mut().set("cos".to_string(), Value::BuiltinFunction {
                name: "cos".to_string(),
                func: Rc::new(|args, _interp| {
                    if args.len() != 1 { return Err(RuntimeError { message: "cos() takes 1 argument".into(), span: Span::default() }); }
                    let val = match &args[0] { Value::Int(n) => *n as f64, Value::Float(f) => *f, _ => return Err(RuntimeError { message: "cos() arg must be a number".into(), span: Span::default() }) };
                    Ok(Value::Float(val.cos()))
                }),
            });
            math_env.borrow_mut().set("tan".to_string(), Value::BuiltinFunction {
                name: "tan".to_string(),
                func: Rc::new(|args, _interp| {
                    if args.len() != 1 { return Err(RuntimeError { message: "tan() takes 1 argument".into(), span: Span::default() }); }
                    let val = match &args[0] { Value::Int(n) => *n as f64, Value::Float(f) => *f, _ => return Err(RuntimeError { message: "tan() arg must be a number".into(), span: Span::default() }) };
                    Ok(Value::Float(val.tan()))
                }),
            });
            math_env.borrow_mut().set("floor".to_string(), Value::BuiltinFunction {
                name: "floor".to_string(),
                func: Rc::new(|args, _interp| {
                    if args.len() != 1 { return Err(RuntimeError { message: "floor() takes 1 argument".into(), span: Span::default() }); }
                    let val = match &args[0] { Value::Int(n) => *n, Value::Float(f) => f.floor() as i64, _ => return Err(RuntimeError { message: "floor() arg must be a number".into(), span: Span::default() }) };
                    Ok(Value::Int(val))
                }),
            });
            math_env.borrow_mut().set("ceil".to_string(), Value::BuiltinFunction {
                name: "ceil".to_string(),
                func: Rc::new(|args, _interp| {
                    if args.len() != 1 { return Err(RuntimeError { message: "ceil() takes 1 argument".into(), span: Span::default() }); }
                    let val = match &args[0] { Value::Int(n) => *n, Value::Float(f) => f.ceil() as i64, _ => return Err(RuntimeError { message: "ceil() arg must be a number".into(), span: Span::default() }) };
                    Ok(Value::Int(val))
                }),
            });
            math_env.borrow_mut().set("abs".to_string(), Value::BuiltinFunction {
                name: "abs".to_string(),
                func: Rc::new(|args, _interp| {
                    if args.len() != 1 { return Err(RuntimeError { message: "abs() takes 1 argument".into(), span: Span::default() }); }
                    match &args[0] {
                        Value::Int(n) => Ok(Value::Int(n.abs())),
                        Value::Float(f) => Ok(Value::Float(f.abs())),
                        _ => Err(RuntimeError { message: "abs() arg must be a number".into(), span: Span::default() }),
                    }
                }),
            });
            return Ok(math_env);
        }

        if module_name == "sys" {
            let sys_env = Rc::new(RefCell::new(Environment::new()));
            sys_env.borrow_mut().set("platform".to_string(), Value::Str(std::env::consts::OS.to_string()));
            sys_env.borrow_mut().set("version".to_string(), Value::Str("0.1.0".to_string()));
            sys_env.borrow_mut().set("argv".to_string(), Value::List(Rc::new(RefCell::new(vec![Value::Str("lucid".to_string())]))));
            return Ok(sys_env);
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
            time_env.borrow_mut().set("time".to_string(), Value::BuiltinFunction { name: "time".to_string(), func: time_fn.clone() });
            time_env.borrow_mut().set("monotonic".to_string(), Value::BuiltinFunction { name: "monotonic".to_string(), func: time_fn.clone() });
            return Ok(time_env);
        }

        // Resolve relative module file
        let dot_count = module_name.chars().take_while(|&c| c == '.').count();
        let rest = &module_name[dot_count..];
        let rel_path = rest.replace('.', "/");

        let current_dir = if let Some(ref cf) = self.current_file {
            cf.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf()
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
                message: format!("cannot find module '{module_name}' (looked at {})", candidate.display()),
                span,
            });
        }

        let canon = std::fs::canonicalize(&candidate).unwrap_or(candidate);
        if let Some(cached) = self.module_cache.get(&canon) {
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

        let mut sub_interp = Interpreter::new();
        sub_interp.current_file = Some(canon.clone());
        sub_interp.module_cache = self.module_cache.clone();
        sub_interp.eval_module(&parsed)?;

        let env = sub_interp.env;
        self.module_cache = sub_interp.module_cache;
        self.module_cache.insert(canon, Rc::clone(&env));

        Ok(env)
    }

    pub fn eval_module(&mut self, module: &Module) -> Result<Value, RuntimeError> {
        let mut last_val = Value::None;
        for stmt in &module.statements {
            last_val = self.eval_statement(stmt)?;
            if let Value::Return(v) = last_val {
                return Ok(*v);
            }
        }
        Ok(last_val)
    }

    pub fn eval_statement(&mut self, stmt: &Stmt) -> Result<Value, RuntimeError> {
        match stmt {
            Stmt::Export(inner) => self.eval_statement(inner),
            Stmt::ClassDef { .. } => {
                if let Stmt::ClassDef { name, type_params, bases, without_traits, body, is_sealed, is_final, span, .. } = stmt.clone() {
                    self.classes.insert(name.clone(), ClassDef {
                        name: name.clone(),
                        type_params,
                        bases,
                        without_traits,
                        body,
                        is_sealed,
                        is_final,
                        span,
                    });

                    // Register class constructor in env
                    let c_name = name.clone();
                    let class_val = Value::BuiltinFunction {
                        name: c_name.clone(),
                        func: Rc::new(move |args, interp| {
                            interp.construct_class(&c_name, args)
                        }),
                    };
                    self.env.borrow_mut().set(name, class_val);
                }
                Ok(Value::None)
            }
            Stmt::TraitDef { name, body, .. } => {
                self.traits.insert(name.clone(), body.clone());
                Ok(Value::None)
            }
            Stmt::Function(func) => {
                let func_val = Value::Function {
                    name: func.name.clone(),
                    params: func.params.clone(),
                    body: func.body.clone(),
                    closure: Rc::clone(&self.env),
                };

                if func.is_dispatch {
                    let f_name = func.name.clone();
                    let param_types: Vec<String> = func.params.iter().map(|p| {
                        p.type_annotation.as_ref().map(|t| match t {
                            TypeExpr::Named { name, .. } => name.clone(),
                            _ => "object".to_string(),
                        }).unwrap_or_else(|| "object".to_string())
                    }).collect();

                    let func_clone = func.clone();
                    let closure = Rc::clone(&self.env);
                    let dispatch_fn = Rc::new(move |args: &[Value], interp: &mut Interpreter| {
                        interp.call_function(&func_clone, args, Rc::clone(&closure))
                    });

                    self.dispatch.register(f_name.clone(), param_types.clone(), dispatch_fn.clone());

                    if f_name == "__add__" {
                        self.dispatch.register("+".to_string(), param_types.clone(), dispatch_fn.clone());
                    } else if f_name == "__sub__" {
                        self.dispatch.register("-".to_string(), param_types.clone(), dispatch_fn.clone());
                    } else if f_name == "__mul__" {
                        self.dispatch.register("*".to_string(), param_types.clone(), dispatch_fn.clone());
                    } else if f_name == "__truediv__" || f_name == "__div__" {
                        self.dispatch.register("/".to_string(), param_types.clone(), dispatch_fn.clone());
                    } else if f_name == "__eq__" {
                        self.dispatch.register("==".to_string(), param_types.clone(), dispatch_fn.clone());
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
            Stmt::VarDef { pattern, value, span, .. } => {
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
                Ok(val)
            }
            Stmt::Assignment { target, value, span } => {
                let val = self.eval_expr(value)?;
                if let Value::Return(_) = val {
                    return Ok(val);
                }
                match target {
                    Expr::Ident { name, .. } => {
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
                                sorted_keys.iter().map(|k| r.borrow().get(k).unwrap().clone()).collect()
                            }
                            _ => Vec::new(),
                        };
                        for ((_, field_expr), item) in fields.iter().zip(items) {
                            if let Expr::Ident { name, .. } = field_expr {
                                if !self.env.borrow_mut().mutate(name, item.clone()) {
                                    self.env.borrow_mut().set(name.clone(), item);
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
                    Expr::Attribute { value: obj_expr, attr, .. } => {
                        let obj = self.eval_expr(obj_expr)?;
                        match obj {
                            Value::Object { fields, is_frozen, class_name } => {
                                if *is_frozen.borrow() {
                                    return Err(RuntimeError {
                                        message: format!("cannot mutate attribute '{attr}' on frozen object !{class_name}"),
                                        span: *span,
                                    });
                                }
                                fields.borrow_mut().insert(attr.clone(), val.clone());
                            }
                            _ => return Err(RuntimeError {
                                message: "cannot set attribute on non-object".to_string(),
                                span: *span,
                            }),
                        }
                    }
                    Expr::Index { value: obj_expr, index: idx_expr, span: idx_span } => {
                        let obj = self.eval_expr(obj_expr)?;
                        let idx = self.eval_expr(idx_expr)?;
                        match obj {
                            Value::List(items) => {
                                match idx {
                                    Value::Int(i) => {
                                        let len = items.borrow().len() as i64;
                                        let actual_i = if i < 0 { len + i } else { i };
                                        if actual_i < 0 || actual_i >= len {
                                            return Err(RuntimeError {
                                                message: format!("list index out of range: index {i}, len {len}"),
                                                span: *idx_span,
                                            });
                                        }
                                        items.borrow_mut()[actual_i as usize] = val.clone();
                                    }
                                    _ => return Err(RuntimeError {
                                        message: format!("list indices must be integers, got {}", idx.type_name()),
                                        span: *idx_span,
                                    }),
                                }
                            }
                            Value::Dict(d) => {
                                let key_str = match &idx {
                                    Value::Str(s) => s.clone(),
                                    Value::Int(n) => n.to_string(),
                                    other => format!("{other:?}"),
                                };
                                d.borrow_mut().insert(key_str, val.clone());
                            }
                            _ => return Err(RuntimeError {
                                message: format!("cannot index assign into {}", obj.type_name()),
                                span: *idx_span,
                            }),
                        }
                    }
                    _ => return Err(RuntimeError {
                        message: "invalid assignment target".to_string(),
                        span: *span,
                    }),
                }
                Ok(val)
            }
            Stmt::AugAssign { target, op, value, span } => {
                let rhs = self.eval_expr(value)?;
                if let Value::Return(_) = rhs {
                    return Ok(rhs);
                }
                match target {
                    Expr::Ident { name, span: id_span } => {
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
                    Expr::Index { value: obj_expr, index: idx_expr, span: idx_span } => {
                        let obj = self.eval_expr(obj_expr)?;
                        let idx = self.eval_expr(idx_expr)?;
                        match obj {
                            Value::List(items) => {
                                match idx {
                                    Value::Int(i) => {
                                        let len = items.borrow().len() as i64;
                                        let actual_i = if i < 0 { len + i } else { i };
                                        if actual_i < 0 || actual_i >= len {
                                            return Err(RuntimeError {
                                                message: format!("list index out of range: index {i}, len {len}"),
                                                span: *idx_span,
                                            });
                                        }
                                        let cur = items.borrow()[actual_i as usize].clone();
                                        let new_val = self.eval_binary_op(op, cur, rhs, span)?;
                                        items.borrow_mut()[actual_i as usize] = new_val.clone();
                                        Ok(new_val)
                                    }
                                    _ => return Err(RuntimeError {
                                        message: format!("list indices must be integers, got {}", idx.type_name()),
                                        span: *idx_span,
                                    }),
                                }
                            }
                            Value::Dict(d) => {
                                let key_str = match &idx {
                                    Value::Str(s) => s.clone(),
                                    Value::Int(n) => n.to_string(),
                                    other => format!("{other:?}"),
                                };
                                let cur = d.borrow().get(&key_str).cloned().unwrap_or(Value::Int(0));
                                let new_val = self.eval_binary_op(op, cur, rhs, span)?;
                                d.borrow_mut().insert(key_str, new_val.clone());
                                Ok(new_val)
                            }
                            _ => return Err(RuntimeError {
                                message: format!("cannot index mutate {}", obj.type_name()),
                                span: *idx_span,
                            }),
                        }
                    }
                    Expr::Attribute { value: obj_expr, attr, span: attr_span } => {
                        let obj = self.eval_expr(obj_expr)?;
                        match obj {
                            Value::Object { fields, is_frozen, class_name } => {
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
                            _ => return Err(RuntimeError {
                                message: "cannot set attribute on non-object".to_string(),
                                span: *attr_span,
                            }),
                        }
                    }
                    _ => Err(RuntimeError {
                        message: "invalid augmented assignment target".to_string(),
                        span: *span,
                    }),
                }
            }
            Stmt::If { condition, then_branch, elif_branches, else_branch, .. } => {
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
            Stmt::For { target, iterable, body, if_broken, span } => {
                let iter_val = self.eval_expr(iterable)?;
                let mut broken = false;

                if let Value::Range { start, stop, step } = iter_val {
                    let mut cur = start;
                    while (step > 0 && cur < stop) || (step < 0 && cur > stop) {
                        let iter_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&self.env))));
                        let prev_env = Rc::clone(&self.env);
                        self.env = iter_env;

                        self.bind_pattern(target, Value::Int(cur), *span)?;
                        let res = self.eval_block(body);
                        self.env = prev_env;

                        match res {
                            Ok(Value::Sentinel(s)) if s == "__break__" => {
                                broken = true;
                                break;
                            }
                            Ok(Value::Sentinel(s)) if s == "__continue__" => {}
                            Ok(Value::Return(val)) => return Ok(Value::Return(val)),
                            Ok(v) => { let _ = v; }
                            Err(e) => return Err(e),
                        }
                        cur += step;
                    }
                } else {
                    let items = match iter_val {
                        Value::List(items) => items.borrow().clone(),
                        Value::Set(items) => items.borrow().clone(),
                        _ => return Err(RuntimeError {
                            message: "value is not iterable".to_string(),
                            span: *span,
                        }),
                    };

                    for item in items {
                        // Fresh iteration binding (basedpython semantics)
                        let iter_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&self.env))));
                        let prev_env = Rc::clone(&self.env);
                        self.env = iter_env;

                        self.bind_pattern(target, item, *span)?;
                        let res = self.eval_block(body);
                        self.env = prev_env;

                        match res {
                            Ok(Value::Sentinel(s)) if s == "__break__" => {
                                broken = true;
                                break;
                            }
                            Ok(Value::Sentinel(s)) if s == "__continue__" => continue,
                            Ok(Value::Return(val)) => return Ok(Value::Return(val)),
                            Ok(v) => { let _ = v; }
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
            Stmt::While { condition, body, if_broken, .. } => {
                let mut broken = false;
                loop {
                    let cond = self.eval_expr(condition)?;
                    if !self.is_truthy(&cond) {
                        break;
                    }
                    let res = self.eval_block(body);
                    match res {
                        Ok(Value::Sentinel(s)) if s == "__break__" => {
                            broken = true;
                            break;
                        }
                        Ok(Value::Sentinel(s)) if s == "__continue__" => continue,
                        Ok(Value::Return(val)) => return Ok(Value::Return(val)),
                        Ok(v) => { let _ = v; }
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
            Stmt::Match { subject, subject_alias, arms, span } => {
                let subj_val = self.eval_expr(subject)?;
                if let Some(ref alias) = subject_alias {
                    self.env.borrow_mut().set(alias.clone(), subj_val.clone());
                }

                for arm in arms {
                    if self.matches_pattern(&arm.pattern, &subj_val) {
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
            Stmt::Break(_) => Ok(Value::Sentinel("__break__".to_string())),
            Stmt::Continue(_) => Ok(Value::Sentinel("__continue__".to_string())),
            Stmt::With { items, body, .. } => {
                for item in items {
                    let ctx_val = self.eval_expr(&item.context_expr)?;
                    if let Some(ref target) = item.target {
                        self.bind_pattern(target, ctx_val, item.context_expr.span())?;
                    }
                }
                self.eval_block(body)
            }
            Stmt::Import { module, alias, span } => {
                let mod_env = self.load_module(module, *span)?;
                let bound_name = alias.clone().unwrap_or_else(|| {
                    module.rsplit('.').next().unwrap_or(module).to_string()
                });
                let mod_val = Value::Module {
                    name: bound_name.clone(),
                    env: mod_env,
                };
                self.env.borrow_mut().set(bound_name, mod_val);
                Ok(Value::None)
            }
            Stmt::FromImport { module, names, span, .. } => {
                let mod_env = self.load_module(module, *span)?;
                for (name, alias) in names {
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
            _ => Ok(Value::None),
        }
    }

    fn eval_block(&mut self, stmts: &[Stmt]) -> Result<Value, RuntimeError> {
        let mut last_val = Value::None;
        for s in stmts {
            last_val = self.eval_statement(s)?;
            if matches!(last_val, Value::Sentinel(ref s) if s == "__break__" || s == "__continue__") {
                return Ok(last_val);
            }
            if let Value::Return(_) = last_val {
                return Ok(last_val);
            }
        }
        Ok(last_val)
    }

    pub fn eval_expr(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
        match expr {
            Expr::Literal { value, .. } => Ok(match value {
                LiteralValue::Int(n) => Value::Int(*n),
                LiteralValue::Float(f) => Value::Float(*f),
                LiteralValue::Bool(b) => Value::Bool(*b),
                LiteralValue::Str(s) => Value::Str(s.clone()),
                LiteralValue::None => Value::None,
                LiteralValue::Sentinel(s) => Value::Sentinel(s.clone()),
                LiteralValue::Ellipsis => Value::None,
            }),
            Expr::Ident { name, span } => {
                self.env.borrow().get(name).ok_or_else(|| RuntimeError {
                    message: format!("undefined variable '{name}'"),
                    span: *span,
                })
            }
            Expr::Binary { op, left, right, span } => {
                let lval = self.eval_expr(left)?;
                let rval = self.eval_expr(right)?;
                self.eval_binary_op(op, lval, rval, span)
            }
            Expr::Unary { op, expr, span } => {
                let val = self.eval_expr(expr)?;
                match op {
                    UnaryOp::Neg => match val {
                        Value::Int(n) => Ok(Value::Int(-n)),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        _ => Err(RuntimeError { message: "unsupported operand for unary -".to_string(), span: *span }),
                    },
                    UnaryOp::Pos => match val {
                        Value::Int(n) => Ok(Value::Int(n)),
                        Value::Float(f) => Ok(Value::Float(f)),
                        _ => Err(RuntimeError { message: "unsupported operand for unary +".to_string(), span: *span }),
                    },
                    UnaryOp::Not => Ok(Value::Bool(!self.is_truthy(&val))),
                    UnaryOp::Invert => match val {
                        Value::Int(n) => Ok(Value::Int(!n)),
                        _ => Err(RuntimeError { message: "unsupported operand for ~".to_string(), span: *span }),
                    },
                    UnaryOp::Spread | UnaryOp::GatherSpread => Ok(val),
                }
            }
            Expr::Call { func, args, span } => {
                let func_val = self.eval_expr(func)?;
                let mut evaluated_args = Vec::new();
                for arg in args {
                    if arg.is_spread {
                        let val = self.eval_expr(&arg.value)?;
                        match val {
                            Value::List(l) => evaluated_args.extend(l.borrow().clone()),
                            Value::Dict(d) => {
                                for (_, v) in d.borrow().iter() {
                                    evaluated_args.push(v.clone());
                                }
                            }
                            other => evaluated_args.push(other),
                        }
                    } else {
                        evaluated_args.push(self.eval_expr(&arg.value)?);
                    }
                }

                match func_val {
                    Value::BuiltinFunction { func, .. } => (func)(&evaluated_args, self),
                    Value::Function { params, body, closure, .. } => {
                        let call_env = Rc::new(RefCell::new(Environment::with_parent(closure)));
                        for (param, val) in params.iter().zip(evaluated_args) {
                            call_env.borrow_mut().set(param.name.clone(), val);
                        }
                        let prev_env = Rc::clone(&self.env);
                        self.env = call_env;
                        let res = self.eval_block(&body);
                        self.env = prev_env;
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
            Expr::Attribute { value, attr, span } => {
                let obj = self.eval_expr(value)?;
                match obj {
                    Value::Object { fields, is_frozen, class_name } => {
                        if let Some(val) = fields.borrow().get(attr) {
                            if let Value::Function { name, params, body, closure } = val {
                                if !params.is_empty() && params[0].name == "self" {
                                    let bound_self = Value::Object {
                                        class_name: class_name.clone(),
                                        fields: Rc::clone(&fields),
                                        is_frozen: Rc::clone(&is_frozen),
                                    };
                                    let p = params.clone();
                                    let b = body.clone();
                                    let c = Rc::clone(closure);
                                    return Ok(Value::BuiltinFunction {
                                        name: name.clone(),
                                        func: Rc::new(move |args, interp| {
                                            let mut call_args = vec![bound_self.clone()];
                                            call_args.extend_from_slice(args);
                                            let call_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&c))));
                                            for (param, arg_val) in p.iter().zip(call_args) {
                                                call_env.borrow_mut().set(param.name.clone(), arg_val);
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
                            Ok(val.clone())
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
                    Value::Module { name: mod_name, env } => {
                        if let Some(val) = env.borrow().get(attr) {
                            Ok(val)
                        } else {
                            Err(RuntimeError {
                                message: format!("module '{mod_name}' has no attribute '{attr}'"),
                                span: *span,
                            })
                        }
                    }
                    Value::Str(s) => {
                        if attr == "split" {
                            let s = s.clone();
                            return Ok(Value::BuiltinFunction {
                                name: "split".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    let pieces: Vec<Value> = if args.is_empty() {
                                        s.split_whitespace().map(|p| Value::Str(p.to_string())).collect()
                                    } else if let Value::Str(sep) = &args[0] {
                                        s.split(sep.as_str()).map(|p| Value::Str(p.to_string())).collect()
                                    } else {
                                        return Err(RuntimeError { message: "split separator must be a string".into(), span: Span::default() });
                                    };
                                    Ok(Value::List(Rc::new(RefCell::new(pieces))))
                                }),
                            });
                        }
                        if attr == "join" {
                            let sep = s.clone();
                            return Ok(Value::BuiltinFunction {
                                name: "join".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError { message: "join() takes exactly 1 argument (iterable)".into(), span: Span::default() });
                                    }
                                    let items: Vec<String> = match &args[0] {
                                        Value::List(l) => l.borrow().iter().map(|v| match v {
                                            Value::Str(s) => s.clone(),
                                            other => format!("{other:?}"),
                                        }).collect(),
                                        Value::Set(s) => s.borrow().iter().map(|v| match v {
                                            Value::Str(s) => s.clone(),
                                            other => format!("{other:?}"),
                                        }).collect(),
                                        _ => return Err(RuntimeError { message: "join() argument must be a list or set".into(), span: Span::default() }),
                                    };
                                    Ok(Value::Str(items.join(&sep)))
                                }),
                            });
                        }
                        if attr == "strip" || attr == "trim" {
                            let s = s.clone();
                            return Ok(Value::BuiltinFunction {
                                name: attr.to_string(),
                                func: Rc::new(move |_args, _interp| {
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
                                        return Err(RuntimeError { message: "replace() takes exactly 2 arguments (from, to)".into(), span: Span::default() });
                                    }
                                    match (&args[0], &args[1]) {
                                        (Value::Str(from), Value::Str(to)) => Ok(Value::Str(s.replace(from, to))),
                                        _ => Err(RuntimeError { message: "replace() arguments must be strings".into(), span: Span::default() }),
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
                                        return Err(RuntimeError { message: "startswith() takes exactly 1 argument (prefix)".into(), span: Span::default() });
                                    }
                                    match &args[0] {
                                        Value::Str(prefix) => Ok(Value::Bool(s.starts_with(prefix))),
                                        _ => Err(RuntimeError { message: "startswith() prefix must be a string".into(), span: Span::default() }),
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
                                        return Err(RuntimeError { message: "endswith() takes exactly 1 argument (suffix)".into(), span: Span::default() });
                                    }
                                    match &args[0] {
                                        Value::Str(suffix) => Ok(Value::Bool(s.ends_with(suffix))),
                                        _ => Err(RuntimeError { message: "endswith() suffix must be a string".into(), span: Span::default() }),
                                    }
                                }),
                            });
                        }
                        if attr == "lower" {
                            let s = s.clone();
                            return Ok(Value::BuiltinFunction {
                                name: "lower".to_string(),
                                func: Rc::new(move |_args, _interp| {
                                    Ok(Value::Str(s.to_lowercase()))
                                }),
                            });
                        }
                        if attr == "upper" {
                            let s = s.clone();
                            return Ok(Value::BuiltinFunction {
                                name: "upper".to_string(),
                                func: Rc::new(move |_args, _interp| {
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
                        if attr == "append" {
                            let l_clone = Rc::clone(&l);
                            return Ok(Value::BuiltinFunction {
                                name: "append".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if let Some(item) = args.first() {
                                        l_clone.borrow_mut().push(item.clone());
                                    }
                                    Ok(Value::None)
                                }),
                            });
                        }
                        if attr == "pop" {
                            let l_clone = Rc::clone(&l);
                            return Ok(Value::BuiltinFunction {
                                name: "pop".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    let mut vec = l_clone.borrow_mut();
                                    if vec.is_empty() {
                                        return Err(RuntimeError { message: "pop from empty list".into(), span: Span::default() });
                                    }
                                    if args.is_empty() {
                                        Ok(vec.pop().unwrap())
                                    } else if let Value::Int(idx) = &args[0] {
                                        let i = if *idx < 0 { vec.len() as i64 + *idx } else { *idx };
                                        if i < 0 || i as usize >= vec.len() {
                                            return Err(RuntimeError { message: format!("pop index {idx} out of range"), span: Span::default() });
                                        }
                                        Ok(vec.remove(i as usize))
                                    } else {
                                        Err(RuntimeError { message: "pop index must be an int".into(), span: Span::default() })
                                    }
                                }),
                            });
                        }
                        if attr == "insert" {
                            let l_clone = Rc::clone(&l);
                            return Ok(Value::BuiltinFunction {
                                name: "insert".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 2 {
                                        return Err(RuntimeError { message: "insert() takes exactly 2 arguments (index, item)".into(), span: Span::default() });
                                    }
                                    if let Value::Int(idx) = &args[0] {
                                        let mut vec = l_clone.borrow_mut();
                                        let len = vec.len() as i64;
                                        let i = if *idx < 0 { (len + *idx).max(0) as usize } else { (*idx).min(len) as usize };
                                        vec.insert(i, args[1].clone());
                                        Ok(Value::None)
                                    } else {
                                        Err(RuntimeError { message: "insert index must be an int".into(), span: Span::default() })
                                    }
                                }),
                            });
                        }
                        if attr == "extend" {
                            let l_clone = Rc::clone(&l);
                            return Ok(Value::BuiltinFunction {
                                name: "extend".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError { message: "extend() takes exactly 1 argument (iterable)".into(), span: Span::default() });
                                    }
                                    match &args[0] {
                                        Value::List(other) => {
                                            l_clone.borrow_mut().extend(other.borrow().iter().cloned());
                                            Ok(Value::None)
                                        }
                                        Value::Set(other) => {
                                            l_clone.borrow_mut().extend(other.borrow().iter().cloned());
                                            Ok(Value::None)
                                        }
                                        _ => Err(RuntimeError { message: "extend argument must be a list or set".into(), span: Span::default() }),
                                    }
                                }),
                            });
                        }
                        if attr == "clear" {
                            let l_clone = Rc::clone(&l);
                            return Ok(Value::BuiltinFunction {
                                name: "clear".to_string(),
                                func: Rc::new(move |_args, _interp| {
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
                                        return Err(RuntimeError { message: "get() takes 1 or 2 arguments (key, [default])".into(), span: Span::default() });
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
                                func: Rc::new(move |_args, _interp| {
                                    let keys: Vec<Value> = d_clone.borrow().keys().cloned().map(Value::Str).collect();
                                    Ok(Value::List(Rc::new(RefCell::new(keys))))
                                }),
                            });
                        }
                        if attr == "values" {
                            let d_clone = Rc::clone(&d);
                            return Ok(Value::BuiltinFunction {
                                name: "values".to_string(),
                                func: Rc::new(move |_args, _interp| {
                                    let vals: Vec<Value> = d_clone.borrow().values().cloned().collect();
                                    Ok(Value::List(Rc::new(RefCell::new(vals))))
                                }),
                            });
                        }
                        if attr == "items" {
                            let d_clone = Rc::clone(&d);
                            return Ok(Value::BuiltinFunction {
                                name: "items".to_string(),
                                func: Rc::new(move |_args, _interp| {
                                    let items: Vec<Value> = d_clone.borrow().iter().map(|(k, v)| {
                                        Value::List(Rc::new(RefCell::new(vec![Value::Str(k.clone()), v.clone()])))
                                    }).collect();
                                    Ok(Value::List(Rc::new(RefCell::new(items))))
                                }),
                            });
                        }
                        if attr == "contains" {
                            let d_clone = Rc::clone(&d);
                            return Ok(Value::BuiltinFunction {
                                name: "contains".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError { message: "contains() takes exactly 1 argument (key)".into(), span: Span::default() });
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
                        if attr == "add" {
                            let s_clone = Rc::clone(&s);
                            return Ok(Value::BuiltinFunction {
                                name: "add".to_string(),
                                func: Rc::new(move |args, _interp| {
                                    if args.len() != 1 {
                                        return Err(RuntimeError { message: "add() takes exactly 1 argument (item)".into(), span: Span::default() });
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
                                        return Err(RuntimeError { message: "remove() takes exactly 1 argument (item)".into(), span: Span::default() });
                                    }
                                    let mut vec = s_clone.borrow_mut();
                                    if let Some(pos) = vec.iter().position(|x| x == &args[0]) {
                                        vec.remove(pos);
                                    }
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
                                        return Err(RuntimeError { message: "contains() takes exactly 1 argument (item)".into(), span: Span::default() });
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
                if let Expr::Slice { ref start, ref stop, ref step, span: slice_span } = **index {
                    let start_val = if let Some(ref s) = start {
                        match self.eval_expr(s)? {
                            Value::Int(i) => Some(i),
                            _ => return Err(RuntimeError { message: "slice start must be an integer".into(), span: slice_span }),
                        }
                    } else { None };
                    let stop_val = if let Some(ref s) = stop {
                        match self.eval_expr(s)? {
                            Value::Int(i) => Some(i),
                            _ => return Err(RuntimeError { message: "slice stop must be an integer".into(), span: slice_span }),
                        }
                    } else { None };
                    let step_val = if let Some(ref s) = step {
                        match self.eval_expr(s)? {
                            Value::Int(i) => {
                                if i == 0 {
                                    return Err(RuntimeError { message: "slice step cannot be zero".into(), span: slice_span });
                                }
                                i
                            }
                            _ => return Err(RuntimeError { message: "slice step must be an integer".into(), span: slice_span }),
                        }
                    } else { 1 };

                    match obj {
                        Value::List(l) => {
                            let items = l.borrow();
                            let len = items.len() as i64;
                            let mut cur = start_val.map(|s| if s < 0 { (len + s).max(0) } else { s.min(len) }).unwrap_or(if step_val > 0 { 0 } else { len - 1 });
                            let end = stop_val.map(|s| if s < 0 { (len + s).max(0) } else { s.min(len) }).unwrap_or(if step_val > 0 { len } else { -1 });
                            let mut res = Vec::new();
                            if step_val > 0 {
                                while cur < end && cur < len {
                                    res.push(items[cur as usize].clone());
                                    cur += step_val;
                                }
                            } else {
                                while cur > end && cur >= 0 {
                                    res.push(items[cur as usize].clone());
                                    cur += step_val;
                                }
                            }
                            return Ok(Value::List(Rc::new(RefCell::new(res))));
                        }
                        Value::Str(s) => {
                            let chars: Vec<char> = s.chars().collect();
                            let len = chars.len() as i64;
                            let mut cur = start_val.map(|st| if st < 0 { (len + st).max(0) } else { st.min(len) }).unwrap_or(if step_val > 0 { 0 } else { len - 1 });
                            let end = stop_val.map(|st| if st < 0 { (len + st).max(0) } else { st.min(len) }).unwrap_or(if step_val > 0 { len } else { -1 });
                            let mut res = String::new();
                            if step_val > 0 {
                                while cur < end && cur < len {
                                    res.push(chars[cur as usize]);
                                    cur += step_val;
                                }
                            } else {
                                while cur > end && cur >= 0 {
                                    res.push(chars[cur as usize]);
                                    cur += step_val;
                                }
                            }
                            return Ok(Value::Str(res));
                        }
                        _ => return Err(RuntimeError { message: format!("slice not supported on {}", obj.type_name()), span: *span }),
                    }
                }

                let idx = self.eval_expr(index)?;
                match (obj, idx) {
                    (Value::List(list), Value::Int(i)) => {
                        let vec = list.borrow();
                        let actual_idx = if i < 0 {
                            vec.len() as i64 + i
                        } else {
                            i
                        };
                        if actual_idx < 0 || actual_idx as usize >= vec.len() {
                            return Err(RuntimeError {
                                message: format!("index {i} out of range"),
                                span: *span,
                            });
                        }
                        Ok(vec[actual_idx as usize].clone())
                    }
                    (Value::Str(s), Value::Int(i)) => {
                        let chars: Vec<char> = s.chars().collect();
                        let actual_idx = if i < 0 {
                            chars.len() as i64 + i
                        } else {
                            i
                        };
                        if actual_idx < 0 || actual_idx as usize >= chars.len() {
                            return Err(RuntimeError {
                                message: format!("index {i} out of range"),
                                span: *span,
                            });
                        }
                        Ok(Value::Str(chars[actual_idx as usize].to_string()))
                    }
                    (Value::Dict(dict), Value::Str(k)) => {
                        dict.borrow().get(&k).cloned().ok_or_else(|| RuntimeError {
                            message: format!("key '{k}' not found"),
                            span: *span,
                        })
                    }
                    (Value::Record(fields), Value::Int(i)) => {
                        let key = format!("{i}");
                        fields.borrow().get(&key).cloned().ok_or_else(|| RuntimeError {
                            message: format!("record field index {i} not found"),
                            span: *span,
                        })
                    }
                    (Value::Record(fields), Value::Str(k)) => {
                        fields.borrow().get(&k).cloned().ok_or_else(|| RuntimeError {
                            message: format!("record field '{k}' not found"),
                            span: *span,
                        })
                    }
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
            Expr::Trust { expr, .. } => {
                self.eval_expr(expr)
            }
            Expr::Skip(_) => Ok(Value::Skip),
            Expr::IfExpr { condition, then_branch, else_branch, .. } => {
                let cond_val = self.eval_expr(condition)?;
                if self.is_truthy(&cond_val) {
                    self.eval_expr(then_branch)
                } else {
                    self.eval_expr(else_branch)
                }
            }
            Expr::ListComp { element, target, iter, condition, .. } => {
                let iter_val = self.eval_expr(iter)?;
                let items: Vec<Value> = match iter_val {
                    Value::List(l) => l.borrow().clone(),
                    Value::Set(s) => s.borrow().clone(),
                    Value::Range { start, stop, step } => {
                        let mut r = Vec::new();
                        let mut cur = start;
                        if step > 0 {
                            while cur < stop { r.push(Value::Int(cur)); cur += step; }
                        } else {
                            while cur > stop { r.push(Value::Int(cur)); cur += step; }
                        }
                        r
                    }
                    _ => Vec::new(),
                };
                let mut results = Vec::new();
                for item in items {
                    self.bind_pattern(target, item, Span::default())?;
                    let keep = if let Some(cond) = condition {
                        let c = self.eval_expr(cond)?;
                        self.is_truthy(&c)
                    } else {
                        true
                    };
                    if keep {
                        let val = self.eval_expr(element)?;
                        if !matches!(val, Value::Skip) {
                            results.push(val);
                        }
                    }
                }
                Ok(Value::List(Rc::new(RefCell::new(results))))
            }
            Expr::SetComp { element, target, iter, condition, .. } => {
                let iter_val = self.eval_expr(iter)?;
                let items: Vec<Value> = match iter_val {
                    Value::List(l) => l.borrow().clone(),
                    Value::Set(s) => s.borrow().clone(),
                    Value::Range { start, stop, step } => {
                        let mut r = Vec::new();
                        let mut cur = start;
                        if step > 0 {
                            while cur < stop { r.push(Value::Int(cur)); cur += step; }
                        } else {
                            while cur > stop { r.push(Value::Int(cur)); cur += step; }
                        }
                        r
                    }
                    _ => Vec::new(),
                };
                let mut results = Vec::new();
                for item in items {
                    self.bind_pattern(target, item, Span::default())?;
                    let keep = if let Some(cond) = condition {
                        let c = self.eval_expr(cond)?;
                        self.is_truthy(&c)
                    } else {
                        true
                    };
                    if keep {
                        let val = self.eval_expr(element)?;
                        if !matches!(val, Value::Skip) && !results.contains(&val) {
                            results.push(val);
                        }
                    }
                }
                Ok(Value::Set(Rc::new(RefCell::new(results))))
            }
            Expr::DictComp { key, value, target, iter, condition, .. } => {
                let iter_val = self.eval_expr(iter)?;
                let items: Vec<Value> = match iter_val {
                    Value::List(l) => l.borrow().clone(),
                    Value::Set(s) => s.borrow().clone(),
                    Value::Range { start, stop, step } => {
                        let mut r = Vec::new();
                        let mut cur = start;
                        if step > 0 {
                            while cur < stop { r.push(Value::Int(cur)); cur += step; }
                        } else {
                            while cur > stop { r.push(Value::Int(cur)); cur += step; }
                        }
                        r
                    }
                    _ => Vec::new(),
                };
                let mut results = HashMap::new();
                for item in items {
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
                        let v = self.eval_expr(value)?;
                        results.insert(k, v);
                    }
                }
                Ok(Value::Dict(Rc::new(RefCell::new(results))))
            }
            Expr::Type(_) => Ok(Value::None),
            Expr::AnonymousDef { params, body, .. } => {
                Ok(Value::Function {
                    name: "<def>".to_string(),
                    params: params.clone(),
                    body: body.clone(),
                    closure: Rc::clone(&self.env),
                })
            }
            _ => Ok(Value::None),
        }
    }

    fn construct_class(&mut self, class_name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
        let class_def = self.classes.get(class_name).cloned().ok_or_else(|| RuntimeError {
            message: format!("class '{class_name}' not found"),
            span: Span::default(),
        })?;

        let mut fields = HashMap::new();
        let mut field_names = Vec::new();

        for member in &class_def.body {
            if let ClassMember::Field(f) = member {
                field_names.push(f.name.clone());
            }
        }

        // Inherited trait methods
        for base in &class_def.bases {
            if let TypeExpr::Named { name: trait_name, .. } = base {
                if !class_def.without_traits.contains(trait_name) {
                    if let Some(trait_body) = self.traits.get(trait_name) {
                        for member in trait_body {
                            if let TraitMember::Method(m) = member {
                                fields.insert(m.name.clone(), Value::Function {
                                    name: m.name.clone(),
                                    params: m.params.clone(),
                                    body: m.body.clone(),
                                    closure: Rc::clone(&self.env),
                                });
                            }
                        }
                    }
                }
            }
        }

        // Declared class methods
        for member in &class_def.body {
            if let ClassMember::Method(m) = member {
                fields.insert(m.name.clone(), Value::Function {
                    name: m.name.clone(),
                    params: m.params.clone(),
                    body: m.body.clone(),
                    closure: Rc::clone(&self.env),
                });
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

    fn call_function(&mut self, func: &FunctionDef, args: &[Value], closure: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
        let call_env = Rc::new(RefCell::new(Environment::with_parent(closure)));
        for (param, val) in func.params.iter().zip(args) {
            call_env.borrow_mut().set(param.name.clone(), val.clone());
        }
        let prev_env = Rc::clone(&self.env);
        self.env = call_env;
        let res = self.eval_block(&func.body);
        self.env = prev_env;
        match res {
            Ok(Value::Return(v)) => Ok(*v),
            other => other,
        }
    }

    fn bind_pattern(&mut self, pattern: &Pattern, value: Value, span: Span) -> Result<(), RuntimeError> {
        match pattern {
            Pattern::Ident(name, _) => {
                self.env.borrow_mut().set(name.clone(), value);
                Ok(())
            }
            Pattern::Tuple(elements, _) => {
                match value {
                    Value::List(items) => {
                        let items_ref = items.borrow();
                        if items_ref.len() != elements.len() {
                            return Err(RuntimeError {
                                message: format!("unpacking mismatch: expected {} items, got {}", elements.len(), items_ref.len()),
                                span,
                            });
                        }
                        for (pat, val) in elements.iter().zip(items_ref.iter()) {
                            self.bind_pattern(pat, val.clone(), span)?;
                        }
                        Ok(())
                    }
                    _ => Err(RuntimeError {
                        message: "cannot unpack non-sequence".to_string(),
                        span,
                    }),
                }
            }
            _ => Ok(()),
        }
    }

    fn matches_pattern(&self, pattern: &Pattern, value: &Value) -> bool {
        match pattern {
            Pattern::Wildcard(_) => true,
            Pattern::Ident(name, _) => match value {
                Value::Int(_) if name == "int" => true,
                Value::Float(_) if name == "float" => true,
                Value::Bool(_) if name == "bool" => true,
                Value::Str(_) if name == "str" => true,
                Value::None if name == "none" => true,
                Value::Object { class_name, .. } if class_name == name => true,
                _ => false,
            },
            Pattern::Literal(lit, _) => match (lit, value) {
                (LiteralValue::Int(a), Value::Int(b)) => a == b,
                (LiteralValue::Bool(a), Value::Bool(b)) => a == b,
                (LiteralValue::Str(a), Value::Str(b)) => a == b,
                (LiteralValue::None, Value::None) => true,
                _ => false,
            },
            Pattern::Type(te, _) => match te {
                TypeExpr::Named { name, .. } => match value {
                    Value::List(_) if name == "list" => true,
                    Value::Dict(_) if name == "dict" => true,
                    Value::Object { class_name, .. } if class_name == name => true,
                    Value::Int(_) if name == "int" => true,
                    Value::Float(_) if name == "float" => true,
                    Value::Str(_) if name == "str" => true,
                    Value::Bool(_) if name == "bool" => true,
                    _ => false,
                },
                _ => true,
            },
            _ => false,
        }
    }

    fn is_truthy(&self, val: &Value) -> bool {
        match val {
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::None => false,
            Value::List(l) => !l.borrow().is_empty(),
            Value::Set(s) => !s.borrow().is_empty(),
            Value::Dict(d) => !d.borrow().is_empty(),
            Value::Skip => false,
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(err.unwrap_err().message.contains("cannot mutate attribute 'balance' on frozen object !Account"));
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
    fn test_builtins_range_len_min_max_sum() {
        let src = r#"
r = range(5)
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

        assert_eq!(interp.env.borrow().get("stripped").unwrap(), Value::Str("hello world".into()));
        assert_eq!(interp.env.borrow().get("joined").unwrap(), Value::Str("hello-world".into()));
        assert_eq!(interp.env.borrow().get("up").unwrap(), Value::Str("HELLO-WORLD".into()));
        assert_eq!(interp.env.borrow().get("sw").unwrap(), Value::Bool(true));
        assert_eq!(interp.env.borrow().get("ew").unwrap(), Value::Bool(true));
        assert_eq!(interp.env.borrow().get("rep").unwrap(), Value::Str("HELLO-LUCID".into()));
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

        assert_eq!(interp.env.borrow().get("in_list").unwrap(), Value::Bool(true));
        assert_eq!(interp.env.borrow().get("not_in_list").unwrap(), Value::Bool(true));
        assert_eq!(interp.env.borrow().get("in_str").unwrap(), Value::Bool(true));
        assert_eq!(interp.env.borrow().get("le").unwrap(), Value::Bool(true));
        assert_eq!(interp.env.borrow().get("ge").unwrap(), Value::Bool(true));
        assert_eq!(interp.env.borrow().get("rem").unwrap(), Value::Int(2));
    }

    #[test]
    fn test_module_loading_and_math() {
        let src = r#"
import math
from math import sqrt, pi

sq = sqrt(16)
p = pi > 3.0
abs_val = math.abs(-42)
"#;
        let module = parse(src).unwrap();
        let mut interp = Interpreter::new();
        let _ = interp.eval_module(&module).unwrap();

        assert_eq!(interp.env.borrow().get("sq").unwrap(), Value::Float(4.0));
        assert_eq!(interp.env.borrow().get("p").unwrap(), Value::Bool(true));
        assert_eq!(interp.env.borrow().get("abs_val").unwrap(), Value::Int(42));
    }
}
