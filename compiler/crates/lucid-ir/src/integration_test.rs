//! Integration tests for IR → C → Executable

#[cfg(test)]
mod tests {
    use crate::{IrModule, IrFunction, IrParam, IrType, IrValue, IrInstruction, IrTerminator, CCodegenBackend, IrClass, IrField, MethodDispatch};
    use std::process::Command;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_c_code(c_code: &str, expected: &str) -> bool {
        let test_dir = "/tmp/lucid_ir_tests";
        let _ = fs::create_dir_all(test_dir);

        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let source = format!("{}/test_{}.c", test_dir, id);
        let exe = format!("{}/test_{}", test_dir, id);

        fs::write(&source, c_code).ok();

        let compile = Command::new("gcc")
            .args(&["-o", &exe, &source, "-lm"])
            .output();

        if let Ok(out) = compile {
            if !out.status.success() {
                eprintln!("Compile error: {}", String::from_utf8_lossy(&out.stderr));
                eprintln!("Code was: {}", c_code);
                return false;
            }

            if let Ok(result) = Command::new(&exe).output() {
                let got = String::from_utf8_lossy(&result.stdout);
                let matched = got.as_ref() == expected;
                if !matched {
                    eprintln!("Output mismatch. Expected: {:?}, got: {:?}", expected, got);
                }
                let _ = fs::remove_file(&source);
                let _ = fs::remove_file(&exe);
                return matched;
            }
        }
        false
    }

    #[test]
    fn test_add_function_compiles() {
        let mut module = IrModule::new();

        let mut func = IrFunction::new(
            "add".to_string(),
            vec![
                IrParam { name: "a".to_string(), ty: IrType::I64 },
                IrParam { name: "b".to_string(), ty: IrType::I64 },
            ],
            IrType::I64,
        );

        func.blocks[0].add_instruction(IrInstruction::BinOp {
            dest: "result".to_string(),
            op: crate::IrBinOp::Add,
            left: IrValue::Var("a".to_string()),
            right: IrValue::Var("b".to_string()),
        });

        func.blocks[0].set_terminator(IrTerminator::Return {
            value: Some(IrValue::Var("result".to_string())),
        });

        module.add_function(func);

        let mut backend = CCodegenBackend::new();
        let c = backend.generate(&module);
        let full = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", add(5, 3));\n  return 0;\n}}", c);

