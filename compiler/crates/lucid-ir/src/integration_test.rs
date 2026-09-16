//! Integration tests for IR → C → Executable

#[cfg(test)]
mod tests {
    use crate::{IrModule, IrFunction, IrParam, IrType, IrValue, IrInstruction, IrTerminator, CCodegenBackend, IrClass, IrField};
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
            .args(&["-o", &exe, &source])
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
}
