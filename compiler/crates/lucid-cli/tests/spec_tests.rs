use lucid_checker::TypeChecker;
use lucid_runtime::{Interpreter, Value};
use lucid_syntax::parse;

fn run_lucid(source: &str) -> (Result<(), String>, Result<Value, String>) {
    let module = match parse(source) {
        Ok(m) => m,
        Err(e) => {
            return (
                Err(format!("parse error: {e}")),
                Err(format!("parse error: {e}")),
            );
        }
    };
    let mut checker = TypeChecker::new();
    let check_res = checker
        .check_module(&module)
        .map_err(|e| format!("type error: {}", e.message));
    let mut interp = Interpreter::new();
    let eval_res = interp
        .eval_module(&module)
        .map_err(|e| format!("runtime error: {}", e.message));
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
        assert!(
            *is_frozen.borrow(),
            "freeze() must transition object to deeply frozen"
        );
    } else {
        panic!("expected Object, got {:?}", val);
    }
}

#[test]
fn test_identity_checks_accept_declaration_kind_rhs() {
    let src = r#"
class Box:
    pass

value = Box()
is_class = value is class
is_trait = value is trait
"#;
    let (chk, evl) = run_lucid(src);
    assert!(chk.is_ok(), "typecheck failed: {:?}", chk.err());
    assert!(evl.is_ok(), "evaluation failed: {:?}", evl.err());
}

#[test]
fn test_identity_checks_reject_dead_exact_class_tests() {
    for source in [
        "class HasLen:\n    def __len__(self) -> int:\n        return 1\nvalue = HasLen()\ncheck = value is Sized\n",
        "class Animal:\n    pass\nclass Dog(Animal):\n    pass\nvalue = Animal()\ncheck = value is Dog\n",
    ] {
        let (chk, _evl) = run_lucid(source);
        assert!(chk.is_err());
        assert!(chk.unwrap_err().contains("exact class"));
    }
}

