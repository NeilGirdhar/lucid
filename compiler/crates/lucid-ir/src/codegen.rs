//! Code generation from IR to C

use crate::{IrModule, IrFunction, IrBlock, IrInstruction, IrValue, IrTerminator, IrType};

/// Generates C code from IR
pub struct CCodegenBackend {
    indent_level: usize,
    output: String,
    declared_vars: std::collections::HashSet<String>,
    var_types: std::collections::HashMap<String, String>, // Variable -> C type
    classes_with_methods: std::collections::HashSet<String>, // Track which classes have methods/vtables
}

impl CCodegenBackend {
    pub fn new() -> Self {
        Self {
            indent_level: 0,
            output: String::new(),
            declared_vars: std::collections::HashSet::new(),
            var_types: std::collections::HashMap::new(),
            classes_with_methods: std::collections::HashSet::new(),
        }
    }

    pub fn generate(&mut self, module: &IrModule) -> String {
        self.emit_includes();
        self.emit_line("");

        // Generate specialized type definitions (List[T], Dict[K,V])
        self.generate_specialized_types(module);
        self.emit_line("");

        // Generate forward declarations for all functions (needed for vtable initialization)
        for function in &module.functions {
            let return_ctype = function.return_type.c_type();
            let param_list = function.params.iter()
                .map(|p| format!("{} {}", p.ty.c_type(), p.name))
                .collect::<Vec<_>>()
                .join(", ");
            self.emit_line(&format!("{} {}({});", return_ctype, function.name, param_list));
        }
        self.emit_line("");

        // Generate struct definitions for classes (needs access to function signatures for vtable)
        for class in &module.classes {
            self.generate_class_with_vtable(class, module);
            self.emit_line("");
        }

        // Generate function implementations
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
        self.emit_line("");

        // Generate sort function (only for comparable types)
        if elem_type == "int64_t" || elem_type == "double" {
            let sort_func = if elem_type == "int64_t" {
                "lucid_quicksort_int"
            } else {
                "lucid_quicksort_double"
            };

            self.emit_line(&format!(
                "void {0}_sort(struct {0}* list) {{\n  if (list->length > 1) {{\n    {1}(list->items, 0, list->length - 1);\n  }}\n}}",
                type_name, sort_func
            ));
        }
        self.emit_line("");

        // Generate sum function (for numeric types)
        self.emit_line(&format!(
            "{1} {0}_sum(struct {0}* list) {{\n  {1} total = 0;\n  for (int64_t i = 0; i < list->length; i++) {{\n    total += list->items[i];\n  }}\n  return total;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate min function (for numeric types)
        self.emit_line(&format!(
            "{1} {0}_min(struct {0}* list) {{\n  if (list->length == 0) return 0;\n  {1} min_val = list->items[0];\n  for (int64_t i = 1; i < list->length; i++) {{\n    if (list->items[i] < min_val) min_val = list->items[i];\n  }}\n  return min_val;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate max function (for numeric types)
        self.emit_line(&format!(
            "{1} {0}_max(struct {0}* list) {{\n  if (list->length == 0) return 0;\n  {1} max_val = list->items[0];\n  for (int64_t i = 1; i < list->length; i++) {{\n    if (list->items[i] > max_val) max_val = list->items[i];\n  }}\n  return max_val;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate flatten function (for nested lists - assumes elem_type contains list structure)
        // This allows flattening List[List[T]] into List[T]
        self.emit_line(&format!(
            "struct {0} {0}_flatten(struct {0}* list) {{\n  struct {0} result = {0}_new();\n  for (int64_t i = 0; i < list->length; i++) {{\n    struct {0}* inner = (struct {0}*)list->items[i];\n    if (inner != NULL) {{\n      for (int64_t j = 0; j < inner->length; j++) {{\n        {0}_append(&result, inner->items[j]);\n      }}\n    }}\n  }}\n  return result;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate any method - checks if any element matches predicate
        // For simplicity, this is a helper that works with equality
        self.emit_line(&format!(
            "bool {0}_any(struct {0}* list, {1} target) {{\n  for (int64_t i = 0; i < list->length; i++) {{\n    if (list->items[i] == target) return true;\n  }}\n  return false;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate all method - checks if all elements match a value
        self.emit_line(&format!(
            "bool {0}_all(struct {0}* list, {1} target) {{\n  if (list->length == 0) return false;\n  for (int64_t i = 0; i < list->length; i++) {{\n    if (list->items[i] != target) return false;\n  }}\n  return true;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate unique method - removes duplicate elements
        self.emit_line(&format!(
            "struct {0} {0}_unique(struct {0}* list) {{\n  struct {0} result = {0}_new();\n  for (int64_t i = 0; i < list->length; i++) {{\n    bool found = false;\n    for (int64_t j = 0; j < result.length; j++) {{\n      if (result.items[j] == list->items[i]) {{\n        found = true;\n        break;\n      }}\n    }}\n    if (!found) {{\n      {0}_append(&result, list->items[i]);\n    }}\n  }}\n  return result;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate take method - returns first n elements
        self.emit_line(&format!(
            "struct {0} {0}_take(struct {0}* list, int64_t n) {{\n  struct {0} result = {0}_new();\n  if (n < 0) return result;\n  for (int64_t i = 0; i < list->length && i < n; i++) {{\n    {0}_append(&result, list->items[i]);\n  }}\n  return result;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate drop method - skips first n elements
        self.emit_line(&format!(
            "struct {0} {0}_drop(struct {0}* list, int64_t n) {{\n  struct {0} result = {0}_new();\n  if (n < 0) n = 0;\n  for (int64_t i = n; i < list->length; i++) {{\n    {0}_append(&result, list->items[i]);\n  }}\n  return result;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate concat method - concatenates two lists
        self.emit_line(&format!(
            "struct {0} {0}_concat(struct {0}* list1, struct {0}* list2) {{\n  struct {0} result = {0}_new();\n  for (int64_t i = 0; i < list1->length; i++) {{\n    {0}_append(&result, list1->items[i]);\n  }}\n  for (int64_t i = 0; i < list2->length; i++) {{\n    {0}_append(&result, list2->items[i]);\n  }}\n  return result;\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate join method for string lists (combines list of strings)
        if elem_type == "const char*" {
            self.emit_line(&format!(
                "const char* {0}_join(struct {0}* list, const char* sep) {{\n  if (list->length == 0) return \"\";\n  static char result[4096];\n  int pos = 0;\n  for (int64_t i = 0; i < list->length && pos < 4090; i++) {{\n    int len = strlen((const char*)list->items[i]);\n    if (pos + len < 4090) {{\n      strcpy(result + pos, (const char*)list->items[i]);\n      pos += len;\n    }}\n    if (i < list->length - 1 && sep && pos + strlen(sep) < 4090) {{\n      strcpy(result + pos, sep);\n      pos += strlen(sep);\n    }}\n  }}\n  result[pos] = '\\0';\n  return result;\n}}",
                type_name
            ));
            self.emit_line("");
        }

        // Generate last_index_of method
        self.emit_line(&format!(
            "int64_t {0}_last_index_of(struct {0}* list, {1} item) {{\n  for (int64_t i = list->length - 1; i >= 0; i--) {{\n    if (list->items[i] == item) return i;\n  }}\n  return -1;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate find_all method (returns indices of matching items)
        self.emit_line(&format!(
            "void {0}_find_all(struct {0}* list, {1} item, int64_t* indices, int64_t* count) {{\n  *count = 0;\n  for (int64_t i = 0; i < list->length; i++) {{\n    if (list->items[i] == item) {{\n      indices[(*count)++] = i;\n    }}\n  }}\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate insert method - insert at specific index
        self.emit_line(&format!(
            "void {0}_insert(struct {0}* list, int64_t index, {1} item) {{\n  if (index < 0 || index > list->length) return;\n  if (list->length >= list->capacity) {{\n    list->capacity = list->capacity > 0 ? list->capacity * 2 : 10;\n    list->items = realloc(list->items, list->capacity * sizeof({1}));\n  }}\n  for (int64_t i = list->length; i > index; i--) {{\n    list->items[i] = list->items[i - 1];\n  }}\n  list->items[index] = item;\n  list->length++;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate remove_at method - remove at specific index
        self.emit_line(&format!(
            "{1} {0}_remove_at(struct {0}* list, int64_t index) {{\n  if (index < 0 || index >= list->length) return ({1})0;\n  {1} item = list->items[index];\n  for (int64_t i = index; i < list->length - 1; i++) {{\n    list->items[i] = list->items[i + 1];\n  }}\n  list->length--;\n  return item;\n}}",
            type_name, elem_type
        ));
        self.emit_line("");

        // Generate fill method - fill list with value
        self.emit_line(&format!(
            "void {0}_fill(struct {0}* list, {1} item) {{\n  for (int64_t i = 0; i < list->length; i++) {{\n    list->items[i] = item;\n  }}\n}}",
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
        self.emit_line("");

        // Generate merge function (merges another dict into this one)
        self.emit_line(&format!(
            "void {0}_merge(struct {0}* dict1, struct {0}* dict2) {{\n  for (int64_t i = 0; i < dict2->length; i++) {{\n    {0}_set(dict1, dict2->keys[i], dict2->values[i]);\n  }}\n}}",
            type_name
        ));
        self.emit_line("");

        // Generate has_value function (check if value exists)
        self.emit_line(&format!(
            "bool {0}_has_value(struct {0}* dict, {1} value) {{\n  for (int64_t i = 0; i < dict->length; i++) {{\n    if (dict->values[i] == value) return true;\n  }}\n  return false;\n}}",
            type_name, val_type
        ));
        self.emit_line("");

        // Generate update function (update or insert key-value pair)
        self.emit_line(&format!(
            "void {0}_update(struct {0}* dict, {1} key, {2} value) {{\n  {0}_set(dict, key, value);\n}}",
            type_name, key_type, val_type
        ));
    }

    fn generate_class_with_vtable(&mut self, class: &crate::IrClass, module: &IrModule) {
        // Generate vtable structure if class has methods
        if !class.methods.is_empty() {
            self.classes_with_methods.insert(class.name.clone());
            self.emit_line(&format!("struct {}_VTable {{", class.name));
            self.indent_level += 1;

            for method in &class.methods {
                // Look up the actual function to get the return type
                let return_type = if let Some(func) = module.functions.iter().find(|f| f.name == method.impl_function) {
                    func.return_type.c_type().to_string()
                } else {
                    "int64_t".to_string()
                };

                // Generate function pointer for each method
                self.emit_line(&format!(
                    "{}(*{})(void*);",
                    return_type, method.method_name
                ));
            }

            self.indent_level -= 1;
            self.emit_line("};");
            self.emit_line("");

            // Generate static vtable instance
            self.emit_line(&format!("struct {}_VTable {}_vtable = {{", class.name, class.name));
            self.indent_level += 1;
            for method in &class.methods {
                // Get the return type for casting if needed
                let return_type = if let Some(func) = module.functions.iter().find(|f| f.name == method.impl_function) {
                    func.return_type.c_type().to_string()
                } else {
                    "int64_t".to_string()
                };

                self.emit_line(&format!(".{} = ({} (*)(void*)){},",
                    method.method_name, return_type, method.impl_function));
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

                // Initialize vtable pointer if the class has methods
                if self.classes_with_methods.contains(class_name) {
                    self.emit_line(&format!(
                        "{}->__vtable = &{}_vtable;",
                        dest, class_name
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
            IrInstruction::Raise { message, condition_failed } => {
                // Generate code to panic/abort on broken invariant
                if let Some(cond) = condition_failed {
                    let cond_code = self.value_to_c(cond);
                    self.emit_line(&format!(
                        "if (!{}) {{ fprintf(stderr, \"Invariant failed: {}\\n\"); abort(); }}",
                        cond_code, message
                    ));
                } else {
                    self.emit_line(&format!(
                        "{{ fprintf(stderr, \"Invariant failed: {}\\n\"); abort(); }}",
                        message
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
        self.emit_line("#include <math.h>");
        self.emit_line("#include <string.h>");
        self.emit_line("#include <ctype.h>");
        self.emit_line("#include <stdarg.h>");
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

        self.emit_line("// Math helper: min for doubles");
        self.emit_line("double lucid_min_double(double a, double b) {");
        self.indent_level += 1;
        self.emit_line("return a < b ? a : b;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: max for doubles");
        self.emit_line("double lucid_max_double(double a, double b) {");
        self.indent_level += 1;
        self.emit_line("return a > b ? a : b;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: modulo for integers");
        self.emit_line("int64_t lucid_modulo(int64_t a, int64_t b) {");
        self.indent_level += 1;
        self.emit_line("if (b == 0) return 0;");
        self.emit_line("return a % b;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: remainder for doubles");
        self.emit_line("double lucid_remainder(double a, double b) {");
        self.indent_level += 1;
        self.emit_line("if (b == 0.0) return 0.0;");
        self.emit_line("return remainder(a, b);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: integer square root");
        self.emit_line("int64_t lucid_sqrt_int(int64_t x) {");
        self.indent_level += 1;
        self.emit_line("if (x < 0) return 0;");
        self.emit_line("return (int64_t)sqrt((double)x);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: degrees to radians");
        self.emit_line("double lucid_radians(double degrees) {");
        self.indent_level += 1;
        self.emit_line("return degrees * 3.14159265358979323846 / 180.0;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math helper: radians to degrees");
        self.emit_line("double lucid_degrees(double radians) {");
        self.indent_level += 1;
        self.emit_line("return radians * 180.0 / 3.14159265358979323846;");
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
        self.emit_line("");

        // Utility functions
        self.emit_line("// Type conversion utilities");
        self.emit_line("const char* lucid_to_string_int(int64_t x) {");
        self.indent_level += 1;
        self.emit_line("static char result[256];");
        self.emit_line("snprintf(result, sizeof(result), \"%lld\", (long long)x);");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("const char* lucid_to_string_double(double x) {");
        self.indent_level += 1;
        self.emit_line("static char result[256];");
        self.emit_line("snprintf(result, sizeof(result), \"%.15g\", x);");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Random number helper");
        self.emit_line("int64_t lucid_random_int(int64_t max) {");
        self.indent_level += 1;
        self.emit_line("return (int64_t)(((double)rand() / RAND_MAX) * max);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("double lucid_random_double(void) {");
        self.indent_level += 1;
        self.emit_line("return (double)rand() / RAND_MAX;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // Sorting utilities
        self.emit_line("// Quicksort implementation for integers");
        self.emit_line("void lucid_quicksort_int(int64_t* arr, int64_t low, int64_t high) {");
        self.indent_level += 1;
        self.emit_line("if (low < high) {");
        self.indent_level += 1;
        self.emit_line("int64_t pivot = arr[high];");
        self.emit_line("int64_t i = low - 1;");
        self.emit_line("for (int64_t j = low; j < high; j++) {");
        self.indent_level += 1;
        self.emit_line("if (arr[j] < pivot) {");
        self.indent_level += 1;
        self.emit_line("i++;");
        self.emit_line("int64_t temp = arr[i];");
        self.emit_line("arr[i] = arr[j];");
        self.emit_line("arr[j] = temp;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("int64_t temp = arr[i + 1];");
        self.emit_line("arr[i + 1] = arr[high];");
        self.emit_line("arr[high] = temp;");
        self.emit_line("lucid_quicksort_int(arr, low, i);");
        self.emit_line("lucid_quicksort_int(arr, i + 2, high);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Quicksort for floating point");
        self.emit_line("void lucid_quicksort_double(double* arr, int64_t low, int64_t high) {");
        self.indent_level += 1;
        self.emit_line("if (low < high) {");
        self.indent_level += 1;
        self.emit_line("double pivot = arr[high];");
        self.emit_line("int64_t i = low - 1;");
        self.emit_line("for (int64_t j = low; j < high; j++) {");
        self.indent_level += 1;
        self.emit_line("if (arr[j] < pivot) {");
        self.indent_level += 1;
        self.emit_line("i++;");
        self.emit_line("double temp = arr[i];");
        self.emit_line("arr[i] = arr[j];");
        self.emit_line("arr[j] = temp;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("double temp = arr[i + 1];");
        self.emit_line("arr[i + 1] = arr[high];");
        self.emit_line("arr[high] = temp;");
        self.emit_line("lucid_quicksort_double(arr, low, i);");
        self.emit_line("lucid_quicksort_double(arr, i + 2, high);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Binary search");
        self.emit_line("int64_t lucid_binary_search(int64_t* arr, int64_t len, int64_t target) {");
        self.indent_level += 1;
        self.emit_line("int64_t left = 0, right = len - 1;");
        self.emit_line("while (left <= right) {");
        self.indent_level += 1;
        self.emit_line("int64_t mid = left + (right - left) / 2;");
        self.emit_line("if (arr[mid] == target) return mid;");
        self.emit_line("if (arr[mid] < target) left = mid + 1;");
        self.emit_line("else right = mid - 1;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("return -1;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // Additional string formatting functions
        self.emit_line("// Left trim (remove leading whitespace)");
        self.emit_line("const char* lucid_string_lstrip(const char* str) {");
        self.indent_level += 1;
        self.emit_line("while (*str && isspace((unsigned char)*str)) str++;");
        self.emit_line("return str;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Right trim (remove trailing whitespace)");
        self.emit_line("const char* lucid_string_rstrip(const char* str) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("strcpy(result, str);");
        self.emit_line("char* end = result + strlen(result) - 1;");
        self.emit_line("while (end >= result && isspace((unsigned char)*end)) {");
        self.indent_level += 1;
        self.emit_line("*end = '\\0';");
        self.emit_line("end--;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Center string in field of given width");
        self.emit_line("const char* lucid_string_center(const char* str, int64_t width) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("int len = strlen(str);");
        self.emit_line("if (len >= width) { strcpy(result, str); return result; }");
        self.emit_line("int total_pad = width - len;");
        self.emit_line("int left_pad = total_pad / 2;");
        self.emit_line("int right_pad = total_pad - left_pad;");
        self.emit_line("int pos = 0;");
        self.emit_line("for (int i = 0; i < left_pad; i++) result[pos++] = ' ';");
        self.emit_line("strcpy(result + pos, str);");
        self.emit_line("pos += len;");
        self.emit_line("for (int i = 0; i < right_pad; i++) result[pos++] = ' ';");
        self.emit_line("result[pos] = '\\0';");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Left justify string in field of given width");
        self.emit_line("const char* lucid_string_ljust(const char* str, int64_t width) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("int len = strlen(str);");
        self.emit_line("strcpy(result, str);");
        self.emit_line("for (int i = len; i < width; i++) result[i] = ' ';");
        self.emit_line("result[width] = '\\0';");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Right justify string in field of given width");
        self.emit_line("const char* lucid_string_rjust(const char* str, int64_t width) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("int len = strlen(str);");
        self.emit_line("if (len >= width) { strcpy(result, str); return result; }");
        self.emit_line("int pad = width - len;");
        self.emit_line("for (int i = 0; i < pad; i++) result[i] = ' ';");
        self.emit_line("strcpy(result + pad, str);");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// String format - simple sprintf wrapper");
        self.emit_line("const char* lucid_string_format(const char* fmt, ...) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("va_list args;");
        self.emit_line("va_start(args, fmt);");
        self.emit_line("vsnprintf(result, sizeof(result), fmt, args);");
        self.emit_line("va_end(args);");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // Additional utility functions for character/digit checking
        self.emit_line("// Check if character is a digit");
        self.emit_line("bool lucid_is_digit(char c) {");
        self.indent_level += 1;
        self.emit_line("return c >= '0' && c <= '9';");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Check if character is alphabetic");
        self.emit_line("bool lucid_is_alpha(char c) {");
        self.indent_level += 1;
        self.emit_line("return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Check if character is whitespace");
        self.emit_line("bool lucid_is_space(char c) {");
        self.indent_level += 1;
        self.emit_line("return c == ' ' || c == '\\t' || c == '\\n' || c == '\\r';");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Parse string to integer");
        self.emit_line("int64_t lucid_parse_int(const char* str) {");
        self.indent_level += 1;
        self.emit_line("return strtoll(str, NULL, 10);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Parse string to double");
        self.emit_line("double lucid_parse_double(const char* str) {");
        self.indent_level += 1;
        self.emit_line("return strtod(str, NULL);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// String to integer with error handling");
        self.emit_line("bool lucid_try_parse_int(const char* str, int64_t* out) {");
        self.indent_level += 1;
        self.emit_line("char* endptr;");
        self.emit_line("*out = strtoll(str, &endptr, 10);");
        self.emit_line("return *endptr == '\\0';");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Get character at index in string");
        self.emit_line("char lucid_string_at(const char* str, int64_t index) {");
        self.indent_level += 1;
        self.emit_line("if (index < 0 || index >= (int64_t)strlen(str)) return '\\0';");
        self.emit_line("return str[index];");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// String length function");
        self.emit_line("int64_t lucid_string_length(const char* str) {");
        self.indent_level += 1;
        self.emit_line("return str ? (int64_t)strlen(str) : 0;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // Error context and stack traces
        self.emit_line("// Error context structure for stack traces");
        self.emit_line("struct ErrorContext {");
        self.indent_level += 1;
        self.emit_line("const char* function_name;");
        self.emit_line("const char* error_type;");
        self.emit_line("const char* error_message;");
        self.emit_line("int line_number;");
        self.indent_level -= 1;
        self.emit_line("};");
        self.emit_line("");

        self.emit_line("// Print error stack trace");
        self.emit_line("void lucid_print_error_trace(struct ErrorContext* contexts, int64_t count) {");
        self.indent_level += 1;
        self.emit_line("fprintf(stderr, \"Error stack trace:\\n\");");
        self.emit_line("for (int64_t i = 0; i < count; i++) {");
        self.indent_level += 1;
        self.emit_line("fprintf(stderr, \"  at %s: %s (%s)\\n\",");
        self.emit_line("    contexts[i].function_name,");
        self.emit_line("    contexts[i].error_message,");
        self.emit_line("    contexts[i].error_type);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Format error message with context");
        self.emit_line("const char* lucid_format_error(const char* func, const char* msg) {");
        self.indent_level += 1;
        self.emit_line("static char buffer[512];");
        self.emit_line("snprintf(buffer, sizeof(buffer), \"%s: %s\", func, msg);");
        self.emit_line("return buffer;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // Additional string methods
        self.emit_line("// String character at index");
        self.emit_line("char lucid_string_char_at(const char* str, int64_t index) {");
        self.indent_level += 1;
        self.emit_line("int len = strlen(str);");
        self.emit_line("if (index >= 0 && index < len) return str[index];");
        self.emit_line("return '\\0';");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// String reverse");
        self.emit_line("const char* lucid_string_reverse(const char* str) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("int len = strlen(str);");
        self.emit_line("for (int i = 0; i < len; i++) {");
        self.indent_level += 1;
        self.emit_line("result[i] = str[len - 1 - i];");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("result[len] = '\\0';");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// String count occurrences");
        self.emit_line("int64_t lucid_string_count(const char* str, const char* substr) {");
        self.indent_level += 1;
        self.emit_line("int64_t count = 0;");
        self.emit_line("const char* pos = str;");
        self.emit_line("while ((pos = strstr(pos, substr)) != NULL) {");
        self.indent_level += 1;
        self.emit_line("count++;");
        self.emit_line("pos += strlen(substr);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("return count;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// String pad left");
        self.emit_line("const char* lucid_string_pad_left(const char* str, int64_t width, char pad_char) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("int len = strlen(str);");
        self.emit_line("int pad_count = width > len ? width - len : 0;");
        self.emit_line("for (int i = 0; i < pad_count; i++) result[i] = pad_char;");
        self.emit_line("strcpy(result + pad_count, str);");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// String pad right");
        self.emit_line("const char* lucid_string_pad_right(const char* str, int64_t width, char pad_char) {");
        self.indent_level += 1;
        self.emit_line("static char result[4096];");
        self.emit_line("int len = strlen(str);");
        self.emit_line("strcpy(result, str);");
        self.emit_line("int pad_count = width > len ? width - len : 0;");
        self.emit_line("for (int i = 0; i < pad_count; i++) result[len + i] = pad_char;");
        self.emit_line("result[len + pad_count] = '\\0';");
        self.emit_line("return result;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Check if is numeric");
        self.emit_line("bool lucid_string_is_numeric(const char* str) {");
        self.indent_level += 1;
        self.emit_line("if (!str || !*str) return false;");
        self.emit_line("for (int i = 0; str[i]; i++) {");
        self.indent_level += 1;
        self.emit_line("if (!isdigit((unsigned char)str[i]) && str[i] != '-' && str[i] != '.') return false;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("return true;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        // Advanced math functions
        self.emit_line("// Math: sign function");
        self.emit_line("int64_t lucid_sign(int64_t x) {");
        self.indent_level += 1;
        self.emit_line("return (x > 0) - (x < 0);");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math: clamp value");
        self.emit_line("int64_t lucid_clamp(int64_t value, int64_t min, int64_t max) {");
        self.indent_level += 1;
        self.emit_line("if (value < min) return min;");
        self.emit_line("if (value > max) return max;");
        self.emit_line("return value;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math: gcd (greatest common divisor)");
        self.emit_line("int64_t lucid_gcd(int64_t a, int64_t b) {");
        self.indent_level += 1;
        self.emit_line("a = a < 0 ? -a : a;");
        self.emit_line("b = b < 0 ? -b : b;");
        self.emit_line("while (b != 0) {");
        self.indent_level += 1;
        self.emit_line("int64_t temp = b;");
        self.emit_line("b = a % b;");
        self.emit_line("a = temp;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("return a;");
        self.indent_level -= 1;
        self.emit_line("}");
        self.emit_line("");

        self.emit_line("// Math: lcm (least common multiple)");
        self.emit_line("int64_t lucid_lcm(int64_t a, int64_t b) {");
        self.indent_level += 1;
        self.emit_line("return (a / lucid_gcd(a, b)) * b;");
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
