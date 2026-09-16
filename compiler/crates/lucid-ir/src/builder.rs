//! IR Builder: Converts Lucid AST to IR

use lucid_syntax::ast::*;
use crate::{
    IrModule, IrFunction, IrBlock, IrInstruction, IrValue, IrTerminator, IrType, IrParam, IrBinOp, IrUnaryOp,
};
use std::collections::HashMap;

/// Builds IR from Lucid AST
pub struct IrBuilder {
    module: IrModule,
    var_types: HashMap<String, IrType>,
    next_var_id: usize,
    current_function: Option<usize>,
    current_block: usize,
}

impl IrBuilder {
    pub fn new() -> Self {
        Self {
            module: IrModule::new(),
            var_types: HashMap::new(),
            next_var_id: 0,
            current_function: None,
            current_block: 0,
        }
    }

    fn emit(&mut self, instr: IrInstruction) {
        if let Some(func_idx) = self.current_function {
            let func = &mut self.module.functions[func_idx];
            func.blocks[self.current_block].add_instruction(instr);
        }
    }

    fn terminate(&mut self, term: IrTerminator) {
        if let Some(func_idx) = self.current_function {
            let func = &mut self.module.functions[func_idx];
            func.blocks[self.current_block].set_terminator(term);
        }
    }

    fn fresh_block(&mut self, label: &str) -> usize {
        if let Some(func_idx) = self.current_function {
            let func = &mut self.module.functions[func_idx];
            func.new_block(label.to_string())
        } else {
            0
        }
    }

    fn current_block_mut(&mut self) -> Option<&mut IrBlock> {
        if let Some(func_idx) = self.current_function {
            let func = &mut self.module.functions[func_idx];
            Some(func.current_block_mut())
        } else {
            None
        }
    }

    pub fn build_module(mut self, module: &Module) -> IrModule {
        for stmt in &module.statements {
            self.build_statement(stmt);
        }
        self.module
    }