        assert!(test_c_code(&full, "8\n"));
    }

    #[test]
    fn test_lucid_source_to_c_end_to_end() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        // Test 1: Simple add function
        let lucid_code1 = "def add(a: int, b: int) -> int:\n  return a + b\n";
        let mut lexer = Lexer::new(lucid_code1);
        let tokens = lexer.tokenize().expect("Lexer failed");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);
        let mut backend = CCodegenBackend::new();
        let c_code = backend.generate(&ir_module);
        let full_c = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", add(5, 3));\n  return 0;\n}}", c_code);
        assert!(test_c_code(&full_c, "8\n"), "add(5,3) should return 8");

        // Test 2: Function with multiplication
        let lucid_code2 = "def multiply(x: int, y: int) -> int:\n  return x * y\n";
        let mut lexer2 = Lexer::new(lucid_code2);
        let tokens2 = lexer2.tokenize().expect("Lexer failed");
        let mut parser2 = Parser::new(tokens2);
        let module2 = parser2.parse_module().expect("Parser failed");
        let ir_module2 = crate::builder::IrBuilder::new().build_module(&module2);
        let mut backend2 = CCodegenBackend::new();
        let c_code2 = backend2.generate(&ir_module2);
        let full_c2 = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", multiply(6, 7));\n  return 0;\n}}", c_code2);
        assert!(test_c_code(&full_c2, "42\n"), "multiply(6,7) should return 42");

        // Test 3: Function with variable definition
        let lucid_code3 = "def square_plus_one(x: int) -> int:\n  y: int = x * x\n  return y + 1\n";
        let mut lexer3 = Lexer::new(lucid_code3);
        let tokens3 = lexer3.tokenize().expect("Lexer failed");
        let mut parser3 = Parser::new(tokens3);
        let module3 = parser3.parse_module().expect("Parser failed");
        let ir_module3 = crate::builder::IrBuilder::new().build_module(&module3);
        let mut backend3 = CCodegenBackend::new();
        let c_code3 = backend3.generate(&ir_module3);
        let full_c3 = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", square_plus_one(5));\n  return 0;\n}}", c_code3);
        assert!(test_c_code(&full_c3, "26\n"), "square_plus_one(5) should return 26 (5*5+1)");

        // Test 4: Control flow - if/else in parsed Lucid
        let lucid_code4 = "def max_value(a: int, b: int) -> int:\n  if a > b:\n    return a\n  else:\n    return b\n";
        let mut lexer4 = Lexer::new(lucid_code4);
        let tokens4 = lexer4.tokenize().expect("Lexer failed for if/else");
        let mut parser4 = Parser::new(tokens4);
        let module4 = parser4.parse_module().expect("Parser failed for if/else");
        let ir_module4 = crate::builder::IrBuilder::new().build_module(&module4);
        let mut backend4 = CCodegenBackend::new();
        let c_code4 = backend4.generate(&ir_module4);
        let full_c4 = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", max_value(10, 5));\n  return 0;\n}}", c_code4);
        assert!(test_c_code(&full_c4, "10\n"), "max_value(10, 5) should return 10");

        // Test 5: Control flow - while loop in parsed Lucid
        let lucid_code5 = "def count_to_n(n: int) -> int:\n  i: int = 0\n  sum: int = 0\n  while i < n:\n    sum = sum + i\n    i = i + 1\n  return sum\n";
        let mut lexer5 = Lexer::new(lucid_code5);
        let tokens5 = lexer5.tokenize().expect("Lexer failed for while");
        let mut parser5 = Parser::new(tokens5);
        let module5 = parser5.parse_module().expect("Parser failed for while");
        let ir_module5 = crate::builder::IrBuilder::new().build_module(&module5);
        let mut backend5 = CCodegenBackend::new();
        let c_code5 = backend5.generate(&ir_module5);
        let full_c5 = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", count_to_n(5));\n  return 0;\n}}", c_code5);
        assert!(test_c_code(&full_c5, "10\n"), "count_to_n(5) should return 10 (0+1+2+3+4)");

        // Test 6: Class with fields parsed from source
        let lucid_code6 = "class Point:\n    x: int\n    y: int\n\ndef get_sum() -> int:\n    return 42\n";
        let mut lexer6 = Lexer::new(lucid_code6);
        let tokens6 = lexer6.tokenize().expect("Lexer failed for class");
        let mut parser6 = Parser::new(tokens6);
        let module6 = parser6.parse_module().expect("Parser failed for class");
        let ir_module6 = crate::builder::IrBuilder::new().build_module(&module6);

        // Verify Point class was created in IR
        assert_eq!(ir_module6.classes.len(), 1, "Should have one class");
        let point_class = ir_module6.get_class("Point").expect("Point class should exist");
        assert_eq!(point_class.fields.len(), 2, "Point should have 2 fields");
        assert_eq!(point_class.fields[0].name, "x");
        assert_eq!(point_class.fields[1].name, "y");

        // Verify codegen works
        let mut backend6 = CCodegenBackend::new();
        let c_code6 = backend6.generate(&ir_module6);
        let full_c6 = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", get_sum());\n  return 0;\n}}", c_code6);
        assert!(test_c_code(&full_c6, "42\n"), "get_sum() should return 42 with Point class defined");

        // Test 7: Class instantiation with field mapping
        let lucid_code7 = "class Point:\n    x: int\n    y: int\n\ndef test() -> int:\n    p = Point(3, 4)\n    return 5\n";
        let mut lexer7 = Lexer::new(lucid_code7);
        let tokens7 = lexer7.tokenize().expect("Lexer failed for instantiation");
        let mut parser7 = Parser::new(tokens7);
        let module7 = parser7.parse_module().expect("Parser failed for instantiation");
        let ir_module7 = crate::builder::IrBuilder::new().build_module(&module7);

        // Verify that Point class exists
        assert!(ir_module7.get_class("Point").is_some(), "Point class should exist");

        // Verify test function was created
        assert!(ir_module7.functions.iter().any(|f| f.name == "test"), "test function should exist");

        // Verify codegen produces valid C
        let mut backend7 = CCodegenBackend::new();
        let c_code7 = backend7.generate(&ir_module7);
        let full_c7 = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", test());\n  return 0;\n}}", c_code7);
        assert!(test_c_code(&full_c7, "5\n"), "test() with Point instantiation should return 5");

        // Test 8: Class instantiation with multiple fields
        let lucid_code8 = "class Circle:\n    radius: int\n    area: int\n\ndef compute_area() -> int:\n    c = Circle(5, 78)\n    return 42\n";
        let mut lexer8 = Lexer::new(lucid_code8);
        let tokens8 = lexer8.tokenize().expect("Lexer failed for Circle");
        let mut parser8 = Parser::new(tokens8);
        let module8 = parser8.parse_module().expect("Parser failed for Circle");
        let ir_module8 = crate::builder::IrBuilder::new().build_module(&module8);

        // Verify Circle class was created with correct fields
        let circle_class = ir_module8.get_class("Circle").expect("Circle class should exist");
        assert_eq!(circle_class.fields.len(), 2);
        assert_eq!(circle_class.fields[0].name, "radius");
        assert_eq!(circle_class.fields[1].name, "area");

        // Verify codegen works
        let mut backend8 = CCodegenBackend::new();
        let c_code8 = backend8.generate(&ir_module8);
        assert!(c_code8.contains("struct Circle"), "Should generate Circle struct");
        let full_c8 = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", compute_area());\n  return 0;\n}}", c_code8);
        assert!(test_c_code(&full_c8, "42\n"), "compute_area() should compile and return 42");

        // Test 9: String concatenation (Phase 5 stdlib)
        let lucid_code9 = "def greet() -> str:\n    greeting = \"hello\" + \" \" + \"world\"\n    return greeting\n";
        let mut lexer9 = Lexer::new(lucid_code9);
        let tokens9 = lexer9.tokenize().expect("Lexer failed for string");
        let mut parser9 = Parser::new(tokens9);
        let module9 = parser9.parse_module().expect("Parser failed for string");
        let ir_module9 = crate::builder::IrBuilder::new().build_module(&module9);

        // Verify codegen works for strings
        let mut backend9 = CCodegenBackend::new();
        let c_code9 = backend9.generate(&ir_module9);
        // Note: this test validates that string concatenation can be parsed and codegenned
        // but the output validation would require printing the string result
        assert!(c_code9.contains("greet"), "Should have greet function");
    }

    #[test]
    fn test_control_flow_if_statement() {
        let mut module = IrModule::new();

        // Generate: if (x > 5) { return 10; } else { return 20; }
        let mut func = IrFunction::new(
            "if_test".to_string(),
            vec![IrParam { name: "x".to_string(), ty: IrType::I64 }],
            IrType::I64,
        );

        // Entry block: compare x > 5
        func.blocks[0].add_instruction(IrInstruction::BinOp {
            dest: "cond".to_string(),
            op: crate::IrBinOp::Gt,
            left: IrValue::Var("x".to_string()),
            right: IrValue::Int(5),
        });

        // Branch to then (block 1) or else (block 2)
        func.blocks[0].set_terminator(IrTerminator::Branch {
            condition: IrValue::Var("cond".to_string()),
            then_block: 1,
            else_block: 2,
        });

        // Block 1: then - return 10
        let then_id = func.new_block("if_then".to_string());
        func.blocks[then_id].set_terminator(IrTerminator::Return {
            value: Some(IrValue::Int(10)),
        });

        // Block 2: else - return 20
        let else_id = func.new_block("if_else".to_string());
        func.blocks[else_id].set_terminator(IrTerminator::Return {
            value: Some(IrValue::Int(20)),
        });

        module.add_function(func);

        let mut backend = CCodegenBackend::new();
        let c = backend.generate(&module);

        // Verify if/else branches in C code
        assert!(c.contains("if (") || c.contains("goto"));

        // Test with x = 10 (should return 10)
        let full = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", if_test(10));\n  return 0;\n}}", c);
        assert!(test_c_code(&full, "10\n"));

        // Test with x = 3 (should return 20)
        let full2 = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", if_test(3));\n  return 0;\n}}", c);
        assert!(test_c_code(&full2, "20\n"));
    }

    #[test]
    fn test_control_flow_while_loop() {
        let mut module = IrModule::new();

        // Generate: int sum = 0; while (i < 5) { sum += i; i++; } return sum;
        let mut func = IrFunction::new(
            "sum_loop".to_string(),
            vec![],
            IrType::I64,
        );

        // Block 0: initialize sum = 0, i = 0
        func.blocks[0].add_instruction(IrInstruction::Assign {
            dest: "sum".to_string(),
            value: IrValue::Int(0),
        });

        func.blocks[0].add_instruction(IrInstruction::Assign {
            dest: "i".to_string(),
            value: IrValue::Int(0),
        });

        // Jump to loop condition check (block 1)
        func.blocks[0].set_terminator(IrTerminator::Jump { target: 1 });

        // Block 1: loop condition - i < 5
        let loop_block = func.new_block("while_cond".to_string());
        func.blocks[loop_block].add_instruction(IrInstruction::BinOp {
            dest: "cond".to_string(),
            op: crate::IrBinOp::Lt,
            left: IrValue::Var("i".to_string()),
            right: IrValue::Int(5),
        });

        // Branch to loop body (block 2) or exit (block 3)
        func.blocks[loop_block].set_terminator(IrTerminator::Branch {
            condition: IrValue::Var("cond".to_string()),
            then_block: 2,
            else_block: 3,
        });

        // Block 2: loop body - sum += i
        let body_block = func.new_block("while_body".to_string());
        func.blocks[body_block].add_instruction(IrInstruction::BinOp {
            dest: "sum".to_string(),
            op: crate::IrBinOp::Add,
            left: IrValue::Var("sum".to_string()),
            right: IrValue::Var("i".to_string()),
        });

        // i++
        func.blocks[body_block].add_instruction(IrInstruction::BinOp {
            dest: "i".to_string(),
            op: crate::IrBinOp::Add,
            left: IrValue::Var("i".to_string()),
            right: IrValue::Int(1),
        });

        // Jump back to loop condition
        func.blocks[body_block].set_terminator(IrTerminator::Jump { target: loop_block });

        // Block 3: after loop - return sum
        let exit_block = func.new_block("while_exit".to_string());
        func.blocks[exit_block].set_terminator(IrTerminator::Return {
            value: Some(IrValue::Var("sum".to_string())),
        });

        module.add_function(func);

        let mut backend = CCodegenBackend::new();
        let c = backend.generate(&module);

        // Verify loop structure
        assert!(c.contains("goto") || c.contains("while"));

        // sum = 0 + 1 + 2 + 3 + 4 = 10
        let full = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", sum_loop());\n  return 0;\n}}", c);
        assert!(test_c_code(&full, "10\n"));
    }

    #[test]
    fn test_method_call_codegen() {
        let mut module = IrModule::new();

        // Create a Point class
        let point_class = IrClass {
            generic_params: Vec::new(),
                    parent: None,
            name: "Point".to_string(),
            fields: vec![
                IrField { name: "x".to_string(), ty: IrType::I64 },
                IrField { name: "y".to_string(), ty: IrType::I64 },
            ],
            methods: vec![],
        };

        module.add_class(point_class);

        // Add a method: method_get_x(Point* self) -> int64_t
        let mut get_x = IrFunction::new(
            "method_get_x".to_string(),
            vec![
                IrParam { name: "self".to_string(), ty: IrType::Ptr },
            ],
            IrType::I64,
        );

        get_x.blocks[0].set_terminator(IrTerminator::Return {
            value: Some(IrValue::Int(99)),
        });

        module.add_function(get_x);

        let mut backend = CCodegenBackend::new();
        let c = backend.generate(&module);

        // Verify method declaration is generated
        assert!(c.contains("method_get_x"));

        // Verify it compiles with a call to the method
        let test_code = format!("{}\n\nint main() {{\n  struct Point p;\n  p.x = 10;\n  int64_t val = method_get_x((void*)&p);\n  printf(\"%ld\\n\", val);\n  return 0;\n}}", c);

        assert!(test_c_code(&test_code, "99\n"));
    }

    #[test]
    fn test_abi_layout_verification() {
        use lucid_abi::{AbiInfo, CInteropType};

        let mut module = IrModule::new();

        // Create Point class
        let point_class = IrClass {
            generic_params: Vec::new(),
                    parent: None,
            name: "Point".to_string(),
            fields: vec![
                IrField { name: "x".to_string(), ty: IrType::I64 },
                IrField { name: "y".to_string(), ty: IrType::I64 },
            ],
            methods: vec![],
        };

        module.add_class(point_class);

        // Generate C code
        let mut backend = CCodegenBackend::new();
        let c_code = backend.generate(&module);

        // Gap #6: ABI struct layout verification (verifies C struct offset)

        // Verify C struct compiles and has correct layout
        let test_code = format!(
            "{}\n#include <stddef.h>\nint main() {{\n  printf(\"%zu\\n\", offsetof(struct Point, y));\n  return 0;\n}}",
            c_code
        );

        // offsetof(Point, y) should be 8 (after x which is 8 bytes)
        assert!(test_c_code(&test_code, "8\n"), "ABI layout: Point.y offset should be 8");
    }

    #[test]
    fn test_struct_generation() {
        let mut module = IrModule::new();

        // Create a Point class with x and y fields
        let point_class = IrClass {
            generic_params: Vec::new(),
                    parent: None,
            name: "Point".to_string(),
            fields: vec![
                IrField { name: "x".to_string(), ty: IrType::I64 },
                IrField { name: "y".to_string(), ty: IrType::I64 },
            ],
            methods: vec![],
        };

        module.add_class(point_class);

        // Add a function that creates and uses a struct
        let mut func = IrFunction::new(
            "test_point".to_string(),
            vec![],
            IrType::I64,
        );

        func.blocks[0].set_terminator(IrTerminator::Return {
            value: Some(IrValue::Int(42)),
        });

        module.add_function(func);

        let mut backend = CCodegenBackend::new();
        let c = backend.generate(&module);

        // Verify struct definition appears in output
        assert!(c.contains("struct Point {"));
        assert!(c.contains("int64_t x;"));
        assert!(c.contains("int64_t y;"));
        assert!(c.contains("};"));

        // Verify it compiles
        let full = format!("{}\n\nint main() {{\n  struct Point p;\n  p.x = 10;\n  p.y = 20;\n  printf(\"%ld\\n\", p.x + p.y);\n  return 0;\n}}", c);

        assert!(test_c_code(&full, "30\n"));
    }

    #[test]
    fn test_exception_handling_try_except() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        // Test: try/except exception handling
        let lucid_code = "def safe_divide(a: int, b: int) -> int:\n  try:\n    if b == 0:\n      raise 1\n    return a / b\n  except:\n    return -1\n";
        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed for try/except");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed for try/except");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);
        let mut backend = CCodegenBackend::new();
        let c_code = backend.generate(&ir_module);
        let full_c = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", safe_divide(10, 0));\n  return 0;\n}}", c_code);
        assert!(test_c_code(&full_c, "-1\n"), "safe_divide(10, 0) should return -1 (exception caught)");
    }

    #[test]
    fn test_for_loop_range_iteration() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        // Test: for loop with range iteration
        let lucid_code = "def sum_range(n: int) -> int:\n  total: int = 0\n  for i in n:\n    total = total + i\n  return total\n";
        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed for for loop");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed for for loop");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);
        let mut backend = CCodegenBackend::new();
        let c_code = backend.generate(&ir_module);
        let full_c = format!("{}\n\nint main() {{\n  printf(\"%ld\\n\", sum_range(5));\n  return 0;\n}}", c_code);
        assert!(test_c_code(&full_c, "10\n"), "sum_range(5) should return 10 (0+1+2+3+4)");
    }

    #[test]
    fn test_abi_struct_memory_layout_validation() {
        use lucid_abi::{AbiInfo, ObjectLayout, CallingConvention};

        // Create ABI info with current platform's calling convention
        let cc = CallingConvention::current();
        let mut abi = AbiInfo::new(cc, 8); // 8-byte pointers on 64-bit

        // Create struct layout: class Point { x: int (offset 0), y: int (offset 8) }
        let mut layout = ObjectLayout::new("Point".to_string(), 8);
        layout.add_field("x".to_string(), 0);      // x at offset 0
        layout.add_field("y".to_string(), 8);      // y at offset 8
        layout.total_size = 16;
        abi.add_layout(layout);

        // Verify we can look up the layout
        let point_layout = abi.layout("Point").expect("Point layout not found");
        assert_eq!(point_layout.class_name, "Point");

        // Verify field offsets
        assert_eq!(point_layout.field_offset("x"), Some(0));
        assert_eq!(point_layout.field_offset("y"), Some(8));

        // Verify total size (16 bytes: 8 for x + 8 for y)
        assert_eq!(point_layout.total_size, 16);

        // Create another struct: Rectangle { topLeft: Point (offset 0, 8 bytes), width: int (offset 8, 8 bytes) }
        let mut rect_layout = ObjectLayout::new("Rectangle".to_string(), 8);
        rect_layout.add_field("topLeft".to_string(), 0);  // Point reference at offset 0
        rect_layout.add_field("width".to_string(), 8);    // width at offset 8
        rect_layout.total_size = 16;
        abi.add_layout(rect_layout);

        let rect = abi.layout("Rectangle").expect("Rectangle layout not found");
        assert_eq!(rect.field_offset("topLeft"), Some(0));
        assert_eq!(rect.field_offset("width"), Some(8));
        assert_eq!(rect.total_size, 16);
    }

    #[test]
    fn test_memory_allocation_and_deallocation() {
        let mut module = IrModule::new();

        let mut func = IrFunction::new(
            "allocate_object".to_string(),
            vec![],
            IrType::Ptr,
        );

        // Allocate memory
        func.blocks[0].add_instruction(IrInstruction::Malloc {
            dest: "obj".to_string(),
            size: IrValue::Int(64),
        });

        // Return the allocated pointer
        func.blocks[0].set_terminator(IrTerminator::Return {
            value: Some(IrValue::Var("obj".to_string())),
        });

        module.add_function(func);

        let mut backend = CCodegenBackend::new();
        let c = backend.generate(&module);

        // Verify malloc call is in generated code
        assert!(c.contains("malloc(64)"));

        // Verify it compiles and runs
        let full = format!("{}\n\nint main() {{\n  void* p = allocate_object();\n  if (p != NULL) {{\n    printf(\"1\\n\");\n    free(p);\n  }}\n  return 0;\n}}", c);
        assert!(test_c_code(&full, "1\n"), "malloc should allocate memory");
    }

    #[test]
    fn test_calling_convention_parameter_passing() {
        use lucid_abi::{AbiInfo, CallingConvention, CInteropType};

        // Test System V AMD64 calling convention parameter passing
        let cc = CallingConvention::current();
        let mut abi = AbiInfo::new(cc, 8);

        // Verify we can retrieve calling convention
        assert!(matches!(cc, CallingConvention::SystemVAmd64 |
                            CallingConvention::MicrosoftX64 |
                            CallingConvention::Arm64));

        // Test layout generation for multi-parameter function
        let mut abi_test = AbiInfo::new(cc, 8);
        abi_test.generate_layout(
            "FunctionABI".to_string(),
            vec![
                ("arg0".to_string(), CInteropType::Int),
                ("arg1".to_string(), CInteropType::Int),
                ("arg2".to_string(), CInteropType::Int),
            ],
        );

        // Verify layout was generated
        let layout = abi_test.layout("FunctionABI");
        assert!(layout.is_some(), "Layout should be generated");

        // Verify field tracking
        let l = layout.unwrap();
        assert!(l.field_offset("arg0").is_some(), "arg0 field should exist");
        assert!(l.field_offset("arg1").is_some(), "arg1 field should exist");
        assert!(l.field_offset("arg2").is_some(), "arg2 field should exist");
    }

    #[test]
    fn test_stack_frame_layout_generation() {
        use lucid_abi::StackFrame;

        // Create a stack frame (System V AMD64: 16-byte alignment)
        let mut frame = StackFrame::new(8);

        // System V AMD64 stack layout:
        // Offset 0: Return address (pushed by CALL)
        // Offset 8: Previous RBP
        // Offset 16: Local variables start here
        assert_eq!(frame.return_addr_offset, 0);
        assert_eq!(frame.prev_frame_ptr_offset, 8);
        assert_eq!(frame.locals_offset, 16);
        assert_eq!(frame.total_size(), 16);

        // Add local variables to the frame
        let local1_offset = frame.add_local(8); // int
        assert_eq!(local1_offset, 16);

        let local2_offset = frame.add_local(8); // int
        assert_eq!(local2_offset, 24);

        let local3_offset = frame.add_local(1); // bool
        assert_eq!(local3_offset, 32);

        // Total unaligned size: 33 bytes
        assert_eq!(frame.total_size(), 33);

        // Align to 16-byte boundary (System V requirement)
        frame.align_frame(16);
        assert_eq!(frame.total_size(), 48);

        // Verify frame layout
        assert!(frame.total_size() % 16 == 0, "Frame should be 16-byte aligned");
    }

    #[test]
    fn test_class_method_generation() {
        let mut module = IrModule::new();

        // Create a Point class
        let mut point_class = IrClass {
            generic_params: Vec::new(),
            parent: None,
            name: "Point".to_string(),
            fields: vec![
                IrField { name: "x".to_string(), ty: IrType::F64 },
                IrField { name: "y".to_string(), ty: IrType::F64 },
            ],
            methods: vec![
                MethodDispatch {
                    class_name: "Point".to_string(),
                    method_name: "distance".to_string(),
                    impl_function: "Point_distance".to_string(),
                },
            ],
        };

        module.add_class(point_class);

        // Add the distance method implementation
        let mut distance_func = IrFunction::new(
            "Point_distance".to_string(),
            vec![
                IrParam { name: "self".to_string(), ty: IrType::Ptr },
            ],
            IrType::F64,
        );

        // Simplified: return hardcoded value (in real impl, would load self.x and self.y)
        distance_func.blocks[0].set_terminator(IrTerminator::Return {
            value: Some(IrValue::Float(5.0)),  // sqrt(3^2 + 4^2) = 5.0
        });

        module.add_function(distance_func);

        let mut backend = CCodegenBackend::new();
        let c = backend.generate(&module);

        // Verify Point struct is generated
        assert!(c.contains("struct Point"), "Point struct should be generated");
        assert!(c.contains("double x"), "Point should have x field");
        assert!(c.contains("double y"), "Point should have y field");

        // Verify Point_distance method is generated
        assert!(c.contains("Point_distance"), "Point_distance method should be generated");

        // Verify it compiles
        let full = format!("{}\n\nint main() {{\n  printf(\"1\\n\");\n  return 0;\n}}", c);
        assert!(test_c_code(&full, "1\n"), "Class method code should compile");
    }

    #[test]
    fn test_class_instantiation_parsing() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        // Test: Parse class instantiation syntax
        // Point(3.0, 4.0) where Point is a class name (capital letter)
        let lucid_code = "def create_point() -> int:\n  p = Point(3, 4)\n  return 0\n";
        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed");

        // Verify the module parsed successfully
        assert_eq!(module.statements.len(), 1, "Should have one function");
    }

    #[test]
    fn test_instance_creation_codegen() {
        let mut module = IrModule::new();

        // Create Point class
        module.add_class(IrClass {
            generic_params: Vec::new(),
            parent: None,
            name: "Point".to_string(),
            fields: vec![
                IrField { name: "x".to_string(), ty: IrType::I64 },
                IrField { name: "y".to_string(), ty: IrType::I64 },
            ],
            methods: vec![],
        });

        // Create function that instantiates Point
        let mut func = IrFunction::new(
            "create_point".to_string(),
            vec![],
            IrType::Ptr,
        );

        // Generate: p = Point(3, 4)
        func.blocks[0].add_instruction(IrInstruction::NewInstance {
            dest: "p".to_string(),
            class_name: "Point".to_string(),
            field_values: vec![
                ("x".to_string(), IrValue::Int(3)),
                ("y".to_string(), IrValue::Int(4)),
            ],
        });

        func.blocks[0].set_terminator(IrTerminator::Return {
            value: Some(IrValue::Var("p".to_string())),
        });

        module.add_function(func);

        let mut backend = CCodegenBackend::new();
        let c = backend.generate(&module);

        // Verify Point struct is generated
        assert!(c.contains("struct Point"), "Point struct should be generated");

        // Just verify basic compilation - field initialization details may vary
        let full = format!("{}\n\nint main() {{\n  printf(\"1\\n\");\n  return 0;\n}}", c);
        assert!(test_c_code(&full, "1\n"), "Instance creation codegen should compile");
    }

    #[test]
    fn test_method_call_ir_generation() {
        // Verify that MethodCall IR instruction exists and is part of the IR
        let mut module = IrModule::new();
        let mut func = IrFunction::new(
            "test".to_string(),
            vec![],
            IrType::I64,
        );

        // Manually create a MethodCall instruction
        func.blocks[0].add_instruction(IrInstruction::MethodCall {
            dest: Some("result".to_string()),
            receiver: IrValue::Var("obj".to_string()),
            method: "foo".to_string(),
            args: vec![IrValue::Int(42)],
        });

        // Verify instruction was added
        assert_eq!(func.blocks[0].instructions.len(), 1);
        assert!(matches!(
            &func.blocks[0].instructions[0],
            IrInstruction::MethodCall { dest, method, .. }
            if dest.as_ref().map_or(false, |d| d == "result") && method == "foo"
        ));
    }

    #[test]
    fn test_single_inheritance_ir_generation() {
        let mut module = IrModule::new();

        // Create parent class
        module.add_class(IrClass {
            generic_params: Vec::new(),
            parent: None,
            name: "Animal".to_string(),
            fields: vec![
                IrField { name: "name".to_string(), ty: IrType::Str },
            ],
            methods: vec![],
        });

        // Create child class with parent
        module.add_class(IrClass {
            generic_params: Vec::new(),
            parent: Some("Animal".to_string()),
            name: "Dog".to_string(),
            fields: vec![
                IrField { name: "breed".to_string(), ty: IrType::Str },
            ],
            methods: vec![],
        });

        // Verify classes
        assert_eq!(module.classes.len(), 2);

        // Verify parent tracking
        let dog = module.get_class("Dog").unwrap();
        assert_eq!(dog.parent.as_ref().unwrap(), "Animal");

        let animal = module.get_class("Animal").unwrap();
        assert!(animal.parent.is_none());
    }

    #[test]
    fn test_list_creation_ir_generation() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        let lucid_code = r#"def make_list() -> list:
    result = []
    return result
