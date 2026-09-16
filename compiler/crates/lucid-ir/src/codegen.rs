//! Code generation from IR to C

use crate::{IrModule, IrFunction, IrBlock, IrInstruction, IrValue, IrTerminator, IrType};

/// Generates C code from IR
pub struct CCodegenBackend {
    indent_level: usize,
    output: String,
    declared_vars: std::collections::HashSet<String>,
}

impl CCodegenBackend {
    pub fn new() -> Self {
        Self {
            indent_level: 0,
            output: String::new(),
            declared_vars: std::collections::HashSet::new(),
        }
    }

    pub fn generate(&mut self, module: &IrModule) -> String {
        self.emit_includes();
        self.emit_line("");

        // Generate struct definitions for classes
        for class in &module.classes {
            self.generate_class(class);
            self.emit_line("");
        }

        // Generate function declarations and implementations
        for function in &module.functions {
            self.generate_function(function);
            self.emit_line("");
        }

        self.output.clone()
    }

    fn generate_class(&mut self, class: &crate::IrClass) {
        self.emit_line(&format!("struct {} {{", class.name));
        self.indent_level += 1;

        for field in &class.fields {
            let field_type = field.ty.c_type();
            self.emit_line(&format!("{} {};", field_type, field.name));
        }

        self.indent_level -= 1;
        self.emit_line("};");
    }

    fn generate_function(&mut self, func: &IrFunction) {
        // Clear declared vars for each function
        self.declared_vars.clear();

        // Mark parameters as already declared
        for param in &func.params {
            self.declared_vars.insert(param.name.clone());
        }

        // Function signature
        let return_ctype = func.return_type.c_type();

        // Build parameter list inline for cleaner output
        let mut params_str = String::new();
        if func.params.is_empty() {
            params_str.push_str("void");
        } else {
            for (i, param) in func.params.iter().enumerate() {
                if i > 0 { params_str.push_str(", "); }
                params_str.push_str(param.ty.c_type());
                params_str.push(' ');
                params_str.push_str(&param.name);
            }
        }

        self.emit_line(&format!("{} {}({}) {{", return_ctype, func.name, params_str));
        self.indent_level += 1;

        // Generate basic blocks
        for block in &func.blocks {
            self.generate_block(block);
        }

        self.indent_level -= 1;
        self.emit_line("}");
    }

    fn generate_block(&mut self, block: &IrBlock) {
        // Label (not needed for entry block)
        if block.id > 0 {
            self.emit_line(&format!("block_{}:", block.id));
        }

        // Instructions
        for instr in &block.instructions {
            self.generate_instruction(instr);
        }

        // Terminator
        self.generate_terminator(&block.terminator);
    }

