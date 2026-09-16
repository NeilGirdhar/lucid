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