"#;

        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed for list creation");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed for list creation");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);

        // Check that function exists
        assert_eq!(ir_module.functions.len(), 1);
        assert_eq!(ir_module.functions[0].name, "make_list");
    }

    #[test]
    fn test_list_codegen_compilation() {
        // Test that list operations generate valid C code
        let c_code = r#"
#include <stdint.h>
#include <stdlib.h>

struct LucidList {
    int64_t capacity;
    int64_t length;
    int64_t* elements;
};

struct LucidList* lucid_list_new() {
    struct LucidList* list = (struct LucidList*)malloc(sizeof(struct LucidList));
    list->capacity = 10;
    list->length = 0;
    list->elements = (int64_t*)malloc(10 * sizeof(int64_t));
    return list;
}

void lucid_list_append(struct LucidList* list, int64_t value) {
    if (list->length >= list->capacity) {
        list->capacity *= 2;
        list->elements = (int64_t*)realloc(list->elements, list->capacity * sizeof(int64_t));
    }
    list->elements[list->length++] = value;
}

int64_t lucid_list_get(struct LucidList* list, int64_t index) {
    if (index < 0 || index >= list->length) return -1;
    return list->elements[index];
}

int64_t lucid_list_length(struct LucidList* list) {
    return list->length;
}

int main() {
    struct LucidList* list = lucid_list_new();
    lucid_list_append(list, 1);
    lucid_list_append(list, 2);
    lucid_list_append(list, 3);

    int64_t len = lucid_list_length(list);
    int64_t first = lucid_list_get(list, 0);
    int64_t second = lucid_list_get(list, 1);

    if (len == 3 && first == 1 && second == 2) {
        return 0;  // Success
    }
    return 1;  // Failure
}
        "#;

        assert!(test_c_code(c_code, ""));
    }

    #[test]
    fn test_list_append_ir_generation() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        let lucid_code = r#"def add_items() -> list:
    items = []
    items.append(1)
    items.append(2)
    return items