    fn generate_instruction(&mut self, instr: &IrInstruction) {
        match instr {
            IrInstruction::Assign { dest, value } => {
                let value_code = self.value_to_c(value);
                if !self.declared_vars.contains(dest) {
                    self.emit_line(&format!("int64_t {} = {};", dest, value_code));
                    self.declared_vars.insert(dest.clone());
                } else {
                    self.emit_line(&format!("{} = {};", dest, value_code));
                }
            }
            IrInstruction::BinOp {
                dest, op, left, right,
            } => {
                let left_code = self.value_to_c(left);
                let right_code = self.value_to_c(right);
                let op_str = self.binop_to_c(op);
                if !self.declared_vars.contains(dest) {
                    self.emit_line(&format!(
                        "int64_t {} = {} {} {};",
                        dest, left_code, op_str, right_code
                    ));
                    self.declared_vars.insert(dest.clone());
                } else {
                    self.emit_line(&format!(
                        "{} = {} {} {};",
                        dest, left_code, op_str, right_code
                    ));
                }
            }
            IrInstruction::UnaryOp { dest, op, operand } => {
                let operand_code = self.value_to_c(operand);
                let op_str = self.unaryop_to_c(op);
                if !self.declared_vars.contains(dest) {
                    self.emit_line(&format!(
                        "int64_t {} = {}({}); ",
                        dest, op_str, operand_code
                    ));
                    self.declared_vars.insert(dest.clone());
                } else {
                    self.emit_line(&format!(
                        "{} = {}({}); ",
                        dest, op_str, operand_code
                    ));
                }
            }
            IrInstruction::Call {
                dest,
                func,
                args,
            } => {
                let args_code = args
                    .iter()
                    .map(|arg| self.value_to_c(arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                if let Some(d) = dest {
                    if !self.declared_vars.contains(d) {
                        self.emit_line(&format!("int64_t {} = {}({});", d, func, args_code));
                        self.declared_vars.insert(d.clone());
                    } else {
                        self.emit_line(&format!("{} = {}({});", d, func, args_code));
                    }
                } else {
                    self.emit_line(&format!("{}({});", func, args_code));
                }
            }
            IrInstruction::MethodCall {
                dest,
                receiver,
                method,
                args,
            } => {
                let receiver_code = self.value_to_c(receiver);
                let mut all_args = vec![receiver_code];
                all_args.extend(args.iter().map(|a| self.value_to_c(a)));
                let args_code = all_args.join(", ");

                // Translate receiver.method(args) to method(receiver, args)
                // In a real implementation, we'd look up the class type to generate the right function name
                let func_name = format!("method_{}", method);

                if let Some(d) = dest {
                    if !self.declared_vars.contains(d) {
                        self.emit_line(&format!("int64_t {} = {}({});", d, func_name, args_code));
                        self.declared_vars.insert(d.clone());
                    } else {
                        self.emit_line(&format!("{} = {}({});", d, func_name, args_code));
                    }
                } else {
                    self.emit_line(&format!("{}({});", func_name, args_code));
                }
            }
            IrInstruction::Load { dest, addr } => {
                let addr_code = self.value_to_c(addr);
                if !self.declared_vars.contains(dest) {
                    self.emit_line(&format!("int64_t {} = *(int64_t*){};", dest, addr_code));
                    self.declared_vars.insert(dest.clone());
                } else {
                    self.emit_line(&format!("{} = *(int64_t*){};", dest, addr_code));
                }
            }
            IrInstruction::Store { addr, value } => {
                let addr_code = self.value_to_c(addr);
                let value_code = self.value_to_c(value);
                self.emit_line(&format!("*(int64_t*){} = {};", addr_code, value_code));
            }
            IrInstruction::Cast {
                dest, from, to_type,
            } => {
                let from_code = self.value_to_c(from);
                let to_ctype = to_type.c_type();
                if !self.declared_vars.contains(dest) {
                    self.emit_line(&format!(
                        "{} {} = ({}){};",
                        to_ctype, dest, to_ctype, from_code
                    ));
                    self.declared_vars.insert(dest.clone());
                } else {
                    self.emit_line(&format!(
                        "{} = ({}){};",
                        dest, to_ctype, from_code
                    ));
                }
            }
            IrInstruction::Malloc { dest, size } => {
                let size_code = self.value_to_c(size);
                if !self.declared_vars.contains(dest) {
                    self.emit_line(&format!(
                        "void* {} = malloc({});",
                        dest, size_code
                    ));
                    self.declared_vars.insert(dest.clone());
                } else {
                    self.emit_line(&format!(
                        "{} = malloc({});",
                        dest, size_code
                    ));
                }
            }
            IrInstruction::Free { addr } => {
                let addr_code = self.value_to_c(addr);
                self.emit_line(&format!("free({});", addr_code));
            }
            IrInstruction::FieldRead { dest, object, field, object_type } => {
                let obj_code = self.value_to_c(object);
                // Generate: dest = ((StructType*)obj)->field
                if !self.declared_vars.contains(dest) {
                    self.emit_line(&format!(
                        "int64_t {} = (({} *){}).{};",
                        dest, object_type, obj_code, field
                    ));
                    self.declared_vars.insert(dest.clone());
                } else {
                    self.emit_line(&format!(
                        "{} = (({} *){}).{};",
                        dest, object_type, obj_code, field
                    ));
                }
            }
            IrInstruction::FieldWrite { object, field, value, object_type } => {
                let obj_code = self.value_to_c(object);
                let val_code = self.value_to_c(value);
                // Generate: ((StructType*)obj)->field = value
                self.emit_line(&format!(
                    "(({} *){}).{} = {};",
                    object_type, obj_code, field, val_code
                ));
            }
            IrInstruction::NewInstance { dest, class_name, field_values } => {
                // Allocate memory for the instance
                if !self.declared_vars.contains(dest) {
                    self.emit_line(&format!(
                        "struct {} *{} = (struct {} *)malloc(sizeof(struct {}));",
                        class_name, dest, class_name, class_name
                    ));
                    self.declared_vars.insert(dest.clone());
                } else {
                    self.emit_line(&format!(
                        "{} = (struct {} *)malloc(sizeof(struct {}));",
                        dest, class_name, class_name
                    ));
                }

                // Initialize fields
                for (field_name, field_value) in field_values {
                    let val_code = self.value_to_c(field_value);
                    self.emit_line(&format!(
                        "{}->{} = {};",
                        dest, field_name, val_code
                    ));
                }
            }
        }
    }

    fn generate_terminator(&mut self, terminator: &IrTerminator) {
        match terminator {
            IrTerminator::Jump { target } => {
                // Simplified: would need label tracking
                self.emit_line(&format!("goto block_{};", target));
            }
            IrTerminator::Branch {
                condition,
                then_block,
                else_block,
            } => {
                let cond_code = self.value_to_c(condition);
                self.emit_line(&format!("if ({}) {{", cond_code));
                self.indent_level += 1;
                self.emit_line(&format!("goto block_{};", then_block));
                self.indent_level -= 1;
                self.emit_line("} else {");
                self.indent_level += 1;
                self.emit_line(&format!("goto block_{};", else_block));
                self.indent_level -= 1;
                self.emit_line("}");
            }
            IrTerminator::Return { value } => {
                if let Some(val) = value {
                    let val_code = self.value_to_c(val);
                    self.emit_line(&format!("return {};", val_code));
                } else {
                    self.emit_line("return;");
                }
            }
            IrTerminator::Unreachable => {
                self.emit_line("__builtin_unreachable();");
            }
        }
    }

    fn value_to_c(&self, value: &IrValue) -> String {
        match value {
            IrValue::Int(n) => n.to_string(),
            IrValue::Float(f) => f.to_string(),
            IrValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            IrValue::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
            IrValue::Var(name) => name.clone(),
            IrValue::Global(name) => name.clone(),
            IrValue::Null => "NULL".to_string(),
        }
    }

    fn binop_to_c(&self, op: &crate::IrBinOp) -> &'static str {
        use crate::IrBinOp::*;
        match op {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Mod => "%",
            Eq => "==",
            NotEq => "!=",
            Lt => "<",
            LtEq => "<=",
            Gt => ">",
            GtEq => ">=",
            And => "&&",
            Or => "||",
            BitAnd => "&",
            BitOr => "|",
            BitXor => "^",
        }
    }

    fn unaryop_to_c(&self, op: &crate::IrUnaryOp) -> &'static str {
        use crate::IrUnaryOp::*;
        match op {
            Neg => "-",
            Not => "!",
            BitNot => "~",
        }
    }

