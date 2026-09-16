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
    exception_handler_stack: Vec<usize>, // Stack of exception handler block IDs
}

impl IrBuilder {
    pub fn new() -> Self {
        Self {
            module: IrModule::new(),
            var_types: HashMap::new(),
            next_var_id: 0,
            current_function: None,
            current_block: 0,
            exception_handler_stack: Vec::new(),
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
            Stmt::ClassDef { name, bases, body, .. } => {
                self.build_class(name, bases, body);
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
            Stmt::While { condition, body, .. } => {
                if let Some(func_idx) = self.current_function {
                    let loop_cond_id = self.fresh_block("while_cond");
                    let loop_body_id = self.fresh_block("while_body");
                    let loop_exit_id = self.fresh_block("while_exit");

                    // Jump to loop condition
                    self.terminate(IrTerminator::Jump { target: loop_cond_id });

                    // Loop condition block
                    let saved_block = self.current_block;
                    self.current_block = loop_cond_id;
                    let cond_val = self.expr_to_ir_value(condition);
                    self.terminate(IrTerminator::Branch {
                        condition: cond_val,
                        then_block: loop_body_id,
                        else_block: loop_exit_id,
                    });

                    // Loop body block
                    self.current_block = loop_body_id;
                    for stmt in body {
                        self.build_stmt_recursive(stmt);
                    }
                    // Jump back to condition
                    if matches!(self.module.functions[func_idx].blocks[loop_body_id].terminator, IrTerminator::Unreachable) {
                        self.terminate(IrTerminator::Jump { target: loop_cond_id });
                    }

                    // Continue after loop
                    self.current_block = loop_exit_id;
                }
            }
            Stmt::Try { body, handlers, finally_body, .. } => {
                if let Some(func_idx) = self.current_function {
                    let try_body_id = self.fresh_block("try_body");
                    let merge_id = self.fresh_block("try_merge");
                    let finally_id = if finally_body.is_some() {
                        self.fresh_block("try_finally")
                    } else {
                        merge_id
                    };

                    // Create handler block BEFORE processing try body
                    let handler_id = if !handlers.is_empty() {
                        self.fresh_block("try_handler")
                    } else {
                        merge_id
                    };

                    // Push handler onto stack so raises can find it
                    self.exception_handler_stack.push(handler_id);

                    // Jump to try body
                    self.terminate(IrTerminator::Jump { target: try_body_id });

                    // Try body block
                    let saved_block = self.current_block;
                    self.current_block = try_body_id;
                    for stmt in body {
                        self.build_stmt_recursive(stmt);
                    }
                    // Jump to finally/merge on success
                    if matches!(self.module.functions[func_idx].blocks[try_body_id].terminator, IrTerminator::Unreachable) {
                        self.terminate(IrTerminator::Jump { target: finally_id });
                    }

                    // Exception handlers (simplified: first handler for all exceptions)
                    if !handlers.is_empty() {
                        self.current_block = handler_id;

                        // Build first handler's body
                        for stmt in &handlers[0].body {
                            self.build_stmt_recursive(stmt);
                        }
                        // Jump to finally/merge
                        if matches!(self.module.functions[func_idx].blocks[handler_id].terminator, IrTerminator::Unreachable) {
                            self.terminate(IrTerminator::Jump { target: finally_id });
                        }
                    }

                    // Pop handler from stack
                    self.exception_handler_stack.pop();

                    // Finally block
                    if let Some(finally_stmts) = finally_body {
                        self.current_block = finally_id;
                        for stmt in finally_stmts {
                            self.build_stmt_recursive(stmt);
                        }
                        // Jump to merge
                        if matches!(self.module.functions[func_idx].blocks[finally_id].terminator, IrTerminator::Unreachable) {
                            self.terminate(IrTerminator::Jump { target: merge_id });
                        }
                    }

                    // Continue after try/except
                    self.current_block = merge_id;
                }
            }
            Stmt::Raise { exception, .. } => {
                // Jump to the current exception handler
                if let Some(&handler_id) = self.exception_handler_stack.last() {
                    self.terminate(IrTerminator::Jump { target: handler_id });
                } else {
                    // No handler available, return the exception value
                    let ex_val = self.expr_to_ir_value(exception);
                    self.terminate(IrTerminator::Return { value: Some(ex_val) });
                }
            }
            Stmt::For { target, iterable, body, .. } => {
                if let Some(func_idx) = self.current_function {
                    if let Pattern::Ident(loop_var, _) = target {
                        let loop_init_id = self.fresh_block("for_init");
                        let loop_cond_id = self.fresh_block("for_cond");
                        let loop_body_id = self.fresh_block("for_body");
                        let loop_exit_id = self.fresh_block("for_exit");

                        // Jump to loop initialization
                        self.terminate(IrTerminator::Jump { target: loop_init_id });

                        // Loop initialization: i = 0 (simplified for range iteration)
                        self.current_block = loop_init_id;
                        self.emit(IrInstruction::Assign {
                            dest: loop_var.clone(),
                            value: IrValue::Int(0),
                        });
                        self.var_types.insert(loop_var.clone(), IrType::I64);
                        self.terminate(IrTerminator::Jump { target: loop_cond_id });

                        // Loop condition: i < iterable (simplified - assume iterable is an int)
                        self.current_block = loop_cond_id;
                        let iter_val = self.expr_to_ir_value(iterable);
                        let cond_dest = self.fresh_var("for_cond");
                        self.emit(IrInstruction::BinOp {
                            dest: cond_dest.clone(),
                            op: IrBinOp::Lt,
                            left: IrValue::Var(loop_var.clone()),
                            right: iter_val,
                        });
                        self.terminate(IrTerminator::Branch {
                            condition: IrValue::Var(cond_dest),
                            then_block: loop_body_id,
                            else_block: loop_exit_id,
                        });

                        // Loop body
                        self.current_block = loop_body_id;
                        for stmt in body {
                            self.build_stmt_recursive(stmt);
                        }

                        // Loop increment: i = i + 1
                        if matches!(self.module.functions[func_idx].blocks[loop_body_id].terminator, IrTerminator::Unreachable) {
                            let inc_dest = self.fresh_var("for_inc");
                            self.emit(IrInstruction::BinOp {
                                dest: inc_dest.clone(),
                                op: IrBinOp::Add,
                                left: IrValue::Var(loop_var.clone()),
                                right: IrValue::Int(1),
                            });
                            self.emit(IrInstruction::Assign {
                                dest: loop_var.clone(),
                                value: IrValue::Var(inc_dest),
                            });
                            self.terminate(IrTerminator::Jump { target: loop_cond_id });
                        }

                        // Continue after loop
                        self.current_block = loop_exit_id;
                    }
                }
            }
            Stmt::Match { subject, subject_alias: _, arms, .. } => {
                // Match statement: evaluate subject and branch to matching arm based on patterns
                if let Some(func_idx) = self.current_function {
                    let subject_val = self.expr_to_ir_value(subject);

                    // Create blocks for each arm
                    let mut arm_blocks = Vec::new();
                    for _ in arms {
                        arm_blocks.push(self.fresh_block("match_arm"));
                    }
                    let merge_id = self.fresh_block("match_merge");

                    // Implement pattern matching: check each pattern in order
                    let mut last_check_block = self.current_block;
                    for (arm_idx, (arm, arm_block_id)) in arms.iter().zip(arm_blocks.iter()).enumerate() {
                        // Check if this pattern matches
                        match &arm.pattern {
                            Pattern::Wildcard(_) => {
                                // Wildcard always matches, branch directly to this arm
                                self.current_block = last_check_block;
                                self.terminate(IrTerminator::Jump { target: *arm_block_id });
                            }
                            Pattern::Literal(lit_val, _) => {
                                // Compare subject to literal, converting to IrValue
                                let cond_var = self.fresh_var("pattern_match");
                                let ir_lit_val = match lit_val {
                                    LiteralValue::Int(n) => IrValue::Int(*n),
                                    LiteralValue::Float(f) => IrValue::Float(*f),
                                    LiteralValue::Bool(b) => IrValue::Bool(*b),
                                    LiteralValue::Str(s) => IrValue::String(s.clone()),
                                    LiteralValue::None => IrValue::Null,
                                    _ => IrValue::Int(0),
                                };
                                self.current_block = last_check_block;
                                self.emit(IrInstruction::BinOp {
                                    dest: cond_var.clone(),
                                    op: IrBinOp::Eq,
                                    left: subject_val.clone(),
                                    right: ir_lit_val,
                                });

                                let next_check_block = if arm_idx < arms.len() - 1 {
                                    self.fresh_block("pattern_check")
                                } else {
                                    merge_id
                                };

                                self.terminate(IrTerminator::Branch {
                                    condition: IrValue::Var(cond_var),
                                    then_block: *arm_block_id,
                                    else_block: next_check_block,
                                });
                                last_check_block = next_check_block;
                            }
                            _ => {
                                // For other patterns, treat as matching for now
                                // Full pattern support would handle Variant, Tuple, etc.
                                self.current_block = last_check_block;
                                self.terminate(IrTerminator::Jump { target: *arm_block_id });
                            }
                        }
                    }

                    // Generate code for each arm
                    for (arm, arm_block_id) in arms.iter().zip(arm_blocks.iter()) {
                        self.current_block = *arm_block_id;
                        for stmt in &arm.body {
                            self.build_stmt_recursive(stmt);
                        }
                        // Jump to merge if not already terminated
                        if matches!(self.module.functions[func_idx].blocks[*arm_block_id].terminator, IrTerminator::Unreachable) {
                            self.terminate(IrTerminator::Jump { target: merge_id });
                        }
                    }

                    self.current_block = merge_id;
                }
            }
            _ => {
                // Other statements not yet handled
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
                // Check if this is a method call: obj.method(args)
                if let Expr::Attribute { value, attr, .. } = &**func {
                    // This is a method call
                    let receiver = self.expr_to_ir_value(value);
                    let ir_args: Vec<IrValue> = args.iter().map(|arg| self.expr_to_ir_value(&arg.value)).collect();
                    let dest = self.fresh_var("method_result");
                    self.emit(IrInstruction::MethodCall {
                        dest: Some(dest.clone()),
                        receiver,
                        method: attr.clone(),
                        args: ir_args,
                    });
                    IrValue::Var(dest)
                } else if let Expr::Ident { name, .. } = &**func {
                    // Regular function call
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
            Expr::Construct { class_name, args, .. } => {
                let dest = self.fresh_var("obj");

                // Get field names from the class definition (extract before borrowing for emit)
                let field_names: Vec<String> = if let Some(cls) = self.module.get_class(class_name) {
                    cls.fields.iter().map(|f| f.name.clone()).collect()
                } else {
                    Vec::new()
                };

                // Build field value pairs from arguments, mapping positional args to field names
                let mut field_values = Vec::new();
                for (idx, arg) in args.iter().enumerate() {
                    let field_name = if let Some(name) = &arg.name {
                        // Named argument: use provided name
                        name.clone()
                    } else if idx < field_names.len() {
                        // Positional argument: use class field name at this index
                        field_names[idx].clone()
                    } else {
                        // Too many positional args or no class, fall back to field_N
                        format!("field_{}", idx)
                    };
                    let value = self.expr_to_ir_value(&arg.value);
                    field_values.push((field_name, value));
                }

                // Emit NewInstance instruction which handles allocation + initialization
                self.emit(IrInstruction::NewInstance {
                    dest: dest.clone(),
                    class_name: class_name.clone(),
                    field_values,
                });

                IrValue::Var(dest)
            }
            Expr::Propagate { expr, .. } => {
                // The ? operator: error propagation
                // For MVP, generate a call to unwrap_result that handles error checking
                // In production, this would generate proper control flow with early return
                let result_value = self.expr_to_ir_value(expr);
                let dest = self.fresh_var("unwrapped");

                // Call lucid_result_unwrap to check result and propagate errors
                self.emit(IrInstruction::Call {
                    dest: Some(dest.clone()),
                    func: "lucid_result_unwrap".to_string(),
                    args: vec![result_value],
                });

                IrValue::Var(dest)
            }
            Expr::Attribute { value, attr, .. } => {
                // Field access: obj.field
                let obj_value = self.expr_to_ir_value(value);
                let dest = self.fresh_var("field");

                // For now, emit a FieldRead instruction
                // The type should be inferred from the class definition
                // For MVP, assume it's the same object's class
                self.emit(IrInstruction::FieldRead {
                    dest: dest.clone(),
                    object: obj_value,
                    field: attr.clone(),
                    object_type: "Object".to_string(), // TODO: infer from value type
                });

                IrValue::Var(dest)
            }
            Expr::List { elements, .. } => {
                // List literal: [1, 2, 3]
                let dest = self.fresh_var("list");

                // Create empty list
                self.emit(IrInstruction::Call {
                    dest: Some(dest.clone()),
                    func: "lucid_list_new".to_string(),
                    args: vec![],
                });

                // Append each element
                for elem_expr in elements {
                    let elem_val = self.expr_to_ir_value(elem_expr);
                    self.emit(IrInstruction::Call {
                        dest: None,
                        func: "lucid_list_append".to_string(),
                        args: vec![IrValue::Var(dest.clone()), elem_val],
                    });
                }

                IrValue::Var(dest)
            }
            Expr::Index { value, index, .. } => {
                // Array/list/dict indexing: obj[index]
                let obj_value = self.expr_to_ir_value(value);
                let idx_value = self.expr_to_ir_value(index);
                let dest = self.fresh_var("elem");

                // Heuristic: if index is a string, assume dict; otherwise list
                let is_dict_access = matches!(idx_value, IrValue::String(_));

                if is_dict_access {
                    // Dictionary access: dict[key]
                    self.emit(IrInstruction::DictAccess {
                        dest: dest.clone(),
                        dict: obj_value,
                        key: idx_value,
                    });
                } else {
                    // List indexing: list[index]
                    self.emit(IrInstruction::Call {
                        dest: Some(dest.clone()),
                        func: "lucid_list_get".to_string(),
                        args: vec![obj_value, idx_value],
                    });
                }

                IrValue::Var(dest)
            }
            Expr::Dict { entries, .. } => {
                // Dictionary literal: {key: value, ...}
                let dest = self.fresh_var("dict");

                // Create an empty dictionary
                self.emit(IrInstruction::Call {
                    dest: Some(dest.clone()),
                    func: "lucid_dict_new".to_string(),
                    args: vec![],
                });

                // Add entries to the dictionary
                for (key_expr, val_expr) in entries {
                    let key_val = self.expr_to_ir_value(key_expr);
                    let val_val = self.expr_to_ir_value(val_expr);
                    self.emit(IrInstruction::Call {
                        dest: None,
                        func: "lucid_dict_set".to_string(),
                        args: vec![IrValue::Var(dest.clone()), key_val, val_val],
                    });
                }

                IrValue::Var(dest)
            }
            _ => IrValue::Null,
        }
    }

    fn build_class(&mut self, name: &str, bases: &[TypeExpr], body: &[ClassMember]) {
        // Extract parent class (Lucid supports single inheritance)
        let parent = bases.first().and_then(|base| {
            if let TypeExpr::Named { name: parent_name, .. } = base {
                Some(parent_name.clone())
            } else {
                None
            }
        });

        // Extract fields and methods from class members
        let mut fields = Vec::new();
        let mut methods = Vec::new();

        for member in body {
            match member {
                ClassMember::Field(field_def) => {
                    let field_type = self.lucid_type_to_ir_type(Some(&field_def.type_annotation));
                    fields.push(crate::IrField {
                        mutability: crate::MutabilityView::Exclusive,
                        name: field_def.name.clone(),
                        ty: field_type,
                    });
                }
                ClassMember::Method(func_def) => {
                    // Generate unique function name for this method
                    let func_name = format!("{}_{}", name, func_def.name);

                    // Build the method as an IrFunction with self as first parameter
                    let mut method_params = vec![IrParam {
                        name: "self".to_string(),
                        ty: IrType::Ptr, // self is a pointer to the object
                    }];

                    // Add the original function parameters
                    for param in &func_def.params {
                        method_params.push(IrParam {
                            name: param.name.clone(),
                            ty: self.lucid_type_to_ir_type(param.type_annotation.as_ref()),
                        });
                    }

                    let return_type = self.lucid_type_to_ir_type(func_def.return_type.as_ref());
                    let mut method_func = IrFunction::new(func_name.clone(), method_params, return_type);

                    // Build the method body
                    self.current_function = Some(self.module.functions.len());
                    self.module.add_function(method_func);

                    // Build statements in the method body
                    for stmt in &func_def.body {
                        self.build_statement(stmt);
                    }

                    self.current_function = None;

                    methods.push(crate::MethodDispatch {
                        class_name: name.to_string(),
                        method_name: func_def.name.clone(),
                        impl_function: func_name,
                    });
                }
                _ => {} // Ignore other member types for now
            }
        }

        // Create IrClass with parent tracking, extracted fields, and methods
        let ir_class = crate::IrClass {
            name: name.to_string(),
            generic_params: Vec::new(),
            parent,
            fields,
            methods,
        };

        self.module.add_class(ir_class);
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
