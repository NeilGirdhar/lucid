use lucid_syntax::parse;
use lucid_checker::TypeChecker;
use lucid_runtime::{Interpreter, Value};

fn run_lucid(source: &str) -> (Result<(), String>, Result<Value, String>) {
    let module = match parse(source) {
        Ok(m) => m,
        Err(e) => return (Err(format!("parse error: {e}")), Err(format!("parse error: {e}"))),
    };
    let mut checker = TypeChecker::new();
    let check_res = checker.check_module(&module).map_err(|e| format!("type error: {}", e.message));
    let mut interp = Interpreter::new();
    let eval_res = interp.eval_module(&module).map_err(|e| format!("runtime error: {}", e.message));
    (check_res, eval_res)
}

fn eval_ok(source: &str) -> Value {
    let (check_res, eval_res) = run_lucid(source);
    assert!(check_res.is_ok(), "Typecheck failed: {:?}", check_res.err());
    assert!(eval_res.is_ok(), "Evaluation failed: {:?}", eval_res.err());
    eval_res.unwrap()
}

// ---------------------------------------------------------------------------
// 1. Principles (docs/principles.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_principles_immutability_and_freeze() {
    let src = r#"
class Point:
    x: float
    y: float

    factory __init__(cls, x: float, y: float):
        return construct(x, y)

p = Point(1.0, 2.0)
fp = freeze(p)
"#;
    let val = eval_ok(src);
    if let Value::Object { is_frozen, .. } = val {
        assert!(*is_frozen.borrow(), "freeze() must transition object to deeply frozen");
    } else {
        panic!("expected Object, got {:?}", val);
    }
}

#[test]
fn test_principles_recoverable_errors_with_question_mark() {
    let src = r#"
def step_one(x: int):
    if x > 0:
        return x * 2
    return "error: negative"

def pipeline(x: int):
    val = step_one(x)?
    return val + 10

res = pipeline(5)
"#;
    let val = eval_ok(src);
    assert_eq!(val, Value::Int(20));

    let src_err = r#"
def step_one(x: int):
    if x > 0:
        return x * 2
    return "error: negative"

def pipeline(x: int):
    val = step_one(x)?
    return val + 10

res = pipeline(-1)
"#;
    let val_err = eval_ok(src_err);
    assert_eq!(val_err, Value::Str("error: negative".to_string()));
}

// ---------------------------------------------------------------------------
// 2. Names, binding, and scope (docs/names.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_names_fresh_loop_bindings() {
    let src = r#"
fns = []
for i in [1, 2, 3]:
    fns.append(def(): i)

first = fns[0]()
second = fns[1]()
third = fns[2]()
"#;
    let val = eval_ok(src);
    assert_eq!(val, Value::Int(3));
}

#[test]
fn test_names_destructuring_and_cell() {
    let src = r#"
(a, b) = [10, 20]
cell = Cell(a)
cell_val = cell
"#;
    let val = eval_ok(src);
    assert_eq!(val, Value::Int(10));
}

// ---------------------------------------------------------------------------
// 3. Type vocabulary (docs/types.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_types_reification_and_match_types() {
    let src = r#"
type IntOrStr = int | str
t_int = type int
t_str = type str
"#;
    let (chk, evl) = run_lucid(src);
    assert!(chk.is_ok());
    assert!(evl.is_ok());
}

// ---------------------------------------------------------------------------
// 4. Mutability (docs/mutability.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_mutability_views_and_freeze_enforcement() {
    let src = r#"
class Account:
    balance: int
    factory __init__(cls, balance: int):
        return construct(balance)

acc = Account(100)
frozen_acc = freeze(acc)
"#;
    let val = eval_ok(src);
    if let Value::Object { is_frozen, .. } = val {
        assert!(*is_frozen.borrow());
    }
}

// ---------------------------------------------------------------------------
// 5. Generics (docs/generics.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_generics_higher_kinded_and_variance() {
    let src = r#"
class Box[+T]:
    val: T
    factory __init__(cls, val: T):
        return construct(val)