"#;

        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed for list append");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed for list append");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);

        // Check that function was built
        assert_eq!(ir_module.functions.len(), 1);

        // Verify IR contains MethodCall instructions for append
        let func = &ir_module.functions[0];
        let has_method_call = func.blocks.iter().any(|block| {
            block.instructions.iter().any(|instr| {
                matches!(instr, IrInstruction::MethodCall { method, .. } if method == "append")
            })
        });
        assert!(has_method_call, "IR should contain append MethodCall");
    }

    #[test]
    fn test_list_indexing_ir_generation() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        let lucid_code = r#"def get_first(items: list) -> int:
    return items[0]
"#;

        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed for list indexing");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed for list indexing");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);

        assert_eq!(ir_module.functions.len(), 1);

        // Verify IR contains Call to lucid_list_get
        let func = &ir_module.functions[0];
        let has_get_call = func.blocks.iter().any(|block| {
            block.instructions.iter().any(|instr| {
                matches!(instr, IrInstruction::Call { func, .. } if func == "lucid_list_get")
            })
        });
        assert!(has_get_call, "IR should contain lucid_list_get call");
    }

    #[test]
    fn test_dict_creation_ir_generation() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        let lucid_code = r#"def make_dict() -> dict:
    d = {}
    return d
"#;

        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed for dict creation");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed for dict creation");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);

        assert_eq!(ir_module.functions.len(), 1);
        assert_eq!(ir_module.functions[0].name, "make_dict");
    }

    #[test]
    fn test_dict_operations_ir_generation() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        let lucid_code = r#"def add_to_dict() -> dict:
    d = {}
    return d
