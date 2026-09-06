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
    Set(Rc<RefCell<Vec<Value>>>),
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
            Value::Skip => "skip",
            Value::Record(_) => "record",
            Value::Object { class_name, .. } => class_name.as_str(),
            Value::Function { .. } => "function",
            Value::BuiltinFunction { .. } => "builtin_function",
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
            (Value::List(a), Value::List(b)) => *a.borrow() == *b.borrow(),
            (Value::Set(a), Value::Set(b)) => *a.borrow() == *b.borrow(),
            (Value::Dict(a), Value::Dict(b)) => *a.borrow() == *b.borrow(),
            (Value::Record(a), Value::Record(b)) => *a.borrow() == *b.borrow(),
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

pub struct Interpreter {
    pub env: Rc<RefCell<Environment>>,
    pub classes: HashMap<String, ClassDef>,
    pub traits: HashMap<String, Vec<TraitMember>>,
    pub dispatch: DispatchTable,
    pub output: Vec<String>,
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
        };

        interp.register_builtins();
        interp
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
                    let param_types = func.params.iter().map(|p| {
                        p.type_annotation.as_ref().map(|t| match t {
                            TypeExpr::Named { name, .. } => name.clone(),
                            _ => "object".to_string(),
                        }).unwrap_or_else(|| "object".to_string())
                    }).collect();

                    let func_clone = func.clone();
                    let closure = Rc::clone(&self.env);
                    self.dispatch.register(f_name.clone(), param_types, Rc::new(move |args, interp| {
                        interp.call_function(&func_clone, args, Rc::clone(&closure))
                    }));

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
                    _ => return Err(RuntimeError {
                        message: "invalid assignment target".to_string(),
                        span: *span,
                    }),
                }
                Ok(val)
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
                let items = match iter_val {
                    Value::List(items) => items.borrow().clone(),
                    _ => return Err(RuntimeError {
                        message: "value is not iterable".to_string(),
                        span: *span,
                    }),
                };

                let mut broken = false;
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

                match op {
                    BinaryOp::Add => self.call_dispatch("+", &[lval, rval]),
                    BinaryOp::Sub => self.call_dispatch("-", &[lval, rval]),
                    BinaryOp::Mul => self.call_dispatch("*", &[lval, rval]),
                    BinaryOp::Div => self.call_dispatch("/", &[lval, rval]),
                    BinaryOp::Eq => self.call_dispatch("==", &[lval, rval]),
                    BinaryOp::NotEq => Ok(Value::Bool(lval != rval)),
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
                        _ => Err(RuntimeError { message: "unsupported operands for <".to_string(), span: *span }),
                    },
                    BinaryOp::Gt => match (&lval, &rval) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
                        (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
                        _ => Err(RuntimeError { message: "unsupported operands for >".to_string(), span: *span }),
                    },
                    _ => Err(RuntimeError { message: "unimplemented operator".to_string(), span: *span }),
                }
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
                        Err(RuntimeError {
                            message: format!("list has no attribute '{attr}'"),
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
                    _ => Err(RuntimeError {
                        message: "indexing not supported on this type".to_string(),
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
}