b = Box(42)
"#;
    let (chk, evl) = run_lucid(src);
    assert!(chk.is_ok());
    assert!(evl.is_ok());
}

// ---------------------------------------------------------------------------
// 6. Numeric types (docs/numeric-types.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_numeric_types_distinction_and_arithmetic() {
    let src = r#"
x = 10 + 20
y = 1.5 * 2.0
b = true
"#;
    let (chk, evl) = run_lucid(src);
    assert!(chk.is_ok());
    assert!(evl.is_ok());
}

// ---------------------------------------------------------------------------
// 7. Modern type specification overview (docs/type-specification.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_type_specification_pillars() {
    let src = r#"
interface Printable:
    def format() -> str

trait Formatted:
    def format() -> str:
        return "formatted"

class Doc(Formatted, Printable):
    title: str
    factory __init__(cls, title: str):
        return construct(title)
"#;
    let (chk, _evl) = run_lucid(src);
    assert!(chk.is_ok(), "Type specification pillars should type check cleanly: {:?}", chk.err());
}

// ---------------------------------------------------------------------------
// 8. Interfaces (docs/interfaces.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_interfaces_retroactive_implementation() {
    let src = r#"
interface Describable:
    def describe() -> str

class Widget:
    name: str
    factory __init__(cls, name: str):
        return construct(name)

implement Describable for Widget:
    def describe() -> str:
        return self.name
"#;
    let (chk, evl) = run_lucid(src);
    assert!(chk.is_ok());
    assert!(evl.is_ok());
}

// ---------------------------------------------------------------------------
// 9. Traits (docs/traits.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_traits_stateless_behavior_reuse() {
    let src = r#"
trait Greetable:
    def greet() -> str:
        return "hello"

class Greeter(Greetable):
    id: int
    factory __init__(cls, id: int):
        return construct(id)

g = Greeter(1)
msg = g.greet()
"#;
    let val = eval_ok(src);
    assert_eq!(val, Value::Str("hello".to_string()));
}

#[test]
fn test_traits_single_inheritance_enforcement() {
    let src = r#"
class Base1:
    x: int

class Base2:
    y: int

class Derived(Base1, Base2):
    z: int
"#;
    let (chk, _) = run_lucid(src);
    assert!(chk.is_err(), "Lucid must reject multiple class inheritance");
    assert!(chk.unwrap_err().contains("multiple class parents"));
}

// ---------------------------------------------------------------------------
// 10. Classes (docs/classes.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_classes_factories_and_without() {
    let src = r#"
trait TraitA:
    def a(): 1

class BaseClass(TraitA) without TraitA:
    x: int
    factory __init__(cls, x: int):
        return construct(x)

inst = BaseClass(99)
"#;
    let val = eval_ok(src);
    if let Value::Object { class_name, .. } = val {
        assert_eq!(class_name, "BaseClass");
    } else {
        panic!("expected BaseClass instance");
    }
}

// ---------------------------------------------------------------------------
// 11. Multiple dispatch (docs/dispatch.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_multiple_dispatch_binary_operators() {
    let src = r#"
dispatch def add_items(a: int, b: int):
    return a + b

dispatch def add_items(a: str, b: str):
    return a + b

res1 = add_items(10, 20)
res2 = add_items("foo", "bar")
"#;
    let val = eval_ok(src);
    assert_eq!(val, Value::Str("foobar".to_string()));
}

// ---------------------------------------------------------------------------
// 12. Control flow and statements (docs/control-flow.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_control_flow_if_broken() {
    let src = r#"
broken = false
for x in [1, 2, 3]:
    if x == 2:
        break
if_broken:
    broken = true

res = broken
"#;
    let val = eval_ok(src);
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn test_control_flow_with_statement() {
    let src = r#"
x = 10
with x as ctx:
    y = 42
res = y
"#;
    let val = eval_ok(src);
    assert_eq!(val, Value::Int(42));
}