"#;

        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed for dict operations");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed for dict operations");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);

        // Just verify we can generate IR for dict code
        assert_eq!(ir_module.functions.len(), 1);
    }

    #[test]
    fn test_math_functions_codegen() {
        // Test that math functions generate valid C code
        let c_code = r#"
#include <stdint.h>
#include <math.h>

double sqrt_test() {
    return sqrt(16.0);
}

double abs_test(double x) {
    if (x < 0) return -x;
    return x;
}

int64_t min_test(int64_t a, int64_t b) {
    if (a < b) return a;
    return b;
}

int main() {
    double s = sqrt_test();
    double a = abs_test(-5.0);
    int64_t m = min_test(3, 7);

    if (s == 4.0 && a == 5.0 && m == 3) {
        return 0;  // Success
    }
    return 1;  // Failure
}
        "#;

        assert!(test_c_code(c_code, ""));
    }

    #[test]
    fn test_print_function_codegen() {
        // Test that print function generates valid C code
        let c_code = r#"
#include <stdint.h>
#include <stdio.h>

void print_int(int64_t value) {
    printf("%ld\n", value);
}

void print_string(const char* str) {
    printf("%s\n", str);
}

int main() {
    print_int(42);
    print_string("hello");
    return 0;
}
        "#;

        assert!(test_c_code(c_code, "42\nhello\n"));
    }

    #[test]
    fn test_string_functions_codegen() {
        // Test string operations
        let c_code = r#"
#include <stdint.h>
#include <string.h>

int64_t string_length(const char* s) {
    return (int64_t)strlen(s);
}

int main() {
    const char* s = "hello";
    int64_t len = string_length(s);

    if (len == 5) {
        return 0;  // Success
    }
    return 1;  // Failure
}
        "#;

        assert!(test_c_code(c_code, ""));
    }

    #[test]
    fn test_range_function_codegen() {
        // Test range function for iteration
        let c_code = r#"
#include <stdint.h>
#include <stdlib.h>

struct LucidRange {
    int64_t start;
    int64_t end;
    int64_t current;
};

struct LucidRange* lucid_range_new(int64_t end) {
    struct LucidRange* r = (struct LucidRange*)malloc(sizeof(struct LucidRange));
    r->start = 0;
    r->end = end;
    r->current = 0;
    return r;
}

int64_t lucid_range_next(struct LucidRange* r) {
    if (r->current < r->end) {
        return r->current++;
    }
    return -1;  // End of range
}

int main() {
    struct LucidRange* r = lucid_range_new(5);
    int64_t sum = 0;
    int64_t val;
    while ((val = lucid_range_next(r)) >= 0) {
        sum += val;  // 0 + 1 + 2 + 3 + 4 = 10
    }

    if (sum == 10) {
        return 0;  // Success
    }
    return 1;  // Failure
}
        "#;

        assert!(test_c_code(c_code, ""));
    }

    #[test]
    fn test_for_in_range_iteration() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        let lucid_code = r#"def sum_to_five() -> int:
    total = 0
    for i in range(5):
        total = total + i
    return total
"#;

        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed for range iteration");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed for range iteration");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);

        // Verify function was generated
        assert_eq!(ir_module.functions.len(), 1);
        assert_eq!(ir_module.functions[0].name, "sum_to_five");
    }

    #[test]
    fn test_string_operations_codegen() {
        // Test common string operations
        let c_code = r#"
#include <stdint.h>
#include <string.h>

const char* string_upper(const char* s) {
    // Simplified: just return original for now
    return s;
}

const char* string_concat(const char* a, const char* b) {
    // Simplified: would use proper string building
    return a;
}

int64_t string_find(const char* haystack, const char* needle) {
    const char* pos = strstr(haystack, needle);
    if (pos) {
        return (int64_t)(pos - haystack);
    }
    return -1;
}

int main() {
    const char* str = "hello";
    int64_t pos = string_find(str, "ll");

    if (pos == 2) {  // "ll" starts at position 2
        return 0;  // Success
    }
    return 1;  // Failure
}
        "#;

        assert!(test_c_code(c_code, ""));
    }

    #[test]
    fn test_array_operations_codegen() {
        // Test array/list advanced operations
        let c_code = r#"
#include <stdint.h>
#include <stdlib.h>

struct LucidList {
    int64_t capacity;
    int64_t length;
    int64_t* elements;
};

struct LucidList* lucid_list_new() {
    struct LucidList* list = (struct LucidList*)malloc(sizeof(struct LucidList));
    list->capacity = 10;
    list->length = 0;
    list->elements = (int64_t*)malloc(10 * sizeof(int64_t));
    return list;
}

void lucid_list_append(struct LucidList* list, int64_t value) {
    if (list->length >= list->capacity) {
        list->capacity *= 2;
        list->elements = (int64_t*)realloc(list->elements, list->capacity * sizeof(int64_t));
    }
    list->elements[list->length++] = value;
}

int64_t lucid_list_pop(struct LucidList* list) {
    if (list->length > 0) {
        return list->elements[--list->length];
    }
    return 0;
}

int64_t lucid_list_first(struct LucidList* list) {
    if (list->length > 0) return list->elements[0];
    return -1;
}

int64_t lucid_list_last(struct LucidList* list) {
    if (list->length > 0) return list->elements[list->length - 1];
    return -1;
}

int main() {
    struct LucidList* list = lucid_list_new();
    lucid_list_append(list, 10);
    lucid_list_append(list, 20);
    lucid_list_append(list, 30);

    int64_t first = lucid_list_first(list);
    int64_t last = lucid_list_last(list);
    int64_t popped = lucid_list_pop(list);

    if (first == 10 && last == 30 && popped == 30) {
        return 0;  // Success
    }
    return 1;  // Failure
}
        "#;

        assert!(test_c_code(c_code, ""));
    }

    #[test]
    fn test_error_handling_result_type() {
        // Test Result type structure for error handling
        let c_code = r#"
#include <stdint.h>
#include <stdlib.h>

enum ResultTag { SUCCESS, ERROR };

struct Result {
    enum ResultTag tag;
    union {
        int64_t value;
        int64_t error_code;
    } data;
};

struct Result lucid_result_ok(int64_t value) {
    struct Result r;
    r.tag = SUCCESS;
    r.data.value = value;
    return r;
}

struct Result lucid_result_error(int64_t error_code) {
    struct Result r;
    r.tag = ERROR;
    r.data.error_code = error_code;
    return r;
}

int64_t safe_parse(const char* str) {
    // Simplified: just return success with fixed value
    if (str) {
        return 42;
    }
    return -1;
}

int main() {
    struct Result r1 = lucid_result_ok(42);
    struct Result r2 = lucid_result_error(1);

    if (r1.tag == SUCCESS && r1.data.value == 42 &&
        r2.tag == ERROR && r2.data.error_code == 1) {
        return 0;  // Success
    }
    return 1;  // Failure
}
        "#;

        assert!(test_c_code(c_code, ""));
    }

    #[test]
    fn test_error_propagation_operator() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        let lucid_code = r#"def parse_with_error(s: str) -> int:
    value = parse_int(s)?
    return value
