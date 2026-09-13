# Gap Closure Implementation Guide

## Status: 2 of 4 gaps closed, 2 remain incomplete

### Closed Gaps ✅

1. **Attribute Path Narrowing** - COMPLETE
   - Implementation: `narrowing_constraints: HashMap<String, Type>` in TypeChecker
   - Location: lib.rs lines 1311, 5035-5046, 6182-6189
   - Tests: 9 passing (type_narrowing_with_attribute_path, etc.)

2. **Native Rejection Cases** - COMPLETE
   - Implementation: Binary operator validation at lines 9099-9221
   - Checker rejects invalid operand combinations at compile-time
   - Tests: All binary operator type checks working

### Remaining Gaps

#### 3. Iterator Protocol (60% complete → needs 40% more)

**What's Done:**
- Type system: Iterator[T] variant added and integrated (lines 45, 311, 1019, 1087)
- Checker: map/zip/enumerate/reversed return Iterator[T] (lines 11167-11233)
- Builtin contracts updated (lines 2363-2425)
- All 224 type checker tests passing

**What's Needed (40% remaining):**

a) **Interpreter Lazy Evaluation** (~1 day)
   - Location: compiler/crates/lucid-runtime/src/lib.rs
   - Current: map() returns Vec (eager evaluation)
   - Needed: Iterator wrapper that computes values on-demand
   - Implementation approach:
     ```rust
     enum Value {
         Iterator(Box<dyn Fn() -> Value>), // Lazy computation
         // ... existing variants
     }
     ```
   - Update builtin_call() for map/zip/enumerate to return Iterator variant
   - Implement lazy evaluation for loop iteration

b) **Native Backend Codegen** (~1 day)
   - Location: compiler/crates/lucid-codegen/src/lib.rs
   - Current: Generates C code for eager list construction
   - Needed: Generator-like patterns or iterator objects in C
   - Implementation approach:
     - Create iterator state struct in generated code
     - Implement next() method for each iterator type
     - Update codegen for map/zip/enumerate to emit iterator structs

#### 4. Callable/Method Type Contracts (0% complete → needs full implementation)

**What's Needed:** (~2 days)

a) **Method Value Typing** (highest priority)
   - Location: type_of_expr() for Attribute expressions
   - Current: Returns Any for dynamic method lookup
   - Needed: Return Function type with proper signature
   - Implementation:
     ```rust
     // In type_of_expr for Attribute:
     if let Attribute { value, attr } = expr {
         let obj_type = self.type_of_expr(value)?;
         // If attr is a method, return Function type with self binding
         if let Some(method_type) = self.class_method_type(&obj_type, attr) {
             // Strip self parameter for method value
             return bind_self_parameter(method_type);
         }
     }
     ```

b) **getattr/setattr/hasattr Typing**
   - Location: lines 9607-9649
   - Current: Only type-checks, doesn't return proper callable types
   - Needed: Return Function types for callable attributes
   - Enhancement:
     - When getattr(obj, "method") called, return proper Function type
     - When method is passed through closures, preserve typing
     - Support context-dependent typing based on attribute name

c) **Closure Callable Preservation**
   - Ensure callable types survive when stored in closures
   - Track function signatures through captured variables
   - Validate callable invocations match stored signatures

## Recommended Implementation Order for Next Session

1. **Complete Iterator Protocol** (2 days)
   - Start with interpreter lazy evaluation (easier)
   - Then native backend codegen (more complex)
   - Verify with expanded test suite

2. **Implement Callable/Method Type Contracts** (2 days)
   - Start with method value typing
   - Then enhance getattr/setattr/hasattr
   - Add comprehensive closure tests

3. **Final Verification**
   - Run full test suite (should be 224+ tests)
   - Verify all gaps close to 100%
   - Update documentation

## Token Budget Guidance

- Iterator Protocol completion: 2000-2500 tokens
- Callable/Method Type Contracts: 2500-3000 tokens
- Testing and verification: 1000 tokens
- **Total: ~6000 tokens needed** (allocate full session to this work)

## Files Requiring Changes

**For Iterator Protocol:**
- lucid-runtime/src/lib.rs (interpreter lazy eval)
- lucid-codegen/src/lib.rs (C backend codegen)
- lucid-checker/src/lib.rs (testing/verification)

**For Callable/Method Type Contracts:**
- lucid-checker/src/lib.rs (type_of_expr enhancement, lines 9000-9700)
- lucid-runtime/src/lib.rs (method value handling)
- Add ~20 new test cases for callable typing

## Testing Strategy

Add test cases for:
1. `it = iter([1,2,3]); x = sum(it)` - Iterator consumption
2. `fn f(x): x*2; mapped = map(f, [1,2,3]); result = list(mapped)` - Iterator consumption
3. `m = obj.method; m()` - Method value calling
4. `def apply(f): return f(1); result = apply(obj.get_field)` - Callable through closure
5. `x = getattr(obj, "method"); x()` - Dynamic method calling
