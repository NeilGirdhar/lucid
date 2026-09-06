//! Lucid Native Code Generator
//! Compiles Lucid AST to highly-optimized native machine code via C99/GCC.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use lucid_syntax::ast::*;

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
        Expr::Literal { value: LiteralValue::None, .. } => true,
        Expr::Ident { name, .. } => name == "none" || name == "None",
        _ => false,
    }
}

pub struct CCodeGenerator {
    buffer: String,
    indent: usize,
    temp_var_id: usize,
    in_function: bool,
    current_fn_ret_type: Option<String>,
    known_classes: HashMap<String, Vec<String>>,
    known_methods: HashMap<String, String>,
    known_fns: HashMap<String, String>,
    var_types: HashMap<String, String>,
    global_vars: HashMap<String, String>,
}

impl CCodeGenerator {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            indent: 0,
            temp_var_id: 0,
            in_function: false,
            current_fn_ret_type: None,
            known_classes: HashMap::new(),
            known_methods: HashMap::new(),
            known_fns: HashMap::new(),
            var_types: HashMap::new(),
            global_vars: HashMap::new(),
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

    pub fn generate(&mut self, module: &Module) -> Result<String, CodegenError> {
        // First pass: collect class metadata and function signatures
        for stmt in &module.statements {
            if let Stmt::ClassDef { name, body, .. } = stmt {
                let mut fields = Vec::new();
                for member in body {
                    match member {
                        ClassMember::Field(f) => fields.push(f.name.clone()),
                        ClassMember::Method(m) => {
                            self.known_methods.insert(m.name.clone(), name.clone());
                        }
                        _ => {}
                    }
                }
                self.known_classes.insert(name.clone(), fields);
            } else if let Stmt::Function(f) = stmt {
                let ret_ty = self.map_type_expr(f.return_type.as_ref());
                self.known_fns.insert(f.name.clone(), ret_ty);
            }
        }

        self.emit_preamble();

        // 1. Emit class forward declarations
        for stmt in &module.statements {
            if let Stmt::ClassDef { name, .. } = stmt {
                self.emit_line(&format!("typedef struct {name} {name};"));
            }
        }
        self.emit_line("");

        // 2. Emit class struct definitions & constructors
        for stmt in &module.statements {
            if let Stmt::ClassDef { name, body, .. } = stmt {
                self.emit_class_def(name, body)?;
            }
        }

        // Collect top-level statements and global variables
        let top_level_stmts: Vec<&Stmt> = module.statements.iter().filter(|s| !matches!(s,
            Stmt::Function(_) | Stmt::ClassDef { .. } | Stmt::InterfaceDef { .. } | Stmt::TraitDef { .. }
        )).collect();

        let mut top_vars = HashMap::new();
        for stmt in &top_level_stmts {
            self.collect_vars_from_stmt(stmt, &mut top_vars);
        }
        self.global_vars = top_vars.clone();

        // Emit global variable declarations at file scope
        for (name, ty) in &top_vars {
            self.emit_line(&format!("static {ty} lucid_var_{name};"));
        }
        self.emit_line("");

        // 3. Emit function prototypes
        for stmt in &module.statements {
            if let Stmt::Function(f) = stmt {
                let ret_ty = self.map_type_expr(f.return_type.as_ref());
                let params = self.emit_param_list(&f.params);
                self.emit_line(&format!("{ret_ty} lucid_fn_{}({params});", f.name));
            }
        }
        self.emit_line("");

        // 4. Emit function bodies
        for stmt in &module.statements {
            if let Stmt::Function(f) = stmt {
                self.emit_function(f)?;
            }
        }

        // 5. Emit main() for top-level code
        self.emit_line("int main(int argc, char** argv) {");
        self.indent += 1;
        self.emit_line("(void)argc; (void)argv;");
        self.var_types.clear();

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
#include <string.h>
#include <math.h>
#include <time.h>

typedef struct LucidList LucidList;

typedef enum {
    LUCID_TYPE_NONE = 0,
    LUCID_TYPE_INT,
    LUCID_TYPE_FLOAT,
    LUCID_TYPE_BOOL,
    LUCID_TYPE_STR,
    LUCID_TYPE_LIST,
    LUCID_TYPE_PTR
} LucidValType;

typedef struct {
    int64_t i;
    double f;
    bool b;
    const char* s;
    LucidList* list;
    void* ptr;
    LucidValType type;
} LucidVal;

struct LucidList {
    LucidVal* items;
    int64_t len;
    int64_t cap;
};

static inline const char* lucid_str_index(const char* s, int64_t idx) {
    static char buf[256][2];
    static bool initialized = false;
    if (!initialized) {
        for (int i = 0; i < 256; i++) {
            buf[i][0] = (char)i;
            buf[i][1] = '\0';
        }
        initialized = true;
    }
    if (!s) return "";
    int64_t len = (int64_t)strlen(s);
    if (idx < 0) idx += len;
    if (idx < 0 || idx >= len) return "";
    return buf[(unsigned char)s[idx]];
}

static inline LucidVal lucid_none(void) {
    LucidVal v = {0}; v.type = LUCID_TYPE_NONE; return v;
}
static inline LucidVal lucid_int(int64_t i) {
    LucidVal v = {0}; v.type = LUCID_TYPE_INT; v.i = i; v.f = (double)i; v.b = (i != 0); return v;
}
static inline LucidVal lucid_float(double f) {
    LucidVal v = {0}; v.type = LUCID_TYPE_FLOAT; v.f = f; v.i = (int64_t)f; v.b = (f != 0.0); return v;
}
static inline LucidVal lucid_bool(bool b) {
    LucidVal v = {0}; v.type = LUCID_TYPE_BOOL; v.b = b; v.i = b ? 1 : 0; v.f = b ? 1.0 : 0.0; return v;
}
static inline LucidVal lucid_str(const char* s) {
    LucidVal v = {0}; v.type = LUCID_TYPE_STR; v.s = s; v.ptr = (void*)s; return v;
}
static inline LucidVal lucid_char_str(char* s) {
    return lucid_str((const char*)s);
}
static inline LucidVal lucid_list_val(LucidList* l) {
    LucidVal v = {0}; v.type = LUCID_TYPE_LIST; v.list = l; v.ptr = (void*)l; return v;
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
    LucidVal: _w_val, \
    default: lucid_ptr_val \
)(x)

static inline int64_t lucid_as_int(LucidVal v) {
    if (v.type == LUCID_TYPE_INT) return v.i;
    if (v.type == LUCID_TYPE_FLOAT) return (int64_t)v.f;
    if (v.type == LUCID_TYPE_BOOL) return v.b ? 1 : 0;
    return 0;
}
static inline double lucid_as_float(LucidVal v) {
    if (v.type == LUCID_TYPE_FLOAT) return v.f;
    if (v.type == LUCID_TYPE_INT) return (double)v.i;
    return 0.0;
}
static inline bool lucid_as_bool(LucidVal v) {
    if (v.type == LUCID_TYPE_BOOL) return v.b;
    if (v.type == LUCID_TYPE_INT) return v.i != 0;
    if (v.type == LUCID_TYPE_FLOAT) return v.f != 0.0;
    if (v.type == LUCID_TYPE_STR) return v.s && v.s[0] != '\0';
    if (v.type == LUCID_TYPE_LIST) return v.list && v.list->len > 0;
    return v.type != LUCID_TYPE_NONE;
}
static inline const char* lucid_as_str(LucidVal v) {
    if (v.type == LUCID_TYPE_STR) return v.s ? v.s : "";
    return "";
}
static inline LucidList* lucid_as_list(LucidVal v) {
    if (v.type == LUCID_TYPE_LIST) return v.list;
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
    if (a.type == LUCID_TYPE_INT || b.type == LUCID_TYPE_INT) return a.i == b.i;
    if (a.type == LUCID_TYPE_BOOL && b.type == LUCID_TYPE_BOOL) return a.b == b.b;
    return a.ptr == b.ptr;
}
static inline bool lucid_lt(LucidVal a, LucidVal b) {
    if (a.type == LUCID_TYPE_FLOAT || b.type == LUCID_TYPE_FLOAT) return a.f < b.f;
    return a.i < b.i;
}
static inline bool lucid_lte(LucidVal a, LucidVal b) {
    if (a.type == LUCID_TYPE_FLOAT || b.type == LUCID_TYPE_FLOAT) return a.f <= b.f;
    return a.i <= b.i;
}
static inline bool lucid_gt(LucidVal a, LucidVal b) {
    if (a.type == LUCID_TYPE_FLOAT || b.type == LUCID_TYPE_FLOAT) return a.f > b.f;
    return a.i > b.i;
}
static inline bool lucid_gte(LucidVal a, LucidVal b) {
    if (a.type == LUCID_TYPE_FLOAT || b.type == LUCID_TYPE_FLOAT) return a.f >= b.f;
    return a.i >= b.i;
}

static inline LucidList* lucid_list_new(int64_t cap) {
    LucidList* l = (LucidList*)malloc(sizeof(LucidList));
    l->len = 0;
    l->cap = cap < 8 ? 8 : cap;
    l->items = (LucidVal*)malloc(sizeof(LucidVal) * l->cap);
    return l;
}

static inline void lucid_list_append(LucidList* l, LucidVal v) {
    if (!l) return;
    if (l->len >= l->cap) {
        l->cap = l->cap < 8 ? 8 : l->cap * 2;
        l->items = (LucidVal*)realloc(l->items, sizeof(LucidVal) * l->cap);
    }
    l->items[l->len++] = v;
}

static inline LucidVal lucid_list_get(LucidList* l, int64_t idx) {
    if (__builtin_expect(!l, 0)) return lucid_none();
    if (__builtin_expect(idx < 0, 0)) idx += l->len;
    if (__builtin_expect(idx < 0 || idx >= l->len, 0)) return lucid_none();
    return l->items[idx];
}

static inline void lucid_list_set(LucidList* l, int64_t idx, LucidVal v) {
    if (__builtin_expect(!l, 0)) return;
    if (__builtin_expect(idx < 0, 0)) idx += l->len;
    if (__builtin_expect(idx < 0, 0)) return;
    if (__builtin_expect(idx >= l->len, 0)) {
        if (idx >= l->cap) {
            l->cap = idx + 8;
            l->items = (LucidVal*)realloc(l->items, sizeof(LucidVal) * l->cap);
        }
        for (int64_t i = l->len; i < idx; i++) {
            l->items[i] = lucid_none();
        }
        l->len = idx + 1;
    }
    l->items[idx] = v;
}

static inline LucidVal lucid_get_index(LucidVal container, int64_t idx) {
    if (container.type == LUCID_TYPE_LIST) {
        return lucid_list_get(container.list, idx);
    }
    if (container.type == LUCID_TYPE_STR) {
        return lucid_str(lucid_str_index(container.s, idx));
    }
    return lucid_none();
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

static inline LucidList* lucid_range_to_list(int64_t start, int64_t stop, int64_t step) {
    if (step <= 0) step = 1;
    int64_t count = (stop > start) ? (stop - start + step - 1) / step : 0;
    LucidList* l = lucid_list_new(count);
    for (int64_t i = start; i < stop; i += step) {
        lucid_list_append(l, lucid_int(i));
    }
    return l;
}

static inline LucidList* lucid_list_slice(LucidList* l, int64_t start, int64_t stop, int64_t step) {
    if (!l) return lucid_list_new(0);
    if (start < 0) start += l->len;
    if (stop < 0) stop += l->len;
    if (start < 0) start = 0;
    if (stop > l->len) stop = l->len;
    if (step <= 0) step = 1;
    int64_t count = (stop > start) ? (stop - start + step - 1) / step : 0;
    LucidList* res = lucid_list_new(count);
    for (int64_t i = start; i < stop; i += step) {
        lucid_list_append(res, l->items[i]);
    }
    return res;
}

static inline int64_t _len_list(LucidList* l) { return l ? l->len : 0; }
static inline int64_t _len_str(const char* s) { return s ? (int64_t)strlen(s) : 0; }
static inline int64_t _len_val(LucidVal v) {
    if (v.type == LUCID_TYPE_LIST) return v.list ? v.list->len : 0;
    if (v.type == LUCID_TYPE_STR) return v.s ? (int64_t)strlen(v.s) : 0;
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
        case LUCID_TYPE_INT: printf("%ld", v.i); break;
        case LUCID_TYPE_FLOAT:
            if (fabs(v.f - round(v.f)) < 1e-12) {
                printf("%ld", (int64_t)round(v.f));
            } else {
                printf("%.10g", v.f);
            }
            break;
        case LUCID_TYPE_BOOL: printf("%s", v.b ? "true" : "false"); break;
        case LUCID_TYPE_STR: printf("%s", v.s ? v.s : ""); break;
        case LUCID_TYPE_NONE: printf("none"); break;
        case LUCID_TYPE_LIST: printf("[list len=%ld]", v.list ? v.list->len : 0); break;
        case LUCID_TYPE_PTR: printf("[obj %p]", v.ptr); break;
    }
}

"#);
    }