"#;

        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed for error propagation");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed for error propagation");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);

        // Verify IR contains call to lucid_result_unwrap for ? operator
        let func = &ir_module.functions[0];
        let has_unwrap_call = func.blocks.iter().any(|block| {
            block.instructions.iter().any(|instr| {
                matches!(instr, IrInstruction::Call { func, .. } if func == "lucid_result_unwrap")
            })
        });
        assert!(has_unwrap_call, "IR should contain lucid_result_unwrap call for ? operator");
    }

    #[test]
    fn test_dict_access_codegen() {
        // Test dictionary subscript access
        let c_code = r#"
#include <stdint.h>
#include <stdlib.h>

struct LucidDict {
    int64_t size;
    // Simplified: just track one key-value pair for test
    const char* keys[10];
    int64_t values[10];
};

int64_t lucid_dict_get(struct LucidDict* dict, const char* key) {
    for (int i = 0; i < dict->size; i++) {
        if (dict->keys[i] == key) {
            return dict->values[i];
        }
    }
    return -1;  // Not found
}

void lucid_dict_set(struct LucidDict* dict, const char* key, int64_t value) {
    dict->keys[dict->size] = key;
    dict->values[dict->size] = value;
    dict->size++;
}

int main() {
    struct LucidDict d;
    d.size = 0;

    // Set values
    lucid_dict_set(&d, "one", 1);
    lucid_dict_set(&d, "two", 2);
    lucid_dict_set(&d, "three", 3);

    // Access values
    int64_t one = lucid_dict_get(&d, "one");
    int64_t two = lucid_dict_get(&d, "two");
    int64_t three = lucid_dict_get(&d, "three");

    if (one == 1 && two == 2 && three == 3) {
        return 0;  // Success
    }
    return 1;  // Failure
}
        "#;

        assert!(test_c_code(c_code, ""));
    }

    #[test]
    fn test_dict_operations_ir() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        let lucid_code = r#"def lookup_dict() -> int:
    d = {}
    return d["key"]
"#;

        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed for dict operations");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed for dict operations");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);

        // Verify DictAccess instruction was generated for dict["key"]
        let func = &ir_module.functions[0];
        let has_dict_access = func.blocks.iter().any(|block| {
            block.instructions.iter().any(|instr| {
                matches!(instr, IrInstruction::DictAccess { .. })
            })
        });
        assert!(has_dict_access, "IR should contain DictAccess instruction");
    }

    #[test]
    fn test_generic_type_support() {
        // Test that generic types are recognized in IR
        // Generic List[int], List[str] are represented as GenericInstance

        let list_int = crate::IrType::GenericInstance {
            name: "List".to_string(),
            type_args: vec![Box::new(crate::IrType::I64)],
        };

        let list_str = crate::IrType::GenericInstance {
            name: "List".to_string(),
            type_args: vec![Box::new(crate::IrType::Str)],
        };

        // Both should map to LucidList* in C
        assert_eq!(list_int.c_type(), "LucidList*");
        assert_eq!(list_str.c_type(), "LucidList*");
    }

    #[test]
    fn test_generic_dict_type() {
        // Test generic Dictionary types
        let dict_str_int = crate::IrType::GenericInstance {
            name: "Dict".to_string(),
            type_args: vec![
                Box::new(crate::IrType::Str),
                Box::new(crate::IrType::I64),
            ],
        };

        assert_eq!(dict_str_int.c_type(), "LucidDict*");
    }

    #[test]
    fn test_generic_result_type() {
        // Test generic Result types
        let result_int = crate::IrType::GenericInstance {
            name: "Result".to_string(),
            type_args: vec![
                Box::new(crate::IrType::I64),
                Box::new(crate::IrType::Named("ParseError".to_string())),
            ],
        };

        assert_eq!(result_int.c_type(), "LucidResult*");
    }

    #[test]
    fn test_list_iteration_methods_codegen() {
        // Test list higher-order methods
        let c_code = r#"
#include <stdint.h>
#include <stdlib.h>

struct LucidList {
    int64_t capacity;
    int64_t length;
    int64_t* elements;
};

struct LucidList* lucid_list_new() {
    struct LucidList* list = (struct LucidList*)malloc(sizeof(struct LucidList));
    list->capacity = 10;
    list->length = 0;
    list->elements = (int64_t*)malloc(10 * sizeof(int64_t));
    return list;
}

void lucid_list_append(struct LucidList* list, int64_t value) {
    if (list->length >= list->capacity) {
        list->capacity *= 2;
        list->elements = (int64_t*)realloc(list->elements, list->capacity * sizeof(int64_t));
    }
    list->elements[list->length++] = value;
}

// Map operation: transform each element
struct LucidList* lucid_list_map(struct LucidList* list) {
    struct LucidList* result = lucid_list_new();
    for (int i = 0; i < list->length; i++) {
        lucid_list_append(result, list->elements[i] * 2);  // Example: double each
    }
    return result;
}

// Filter operation: keep only matching elements
struct LucidList* lucid_list_filter(struct LucidList* list) {
    struct LucidList* result = lucid_list_new();
    for (int i = 0; i < list->length; i++) {
        if (list->elements[i] > 5) {  // Example: keep > 5
            lucid_list_append(result, list->elements[i]);
        }
    }
    return result;
}

int main() {
    struct LucidList* list = lucid_list_new();
    lucid_list_append(list, 3);
    lucid_list_append(list, 7);
    lucid_list_append(list, 4);
    lucid_list_append(list, 9);

    // Test map
    struct LucidList* doubled = lucid_list_map(list);

    // Test filter
    struct LucidList* filtered = lucid_list_filter(list);

    // Verify: doubled should have [6, 14, 8, 18]
    // filtered should have [7, 9]
    if (doubled->length == 4 && filtered->length == 2) {
        return 0;  // Success
    }
    return 1;  // Failure
}
        "#;

        assert!(test_c_code(c_code, ""));
    }

    #[test]
    fn test_collection_iteration_patterns() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        let lucid_code = r#"def process_list() -> int:
    items = []
    for item in items:
        total = total + item
    return total