    fn build_statement(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Function(func_def) => {
                self.build_function(func_def);
            }
            Stmt::ClassDef { name, body, .. } => {
                self.build_class(name, body);
            }
            _ => {
                // Other statements handled during function building
            }
        }
    }

    fn build_function(&mut self, func: &FunctionDef) {
        let params = func
            .params
            .iter()
            .map(|p| IrParam {
                name: p.name.clone(),
                ty: self.lucid_type_to_ir_type(p.type_annotation.as_ref()),
            })
            .collect();

        let return_type = self.lucid_type_to_ir_type(func.return_type.as_ref());
        let ir_func = IrFunction::new(func.name.clone(), params, return_type);
        self.module.add_function(ir_func);

        let func_idx = self.module.functions.len() - 1;
        self.current_function = Some(func_idx);
        self.current_block = 0;

        self.var_types.clear();
        for param in &func.params {
            self.var_types.insert(
                param.name.clone(),
                self.lucid_type_to_ir_type(param.type_annotation.as_ref()),
            );
        }

        // Build function body
        for stmt in &func.body {
            self.build_stmt_recursive(stmt);
        }

        // Add default return if missing
        if let Some(func_idx) = self.current_function {
            let func = &mut self.module.functions[func_idx];
            let last_block = func.current_block_mut();
            if matches!(last_block.terminator, IrTerminator::Unreachable) {
                last_block.terminator = IrTerminator::Return { value: None };
            }
        }

        self.current_function = None;
    }

    fn build_stmt_recursive(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Return { value, .. } => {
                let ir_val = value.as_ref().map(|v| self.expr_to_ir_value(v));
                self.terminate(IrTerminator::Return { value: ir_val });
            }
            Stmt::VarDef {
                pattern: Pattern::Ident(name, _),
                value,
                ..
            } => {
                if let Some(expr) = value {
                    let ir_val = self.expr_to_ir_value(expr);
                    let ty = self.infer_expr_type(expr);
                    let name_clone = name.clone();
                    self.var_types.insert(name_clone.clone(), ty);
                    self.emit(IrInstruction::Assign {
                        dest: name_clone,
                        value: ir_val,
                    });
                }
            }
            Stmt::Assignment { target, value, .. } => {
                if let Expr::Ident { name, .. } = target {
                    let ir_val = self.expr_to_ir_value(value);
                    let name_clone = name.clone();
                    self.emit(IrInstruction::Assign {
                        dest: name_clone,
                        value: ir_val,
                    });
                }
            }
            Stmt::Expr(expr) => {
                let _ = self.expr_to_ir_value(expr);
            }
            Stmt::If { condition, then_branch, elif_branches, else_branch, .. } => {
                if let Some(func_idx) = self.current_function {
                    let cond_val = self.expr_to_ir_value(condition);
                    let then_id = self.fresh_block("if_then");
                    let merge_id = self.fresh_block("if_merge");

                    // Create else block (or use merge if no else)
                    let else_id = if elif_branches.is_empty() && else_branch.is_none() {
                        merge_id
                    } else {
                        self.fresh_block("if_else")
                    };

                    // Branch in current block
                    self.terminate(IrTerminator::Branch {
                        condition: cond_val,
                        then_block: then_id,
                        else_block: else_id,
                    });

                    // Build then branch
                    let saved_block = self.current_block;
                    self.current_block = then_id;
                    for stmt in then_branch {
                        self.build_stmt_recursive(stmt);
                    }
                    // Jump to merge if not terminated
                    if matches!(self.module.functions[func_idx].blocks[then_id].terminator, IrTerminator::Unreachable) {
                        self.terminate(IrTerminator::Jump { target: merge_id });
                    }

                    // Build else branch (simplified - just the first elif or else)
                    if !elif_branches.is_empty() || else_branch.is_some() {
                        self.current_block = else_id;
                        let else_stmts = if let Some(stmts) = else_branch {
                            stmts
                        } else if !elif_branches.is_empty() {
                            &elif_branches[0].1
                        } else {
                            &vec![]
                        };
                        for stmt in else_stmts {
                            self.build_stmt_recursive(stmt);
                        }
                        // Jump to merge if not terminated
                        if matches!(self.module.functions[func_idx].blocks[else_id].terminator, IrTerminator::Unreachable) {
                            self.terminate(IrTerminator::Jump { target: merge_id });
                        }
                    }

                    // Continue in merge block
                    self.current_block = merge_id;
                }
            }
            _ => {
                // Other control flow not yet handled (While, For, etc.)
            }
        }
    }

    fn expr_to_ir_value(&mut self, expr: &Expr) -> IrValue {
        match expr {
            Expr::Literal { value, .. } => match value {
                LiteralValue::Int(n) => IrValue::Int(*n),
                LiteralValue::Float(f) => IrValue::Float(*f),
                LiteralValue::Bool(b) => IrValue::Bool(*b),
                LiteralValue::Str(s) => IrValue::String(s.clone()),
                LiteralValue::None => IrValue::Null,
                _ => IrValue::Int(0),
            },
            Expr::Ident { name, .. } => IrValue::Var(name.clone()),
            Expr::Binary {
                op, left, right, ..
            } => {
                let left_val = self.expr_to_ir_value(left);
                let right_val = self.expr_to_ir_value(right);
                let ir_op = self.binop_to_ir_binop(op);
                let dest = self.fresh_var("binop");
                self.emit(IrInstruction::BinOp {
                    dest: dest.clone(),
                    op: ir_op,
                    left: left_val,
                    right: right_val,
                });
                IrValue::Var(dest)
            }
            Expr::Unary { op, expr, .. } => {
                let val = self.expr_to_ir_value(expr);
                let ir_op = match op {
                    UnaryOp::Neg => IrUnaryOp::Neg,
                    UnaryOp::Not => IrUnaryOp::Not,
                    UnaryOp::Invert => IrUnaryOp::BitNot,
                    _ => IrUnaryOp::Neg,
                };
                let dest = self.fresh_var("unop");
                self.emit(IrInstruction::UnaryOp {
                    dest: dest.clone(),
                    op: ir_op,
                    operand: val,
                });
                IrValue::Var(dest)
            }
            Expr::Call { func, args, .. } => {
                if let Expr::Ident { name, .. } = &**func {
                    let ir_args: Vec<IrValue> = args.iter().map(|arg| self.expr_to_ir_value(&arg.value)).collect();
                    let dest = self.fresh_var("call");
                    self.emit(IrInstruction::Call {
                        dest: Some(dest.clone()),
                        func: name.clone(),
                        args: ir_args,
                    });
                    IrValue::Var(dest)
                } else {
                    IrValue::Null
                }
            }
            _ => IrValue::Null,
        }
    }

    fn build_class(&mut self, name: &str, _body: &[ClassMember]) {
        // Class compilation would generate constructor functions, method dispatch tables, etc.
        // For now, just register the class type
        self.module.add_type(name.to_string(), IrType::Named(name.to_string()));
    }

    fn lucid_type_to_ir_type(&self, type_expr: Option<&TypeExpr>) -> IrType {
        match type_expr {
            Some(TypeExpr::Named { name, .. }) => match name.as_str() {
                "int" => IrType::I64,
                "float" => IrType::F64,
                "bool" => IrType::Bool,
                "str" => IrType::Str,
                _ => IrType::Named(name.clone()),
            },
            _ => IrType::Ptr, // Default to pointer for unknown types
        }
    }

    fn infer_expr_type(&self, _expr: &Expr) -> IrType {
        IrType::Ptr // Simplified type inference
    }

    fn binop_to_ir_binop(&self, op: &BinaryOp) -> IrBinOp {
        match op {
            BinaryOp::Add => IrBinOp::Add,
            BinaryOp::Sub => IrBinOp::Sub,
            BinaryOp::Mul => IrBinOp::Mul,
            BinaryOp::Div => IrBinOp::Div,
            BinaryOp::Mod => IrBinOp::Mod,
            BinaryOp::Eq => IrBinOp::Eq,
            BinaryOp::NotEq => IrBinOp::NotEq,
            BinaryOp::Lt => IrBinOp::Lt,
            BinaryOp::LtEq => IrBinOp::LtEq,
            BinaryOp::Gt => IrBinOp::Gt,
            BinaryOp::GtEq => IrBinOp::GtEq,
            BinaryOp::And => IrBinOp::And,
            BinaryOp::Or => IrBinOp::Or,
            BinaryOp::BitAnd => IrBinOp::BitAnd,
            BinaryOp::BitOr => IrBinOp::BitOr,
            BinaryOp::BitXor => IrBinOp::BitXor,
            _ => IrBinOp::Add, // Placeholder
        }
    }

    fn fresh_var(&mut self, prefix: &str) -> String {
        let id = self.next_var_id;
        self.next_var_id += 1;
        format!("{}__{}", prefix, id)
    }
}

impl Default for IrBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ir_builder_creation() {
        let builder = IrBuilder::new();
        assert_eq!(builder.module.functions.len(), 0);
    }

    #[test]
    fn test_fresh_var_generation() {
        let mut builder = IrBuilder::new();
        let var1 = builder.fresh_var("test");
        let var2 = builder.fresh_var("test");
        assert_eq!(var1, "test__0");
        assert_eq!(var2, "test__1");
    }

    #[test]
    fn test_type_conversion() {
        let builder = IrBuilder::new();
        let int_type = builder.lucid_type_to_ir_type(Some(&TypeExpr::Named {
            name: "int".to_string(),
            args: Vec::new(),
            span: lucid_syntax::token::Span {
                line: 0,
                column: 0,
                start: 0,
                end: 0,
            },
        }));
        assert_eq!(int_type, IrType::I64);
    }
}