    fn map_type_expr(&self, te: Option<&TypeExpr>) -> String {
        match te {
            Some(TypeExpr::Named { name, .. }) => match name.as_str() {
                "int" => "int64_t".to_string(),
                "float" => "double".to_string(),
                "bool" => "bool".to_string(),
                "str" => "const char*".to_string(),
                "none" | "None" => "void".to_string(),
                "list" => "LucidList*".to_string(),
                other => format!("{other}*"),
            },
            Some(TypeExpr::Union { types, .. }) => {
                for v in types {
                    if let TypeExpr::Named { name, .. } = v {
                        if name != "none" && name != "None" {
                            return format!("{name}*");
                        }
                    }
                }
                "LucidVal".to_string()
            }
            None => "LucidVal".to_string(),
            _ => "LucidVal".to_string(),
        }
    }

    fn emit_param_list(&self, params: &[Param]) -> String {
        if params.is_empty() {
            return "void".to_string();
        }
        let parts: Vec<String> = params.iter().map(|p| {
            let ty = self.map_type_expr(p.type_annotation.as_ref());
            format!("{ty} lucid_var_{}", p.name)
        }).collect();
        parts.join(", ")
    }

    fn infer_expr_type(&self, expr: &Expr, vars: &HashMap<String, String>) -> String {
        match expr {
            Expr::Literal { value, .. } => match value {
                LiteralValue::Int(_) => "int64_t".to_string(),
                LiteralValue::Float(_) => "double".to_string(),
                LiteralValue::Bool(_) => "bool".to_string(),
                LiteralValue::Str(_) => "const char*".to_string(),
                LiteralValue::None => "LucidVal".to_string(),
                _ => "LucidVal".to_string(),
            },
            Expr::List { .. } | Expr::ListComp { .. } => "LucidList*".to_string(),
            Expr::Call { func, .. } => {
                if let Expr::Ident { name, .. } = &**func {
                    if self.known_classes.contains_key(name) {
                        return format!("{name}*");
                    }
                    if let Some(ret) = self.known_fns.get(name) {
                        return ret.clone();
                    }
                    match name.as_str() {
                        "len" | "sum" | "int" => "int64_t".to_string(),
                        "abs" | "round" => "double".to_string(),
                        "list" => "LucidList*".to_string(),
                        _ => "LucidVal".to_string(),
                    }
                } else if let Expr::Attribute { value: obj, attr, .. } = &**func {
                    if let Expr::Ident { name: obj_name, .. } = &**obj {
                        if obj_name == "time" && attr == "time" {
                            return "double".to_string();
                        }
                    }
                    if attr == "next" {
                        return "double".to_string();
                    }
                    "LucidVal".to_string()
                } else {
                    "LucidVal".to_string()
                }
            }
            Expr::Index { .. } => "LucidVal".to_string(),
            Expr::Binary { op, left, right, .. } => {
                if *op == BinaryOp::Mul && matches!(**left, Expr::List { .. }) {
                    return "LucidList*".to_string();
                }
                let l_ty = self.infer_expr_type(left, vars);
                let r_ty = self.infer_expr_type(right, vars);
                match op {
                    BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq | BinaryOp::Is | BinaryOp::IsNot => "bool".to_string(),
                    BinaryOp::Div | BinaryOp::Pow => "double".to_string(),
                    BinaryOp::Mod | BinaryOp::FloorDiv | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr => "int64_t".to_string(),
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                        if l_ty == "double" || r_ty == "double" {
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
            Expr::Unary { op, expr, .. } => {
                match op {
                    UnaryOp::Not => "bool".to_string(),
                    _ => self.infer_expr_type(expr, vars),
                }
            }
            Expr::Ident { name, .. } => {
                if let Some(ty) = vars.get(name).or_else(|| self.var_types.get(name)).or_else(|| self.global_vars.get(name)) {
                    ty.clone()
                } else {
                    "LucidVal".to_string()
                }
            }
            _ => "LucidVal".to_string(),
        }
    }

    fn collect_vars_from_stmt(&self, stmt: &Stmt, vars: &mut HashMap<String, String>) {
        match stmt {
            Stmt::VarDef { pattern, type_annotation, value, .. } => {
                if let Pattern::Ident(name, _) = pattern {
                    let ty = if let Some(ta) = type_annotation {
                        self.map_type_expr(Some(ta))
                    } else if let Some(val) = value {
                        self.infer_expr_type(val, vars)
                    } else {
                        "LucidVal".to_string()
                    };
                    vars.insert(name.clone(), ty);
                }
            }
            Stmt::Assignment { target, value, .. } => {
                if let Expr::Ident { name, .. } = target {
                    if !vars.contains_key(name) {
                        let ty = self.infer_expr_type(value, vars);
                        vars.insert(name.clone(), ty);
                    }
                }
            }
            Stmt::For { target, iterable, body, .. } => {
                let name = match target {
                    Pattern::Ident(n, _) => n.clone(),
                    Pattern::Wildcard(_) => "_".to_string(),
                    _ => "_".to_string(),
                };
                if true {
                    let ty = if let Expr::Call { func, .. } = iterable {
                        if let Expr::Ident { name: fname, .. } = &**func {
                            if fname == "range" {
                                "int64_t".to_string()
                            } else {
                                "LucidVal".to_string()
                            }
                        } else {
                            "LucidVal".to_string()
                        }
                    } else {
                        "LucidVal".to_string()
                    };
                    vars.insert(name.clone(), ty);
                }
                for s in body {
                    self.collect_vars_from_stmt(s, vars);
                }
            }
            Stmt::If { then_branch, elif_branches, else_branch, .. } => {
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
            Stmt::While { body, .. } => {
                for s in body {
                    self.collect_vars_from_stmt(s, vars);
                }
            }
            _ => {}
        }
    }

    fn expr_is_val(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Index { .. } => true,
            Expr::Ident { name, .. } => {
                self.var_types.get(name).or_else(|| self.global_vars.get(name)).map(|ty| ty == "LucidVal").unwrap_or(false)
            }
            Expr::Call { func, .. } => {
                if let Expr::Ident { name, .. } = &**func {
                    self.known_fns.get(name).map(|ty| ty == "LucidVal").unwrap_or(false)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn emit_class_def(&mut self, name: &str, body: &[ClassMember]) -> Result<(), CodegenError> {
        self.emit_line(&format!("struct {name} {{"));
        self.indent += 1;

        let mut fields = Vec::new();
        for m in body {
            if let ClassMember::Field(f) = m {
                let ty = self.map_type_expr(Some(&f.type_annotation));
                self.emit_line(&format!("{ty} {};", f.name));
                fields.push((f.name.clone(), ty));
            }
        }
        self.indent -= 1;
        self.emit_line("};");

        // Emit constructor
        let param_list: Vec<String> = fields.iter().map(|(fname, fty)| format!("{fty} {fname}")).collect();
        self.emit_line(&format!("{name}* {name}_new({}) {{", param_list.join(", ")));
        self.indent += 1;
        self.emit_line(&format!("{name}* self = ({name}*)malloc(sizeof({name}));"));
        for (fname, _) in &fields {
            self.emit_line(&format!("self->{fname} = {fname};"));
        }
        self.emit_line("return self;");
        self.indent -= 1;
        self.emit_line("}");

        // Emit methods
        for m in body {
            if let ClassMember::Method(method) = m {
                let ret_ty = self.map_type_expr(method.return_type.as_ref());
                let mut method_params = vec![format!("{name}* self")];
                for p in method.params.iter().skip(1) {
                    let ty = self.map_type_expr(p.type_annotation.as_ref());
                    method_params.push(format!("{ty} lucid_var_{}", p.name));
                }
                self.emit_line(&format!("{ret_ty} {name}_{}({}) {{", method.name, method_params.join(", ")));
                self.indent += 1;
                for s in &method.body {
                    self.emit_stmt(s)?;
                }
                self.indent -= 1;
                self.emit_line("}");
            }
        }

        self.emit_line("");
        Ok(())
    }

    fn emit_function(&mut self, f: &FunctionDef) -> Result<(), CodegenError> {
        let ret_ty = self.map_type_expr(f.return_type.as_ref());
        let params = self.emit_param_list(&f.params);
        self.emit_line(&format!("{ret_ty} lucid_fn_{}({}) {{", f.name, params));
        self.indent += 1;
        self.in_function = true;
        self.current_fn_ret_type = Some(ret_ty);
        self.var_types.clear();

        // Register params
        for p in &f.params {
            let ty = self.map_type_expr(p.type_annotation.as_ref());
            self.var_types.insert(p.name.clone(), ty);
        }

        // Collect all local variables
        let mut local_vars = HashMap::new();
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
        }

        for s in &f.body {
            self.emit_stmt(s)?;
        }

        self.in_function = false;
        self.current_fn_ret_type = None;
        self.indent -= 1;
        self.emit_line("}\n");
        Ok(())
    }

    fn emit_stmt(&mut self, stmt: &Stmt) -> Result<(), CodegenError> {
        match stmt {
            Stmt::Import { .. } => Ok(()),
            Stmt::VarDef { pattern, value, .. } => {
                if let Pattern::Ident(name, _) = pattern {
                    if let Some(val_expr) = value {
                        let val_code = self.emit_expr(val_expr)?;
                        self.emit_line(&format!("lucid_var_{name} = {val_code};"));
                    }
                }
                Ok(())
            }
            Stmt::Assignment { target, value, .. } => {
                match target {
                    Expr::Ident { name, .. } => {
                        let val_code = self.emit_expr(value)?;
                        let var_ty = self.var_types.get(name).or_else(|| self.global_vars.get(name)).cloned().unwrap_or_else(|| "LucidVal".to_string());
                        if var_ty == "LucidVal" {
                            self.emit_line(&format!("lucid_var_{name} = lucid_wrap({val_code});"));
                        } else if var_ty == "int64_t" && self.expr_is_val(value) {
                            self.emit_line(&format!("lucid_var_{name} = lucid_int_val({val_code});"));
                        } else if var_ty == "double" && self.expr_is_val(value) {
                            self.emit_line(&format!("lucid_var_{name} = lucid_num({val_code});"));
                        } else if var_ty == "const char*" && self.expr_is_val(value) {
                            self.emit_line(&format!("lucid_var_{name} = lucid_as_str({val_code});"));
                        } else if var_ty == "LucidList*" && self.expr_is_val(value) {
                            self.emit_line(&format!("lucid_var_{name} = lucid_as_list({val_code});"));
                        } else {
                            self.emit_line(&format!("lucid_var_{name} = {val_code};"));
                        }
                    }
                    Expr::Index { value: arr_expr, index, .. } => {
                        let idx_code = self.emit_expr(index)?;
                        let val_code = self.emit_expr(value)?;
                        // Check for 2D index assignment: a[i][j] = val
                        if let Expr::Index { value: inner_arr, index: inner_idx, .. } = &**arr_expr {
                            let inner_code = self.emit_expr(inner_arr)?;
                            let inner_idx_code = self.emit_expr(inner_idx)?;
                            self.emit_line(&format!("lucid_list_set(lucid_as_list(lucid_get_index(lucid_wrap({inner_code}), lucid_int_val({inner_idx_code}))), lucid_int_val({idx_code}), lucid_wrap({val_code}));"));
                        } else {
                            let arr_code = self.emit_expr(arr_expr)?;
                            self.emit_line(&format!("lucid_list_set(lucid_as_list(lucid_wrap({arr_code})), lucid_int_val({idx_code}), lucid_wrap({val_code}));"));
                        }
                    }
                    Expr::Attribute { value: obj_expr, attr, .. } => {
                        let obj_code = self.emit_expr(obj_expr)?;
                        let val_code = self.emit_expr(value)?;
                        self.emit_line(&format!("{obj_code}->{attr} = {val_code};"));
                    }
                    _ => {}
                }
                Ok(())
            }
            Stmt::AugAssign { target, op, value, .. } => {
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
                            let var_ty = self.var_types.get(name).or_else(|| self.global_vars.get(name)).cloned().unwrap_or_else(|| "double".to_string());
                            if var_ty == "int64_t" {
                                self.emit_line(&format!("lucid_var_{name} {op_str}= lucid_int_val({val_code});"));
                            } else {
                                self.emit_line(&format!("lucid_var_{name} {op_str}= lucid_num({val_code});"));
                            }
                        } else {
                            self.emit_line(&format!("lucid_var_{name} {op_str}= {val_code};"));
                        }
                    }
                    Expr::Index { value: arr_expr, index, .. } => {
                        let idx_code = self.emit_expr(index)?;
                        let val_code = self.emit_expr(value)?;
                        if let Expr::Index { value: inner_arr, index: inner_idx, .. } = &**arr_expr {
                            let inner_code = self.emit_expr(inner_arr)?;
                            let inner_idx_code = self.emit_expr(inner_idx)?;
                            self.emit_line(&format!("{{ LucidList* _sub = lucid_as_list(lucid_get_index(lucid_wrap({inner_code}), lucid_int_val({inner_idx_code}))); int64_t _i = lucid_int_val({idx_code}); LucidVal _cur = lucid_list_get(_sub, _i); lucid_list_set(_sub, _i, lucid_float(lucid_as_float(_cur) {op_str} lucid_as_float(lucid_wrap({val_code})))); }}"));
                        } else {
                            let arr_code = self.emit_expr(arr_expr)?;
                            self.emit_line(&format!("{{ LucidList* _l = lucid_as_list(lucid_wrap({arr_code})); int64_t _i = lucid_int_val({idx_code}); LucidVal _cur = lucid_list_get(_l, _i); lucid_list_set(_l, _i, lucid_float(lucid_as_float(_cur) {op_str} lucid_as_float(lucid_wrap({val_code})))); }}"));
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            Stmt::If { condition, then_branch, elif_branches, else_branch, .. } => {
                let cond_code = self.emit_expr(condition)?;
                self.emit_line(&format!("if (lucid_bool_val({cond_code})) {{"));
                self.indent += 1;
                for s in then_branch {
                    self.emit_stmt(s)?;
                }
                self.indent -= 1;

                for (elif_cond, elif_stmts) in elif_branches {
                    let elif_code = self.emit_expr(elif_cond)?;
                    self.emit_line(&format!("}} else if (lucid_bool_val({elif_code})) {{"));
                    self.indent += 1;
                    for s in elif_stmts {
                        self.emit_stmt(s)?;
                    }
                    self.indent -= 1;
                }

                if let Some(else_stmts) = else_branch {
                    self.emit_line("} else {");
                    self.indent += 1;
                    for s in else_stmts {
                        self.emit_stmt(s)?;
                    }
                    self.indent -= 1;
                }
                self.emit_line("}");
                Ok(())
            }
            Stmt::While { condition, body, .. } => {
                let cond_code = self.emit_expr(condition)?;
                self.emit_line(&format!("while (lucid_bool_val({cond_code})) {{"));
                self.indent += 1;
                for s in body {
                    self.emit_stmt(s)?;
                }
                self.indent -= 1;
                self.emit_line("}");
                Ok(())
            }
            Stmt::For { target, iterable, body, .. } => {
                let var_name = match target {
                    Pattern::Ident(name, _) => name.clone(),
                    Pattern::Wildcard(_) => "_".to_string(),
                    _ => "_".to_string(),
                };

                // Fast path for range(...)
                if let Expr::Call { func, args, .. } = iterable {
                    if let Expr::Ident { name, .. } = &**func {
                        if name == "range" {
                            let (start, stop, step) = match args.len() {
                                1 => ("0LL".to_string(), self.emit_expr(&args[0].value)?, "1LL".to_string()),
                                2 => (self.emit_expr(&args[0].value)?, self.emit_expr(&args[1].value)?, "1LL".to_string()),
                                3 => (self.emit_expr(&args[0].value)?, self.emit_expr(&args[1].value)?, self.emit_expr(&args[2].value)?),
                                _ => ("0LL".to_string(), "0LL".to_string(), "1LL".to_string()),
                            };
                            self.emit_line(&format!("for (lucid_var_{var_name} = {start}; lucid_var_{var_name} < {stop}; lucid_var_{var_name} += {step}) {{"));
                            self.indent += 1;
                            for s in body {
                                self.emit_stmt(s)?;
                            }
                            self.indent -= 1;
                            self.emit_line("}");
                            return Ok(());
                        }
                    }
                }

                // Generic list iteration: for item in list
                let iter_code = self.emit_expr(iterable)?;
                let tmp_idx = self.new_temp();
                let tmp_list = self.new_temp();
                self.emit_line(&format!("LucidList* {tmp_list} = lucid_as_list(lucid_wrap({iter_code}));"));
                self.emit_line(&format!("for (int64_t {tmp_idx} = 0; {tmp_list} && {tmp_idx} < {tmp_list}->len; {tmp_idx}++) {{"));
                self.indent += 1;
                self.emit_line(&format!("lucid_var_{var_name} = {tmp_list}->items[{tmp_idx}];"));
                for s in body {
                    self.emit_stmt(s)?;
                }
                self.indent -= 1;
                self.emit_line("}");
                Ok(())
            }
            Stmt::Return { value, .. } => {
                if let Some(val_expr) = value {
                    let val_code = self.emit_expr(val_expr)?;
                    if let Some(ret_ty) = &self.current_fn_ret_type {
                        match ret_ty.as_str() {
                            "int64_t" => self.emit_line(&format!("return lucid_as_int(lucid_wrap({val_code}));")),
                            "double" => self.emit_line(&format!("return lucid_as_float(lucid_wrap({val_code}));")),
                            "bool" => self.emit_line(&format!("return lucid_as_bool(lucid_wrap({val_code}));")),
                            "const char*" => self.emit_line(&format!("return lucid_as_str(lucid_wrap({val_code}));")),
                            "LucidList*" => self.emit_line(&format!("return lucid_as_list(lucid_wrap({val_code}));")),
                            "void" => self.emit_line("return;"),
                            other => self.emit_line(&format!("return ({other})lucid_as_ptr(lucid_wrap({val_code}));")),
                        }
                    } else {
                        self.emit_line(&format!("return {val_code};"));
                    }
                } else {
                    self.emit_line("return;");
                }
                Ok(())
            }
            Stmt::Break(_) => {
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
            _ => Ok(()),
        }
    }

    fn emit_expr(&mut self, expr: &Expr) -> Result<String, CodegenError> {
        match expr {
            Expr::Literal { value, .. } => Ok(match value {
                LiteralValue::Int(n) => format!("{n}LL"),
                LiteralValue::Float(f) => {
                    let s = format!("{f}");
                    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
                        format!("{s}.0")
                    } else {
                        s
                    }
                }
                LiteralValue::Str(s) => format!("\"{}\"", s.replace('"', "\\\"")),
                LiteralValue::Bool(b) => if *b { "true".to_string() } else { "false".to_string() },
                LiteralValue::None => "lucid_none()".to_string(),
                _ => "lucid_none()".to_string(),
            }),
            Expr::Ident { name, .. } => {
                match name.as_str() {
                    "true" => Ok("true".to_string()),
                    "false" => Ok("false".to_string()),
                    "none" | "None" => Ok("lucid_none()".to_string()),
                    "self" => Ok("self".to_string()),
                    _ => Ok(format!("lucid_var_{name}")),
                }
            }
            Expr::Binary { op, left, right, .. } => {
                // Check for list repeat: [x] * n
                if *op == BinaryOp::Mul {
                    if let Expr::List { elements, .. } = &**left {
                        if elements.len() == 1 {
                            let elem_code = self.emit_expr(&elements[0])?;
                            let count_code = self.emit_expr(right)?;
                            return Ok(format!("lucid_list_repeat(lucid_wrap({elem_code}), lucid_int_val({count_code}))"));
                        }
                    }
                }

                let l_is_val = self.expr_is_val(left);
                let r_is_val = self.expr_is_val(right);
                let l_str = self.emit_expr(left)?;
                let r_str = self.emit_expr(right)?;

                match op {
                    BinaryOp::Add => {
                        if l_is_val || r_is_val {
                            Ok(format!("(lucid_num({l_str}) + lucid_num({r_str}))"))
                        } else {
                            Ok(format!("({l_str} + {r_str})"))
                        }
                    }
                    BinaryOp::Sub => {
                        if l_is_val || r_is_val {
                            Ok(format!("(lucid_num({l_str}) - lucid_num({r_str}))"))
                        } else {
                            Ok(format!("({l_str} - {r_str})"))
                        }
                    }
                    BinaryOp::Mul => {
                        if l_is_val || r_is_val {
                            Ok(format!("(lucid_num({l_str}) * lucid_num({r_str}))"))
                        } else {
                            Ok(format!("({l_str} * {r_str})"))
                        }
                    }
                    BinaryOp::Div => Ok(format!("((double){l_str} / (double){r_str})")),
                    BinaryOp::FloorDiv => Ok(format!("((int64_t)((int64_t){l_str} / (int64_t){r_str}))")),
                    BinaryOp::Mod => Ok(format!("((int64_t){l_str} % (int64_t){r_str})")),
                    BinaryOp::Pow => Ok(format!("pow((double){l_str}, (double){r_str})")),
                    BinaryOp::Eq => {
                        if l_is_val || r_is_val || matches!(**left, Expr::Literal { value: LiteralValue::Str(_), .. }) || matches!(**right, Expr::Literal { value: LiteralValue::Str(_), .. }) {
                            Ok(format!("lucid_eq(lucid_wrap({l_str}), lucid_wrap({r_str}))"))
                        } else {
                            Ok(format!("({l_str} == {r_str})"))
                        }
                    }
                    BinaryOp::NotEq => {
                        if l_is_val || r_is_val || matches!(**left, Expr::Literal { value: LiteralValue::Str(_), .. }) || matches!(**right, Expr::Literal { value: LiteralValue::Str(_), .. }) {
                            Ok(format!("(!lucid_eq(lucid_wrap({l_str}), lucid_wrap({r_str})))"))
                        } else {
                            Ok(format!("({l_str} != {r_str})"))
                        }
                    }
                    BinaryOp::Lt => {
                        if l_is_val || r_is_val {
                            Ok(format!("lucid_lt(lucid_wrap({l_str}), lucid_wrap({r_str}))"))
                        } else {
                            Ok(format!("({l_str} < {r_str})"))
                        }
                    }
                    BinaryOp::LtEq => {
                        if l_is_val || r_is_val {
                            Ok(format!("lucid_lte(lucid_wrap({l_str}), lucid_wrap({r_str}))"))
                        } else {
                            Ok(format!("({l_str} <= {r_str})"))
                        }
                    }
                    BinaryOp::Gt => {
                        if l_is_val || r_is_val {
                            Ok(format!("lucid_gt(lucid_wrap({l_str}), lucid_wrap({r_str}))"))
                        } else {
                            Ok(format!("({l_str} > {r_str})"))
                        }
                    }
                    BinaryOp::GtEq => {
                        if l_is_val || r_is_val {
                            Ok(format!("lucid_gte(lucid_wrap({l_str}), lucid_wrap({r_str}))"))
                        } else {
                            Ok(format!("({l_str} >= {r_str})"))
                        }
                    }
                    BinaryOp::BitAnd => Ok(format!("((int64_t){l_str} & (int64_t){r_str})")),
                    BinaryOp::BitOr => Ok(format!("((int64_t){l_str} | (int64_t){r_str})")),
                    BinaryOp::BitXor => Ok(format!("((int64_t){l_str} ^ (int64_t){r_str})")),
                    BinaryOp::Shl => Ok(format!("((int64_t){l_str} << (int64_t){r_str})")),
                    BinaryOp::Shr => Ok(format!("((int64_t){l_str} >> (int64_t){r_str})")),
                    BinaryOp::And => Ok(format!("(lucid_bool_val({l_str}) && lucid_bool_val({r_str}))")),
                    BinaryOp::Or => Ok(format!("(lucid_bool_val({l_str}) || lucid_bool_val({r_str}))")),
                    BinaryOp::Is => {
                        if is_none_expr(right) {
                            Ok(format!("lucid_is_none({l_str})"))
                        } else {
                            Ok(format!("({l_str} == {r_str})"))
                        }
                    }
                    BinaryOp::IsNot => {
                        if is_none_expr(right) {
                            Ok(format!("(!lucid_is_none({l_str}))"))
                        } else {
                            Ok(format!("({l_str} != {r_str})"))
                        }
                    }
                    _ => Ok(format!("({l_str} == {r_str})")),
                }
            }
            Expr::Unary { op, expr, .. } => {
                let e_str = self.emit_expr(expr)?;
                match op {
                    UnaryOp::Pos => Ok(format!("(+{e_str})")),
                    UnaryOp::Neg => Ok(format!("(-{e_str})")),
                    UnaryOp::Not => Ok(format!("(!lucid_bool_val({e_str}))")),
                    UnaryOp::Invert => Ok(format!("(~(int64_t){e_str})")),
                    _ => Ok(e_str),
                }
            }
            Expr::Call { func, args, .. } => {
                // Built-in functions
                if let Expr::Ident { name, .. } = &**func {
                    match name.as_str() {
                        "print" => {
                            let mut print_parts = Vec::new();
                            for (i, a) in args.iter().enumerate() {
                                let arg_str = self.emit_expr(&a.value)?;
                                if i > 0 {
                                    print_parts.push("printf(\" \");".to_string());
                                }
                                print_parts.push(format!("lucid_print_val(lucid_wrap({arg_str}));"));
                            }
                            print_parts.push("printf(\"\\n\");".to_string());
                            return Ok(format!("({{ {} lucid_none(); }})", print_parts.join(" ")));
                        }
                        "len" => {
                            if let Some(a) = args.first() {
                                let arg_str = self.emit_expr(&a.value)?;
                                return Ok(format!("lucid_len(lucid_wrap({arg_str}))"));
                            }
                        }
                        "abs" => {
                            if let Some(a) = args.first() {
                                let arg_str = self.emit_expr(&a.value)?;
                                return Ok(format!("fabs(lucid_as_float(lucid_wrap({arg_str})))"));
                            }
                        }
                        "round" => {
                            if args.len() == 1 {
                                let arg_str = self.emit_expr(&args[0].value)?;
                                return Ok(format!("round(lucid_as_float(lucid_wrap({arg_str})))"));
                            } else if args.len() >= 2 {
                                let arg0 = self.emit_expr(&args[0].value)?;
                                let arg1 = self.emit_expr(&args[1].value)?;
                                return Ok(format!("lucid_round_places(lucid_as_float(lucid_wrap({arg0})), lucid_as_int(lucid_wrap({arg1})))"));
                            }
                        }
                        "int" => {
                            if let Some(a) = args.first() {
                                let arg_str = self.emit_expr(&a.value)?;
                                return Ok(format!("lucid_as_int(lucid_wrap({arg_str}))"));
                            }
                        }
                        "float" => {
                            if let Some(a) = args.first() {
                                let arg_str = self.emit_expr(&a.value)?;
                                return Ok(format!("lucid_as_float(lucid_wrap({arg_str}))"));
                            }
                        }
                        "sum" => {
                            if let Some(a) = args.first() {
                                let arg_str = self.emit_expr(&a.value)?;
                                return Ok(format!("lucid_list_sum(lucid_as_list(lucid_wrap({arg_str})))"));
                            }
                        }
                        "list" => {
                            if let Some(a) = args.first() {
                                if let Expr::Call { func: inner_func, args: inner_args, .. } = &a.value {
                                    if let Expr::Ident { name: inner_name, .. } = &**inner_func {
                                        if inner_name == "range" {
                                            let (start, stop, step) = match inner_args.len() {
                                                1 => ("0LL".to_string(), self.emit_expr(&inner_args[0].value)?, "1LL".to_string()),
                                                2 => (self.emit_expr(&inner_args[0].value)?, self.emit_expr(&inner_args[1].value)?, "1LL".to_string()),
                                                3 => (self.emit_expr(&inner_args[0].value)?, self.emit_expr(&inner_args[1].value)?, self.emit_expr(&inner_args[2].value)?),
                                                _ => ("0LL".to_string(), "0LL".to_string(), "1LL".to_string()),
                                            };
                                            return Ok(format!("lucid_range_to_list({start}, {stop}, {step})"));
                                        }
                                    }
                                }
                                let arg_str = self.emit_expr(&a.value)?;
                                return Ok(format!("lucid_as_list(lucid_wrap({arg_str}))"));
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
                            } else {
                                c_args.push("NULL".to_string());
                            }
                        }
                        return Ok(format!("{name}_new({})", c_args.join(", ")));
                    }

                    // User function call
                    let arg_strs: Result<Vec<String>, CodegenError> = args.iter().map(|a| self.emit_expr(&a.value)).collect();
                    return Ok(format!("lucid_fn_{name}({})", arg_strs?.join(", ")));
                }

                // Method call: obj.method(...)
                if let Expr::Attribute { value: obj_expr, attr, .. } = &**func {
                    if let Expr::Ident { name: obj_name, .. } = &**obj_expr {
                        if obj_name == "time" && attr == "time" {
                            return Ok("lucid_time_now()".to_string());
                        }
                    }
                    if attr == "append" {
                        let obj_code = self.emit_expr(obj_expr)?;
                        let val_code = self.emit_expr(&args[0].value)?;
                        return Ok(format!("lucid_list_append(lucid_as_list(lucid_wrap({obj_code})), lucid_wrap({val_code}))"));
                    }
                    if let Some(cls_name) = self.known_methods.get(attr).cloned() {
                        let obj_code = self.emit_expr(obj_expr)?;
                        let mut all_args = vec![obj_code];
                        for a in args {
                            all_args.push(self.emit_expr(&a.value)?);
                        }
                        return Ok(format!("{cls_name}_{attr}({})", all_args.join(", ")));
                    }
                }

                let f_code = self.emit_expr(func)?;
                let arg_strs: Result<Vec<String>, CodegenError> = args.iter().map(|a| self.emit_expr(&a.value)).collect();
                Ok(format!("{f_code}({})", arg_strs?.join(", ")))
            }
            Expr::Index { value, index, .. } => {
                let v_code = self.emit_expr(value)?;
                if let Expr::Slice { start, stop, step, .. } = &**index {
                    let st = if let Some(s) = start { self.emit_expr(s)? } else { "0LL".to_string() };
                    let sp = if let Some(s) = stop { self.emit_expr(s)? } else { format!("lucid_len(lucid_wrap({v_code}))") };
                    let step_code = if let Some(s) = step { self.emit_expr(s)? } else { "1LL".to_string() };
                    return Ok(format!("lucid_list_slice(lucid_as_list(lucid_wrap({v_code})), {st}, {sp}, {step_code})"));
                }
                let idx_code = self.emit_expr(index)?;
                Ok(format!("lucid_get_index(lucid_wrap({v_code}), lucid_int_val({idx_code}))"))
            }
            Expr::Attribute { value, attr, .. } => {
                let v_code = self.emit_expr(value)?;
                Ok(format!("{v_code}->{attr}"))
            }
            Expr::List { elements, .. } => {
                let tmp = self.new_temp();
                let mut instrs = Vec::new();
                instrs.push(format!("LucidList* {tmp} = lucid_list_new({});", elements.len()));
                for e in elements {
                    let elem_code = self.emit_expr(e)?;
                    instrs.push(format!("lucid_list_append({tmp}, lucid_wrap({elem_code}));"));
                }
                Ok(format!("({{ {} {tmp}; }})", instrs.join(" ")))
            }
            Expr::ListComp { element, target, iter, condition, .. } => {
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
                                1 => ("0LL".to_string(), self.emit_expr(&args[0].value)?, "1LL".to_string()),
                                2 => (self.emit_expr(&args[0].value)?, self.emit_expr(&args[1].value)?, "1LL".to_string()),
                                3 => (self.emit_expr(&args[0].value)?, self.emit_expr(&args[1].value)?, self.emit_expr(&args[2].value)?),
                                _ => ("0LL".to_string(), "0LL".to_string(), "1LL".to_string()),
                            };
                            let elem_code = self.emit_expr(element)?;
                            let cond_check = if let Some(cond) = condition {
                                let c = self.emit_expr(cond)?;
                                format!("if (lucid_bool_val({c})) ")
                            } else {
                                "".to_string()
                            };
                            return Ok(format!("({{ LucidList* {tmp_list} = lucid_list_new(8); for (int64_t lucid_var_{var_name} = {start}; lucid_var_{var_name} < {stop}; lucid_var_{var_name} += {step}) {{ {cond_check}lucid_list_append({tmp_list}, lucid_wrap({elem_code})); }} {tmp_list}; }})"));
                        }
                    }
                }

                let iter_code = self.emit_expr(iter)?;
                let tmp_i = self.new_temp();
                let tmp_iter_l = self.new_temp();
                let elem_code = self.emit_expr(element)?;
                let cond_check = if let Some(cond) = condition {
                    let c = self.emit_expr(cond)?;
                    format!("if (lucid_bool_val({c})) ")
                } else {
                    "".to_string()
                };
                Ok(format!("({{ LucidList* {tmp_iter_l} = lucid_as_list(lucid_wrap({iter_code})); LucidList* {tmp_list} = lucid_list_new({tmp_iter_l} ? {tmp_iter_l}->len : 8); for (int64_t {tmp_i} = 0; {tmp_iter_l} && {tmp_i} < {tmp_iter_l}->len; {tmp_i}++) {{ LucidVal lucid_var_{var_name} = {tmp_iter_l}->items[{tmp_i}]; {cond_check}lucid_list_append({tmp_list}, lucid_wrap({elem_code})); }} {tmp_list}; }})"))
            }
            Expr::IfExpr { condition, then_branch, else_branch, .. } => {
                let cond = self.emit_expr(condition)?;
                let th = self.emit_expr(then_branch)?;
                let el = self.emit_expr(else_branch)?;
                Ok(format!("((lucid_bool_val({cond})) ? ({th}) : ({el}))"))
            }
            _ => Ok("lucid_none()".to_string()),
        }
    }
}

/// Compile Lucid AST module to a native executable binary using GCC
pub fn compile_to_native(module: &Module, output_binary: &Path, opt_level: usize) -> Result<(), CodegenError> {
    let mut generator = CCodeGenerator::new();
    let c_code = generator.generate(module)?;

    let temp_dir = std::env::temp_dir();
    let temp_c_path = temp_dir.join(format!("lucid_build_{}.c", std::process::id()));

    let mut file = fs::File::create(&temp_c_path).map_err(|e| CodegenError {
        message: format!("Failed to create temporary C file: {e}"),
    })?;
    file.write_all(c_code.as_bytes()).map_err(|e| CodegenError {
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