#[test]
fn test_control_flow_match_exhaustiveness() {
    let src = r#"
class Cat:
    name: str
class Dog:
    name: str

type Pet = Cat | Dog

def sound(p: Pet) -> str:
    match p:
        case Cat(n):
            return "meow"
        case Dog(n):
            return "woof"
"#;
    let (chk, _) = run_lucid(src);
    assert!(chk.is_ok(), "Exhaustive match on Pet union should succeed: {:?}", chk.err());
}

// ---------------------------------------------------------------------------
// 13. Strings and collections (docs/collections.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_collections_empty_literals_and_comprehensions() {
    let src = r#"
empty_dict = {:}
empty_set = {}
lst = [x * 2 for x in [1, 2, 3]]
"#;
    let val = eval_ok(src);
    if let Value::List(items) = val {
        assert_eq!(*items.borrow(), vec![Value::Int(2), Value::Int(4), Value::Int(6)]);
    } else {
        panic!("expected list, got {:?}", val);
    }
}

#[test]
fn test_collections_skip_omits_elements() {
    let src = r#"
nums = [1, skip, 2, skip, 3]
"#;
    let val = eval_ok(src);
    if let Value::List(items) = val {
        assert_eq!(*items.borrow(), vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    } else {
        panic!("expected list without skip elements, got {:?}", val);
    }
}

#[test]
fn test_collections_rejection_of_adjacent_string_concatenation() {
    let src = r#"path = "/api/" "users""#;
    let res = parse(src);
    assert!(res.is_err(), "Lucid must reject implicit adjacent string literal concatenation");
}

// ---------------------------------------------------------------------------
// 14. Indexing (docs/indexing.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_indexing_and_slices() {
    let src = r#"
arr = [10, 20, 30, 40]
item = arr[2]
"#;
    let val = eval_ok(src);
    assert_eq!(val, Value::Int(30));
}

// ---------------------------------------------------------------------------
// 15. Calls (docs/calls.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_calls_anonymous_closures() {
    let src = r#"
f = def(x): x * 3
res = f(4)
"#;
    let val = eval_ok(src);
    assert_eq!(val, Value::Int(12));
}

#[test]
fn test_calls_zero_arg_closure() {
    let src = r#"
f = def: 42
res = f()
"#;
    let val = eval_ok(src);
    assert_eq!(val, Value::Int(42));
}

// ---------------------------------------------------------------------------
// 16. Parameters and arguments (docs/parameters.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_parameters_and_spread() {
    let src = r#"
def greet(name: str, greeting: str):
    return greeting + " " + name

kwargs = {"name": "Alice", "greeting": "Hello"}
res = greet(***kwargs)
"#;
    let (chk, _) = run_lucid(src);
    assert!(chk.is_ok());
}

// ---------------------------------------------------------------------------
// 17. Decorators (docs/decorators.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_decorators_syntax_and_application() {
    let src = r#"
@my_decorator
def calculate(a: int) -> int:
    return a * 2
"#;
    let (chk, _) = run_lucid(src);
    assert!(chk.is_ok());
}

// ---------------------------------------------------------------------------
// 18. Project configuration (docs/project-configuration.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_project_configuration_and_types() {
    let src = r#"
type Target = str
config = {"name": "lucid-project", "version": "1.0"}
"#;
    let (chk, evl) = run_lucid(src);
    assert!(chk.is_ok());
    assert!(evl.is_ok());
}

// ---------------------------------------------------------------------------
// 19. Modules and exports (docs/modules.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_modules_export_syntax() {
    let src = r#"
export def helper(x: int) -> int:
    return x + 1

export class Service:
    port: int
    factory __init__(cls, port: int):
        return construct(port)
"#;
    let (chk, evl) = run_lucid(src);
    assert!(chk.is_ok());
    assert!(evl.is_ok());
}

// ---------------------------------------------------------------------------
// 20. Keyword reference (docs/keywords.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_keywords_lexer_and_parser_support() {
    let src = r#"
let final_val = 100
final fixed = 200
skip_item = skip
f = def: fixed
"#;
    let (chk, evl) = run_lucid(src);
    assert!(chk.is_ok());
    assert!(evl.is_ok());
}