#[test]
fn test_conditions_accept_bool_protocol_not_len_fallback() {
    let truthy = r#"
class Flag:
    value: bool
    factory __init__(cls, value: bool):
        return construct(value)
    def __bool__(self) -> bool:
        return self.value

if Flag(true):
    pass
"#;
    let (chk, _evl) = run_lucid(truthy);
    assert!(
        chk.is_ok(),
        "__bool__ should satisfy condition: {:?}",
        chk.err()
    );

    let len_only = "class SizedOnly:\n    def __len__(self) -> int:\n        return 1\nif SizedOnly():\n    pass\n";
    let (chk, _evl) = run_lucid(len_only);
    assert!(chk.is_err());
    assert!(chk.unwrap_err().contains("condition must be bool"));
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
cell_val = cell.value
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

#[test]
fn test_bool_does_not_satisfy_numeric_capabilities() {
    let src = r#"
trait SupportsIndex:
    def __index__(self: ~Self) -> int

def repeat(count: SupportsIndex) -> none:
    pass

repeat(true)
"#;
    let (chk, _evl) = run_lucid(src);
    assert!(chk.is_err());
    assert!(chk.unwrap_err().contains("incompatible type"));
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
    assert!(
        chk.is_ok(),
        "Type specification pillars should type check cleanly: {:?}",
        chk.err()
    );
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

#[test]
fn test_class_inheritance_rejects_programmable_type_hooks() {
    for source in [
        "class Hook:\n    def __mro_entries__(self) -> int:\n        return 1\n",
        "class Hook:\n    def __prepare__(self) -> int:\n        return 1\n",
        "class Hook:\n    def __instancecheck__(self) -> bool:\n        return true\n",
        "class Hook:\n    def __subclasscheck__(self) -> bool:\n        return true\n",
    ] {
        let (chk, _evl) = run_lucid(source);
        assert!(chk.is_err());
        assert!(chk.unwrap_err().contains("not supported"));
    }
}

#[test]
fn test_class_inheritance_rejects_keyword_class_bases() {
    let src = "class Meta:\n    pass\nclass Model(metaclass=Meta):\n    pass\n";
    let (chk, _evl) = run_lucid(src);
    assert!(chk.is_err());
    assert!(chk.unwrap_err().contains("unsupported class option"));
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

#[test]
fn test_classes_reject_dynamic_attribute_hooks() {
    for source in [
        "class Hook:\n    def __getattr__(self, name: str) -> int:\n        return 1\n",
        "class Hook:\n    def __getattribute__(self, name: str) -> int:\n        return 1\n",
        "class Hook:\n    def __setattr__(self, name: str, value: int):\n        pass\n",
        "class Hook:\n    def __del__(self):\n        pass\n",
    ] {
        let (chk, _evl) = run_lucid(source);
        assert!(chk.is_err());
        assert!(chk.unwrap_err().contains("not supported"));
    }
}

#[test]
fn test_classes_reject_descriptor_hooks() {
    for source in [
        "class Descriptor:\n    def __get__(self, obj: object, owner: object) -> int:\n        return 1\n",
        "class Descriptor:\n    def __set__(self, obj: object, value: int):\n        pass\n",
        "class Descriptor:\n    def __delete__(self, obj: object):\n        pass\n",
        "class Descriptor:\n    def __set_name__(self, owner: object, name: str):\n        pass\n",
    ] {
        let (chk, _evl) = run_lucid(source);
        assert!(chk.is_err());
        assert!(chk.unwrap_err().contains("not supported"));
    }
}

#[test]
fn test_classes_reject_removed_python_decorators() {
    for source in [
        "class Tools:\n    @staticmethod\n    def answer() -> int:\n        return 42\n",
        "class Circle:\n    @property\n    def area(self) -> int:\n        return 1\n",
        "class Factory:\n    @classmethod\n    def make(cls) -> int:\n        return 1\n",
    ] {
        let (chk, _evl) = run_lucid(source);
        assert!(chk.is_err());
        assert!(chk.unwrap_err().contains("not supported"));
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
contextmanager def managed():
    yield 10

with managed() as ctx:
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
    assert!(
        chk.is_ok(),
        "Exhaustive match on Pet union should succeed: {:?}",
        chk.err()
    );
}

#[test]
fn test_control_flow_match_expression_subject_requires_alias() {
    let src = r#"
def render(value: int) -> int:
    match value + 1:
        case _:
            return value
"#;
    let (chk, _evl) = run_lucid(src);
    assert!(chk.is_err());
    assert!(chk.unwrap_err().contains("requires `as` alias"));
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
        assert_eq!(
            *items.borrow(),
            vec![Value::Int(2), Value::Int(4), Value::Int(6)]
        );
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
        assert_eq!(
            *items.borrow(),
            vec![Value::Int(1), Value::Int(2), Value::Int(3)]
        );
    } else {
        panic!("expected list without skip elements, got {:?}", val);
    }
}

#[test]
fn test_collections_rejection_of_adjacent_string_concatenation() {
    let src = r#"path = "/api/" "users""#;
    let res = parse(src);
    assert!(
        res.is_err(),
        "Lucid must reject implicit adjacent string literal concatenation"
    );
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

#[test]
fn test_indexing_rejects_delitem_dunder() {
    let src = r#"
class Bag:
    def __delitem__(self, index: int):
        pass
"#;
    let (chk, _evl) = run_lucid(src);
    assert!(chk.is_err());
    assert!(chk.unwrap_err().contains("__delitem__ is not supported"));
}

// ---------------------------------------------------------------------------
// 15. Calls (docs/calls.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_calls_anonymous_closures() {
    let src = r#"
f: (int) -> int = def(x): x * 3
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

#[test]
fn test_calls_reject_positional_after_keyword_argument() {
    let src = r#"
def f(left: int, right: int, tail: int) -> int:
    return left + right + tail

res = f(tail=3, *[1, 2])
"#;
    let (chk, _evl) = run_lucid(src);
    assert!(chk.is_err());
    assert!(
        chk.unwrap_err()
            .contains("positional argument follows keyword argument")
    );
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

#[test]
fn test_decorators_reject_python_overload() {
    for source in [
        "@overload\ndef parse(value: str) -> int:\n    return 1\n",
        "@typing.overload\ndef parse(value: str) -> int:\n    return 1\n",
    ] {
        let (chk, _evl) = run_lucid(source);
        assert!(chk.is_err());
        assert!(chk.unwrap_err().contains("overload is not supported"));
    }
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

#[test]
fn test_modules_reject_dunder_all() {
    let src = "__all__ = [\"helper\"]\nhelper = 1\n";
    let (chk, _evl) = run_lucid(src);
    assert!(chk.is_err());
    assert!(chk.unwrap_err().contains("__all__ is not supported"));
}

#[test]
fn test_removed_type_builtin_call_is_rejected() {
    let (chk, _evl) = run_lucid("value = type(1)\n");
    assert!(chk.is_err());
    assert!(chk.unwrap_err().contains("type() is not supported"));
}

#[test]
fn test_removed_string_codepoint_builtins_are_rejected() {
    for source in ["letter = chr(65)\n", "codepoint = ord(\"A\")\n"] {
        let (chk, _evl) = run_lucid(source);
        assert!(chk.is_err());
        assert!(chk.unwrap_err().contains("is not a bare builtin"));
    }
}

// ---------------------------------------------------------------------------
// 20. Keyword reference (docs/keywords.rst)
// ---------------------------------------------------------------------------
#[test]
fn test_keywords_lexer_and_parser_support() {
    let src = r#"
let final_val = 100
final fixed = 200
f = def: fixed
"#;
    let (chk, evl) = run_lucid(src);
    assert!(chk.is_ok());
    assert!(evl.is_ok());
}

#[test]
fn test_skip_is_not_a_standalone_value() {
    for source in ["value = skip\n", "def f():\n    return skip\n"] {
        let (chk, _evl) = run_lucid(source);
        assert!(chk.is_err());
        assert!(chk.unwrap_err().contains("skip cannot be used"));
    }
}

#[test]
fn test_await_identity_for_non_future_values() {
    let src = r#"
async def load() -> int:
    return 40

direct: int = await 2
future: int = await load()
res = direct + future
"#;
    let val = eval_ok(src);
    assert_eq!(val, Value::Int(42));
}

// ---------------------------------------------------------------------------
// 21. Standard Builtins & Method Integration
// ---------------------------------------------------------------------------
#[test]
fn test_builtins_and_methods_integration() {
    let src = r#"
nums = [x * 2 for x in range(5)]
total = sum(nums)
count = len(nums)
smallest = min(nums)
biggest = max(nums)

words = "lucid is expressive and fast".split()
upper_words = [w.upper() for w in words if len(w) > 3]
slug = "-".join(upper_words)
"#;
    let val = eval_ok(src);
    assert_eq!(val, Value::Str("LUCID-EXPRESSIVE-FAST".to_string()));
}

#[test]
fn test_sum_rejects_nonnumeric_elements() {
    for source in ["total = sum([\"bad\"])\n", "total = sum([true])\n"] {
        let (chk, _evl) = run_lucid(source);
        assert!(chk.is_err());
        assert!(chk.unwrap_err().contains("elements must be numeric"));
    }
}

#[test]
fn test_strings_require_chars_for_iterable_builtins() {
    for source in [
        "letters = list(\"abc\")\n",
        "def f(x: str) -> str:\n    return x\nletters = map(f, \"abc\")\n",
        "pairs = zip(\"ab\", [1, 2])\n",
        "letters = reversed(\"abc\")\n",
    ] {
        let (chk, _evl) = run_lucid(source);
        assert!(chk.is_err());
    }

    let (chk, _evl) = run_lucid("letters = list(\"abc\".chars)\n");
    assert!(chk.is_ok(), "str.chars should be iterable: {:?}", chk.err());
}

// ---------------------------------------------------------------------------
// 22. Multi-file Module Imports
// ---------------------------------------------------------------------------
#[test]
fn test_multi_file_module_imports() {
    use std::fs;
    let temp_dir = std::env::temp_dir().join("lucid_test_imports");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let helper_path = temp_dir.join("helper.lucid");
    let main_path = temp_dir.join("main.lucid");

    fs::write(
        &helper_path,
        r#"
export def add_ten(x: int) -> int:
    return x + 10

export multiplier = 3
"#,
    )
    .unwrap();

    fs::write(
        &main_path,
        r#"
from .helper import add_ten, multiplier
import .helper as h

res1 = add_ten(5)
res2 = multiplier * 2
res3 = h.multiplier * 3
final_res = res1 + res2 + res3
"#,
    )
    .unwrap();

    let source = fs::read_to_string(&main_path).unwrap();
    let module = parse(&source).unwrap();

    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut interp = Interpreter::new();
    interp.set_current_file(Some(main_path.clone()));
    let eval_res = interp.eval_module(&module);
    assert!(eval_res.is_ok(), "Evaluation failed: {:?}", eval_res.err());

    let final_res = interp.env.borrow().get("final_res").unwrap();
    // res1 = 15, res2 = 6, res3 = 9 => 15 + 6 + 9 = 30
    assert_eq!(final_res, Value::Int(30));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_native_local_from_imports() {
    use std::fs;
    use std::process::Command;
    let temp_dir =
        std::env::temp_dir().join(format!("lucid_native_imports_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    let helper_path = temp_dir.join("helper.lucid");
    let main_path = temp_dir.join("main.lucid");
    let output_path = temp_dir.join("main_bin");
    fs::write(
        &helper_path,
        "export def add_ten(x: int) -> int:\n    return x + 10\n\nexport multiplier = 3\n",
    )
    .unwrap();
    fs::write(&main_path, "from .helper import add_ten\nimport .helper as h\nprint(add_ten(5))\nprint(h.multiplier * 2)\n").unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "build",
            main_path.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "native import build failed");
    let run = Command::new(&output_path).output().unwrap();
    assert!(
        run.status.success(),
        "native import program failed: {:?}",
        run
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "15\n6\n");
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_native_declaration_only_import_cycle() {
    use std::fs;
    use std::process::Command;
    let temp_dir =
        std::env::temp_dir().join(format!("lucid_native_decl_cycle_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    let a_path = temp_dir.join("a.lucid");
    let b_path = temp_dir.join("b.lucid");
    let output_path = temp_dir.join("a_bin");
    fs::write(&a_path, "from .b import B\nclass A:\n    pass\n").unwrap();
    fs::write(&b_path, "from .a import A\nclass B:\n    pass\n").unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "build",
            a_path.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "declaration cycle build failed");
    let run = Command::new(&output_path).output().unwrap();
    assert!(run.status.success(), "declaration cycle binary failed");
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_spec_command_exits_nonzero_when_positive_snippet_fails() {
    use std::fs;
    use std::process::Command;
    let temp_dir = std::env::temp_dir().join(format!(
        "lucid_spec_exit_failure_{}_{}",
        std::process::id(),
        "typecheck"
    ));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(
        temp_dir.join("broken.md"),
        "```python\nvalue: int = \"wrong\"\n```\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args(["test-spec", temp_dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "test-spec should fail when a positive snippet does not typecheck"
    );
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_spec_command_exits_zero_when_all_snippets_validate() {
    use std::fs;
    use std::process::Command;
    let temp_dir = std::env::temp_dir().join(format!(
        "lucid_spec_exit_success_{}_{}",
        std::process::id(),
        "typecheck"
    ));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(temp_dir.join("ok.md"), "```python\nvalue: int = 42\n```\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args(["test-spec", temp_dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "test-spec should pass when every positive snippet validates: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&temp_dir);
}