    fn emit_includes(&mut self) {
        self.emit_line("#include <stdint.h>");
        self.emit_line("#include <stdbool.h>");
        self.emit_line("#include <stdio.h>");
        self.emit_line("#include <stdlib.h>");
    }

    fn emit_line(&mut self, line: &str) {
        for _ in 0..self.indent_level {
            self.output.push_str("  ");
        }
        self.output.push_str(line);
        self.output.push('\n');
    }
}

impl Default for CCodegenBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IrFunction, IrParam};

    #[test]
    fn test_codegen_simple_function() {
        let mut backend = CCodegenBackend::new();
        let func = IrFunction::new(
            "test".to_string(),
            vec![IrParam {
                name: "x".to_string(),
                ty: IrType::I64,
            }],
            IrType::I64,
        );
        let module = {
            let mut m = IrModule::new();
            m.add_function(func);
            m
        };
        let code = backend.generate(&module);
        assert!(code.contains("int64_t test(int64_t x)"));
        assert!(code.contains("{"));
        assert!(code.contains("#include"));
    }

    #[test]
    fn test_value_to_c() {
        let backend = CCodegenBackend::new();
        assert_eq!(backend.value_to_c(&IrValue::Int(42)), "42");
        assert_eq!(backend.value_to_c(&IrValue::Bool(true)), "true");
        assert_eq!(backend.value_to_c(&IrValue::Null), "NULL");
    }
}
