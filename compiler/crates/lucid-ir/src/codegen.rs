//! Code generation from IR to C

use crate::{IrModule, IrFunction, IrBlock, IrInstruction, IrValue, IrTerminator, IrType};

/// Generates C code from IR
pub struct CCodegenBackend {
    indent_level: usize,
    output: String,
    declared_vars: std::collections::HashSet<String>,
    var_types: std::collections::HashMap<String, String>, // Variable -> C type
}

impl CCodegenBackend {
    pub fn new() -> Self {
        Self {
            indent_level: 0,
            output: String::new(),
            declared_vars: std::collections::HashSet::new(),
            var_types: std::collections::HashMap::new(),
        }
    }

    pub fn generate(&mut self, module: &IrModule) -> String {
        self.emit_includes();
        self.emit_line("");

        // Generate specialized type definitions (List[T], Dict[K,V])
        self.generate_specialized_types(module);
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

    fn generate_specialized_types(&mut self, module: &IrModule) {
        // Generate C structs and helper functions for specialized generic types
        for spec in &module.specializations {
            match spec.generic_name.as_str() {
                "List" => self.generate_specialized_list(spec),
                "Dict" => self.generate_specialized_dict(spec),
                _ => {}
            }
            self.emit_line("");
        }
    }

    fn generate_specialized_list(&mut self, spec: &crate::TypeSpecialization) {
        // For List[T], generate a specialized struct and operations
        // Struct: struct List__T__ { void** items; int64_t length; int64_t capacity; }
        let type_name = &spec.specialized_name;
        let elem_type = if spec.type_args.len() > 0 {
            spec.type_args[0].c_type()
        } else {
            "void*"
        };

        self.emit_line(&format!("struct {} {{", type_name));
        self.indent_level += 1;
        self.emit_line(&format!("{}* items;", elem_type));
        self.emit_line("int64_t length;");
        self.emit_line("int64_t capacity;");
        self.indent_level -= 1;
        self.emit_line("};");
        self.emit_line("");

        // Generate constructor function
        self.emit_line(&format!(
            "struct {0} {0}_new(void) {{\n  struct {0} list;\n  list.items = NULL;\n  list.length = 0;\n  list.capacity = 0;\n  return list;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate append function
        self.emit_line(&format!(
            "void {0}_append(struct {0}* list, {1} item) {{\n  if (list->length >= list->capacity) {{\n    list->capacity = list->capacity > 0 ? list->capacity * 2 : 10;\n    list->items = realloc(list->items, list->capacity * sizeof({1}));\n  }}\n  list->items[list->length++] = item;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate pop function
        self.emit_line(&format!(
            "{1} {0}_pop(struct {0}* list) {{\n  if (list->length > 0) return list->items[--list->length];\n  return ({1})0;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate length function
        self.emit_line(&format!(
            "int64_t {0}_length(struct {0}* list) {{\n  return list->length;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate index access function
        self.emit_line(&format!(
            "{1} {0}_get(struct {0}* list, int64_t index) {{\n  if (index >= 0 && index < list->length) return list->items[index];\n  return ({1})0;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate reverse function
        self.emit_line(&format!(
            "void {0}_reverse(struct {0}* list) {{\n  for (int64_t i = 0; i < list->length / 2; i++) {{\n    {1} temp = list->items[i];\n    list->items[i] = list->items[list->length - 1 - i];\n    list->items[list->length - 1 - i] = temp;\n  }}\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate first function
        self.emit_line(&format!(
            "{1} {0}_first(struct {0}* list) {{\n  if (list->length > 0) return list->items[0];\n  return ({1})0;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate last function
        self.emit_line(&format!(
            "{1} {0}_last(struct {0}* list) {{\n  if (list->length > 0) return list->items[list->length - 1];\n  return ({1})0;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate count function
        self.emit_line(&format!(
            "int64_t {0}_count(struct {0}* list) {{\n  return list->length;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate is_empty function
        self.emit_line(&format!(
            "bool {0}_is_empty(struct {0}* list) {{\n  return list->length == 0;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate clear function
        self.emit_line(&format!(
            "void {0}_clear(struct {0}* list) {{\n  list->length = 0;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate slice function
        self.emit_line(&format!(
            "struct {0} {0}_slice(struct {0}* list, int64_t start, int64_t end) {{\n  struct {0} result = {0}_new();\n  if (start < 0) start = 0;\n  if (end > list->length) end = list->length;\n  if (start >= end) return result;\n  for (int64_t i = start; i < end; i++) {{\n    {0}_append(&result, list->items[i]);\n  }}\n  return result;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate contains function
        self.emit_line(&format!(
            "bool {0}_contains(struct {0}* list, {1} item) {{\n  for (int64_t i = 0; i < list->length; i++) {{\n    if (list->items[i] == item) return true;\n  }}\n  return false;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate index_of function
        self.emit_line(&format!(
            "int64_t {0}_index_of(struct {0}* list, {1} item) {{\n  for (int64_t i = 0; i < list->length; i++) {{\n    if (list->items[i] == item) return i;\n  }}\n  return -1;\n}}",
            type_name, elem_type
        ));
    }

    fn generate_specialized_dict(&mut self, spec: &crate::TypeSpecialization) {
        // For Dict[K,V], generate a specialized struct and operations
        let type_name = &spec.specialized_name;
        let key_type = if spec.type_args.len() > 0 {
            spec.type_args[0].c_type()
        } else {
            "void*"
        };
        let val_type = if spec.type_args.len() > 1 {
            spec.type_args[1].c_type()
        } else {
            "void*"
        };

        self.emit_line(&format!("struct {} {{", type_name));
        self.indent_level += 1;
        self.emit_line(&format!("{}* keys;", key_type));
        self.emit_line(&format!("{}* values;", val_type));
        self.emit_line("int64_t length;");
        self.emit_line("int64_t capacity;");
        self.indent_level -= 1;
        self.emit_line("};");
        self.emit_line("");

        // Generate constructor
        self.emit_line(&format!(
            "struct {0} {0}_new(void) {{\n  struct {0} dict;\n  dict.keys = NULL;\n  dict.values = NULL;\n  dict.length = 0;\n  dict.capacity = 0;\n  return dict;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate set function
        self.emit_line(&format!(
            "void {0}_set(struct {0}* dict, {1} key, {2} value) {{\n  for (int64_t i = 0; i < dict->length; i++) {{\n    if (dict->keys[i] == key) {{\n      dict->values[i] = value;\n      return;\n    }}\n  }}\n  if (dict->length >= dict->capacity) {{\n    dict->capacity = dict->capacity > 0 ? dict->capacity * 2 : 10;\n    dict->keys = realloc(dict->keys, dict->capacity * sizeof({1}));\n    dict->values = realloc(dict->values, dict->capacity * sizeof({2}));\n  }}\n  dict->keys[dict->length] = key;\n  dict->values[dict->length] = value;\n  dict->length++;\n}}",
            type_name, key_type, val_type
        ));
        self.emit_line("");

        // Generate get function
        self.emit_line(&format!(
            "{2} {0}_get(struct {0}* dict, {1} key) {{\n  for (int64_t i = 0; i < dict->length; i++) {{\n    if (dict->keys[i] == key) return dict->values[i];\n  }}\n  return ({2})0;\n}}",
            type_name, key_type, val_type
        ));
        self.emit_line("");

        // Generate length function for dict
        self.emit_line(&format!(
            "int64_t {0}_length(struct {0}* dict) {{\n  return dict->length;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate contains_key function
        self.emit_line(&format!(
            "bool {0}_contains_key(struct {0}* dict, {1} key) {{\n  for (int64_t i = 0; i < dict->length; i++) {{\n    if (dict->keys[i] == key) return true;\n  }}\n  return false;\n}}",
            type_name, key_type
        ));
        self.emit_line("");

        // Generate is_empty function for dict
        self.emit_line(&format!(
            "bool {0}_is_empty(struct {0}* dict) {{\n  return dict->length == 0;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate remove function
        self.emit_line(&format!(
            "void {0}_remove(struct {0}* dict, {1} key) {{\n  for (int64_t i = 0; i < dict->length; i++) {{\n    if (dict->keys[i] == key) {{\n      for (int64_t j = i; j < dict->length - 1; j++) {{\n        dict->keys[j] = dict->keys[j + 1];\n        dict->values[j] = dict->values[j + 1];\n      }}\n      dict->length--;\n      return;\n    }}\n  }}\n}}",
            type_name, key_type
        ));
        self.emit_line("");

        // Generate clear function
        self.emit_line(&format!(
            "void {0}_clear(struct {0}* dict) {{\n  dict->length = 0;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate keys function (returns array of keys)
        self.emit_line(&format!(
            "{1}* {0}_keys(struct {0}* dict) {{\n  {1}* keys_array = malloc(dict->length * sizeof({1}));\n  for (int64_t i = 0; i < dict->length; i++) {{\n    keys_array[i] = dict->keys[i];\n  }}\n  return keys_array;\n}}",
            type_name, key_type
        ));
        self.emit_line("");

        // Generate values function (returns array of values)
        self.emit_line(&format!(
            "{1}* {0}_values(struct {0}* dict) {{\n  {1}* values_array = malloc(dict->length * sizeof({1}));\n  for (int64_t i = 0; i < dict->length; i++) {{\n    values_array[i] = dict->values[i];\n  }}\n  return values_array;\n}}",
            type_name, val_type
        ));
    }

    fn generate_class(&mut self, class: &crate::IrClass) {
        // Generate vtable structure if class has methods
        if !class.methods.is_empty() {
            self.emit_line(&format!("struct {}_VTable {{", class.name));
            self.indent_level += 1;

            for method in &class.methods {
                // Generate function pointer for each method
                // For now, assume all methods return int64_t and take void* self
                self.emit_line(&format!(
                    "int64_t (*{})(void*);",
                    method.method_name
                ));
            }

            self.indent_level -= 1;
            self.emit_line("};");
            self.emit_line("");
        }

        // Generate struct definition
        self.emit_line(&format!("struct {} {{", class.name));
        self.indent_level += 1;

        // Add vtable pointer if there are methods
        if !class.methods.is_empty() {
            self.emit_line(&format!("struct {}_VTable *__vtable;", class.name));
        }

        for field in &class.fields {
            let field_type = field.ty.c_type();
            self.emit_line(&format!("{} {};", field_type, field.name));
        }

        self.indent_level -= 1;
        self.emit_line("};");
    }

    fn generate_function(&mut self, func: &IrFunction) {
        // Clear declared vars and types for each function
        self.declared_vars.clear();
        self.var_types.clear();

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
                    // Determine type: use tracked type, or infer from value if it's a variable
                    let var_type = if let Some(tracked) = self.var_types.get(dest) {
                        tracked.clone()
                    } else if let IrValue::Var(src_var) = value {
                        // If assigning from another variable, propagate its type
                        self.var_types.get(src_var).cloned().unwrap_or_else(|| "int64_t".to_string())
                    } else {
                        "int64_t".to_string()
                    };
                    // Track the type for this new variable
                    self.var_types.insert(dest.clone(), var_type.clone());
                    self.emit_line(&format!("{} {} = {};", var_type, dest, value_code));
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

                // Check if this is string concatenation
                let is_string_op = matches!(left, IrValue::String(_)) || matches!(right, IrValue::String(_));

                if is_string_op && matches!(op, crate::IrBinOp::Add) {
                    // String concatenation: use sprintf
                    if !self.declared_vars.contains(dest) {
                        self.emit_line(&format!(
                            "char {}[1024]; sprintf({}, \"%s%s\", {}, {});",
                            dest, dest, left_code, right_code
                        ));
                        self.declared_vars.insert(dest.clone());
                        self.var_types.insert(dest.clone(), "const char*".to_string());
                    } else {
                        self.emit_line(&format!(
                            "sprintf({}, \"%s%s\", {}, {});",
                            dest, left_code, right_code
                        ));
                    }
                } else {
                    // Normal arithmetic operation
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

                // Detect math functions that return double
                let is_float_func = matches!(func.as_str(), "sqrt" | "sin" | "cos" | "tan" | "log" | "exp");

                if let Some(d) = dest {
                    if !self.declared_vars.contains(d) {
                        let var_type = if is_float_func { "double" } else { "int64_t" };
                        self.emit_line(&format!("{} {} = {}({});", var_type, d, func, args_code));
                        self.declared_vars.insert(d.clone());
                        self.var_types.insert(d.clone(), var_type.to_string());
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
                let mut all_args = vec![receiver_code.clone()];
                all_args.extend(args.iter().map(|a| self.value_to_c(a)));
                let args_code = all_args.join(", ");

                // Check for builtin list methods
                let is_list_method = matches!(method.as_str(),
                    "append" | "pop" | "length" | "get" | "first" | "last" |
                    "map" | "filter" | "foreach" | "reverse" | "sort");

                if is_list_method {
                    let func_name = format!("lucid_list_{}", method);
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
                } else {
                    // Class method call - use virtual dispatch through vtable
                    // receiver->__vtable->method(receiver, args)
                    if let Some(d) = dest {
                        if !self.declared_vars.contains(d) {
                            self.emit_line(&format!(
                                "int64_t {} = {}->__vtable->{}({});",
                                d, receiver_code, method, receiver_code
                            ));
                            self.declared_vars.insert(d.clone());
                        } else {
                            self.emit_line(&format!(
                                "{} = {}->__vtable->{}({});",
                                d, receiver_code, method, receiver_code
                            ));
                        }
                    } else {
                        self.emit_line(&format!(
                            "{}->__vtable->{}({});",
                            receiver_code, method, receiver_code
                        ));
                    }
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
                    // Track the type of this variable for future assignments
                    self.var_types.insert(dest.clone(), format!("struct {} *", class_name));
                } else {
                    self.emit_line(&format!(
                        "{} = (struct {} *)malloc(sizeof(struct {}));",
                        dest, class_name, class_name
                    ));
                }

                // TODO: Initialize vtable pointer if class has methods
                // This requires checking module.get_class(class_name).methods
                // For now, skip vtable init - will be added in post-processing pass

                // Initialize fields
                for (field_name, field_value) in field_values {
                    let val_code = self.value_to_c(field_value);
                    self.emit_line(&format!(
                        "{}->{} = {};",
                        dest, field_name, val_code
                    ));
                }
            }
            IrInstruction::ResultCheck {
                result,
                error_block,
                success_block,
            } => {
                let result_code = self.value_to_c(result);
                // Generate: if (result.tag == ERROR) goto error_block; else goto success_block;
                self.emit_line(&format!("if ({}.tag == ERROR) {{", result_code));
                self.indent_level += 1;
                self.emit_line(&format!("goto block_{};", error_block));
                self.indent_level -= 1;
                self.emit_line("} else {");
                self.indent_level += 1;
                self.emit_line(&format!("goto block_{};", success_block));
                self.indent_level -= 1;
                self.emit_line("}");
            }
            IrInstruction::DictAccess { dest, dict, key } => {
                let dict_code = self.value_to_c(dict);
                let key_code = self.value_to_c(key);

                if !self.declared_vars.contains(dest) {
                    self.emit_line(&format!(
                        "int64_t {} = lucid_dict_get({}, {});",
                        dest, dict_code, key_code
                    ));
                    self.declared_vars.insert(dest.clone());
                } else {
                    self.emit_line(&format!(
                        "{} = lucid_dict_get({}, {});",
                        dest, dict_code, key_code
                    ));
                }
            }
            IrInstruction::FileOpen { dest, path, mode } => {
                let path_code = self.value_to_c(path);
                if !self.declared_vars.contains(dest) {
                    self.emit_line(&format!(
                        "LucidFile {} = fopen({}, \"{}\");",
                        dest, path_code, mode
                    ));
                    self.declared_vars.insert(dest.clone());
                } else {
                    self.emit_line(&format!(
                        "{} = fopen({}, \"{}\");",
                        dest, path_code, mode
                    ));
                }
            }
            IrInstruction::FileWrite { file, content } => {
                let file_code = self.value_to_c(file);
                let content_code = self.value_to_c(content);
                self.emit_line(&format!(
                    "fprintf({}, \"%s\", {});",
                    file_code, content_code
                ));
            }
            IrInstruction::FileRead { dest, file } => {
                let file_code = self.value_to_c(file);
                if !self.declared_vars.contains(dest) {
                    self.emit_line(&format!(
                        "char {}[4096]; fgets({}, 4096, {});",
                        dest, dest, file_code
                    ));
                    self.declared_vars.insert(dest.clone());
                } else {
                    self.emit_line(&format!(
                        "fgets({}, 4096, {});",
                        dest, file_code
                    ));
                }
            }
            IrInstruction::FileClose { file } => {
                let file_code = self.value_to_c(file);
                self.emit_line(&format!("fclose({});", file_code));
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
        self.emit_line("#include <math.h>");
        self.emit_line("#include <string.h>");
        self.emit_line("#include <ctype.h>");
        self.emit_line("typedef FILE* LucidFile;");  // File handle type
        self.emit_line("");

        // String helper functions
        self.emit_line("// String trim (remove leading/trailing whitespace)");
        self.emit_line("const char* lucid_string_trim(const char* str) {");
        self.indent_level += 1;
        self.emit_line("while (*str && isspace((unsigned char)*str)) str++;");
        self.emit_line("const char* end = str + strlen(str) - 1;");
        self.emit_line("while (end > str && isspace((unsigned char)*end)) end--;");
        self.emit_line("static char result[4096];");
        self.emit_line("strncpy(result, str, end - str + 1);");
        self.emit_line("result[end - str + 1] = '\\0';");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // String replace function
        self.emit_line("// String replace (first occurrence)");
        self.emit_line("const char* lucid_string_replace(const char* str, const char* from, const char* to) {");
        self.indent_level += 1;
        self.emit_line("char* pos = strstr((char*)str, from);");
        self.emit_line("if (!pos) return str;");
        self.emit_line("static char result[4096];");
        self.emit_line("int len = pos - str;");
        self.emit_line("strncpy(result, str, len);");
        self.emit_line("strcpy(result + len, to);");
        self.emit_line("strcpy(result + len + strlen(to), pos + strlen(from));");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // String contains function
        self.emit_line("// String contains check");
        self.emit_line("bool lucid_string_contains(const char* str, const char* substr) {");
        self.indent_level += 1;
        self.emit_line("return strstr(str, substr) != NULL;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // String starts_with function
        self.emit_line("// String starts_with check");
        self.emit_line("bool lucid_string_starts_with(const char* str, const char* prefix) {");
        self.indent_level += 1;
        self.emit_line("return strncmp(str, prefix, strlen(prefix)) == 0;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // String ends_with function
        self.emit_line("// String ends_with check");
        self.emit_line("bool lucid_string_ends_with(const char* str, const char* suffix) {");
        self.indent_level += 1;
        self.emit_line("int str_len = strlen(str);");
        self.emit_line("int suffix_len = strlen(suffix);");
        self.emit_line("if (suffix_len > str_len) return false;");
        self.emit_line("return strcmp(str + str_len - suffix_len, suffix) == 0;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // String to_upper function
        self.emit_line("// String to uppercase");
        self.emit_line("const char* lucid_string_to_upper(const char* str) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("for (int i = 0; str[i]; i++) result[i] = toupper(str[i]);");
        self.emit_line("result[strlen(str)] = '\\0';");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // String to_lower function
        self.emit_line("// String to lowercase");
        self.emit_line("const char* lucid_string_to_lower(const char* str) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("for (int i = 0; str[i]; i++) result[i] = tolower(str[i]);");
        self.emit_line("result[strlen(str)] = '\\0';");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // Math helper functions
        self.emit_line("// Math helper: absolute value");
        self.emit_line("int64_t lucid_abs(int64_t x) {");
        self.indent_level += 1;
        self.emit_line("return x < 0 ? -x : x;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: minimum of two integers");
        self.emit_line("int64_t lucid_min(int64_t a, int64_t b) {");
        self.indent_level += 1;
        self.emit_line("return a < b ? a : b;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: maximum of two integers");
        self.emit_line("int64_t lucid_max(int64_t a, int64_t b) {");
        self.indent_level += 1;
        self.emit_line("return a > b ? a : b;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: power");
        self.emit_line("double lucid_pow(double base, double exp) {");
        self.indent_level += 1;
        self.emit_line("return pow(base, exp);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: integer power");
        self.emit_line("int64_t lucid_pow_int(int64_t base, int64_t exp) {");
        self.indent_level += 1;
        self.emit_line("int64_t result = 1;");
        self.emit_line("for (int64_t i = 0; i < exp; i++) result *= base;");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: floating point absolute value");
        self.emit_line("double lucid_fabs(double x) {");
        self.indent_level += 1;
        self.emit_line("return fabs(x);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: round to nearest integer");
        self.emit_line("int64_t lucid_round(double x) {");
        self.indent_level += 1;
        self.emit_line("return (int64_t)(x + 0.5);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: floor");
        self.emit_line("int64_t lucid_floor_int(double x) {");
        self.indent_level += 1;
        self.emit_line("return (int64_t)floor(x);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: ceiling");
        self.emit_line("int64_t lucid_ceil_int(double x) {");
        self.indent_level += 1;
        self.emit_line("return (int64_t)ceil(x);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // JSON helper functions
        self.emit_line("// JSON utilities");
        self.emit_line("// Convert integer to JSON string");
        self.emit_line("const char* lucid_json_int(int64_t x) {");
        self.indent_level += 1;
        self.emit_line("static char result[256];");
        self.emit_line("snprintf(result, sizeof(result), \"%lld\", (long long)x);");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Convert double to JSON string");
        self.emit_line("const char* lucid_json_double(double x) {");
        self.indent_level += 1;
        self.emit_line("static char result[256];");
        self.emit_line("snprintf(result, sizeof(result), \"%.15g\", x);");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Escape string for JSON");
        self.emit_line("const char* lucid_json_escape(const char* str) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("int j = 0;");
        self.emit_line("result[j++] = '\\\"';");
        self.emit_line("for (int i = 0; str[i] && j < 4090; i++) {");
        self.indent_level += 1;
        self.emit_line("switch(str[i]) {");
        self.indent_level += 1;
        self.emit_line("case '\\\"': result[j++]='\\\\'; result[j++]='\\\"'; break;");
        self.emit_line("case '\\\\': result[j++]='\\\\'; result[j++]='\\\\'; break;");
        self.emit_line("case '\\n': result[j++]='\\\\'; result[j++]='n'; break;");
        self.emit_line("case '\\r': result[j++]='\\\\'; result[j++]='r'; break;");
        self.emit_line("case '\\t': result[j++]='\\\\'; result[j++]='t'; break;");
        self.emit_line("default: result[j++]=str[i];");
        self.indent_level -= 1;
        self.emit_line("}");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("result[j++] = '\\\"';");
        self.emit_line("result[j] = '\\0';");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Convert bool to JSON");
        self.emit_line("const char* lucid_json_bool(bool x) {");
        self.indent_level += 1;
        self.emit_line("return x ? \"true\" : \"false\";");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Parse JSON integer");
        self.emit_line("int64_t lucid_json_parse_int(const char* json_str) {");
        self.indent_level += 1;
        self.emit_line("return strtoll(json_str, NULL, 10);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Parse JSON double");
        self.emit_line("double lucid_json_parse_double(const char* json_str) {");
        self.indent_level += 1;
        self.emit_line("return strtod(json_str, NULL);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // String slice operations
        self.emit_line("// String slice (substring)");
        self.emit_line("const char* lucid_string_slice(const char* str, int64_t start, int64_t end) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("int len = strlen(str);");
        self.emit_line("if (start < 0) start = 0;");
        self.emit_line("if (end > len) end = len;");
        self.emit_line("if (start > end) return \"\";");
        self.emit_line("int slice_len = end - start;");
        self.emit_line("strncpy(result, str + start, slice_len);");
        self.emit_line("result[slice_len] = '\\0';");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// String split by delimiter");
        self.emit_line("void lucid_string_split_helper(const char* str, const char* delim, const char** results, int64_t* count) {");
        self.indent_level += 1;
        self.emit_line("char* copy = malloc(strlen(str) + 1);");
        self.emit_line("strcpy(copy, str);");
        self.emit_line("char* token = strtok(copy, delim);");
        self.emit_line("*count = 0;");
        self.emit_line("while (token != NULL && *count < 100) {");
        self.indent_level += 1;
        self.emit_line("results[*count] = token;");
        self.emit_line("(*count)++;");
        self.emit_line("token = strtok(NULL, delim);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("free(copy);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// String repeat");
        self.emit_line("const char* lucid_string_repeat(const char* str, int64_t times) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("result[0] = '\\0';");
        self.emit_line("for (int64_t i = 0; i < times; i++) {");
        self.indent_level += 1;
        self.emit_line("strcat(result, str);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
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