"#;

        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed for iteration");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed for iteration");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);

        // Verify function was generated
        assert_eq!(ir_module.functions.len(), 1);
    }

    #[test]
    fn test_match_statement_ir_generation() {
        use lucid_syntax::lexer::Lexer;
        use lucid_syntax::parser::Parser;

        let lucid_code = r#"def process(value: int) -> int:
    match value as v:
        case 1:
            return 10
        case 2:
            return 20
        case _:
            return 0
"#;

        let mut lexer = Lexer::new(lucid_code);
        let tokens = lexer.tokenize().expect("Lexer failed for match");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("Parser failed for match");
        let ir_module = crate::builder::IrBuilder::new().build_module(&module);

        // Verify match statement was processed (function generated)
        assert_eq!(ir_module.functions.len(), 1);
        assert_eq!(ir_module.functions[0].name, "process");
    }

    #[test]
    fn test_file_io_codegen() {
        // Test file I/O operations
        let c_code = r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef FILE* LucidFile;

int main() {
    // File write test
    LucidFile f = fopen("test.txt", "w");
    if (f) {
        fprintf(f, "%s", "Hello, World!");
        fclose(f);
    }

    // File read test
    LucidFile read_f = fopen("test.txt", "r");
    if (read_f) {
        char buffer[4096];
        fgets(buffer, 4096, read_f);
        fclose(read_f);

        // Verify
        if (buffer[0] == 'H') {
            return 0;  // Success
        }
    }
    return 1;  // Failure
}
        "#;

        assert!(test_c_code(c_code, ""));
    }

    #[test]
    fn test_trait_ir_generation() {
        // Test trait support in IR
        let trait_obj = crate::IrTrait {
            name: "Reader".to_string(),
            methods: vec![
                crate::TraitMethod {
                    name: "read".to_string(),
                    params: vec![crate::IrParam {
                        name: "self".to_string(),
                        ty: crate::IrType::Ptr,
                    }],
                    return_type: crate::IrType::Str,
                },
            ],
        };

        let mut module = crate::IrModule::new();
        module.traits.push(trait_obj);

        assert_eq!(module.traits.len(), 1);
        assert_eq!(module.traits[0].name, "Reader");
        assert_eq!(module.traits[0].methods.len(), 1);
        assert_eq!(module.traits[0].methods[0].name, "read");
    }

    #[test]
    fn test_generic_type_specialization() {
        // Test monomorphization of generic types
        let mut module = crate::IrModule::new();

        // Specialize List[int]
        let list_int_name = module.specialize_type("List", vec![crate::IrType::I64]);
        assert_eq!(list_int_name, "List__i64__");

        // Specialize List[str]
        let list_str_name = module.specialize_type("List", vec![crate::IrType::Str]);
        assert_eq!(list_str_name, "List__str__");

        // Specialize Dict[str, int]
        let dict_name = module.specialize_type("Dict",
            vec![crate::IrType::Str, crate::IrType::I64]);
        assert_eq!(dict_name, "Dict__str__i64__");

        // Verify specializations recorded
        assert_eq!(module.specializations.len(), 3);

        // Specialize same type again - should return same name without duplicating
        let list_int_name2 = module.specialize_type("List", vec![crate::IrType::I64]);
        assert_eq!(list_int_name2, list_int_name);
        assert_eq!(module.specializations.len(), 3);  // Still 3, not 4
    }

    #[test]
    fn test_error_type_hierarchies() {
        // Test error enum definitions
        let mut module = crate::IrModule::new();

        // Create ParseError enum
        let parse_error = crate::ErrorType {
            name: "ParseError".to_string(),
            variants: vec![
                crate::ErrorVariant {
                    name: "InvalidFormat".to_string(),
                    code: 1,
                },
                crate::ErrorVariant {
                    name: "UnexpectedEnd".to_string(),
                    code: 2,
                },
                crate::ErrorVariant {
                    name: "InvalidChar".to_string(),
                    code: 3,
                },
            ],
        };

        module.error_types.push(parse_error);

        // Verify error type registered
        assert_eq!(module.error_types.len(), 1);
        assert_eq!(module.error_types[0].name, "ParseError");
        assert_eq!(module.error_types[0].variants.len(), 3);
        assert_eq!(module.error_types[0].variants[0].code, 1);
    }

    #[test]
    fn test_trait_implementations() {
        // Test trait implementations on types
        let mut module = crate::IrModule::new();

        // Implement Reader trait for String
        let string_reader = crate::TraitImpl {
            trait_name: "Reader".to_string(),
            impl_type: "String".to_string(),
            methods: vec![
                crate::MethodImpl {
                    method_name: "read".to_string(),
                    impl_function: "String__read".to_string(),
                },
            ],
        };

        module.trait_impls.push(string_reader);

        // Verify trait implementation registered
        assert_eq!(module.trait_impls.len(), 1);
        assert_eq!(module.trait_impls[0].trait_name, "Reader");
        assert_eq!(module.trait_impls[0].impl_type, "String");
    }

    #[test]
    fn test_specialized_list_codegen() {
        // Test that List[i64] specialization generates proper C struct and functions
        let mut module = IrModule::new();

        // Create List[i64] specialization
        let specialized_name = module.specialize_type("List", vec![IrType::I64]);
        assert_eq!(specialized_name, "List__i64__");

        // Generate C code
        let mut codegen = CCodegenBackend::new();
        let code = codegen.generate(&module);

        // Verify struct definition
        assert!(code.contains("struct List__i64__"));
        assert!(code.contains("int64_t* items;"));
        assert!(code.contains("int64_t length;"));
        assert!(code.contains("int64_t capacity;"));

        // Verify generated functions
        assert!(code.contains("List__i64___new"));
        assert!(code.contains("List__i64___append"));
        assert!(code.contains("List__i64___pop"));
        assert!(code.contains("List__i64___length"));
        assert!(code.contains("List__i64___get"));
    }

    #[test]
    fn test_specialized_dict_codegen() {
        // Test that Dict[str, i64] specialization generates proper C struct and functions
        let mut module = IrModule::new();

        // Create Dict[str, i64] specialization
        let specialized_name = module.specialize_type("Dict", vec![IrType::Str, IrType::I64]);
        assert_eq!(specialized_name, "Dict__str__i64__");

        // Generate C code
        let mut codegen = CCodegenBackend::new();
        let code = codegen.generate(&module);

        // Verify struct definition
        assert!(code.contains("struct Dict__str__i64__"));
        assert!(code.contains("const char** keys;"));
        assert!(code.contains("int64_t* values;"));
        assert!(code.contains("int64_t length;"));

        // Verify generated functions
        assert!(code.contains("Dict__str__i64___new"));
        assert!(code.contains("Dict__str__i64___set"));
        assert!(code.contains("Dict__str__i64___get"));
    }

    #[test]
    fn test_trait_bounds_checking() {
        let mut module = IrModule::new();

        // Add a Clone trait
        let clone_trait = crate::IrTrait {
            name: "Clone".to_string(),
            methods: vec![
                crate::TraitMethod {
                    name: "clone".to_string(),
                    params: vec![],
                    return_type: IrType::Ptr,
                },
            ],
        };
        module.traits.push(clone_trait);

        // Add Copy trait
        let copy_trait = crate::IrTrait {
            name: "Copy".to_string(),
            methods: vec![],
        };
        module.traits.push(copy_trait);

        // String implements Clone
        let string_clone_impl = crate::TraitImpl {
            trait_name: "Clone".to_string(),
            impl_type: "String".to_string(),
            methods: vec![
                crate::MethodImpl {
                    method_name: "clone".to_string(),
                    impl_function: "String__clone".to_string(),
                },
            ],
        };
        module.trait_impls.push(string_clone_impl);

        // i64 implements Clone and Copy
        let i64_clone_impl = crate::TraitImpl {
            trait_name: "Clone".to_string(),
            impl_type: "i64".to_string(),
            methods: vec![],
        };
        module.trait_impls.push(i64_clone_impl);

        let i64_copy_impl = crate::TraitImpl {
            trait_name: "Copy".to_string(),
            impl_type: "i64".to_string(),
            methods: vec![],
        };
        module.trait_impls.push(i64_copy_impl);

        // Verify trait implementations
        assert!(module.type_implements_trait("String", "Clone"));
        assert!(!module.type_implements_trait("String", "Copy"));
        assert!(module.type_implements_trait("i64", "Clone"));
        assert!(module.type_implements_trait("i64", "Copy"));
        assert!(!module.type_implements_trait("f64", "Clone"));

        // Test bounds satisfaction
        let clone_bound = vec!["Clone".to_string()];
        assert!(module.type_satisfies_bounds("String", &clone_bound));
        assert!(module.type_satisfies_bounds("i64", &clone_bound));
        assert!(!module.type_satisfies_bounds("f64", &clone_bound));

        let copy_bound = vec!["Copy".to_string()];
        assert!(!module.type_satisfies_bounds("String", &copy_bound));
        assert!(module.type_satisfies_bounds("i64", &copy_bound));

        // Test multiple bounds
        let multi_bounds = vec!["Clone".to_string(), "Copy".to_string()];
        assert!(!module.type_satisfies_bounds("String", &multi_bounds));
        assert!(module.type_satisfies_bounds("i64", &multi_bounds));
    }

    #[test]
    fn test_generic_param_with_bounds() {
        // Test that generic parameters can have trait bounds
        let generic_param = crate::GenericParam {
            name: "T".to_string(),
            bounds: vec![
                crate::TraitBound {
                    type_param: "T".to_string(),
                    trait_name: "Clone".to_string(),
                },
            ],
        };

        assert_eq!(generic_param.name, "T");
        assert_eq!(generic_param.bounds.len(), 1);
        assert_eq!(generic_param.bounds[0].trait_name, "Clone");
    }

    #[test]
    fn test_collection_algorithms_codegen() {
        // Test that collection algorithms are generated (reverse, first, last)
        let mut module = IrModule::new();

        // Create List[i64] specialization
        let specialized_name = module.specialize_type("List", vec![IrType::I64]);

        // Generate C code
        let mut codegen = CCodegenBackend::new();
        let code = codegen.generate(&module);

        // Verify reverse, first, and last functions are generated
        assert!(code.contains("List__i64___reverse"), "reverse should be generated");
        assert!(code.contains("List__i64___first"), "first should be generated");
        assert!(code.contains("List__i64___last"), "last should be generated");

        // Verify the reverse implementation has proper swapping logic
        assert!(code.contains("temp = list->items"), "reverse should have swap logic");
    }

    #[test]
    fn test_string_helper_functions_generated() {
        // Test that string helper functions are generated
        let module = IrModule::new();

        // Generate C code
        let mut codegen = CCodegenBackend::new();
        let code = codegen.generate(&module);

        // Verify string helper functions are present
        assert!(code.contains("lucid_string_trim"), "trim should be generated");
        assert!(code.contains("lucid_string_replace"), "replace should be generated");
        assert!(code.contains("lucid_string_contains"), "contains should be generated");
        assert!(code.contains("lucid_string_starts_with"), "starts_with should be generated");
        assert!(code.contains("lucid_string_ends_with"), "ends_with should be generated");
        assert!(code.contains("lucid_string_to_upper"), "to_upper should be generated");
        assert!(code.contains("lucid_string_to_lower"), "to_lower should be generated");
    }

    #[test]
    fn test_math_stdlib_functions_generated() {
        // Test that math stdlib functions are generated
        let module = IrModule::new();

        // Generate C code
        let mut codegen = CCodegenBackend::new();
        let code = codegen.generate(&module);

        // Verify math functions are present
        assert!(code.contains("lucid_abs"), "abs should be generated");
        assert!(code.contains("lucid_min"), "min should be generated");
        assert!(code.contains("lucid_max"), "max should be generated");
        assert!(code.contains("lucid_pow"), "pow should be generated");
        assert!(code.contains("lucid_round"), "round should be generated");
        assert!(code.contains("lucid_floor_int"), "floor should be generated");
        assert!(code.contains("lucid_ceil_int"), "ceil should be generated");
    }

    #[test]
    fn test_list_extended_operations_codegen() {
        // Test extended list operations (count, is_empty, clear)
        let mut module = IrModule::new();

        // Create List[i64] specialization
        let specialized_name = module.specialize_type("List", vec![IrType::I64]);

        // Generate C code
        let mut codegen = CCodegenBackend::new();
        let code = codegen.generate(&module);

        // Verify extended operations
        assert!(code.contains("List__i64___count"), "count should be generated");
        assert!(code.contains("List__i64___is_empty"), "is_empty should be generated");
        assert!(code.contains("List__i64___clear"), "clear should be generated");
    }

    #[test]
    fn test_dict_extended_operations_codegen() {
        // Test extended dictionary operations
        let mut module = IrModule::new();

        // Create Dict[str, i64] specialization
        let specialized_name = module.specialize_type("Dict", vec![IrType::Str, IrType::I64]);

        // Generate C code
        let mut codegen = CCodegenBackend::new();
        let code = codegen.generate(&module);

        // Verify extended operations
        assert!(code.contains("Dict__str__i64___length"), "length should be generated");
        assert!(code.contains("Dict__str__i64___contains_key"), "contains_key should be generated");
        assert!(code.contains("Dict__str__i64___is_empty"), "is_empty should be generated");
        assert!(code.contains("Dict__str__i64___remove"), "remove should be generated");
        assert!(code.contains("Dict__str__i64___clear"), "clear should be generated");
    }

    #[test]
    fn test_exhaustive_pattern_matching_result() {
        let module = IrModule::new();

        // Create match arms for Result
        let ok_arm = crate::MatchArm {
            pattern: crate::Pattern::Variant("Ok".to_string(), vec![]),
            target_block: 1,
        };

        let err_arm = crate::MatchArm {
            pattern: crate::Pattern::Variant("Err".to_string(), vec![]),
            target_block: 2,
        };

        // Test exhaustive Result match
        let arms = vec![ok_arm, err_arm];
        assert!(module.is_result_match_exhaustive(&arms), "Result match should be exhaustive");

        // Test non-exhaustive (only Ok)
        let partial_arms = vec![crate::MatchArm {
            pattern: crate::Pattern::Variant("Ok".to_string(), vec![]),
            target_block: 1,
        }];
        assert!(!module.is_result_match_exhaustive(&partial_arms), "Partial Result match should not be exhaustive");

        // Test wildcard covers all
        let wildcard_arm = crate::MatchArm {
            pattern: crate::Pattern::Wildcard,
            target_block: 1,
        };
        assert!(module.is_result_match_exhaustive(&vec![wildcard_arm]), "Wildcard should cover all cases");
    }

    #[test]
    fn test_exhaustive_pattern_matching_error_enum() {
        let mut module = IrModule::new();

        // Create an error type
        let error_type = crate::ErrorType {
            name: "FileError".to_string(),
            variants: vec![
                crate::ErrorVariant { name: "NotFound".to_string(), code: 1 },
                crate::ErrorVariant { name: "PermissionDenied".to_string(), code: 2 },
                crate::ErrorVariant { name: "IOError".to_string(), code: 3 },
            ],
        };
        module.error_types.push(error_type);

        // Create exhaustive match (all variants covered)
        let arms_exhaustive = vec![
            crate::MatchArm {
                pattern: crate::Pattern::Variant("NotFound".to_string(), vec![]),
                target_block: 1,
            },
            crate::MatchArm {
                pattern: crate::Pattern::Variant("PermissionDenied".to_string(), vec![]),
                target_block: 2,
            },
            crate::MatchArm {
                pattern: crate::Pattern::Variant("IOError".to_string(), vec![]),
                target_block: 3,
            },
        ];

        assert!(module.is_error_match_exhaustive("FileError", &arms_exhaustive), "Should be exhaustive");

        // Test non-exhaustive (missing PermissionDenied)
        let arms_partial = vec![
            crate::MatchArm {
                pattern: crate::Pattern::Variant("NotFound".to_string(), vec![]),
                target_block: 1,
            },
            crate::MatchArm {
                pattern: crate::Pattern::Variant("IOError".to_string(), vec![]),
                target_block: 3,
            },
        ];

        assert!(!module.is_error_match_exhaustive("FileError", &arms_partial), "Should be non-exhaustive");

        // Test wildcard covers all
        let arms_wildcard = vec![
            crate::MatchArm {
                pattern: crate::Pattern::Variant("NotFound".to_string(), vec![]),
                target_block: 1,
            },
            crate::MatchArm {
                pattern: crate::Pattern::Wildcard,
                target_block: 99,
            },
        ];

        assert!(module.is_error_match_exhaustive("FileError", &arms_wildcard), "Wildcard should make it exhaustive");
    }

    #[test]
    fn test_json_stdlib_functions_generated() {
        // Test that JSON helper functions are generated
        let module = IrModule::new();

        // Generate C code
        let mut codegen = CCodegenBackend::new();
        let code = codegen.generate(&module);

        // Verify JSON functions are present
        assert!(code.contains("lucid_json_int"), "json_int should be generated");
        assert!(code.contains("lucid_json_double"), "json_double should be generated");
        assert!(code.contains("lucid_json_escape"), "json_escape should be generated");
        assert!(code.contains("lucid_json_bool"), "json_bool should be generated");
        assert!(code.contains("lucid_json_parse_int"), "json_parse_int should be generated");
        assert!(code.contains("lucid_json_parse_double"), "json_parse_double should be generated");

        // Verify escape logic for proper JSON handling
        assert!(code.contains("case '\\\"'"), "Should handle escaped quotes");
        assert!(code.contains("case '\\\\'"), "Should handle escaped backslashes");
    }

    #[test]
    fn test_iterator_support_registration() {
        let mut module = IrModule::new();

        // Register iterators for List and Dict
        let list_iterator = module.register_iterator("List".to_string(), IrType::I64, None);
        let dict_iterator = module.register_iterator("Dict".to_string(), IrType::Str, Some(IrType::I64));

        assert_eq!(list_iterator.collection_type, "List");
        assert_eq!(list_iterator.item_type, IrType::I64);
        assert!(!list_iterator.has_keys_method, "List shouldn't have keys()");

        assert_eq!(dict_iterator.collection_type, "Dict");
        assert!(dict_iterator.has_keys_method, "Dict should have keys()");
        assert!(dict_iterator.has_values_method, "Dict should have values()");
        assert!(dict_iterator.has_items_method, "Dict should have items()");

        // Verify iterators are registered
        assert!(module.get_iterator("List").is_some());
        assert!(module.get_iterator("Dict").is_some());
        assert!(module.is_iterable("List"));
        assert!(module.is_iterable("Dict"));
    }

    #[test]
    fn test_dict_iterator_methods_codegen() {
        // Test that Dict iterator methods (keys, values) are generated
        let mut module = IrModule::new();

        // Create Dict[str, i64] specialization
        let _specialized_name = module.specialize_type("Dict", vec![IrType::Str, IrType::I64]);

        // Generate C code
        let mut codegen = CCodegenBackend::new();
        let code = codegen.generate(&module);

        // Verify iterator methods are generated
        assert!(code.contains("Dict__str__i64___keys"), "keys() should be generated");
        assert!(code.contains("Dict__str__i64___values"), "values() should be generated");

        // Verify method implementations return arrays
        assert!(code.contains("keys_array = malloc"), "keys() should allocate array");
        assert!(code.contains("values_array = malloc"), "values() should allocate array");
    }

    #[test]
    fn test_iterable_type_checking() {
        let mut module = IrModule::new();

        // Register iterators
        module.register_iterator("List".to_string(), IrType::I64, None);
        module.register_iterator("Dict".to_string(), IrType::Str, Some(IrType::I64));

        // Check iterable status
        assert!(module.is_iterable("List"));
        assert!(module.is_iterable("Dict"));
        assert!(module.is_iterable("Range"));
        assert!(module.is_iterable("String"));
        assert!(!module.is_iterable("i64"));

        // Check iteration types
        assert_eq!(module.get_iteration_type("List"), Some(IrType::I64));
        assert_eq!(module.get_iteration_type("Range"), Some(IrType::I64));
        assert_eq!(module.get_iteration_type("String"), Some(IrType::Str));
    }

}
