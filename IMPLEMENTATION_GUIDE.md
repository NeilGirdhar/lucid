# Lucid Compiler - Complete Implementation Guide

## Current Status
**Completion: ~35% of full language spec**
- Phase 1: ✅ 100% (Compiler infrastructure)
- Phase 2: ✅ 95% (OOP - missing virtual dispatch)
- Phase 3: ⏳ 0% (Type system)
- Phase 4: ⏳ 0% (Error handling)
- Phase 5: ⏳ 0% (Standard library)

## Next Steps to Production-Ready

### Phase 2 Completion (5% remaining)

#### Virtual Method Dispatch Tables
**Effort: 2-3 hours | Priority: Medium**

Currently methods are dispatched as `method_ClassName(self, args)`. For inheritance to work properly:

1. Generate vtable structures:
```c
struct PointVTable {
    int64_t (*distance)(void* self);
};

struct Point_VTable {
    int64_t (*distance)(void* self);
};

struct Circle_VTable {
    int64_t (*area)(void* self);
    int64_t (*distance)(void* self);  // inherited
};
```

2. Add vtable pointer to instances:
```c
struct Point {
    struct PointVTable* __vtable;
    int64_t x;
    int64_t y;
};
```

3. Initialize vtable in NewInstance codegen
4. Translate method calls through vtable: `obj->__vtable->method(obj, args)`

**Files to modify:**
- `lucid-ir/src/codegen.rs`: Add vtable codegen in generate_class()
- `lucid-ir/src/codegen.rs`: Update NewInstance to initialize __vtable
- `lucid-ir/src/codegen.rs`: Update MethodCall to use vtable dispatch
- `lucid-ir/src/integration_test.rs`: Add test for inherited method calls

### Phase 4: Error Handling (Critical for Production)
**Effort: 4-5 hours | Priority: CRITICAL**

Lucid uses Result types (union) + ? operator for error handling.

#### Step 1: Implement ? Operator Codegen
**Files to modify: `lucid-ir/src/builder.rs` (expr_to_ir_value)**

```rust
Expr::Propagate { expr, .. } => {
    // Generate: match expr as outcome:
    //   case Error: return outcome
    //   case _: use outcome
    
    // For now, simplified: just evaluate and pass through
    let result = self.expr_to_ir_value(expr);
    
    // TODO: Generate conditional return if this is an error
    // This requires:
    // 1. Know the return type of the enclosing function
    // 2. Generate a block that checks if result is error variant
    // 3. Emit early return if error
    // 4. Otherwise continue with the value
    
    result  // Temporary: just pass through
}
```

#### Step 2: Union Type Support
**Files to modify: `lucid-checker/src/lib.rs`**

Union types (already in AST) need type checking:
- Verify ? operator only used where error type is in return signature
- Type check match expressions on unions
- Implement exhaustiveness checking

#### Step 3: Error Enum Generation
**Files to modify: `lucid-ir/src/builder.rs`**

When a function returns `T | ErrorType`, generate:
```c
// IR needs representation for union values
enum ResultTag { SUCCESS, ERROR };
struct Result {
    enum ResultTag tag;
    union {
        int64_t value;
        ErrorCode error;
    } data;
};
```

#### Step 4: Codegen Union Handling
**Files to modify: `lucid-ir/src/codegen.rs`**

Generate C code for union allocation and pattern matching.

**Test cases needed:**
```lucid
def parse(text: str) -> int | ParseError:
    value = parse_int(text)?
    return value

def safe_divide(a: int, b: int) -> float | DivideByZero:
    if b == 0:
        return DivideByZero()
    return float(a) / float(b)?

match parse(text) as result:
    case int: return result
    case ParseError: print("Failed")
```

### Phase 5: Standard Library Basics
**Effort: 6-8 hours | Priority: HIGH**

#### String Type
**Files to modify:**
- `lucid-ir/src/lib.rs`: Already has IrType::Str
- `lucid-ir/src/codegen.rs`: Already emits "const char*"
- `lucid-ir/src/builder.rs`: Add string operations

```rust
// In expr_to_ir_value, handle string literals and operations
Expr::Literal { value: LiteralValue::Str(s), .. } => {
    // Emit string literal
    IrValue::String(s.clone())
}

// String concatenation: s1 + s2
// Codegen as: sprintf(...) or strcat
```

#### List[T] Generic Type
**Files to modify:**
- `lucid-ir/src/lib.rs`: Already has IrType::List(Box<IrType>)
- `lucid-ir/src/codegen.rs`: Need to implement LucidList struct and operations

```c
// Generated C code:
struct LucidList {
    int64_t capacity;
    int64_t length;
    int64_t* elements;  // for List[int]
};

// Operations needed:
// - list[index]  -> FieldRead codegen
// - list.append(value)  -> builtin function call
// - list.length() -> FieldRead
```

#### Dictionary[K, V] Type
**Files to modify:**
- `lucid-ir/src/lib.rs`: Add Dict variant
- `lucid-ir/src/codegen.rs`: Generate Dict struct with hash table

**Basic test cases:**
```lucid
def greeting() -> str:
    return "hello" + " " + "world"

def numbers() -> list[int]:
    result = []
    for i in range(5):
        result.append(i)
    return result

def lookup() -> dict[str, int]:
    m = {}
    m["one"] = 1
    m["two"] = 2
    return m
```

### Phase 3: Type System (If Time Permits)
**Effort: 8-10 hours | Priority: MEDIUM**

1. **Generics**: Already parsed, need type checking and codegen
   - Generic function instantiation
   - Monomorphization (generate separate code for each T)

2. **Traits**: Already parsed as trait definitions
   - Trait method table generation
   - Trait bound checking
   - Default method implementations

3. **Pattern Matching**: Already parsed, need type narrowing
   - Union type pattern matching
   - Guard expressions
   - Exhaustiveness checking

## Implementation Checklist

### Phase 2 Completion
- [ ] Implement vtable structure generation
- [ ] Add __vtable pointer to classes
- [ ] Generate vtable initialization in NewInstance
- [ ] Translate MethodCall through vtable
- [ ] Test inheritance method calls
- [ ] Test method override behavior

### Phase 4 Error Handling
- [ ] Implement Propagate expression in builder
- [ ] Generate union type structures
- [ ] Implement error enum generation
- [ ] Type check ? operator usage
- [ ] Implement early return for errors
- [ ] Pattern match on Result types
- [ ] Test error propagation

### Phase 5 Standard Library
- [ ] String concatenation operations
- [ ] String methods (length, substring, etc)
- [ ] List initialization and append
- [ ] List indexing and iteration
- [ ] Dictionary initialization and lookup
- [ ] Range type and for-in loops
- [ ] Math functions (sqrt, abs, etc)

## Testing Strategy

For each phase, follow this pattern:
1. Add real Lucid source test (parse → codegen → compile → run)
2. Verify generated C code
3. Verify execution produces correct output
4. Add edge case tests

Example test structure:
```rust
#[test]
fn test_error_propagation() {
    let lucid_code = r#"
def parse(s: str) -> int | ParseError:
    return parse_int(s)?

def process(s: str) -> int | ParseError:
    value = parse(s)?
    return value + 1
    "#;
    
    let ir = compile_to_ir(lucid_code);
    let c_code = codegen(&ir);
    assert!(c_code.contains("union Result"));
    assert!(c_code.contains("if (result.tag == ERROR)"));
    
    let output = test_c_code(&c_code, ...);
    assert_eq!(output, expected);
}
```

## Performance Targets

For production-ready:
- Compilation: < 1 second for small programs
- Generated code: No runtime overhead over hand-written C
- Memory usage: < 100MB for compiler itself

## Known Limitations to Document

- No async/await (not in Lucid spec)
- No reflection (by design)
- No dynamic features (by design)
- Type system: static, no runtime type info
- Garbage collection: manual (like C/C++)

## Migration Path

Once complete:
1. Version 0.1.0: MVP with Phase 1-2 complete
2. Version 0.2.0: Add Phase 4 (error handling)
3. Version 0.3.0: Add Phase 5 (stdlib)
4. Version 1.0.0: Add Phase 3 (full type system)

## Build & Test Commands

```bash
# Build compiler
cd compiler && cargo build --release

# Run all tests
cargo test --lib

# Run integration tests
cargo test -p lucid-ir --lib

# Test end-to-end compilation
./scripts/test_compilation.sh examples/program.lucid

# Generate documentation
cargo doc --open
```

## Debugging Tips

1. **Print generated C code**: Add `eprintln!("{}", c_code)` to tests
2. **Check IR structure**: Print IrModule contents
3. **Inspect AST**: Add AST dump in parser
4. **Profile codegen**: Use `--release` and measure times
5. **Verify generated C**: Compile with -Wall -Wextra for all warnings

## References

- Lucid spec: docs/
- Implementation examples in lucid-checker (type checking)
- IR to C patterns in lucid-ir/src/codegen.rs
- Parser patterns in lucid-syntax/src/parser.rs
