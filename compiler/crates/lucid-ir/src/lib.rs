//! Lucid Intermediate Representation (IR) for native code generation
//!
//! This IR bridges the gap between Lucid's high-level AST and low-level machine code.
//! It provides a type-safe, machine-independent representation suitable for optimization
//! and code generation.

pub mod builder;
pub mod codegen;

pub use codegen::CCodegenBackend;

#[cfg(test)]
mod integration_test;

use std::collections::HashMap;

/// A Lucid IR module containing functions and type definitions
#[derive(Debug, Clone)]
pub struct IrModule {
    pub functions: Vec<IrFunction>,
    pub types: HashMap<String, IrType>,
    pub globals: Vec<IrGlobal>,
    pub classes: Vec<IrClass>,
    pub traits: Vec<IrTrait>,
    pub trait_impls: Vec<TraitImpl>,
    pub specializations: Vec<TypeSpecialization>,
    pub error_types: Vec<ErrorType>,
    pub iterator_traits: Vec<IteratorTrait>,  // Iterator support for collections
}

/// An IR function with control flow graph
#[derive(Debug, Clone)]
pub struct IrFunction {
    pub name: String,
    pub generic_params: Vec<GenericParam>,  // Generic type parameters [T, K, V, ...]
    pub where_clause: Option<WhereClause>,  // Optional where clause for advanced bounds
    pub params: Vec<IrParam>,
    pub return_type: IrType,
    pub blocks: Vec<IrBlock>,
    pub entry_block: usize,
    pub is_inline: bool,                    // Hint for inline optimization
    pub is_pure: bool,                      // Pure function (no side effects)
}

/// A basic block in the CFG
#[derive(Debug, Clone)]
pub struct IrBlock {
    pub id: usize,
    pub label: String,
    pub instructions: Vec<IrInstruction>,
    pub terminator: IrTerminator,
}

/// A function parameter
#[derive(Debug, Clone)]
pub struct IrParam {
    pub name: String,
    pub ty: IrType,
}

/// A struct field definition
#[derive(Debug, Clone)]
pub struct IrField {
    pub name: String,
    pub ty: IrType,
}

/// A class method definition
#[derive(Debug, Clone)]
pub struct IrMethod {
    pub name: String,
    pub is_factory: bool,      // factory methods (__init__, etc.)
    pub is_getter: bool,        // getter methods
    pub is_setter: bool,        // setter methods
    pub params: Vec<IrParam>,
    pub return_type: IrType,
    pub function_ref: String,   // Name of generated function in IR
}

/// IR types for code generation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IrType {
    /// 64-bit signed integer
    I64,
    /// 64-bit floating point
    F64,
    /// Boolean (1-bit, stored in 8-bit)
    Bool,
    /// Pointer to object (void*)
    Ptr,
    /// String (const char*)
    Str,
    /// List type
    List(Box<IrType>),
    /// Dictionary type
    Dict(Box<IrType>, Box<IrType>),
    /// Range type for iteration
    Range,
    /// Union type (for Result types and error handling)
    Union(Vec<Box<IrType>>),
    /// Generic type (e.g., T in List[T])
    Generic(String),
    /// Parameterized generic type (e.g., List[int])
    GenericInstance {
        name: String,
        type_args: Vec<Box<IrType>>,
    },
    /// Named type (class reference)
    Named(String),
    /// Never type (unreachable)
    Never,
    /// Void (no return value)
    Void,
}

impl IrType {
    /// Get the C type representation
    pub fn c_type(&self) -> &'static str {
        match self {
            IrType::I64 => "int64_t",
            IrType::F64 => "double",
            IrType::Bool => "bool",
            IrType::Ptr => "void*",
            IrType::Str => "const char*",
            IrType::List(_) => "LucidList*",
            IrType::Dict(_, _) => "LucidDict*",
            IrType::Range => "LucidRange*",
            IrType::Union(_) => "LucidResult*",
            IrType::Generic(_) => "void*",  // Type variable - use void* for now
            IrType::GenericInstance { name, .. } => {
                // Parameterized types map to their base type
                match name.as_str() {
                    "List" => "LucidList*",
                    "Dict" => "LucidDict*",
                    "Result" => "LucidResult*",
                    _ => "void*",
                }
            }
            IrType::Named(_) => "void*",
            IrType::Never => "void",
            IrType::Void => "void",
        }
    }
}

/// Global variable in IR
#[derive(Debug, Clone)]
pub struct IrGlobal {
    pub name: String,
    pub ty: IrType,
    pub mutable: bool,
}

/// Method dispatch information for class methods
#[derive(Debug, Clone)]
pub struct MethodDispatch {
    pub class_name: String,
    pub method_name: String,
    pub impl_function: String,
}

/// Trait definition in IR
#[derive(Debug, Clone)]
pub struct IrTrait {
    pub name: String,
    pub methods: Vec<TraitMethod>,
}

/// Trait method signature
#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
    pub params: Vec<IrParam>,
    pub return_type: IrType,
}

/// Associated type in a trait (e.g., Iterator::Item)
#[derive(Debug, Clone)]
pub struct AssociatedType {
    pub name: String,                 // e.g., "Item"
    pub ty: IrType,                   // The concrete type
}

/// Iterator trait support
#[derive(Debug, Clone)]
pub struct IteratorTrait {
    pub collection_type: String,      // "List" or "Dict"
    pub item_type: IrType,           // Element type being iterated
    pub key_type: Option<IrType>,    // For Dict: key type
    pub has_keys_method: bool,        // Dict has keys() method
    pub has_values_method: bool,      // Dict has values() method
    pub has_items_method: bool,       // Dict has items() method
}

/// Error context for stack traces
#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub function_name: String,        // Where error occurred
    pub error_type: String,          // Error type/enum variant
    pub error_message: String,       // Human-readable message
    pub line_number: usize,          // Source line (if available)
}

/// Error stack for tracking context through function calls
#[derive(Debug, Clone)]
pub struct ErrorStack {
    pub contexts: Vec<ErrorContext>,  // Stack of error contexts
}

/// Trait bound for generic type parameters
#[derive(Debug, Clone)]
pub struct TraitBound {
    pub type_param: String,           // e.g., "T" in T: Clone
    pub trait_name: String,           // e.g., "Clone"
}

/// Variance for generic type parameters
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variance {
    Covariant,      // +K: can pass subtype in output positions
    Contravariant,  // -K: can pass supertype in input positions
    Invariant,      // =K: exact type required
}

/// Generic type parameter with optional bounds
#[derive(Debug, Clone)]
pub struct GenericParam {
    pub name: String,                 // e.g., "T", "K", "V"
    pub variance: Variance,           // Covariant (+), Contravariant (-), or Invariant (=)
    pub bounds: Vec<TraitBound>,      // e.g., [T: Clone, T: Copy]
}

/// Where clause predicate for advanced trait bounds
#[derive(Debug, Clone)]
pub struct WhereClausePredicate {
    pub type_name: String,            // Type being constrained (e.g., "T" or "List[T]")
    pub required_traits: Vec<String>, // Traits that must be implemented
}

/// Where clause for generic functions/types
#[derive(Debug, Clone)]
pub struct WhereClause {
    pub predicates: Vec<WhereClausePredicate>,  // e.g., [T: Clone, U: Default]
}

/// Generic type instantiation (monomorphization)
#[derive(Debug, Clone)]
pub struct TypeSpecialization {
    pub generic_name: String,           // "List" or "Dict"
    pub type_args: Vec<IrType>,         // [IrType::I64] for List[int]
    pub specialized_name: String,       // "List__i64__" or "Dict__str__i64__"
}

/// Error type definition for error hierarchies
#[derive(Debug, Clone)]
pub struct ErrorType {
    pub name: String,
    pub variants: Vec<ErrorVariant>,
}

/// Error variant (case) in an error enum
#[derive(Debug, Clone)]
pub struct ErrorVariant {
    pub name: String,
    pub code: i64,
}

/// Trait implementation
#[derive(Debug, Clone)]
pub struct TraitImpl {
    pub trait_name: String,
    pub impl_type: String,
    pub methods: Vec<MethodImpl>,
}

/// Method implementation for a trait
#[derive(Debug, Clone)]
pub struct MethodImpl {
    pub method_name: String,
    pub impl_function: String,
}

/// Pattern for match statement
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Wildcard pattern (_)
    Wildcard,
    /// Literal pattern (integer, string, boolean)
    Literal(IrValue),
    /// Enum variant pattern (e.g., Result::Ok, Result::Err)
    Variant(String, Vec<String>),  // variant_name, captured_bindings
    /// Tuple pattern (for multiple values)
    Tuple(Vec<Pattern>),
}

/// Pattern match arm
#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub target_block: usize,  // Block to execute if pattern matches
}

/// Pattern binding for match arms - extract values from patterns
#[derive(Debug, Clone)]
pub struct PatternBinding {
    pub binding_name: String,         // Variable name to bind to
    pub field_path: Vec<String>,      // Path to field (e.g., ["Ok", "value"])
}

/// Match arm with full pattern and bindings
#[derive(Debug, Clone)]
pub struct MatchArmWithBindings {
    pub pattern: Pattern,
    pub bindings: Vec<PatternBinding>,  // Values extracted from pattern
    pub target_block: usize,
}

/// Pattern match with exhaustiveness tracking
#[derive(Debug, Clone)]
pub struct MatchExpr {
    pub scrutinee: String,           // Value being matched
    pub arms: Vec<MatchArm>,         // All match arms
    pub exhaustive: bool,            // Whether all cases are covered
    pub covered_patterns: Vec<String>, // Which patterns are covered
}

/// Class definition in IR
#[derive(Debug, Clone)]
pub struct IrClass {
    pub name: String,
    pub generic_params: Vec<GenericParam>,  // Generic type parameters
    pub parent: Option<String>,  // Single inheritance: parent class name
    pub fields: Vec<IrField>,
    pub methods: Vec<MethodDispatch>,
}

/// IR instructions (SSA form)
#[derive(Debug, Clone)]
pub enum IrInstruction {
    /// Assign a value to a variable
    Assign {
        dest: String,
        value: IrValue,
    },
    /// Binary operation
    BinOp {
        dest: String,
        op: IrBinOp,
        left: IrValue,
        right: IrValue,
    },
    /// Unary operation
    UnaryOp {
        dest: String,
        op: IrUnaryOp,
        operand: IrValue,
    },
    /// Function call
    Call {
        dest: Option<String>,
        func: String,
        args: Vec<IrValue>,
    },
    /// Method call (receiver.method(args))
    MethodCall {
        dest: Option<String>,
        receiver: IrValue,
        method: String,
        args: Vec<IrValue>,
    },
    /// Load from memory
    Load {
        dest: String,
        addr: IrValue,
    },
    /// Store to memory
    Store {
        addr: IrValue,
        value: IrValue,
    },
    /// Type cast
    Cast {
        dest: String,
        from: IrValue,
        to_type: IrType,
    },
    /// Memory allocation (malloc)
    Malloc {
        dest: String,
        size: IrValue,
    },
    /// Memory deallocation (free)
    Free {
        addr: IrValue,
    },
    /// Field access read (obj.field)
    FieldRead {
        dest: String,
        object: IrValue,
        field: String,
        object_type: String,
    },
    /// Field access write (obj.field = value)
    FieldWrite {
        object: IrValue,
        field: String,
        value: IrValue,
        object_type: String,
    },
    /// New instance with field initialization
    /// Allocates memory for a class and initializes fields from arguments
    NewInstance {
        dest: String,
        class_name: String,
        field_values: Vec<(String, IrValue)>,  // field name -> value pairs
    },
    /// Check if a Result is an error (for ? operator)
    ResultCheck {
        result: IrValue,
        error_block: usize,
        success_block: usize,
    },
    /// Dictionary access: dict[key] -> value
    DictAccess {
        dest: String,
        dict: IrValue,
        key: IrValue,
    },
    /// File I/O: open file
    FileOpen {
        dest: String,
        path: IrValue,
        mode: String,  // "r", "w", "a"
    },
    /// File I/O: write to file
    FileWrite {
        file: IrValue,
        content: IrValue,
    },
    /// File I/O: read from file
    FileRead {
        dest: String,
        file: IrValue,
    },
    /// File I/O: close file
    FileClose {
        file: IrValue,
    },
}

/// Values in IR (operands)
#[derive(Debug, Clone)]
pub enum IrValue {
    /// Immediate integer constant
    Int(i64),
    /// Immediate float constant
    Float(f64),
    /// Boolean constant
    Bool(bool),
    /// String constant
    String(String),
    /// Variable reference
    Var(String),
    /// Global variable reference
    Global(String),
    /// Null pointer
    Null,
}

/// Binary operations in IR
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
}

/// Unary operations in IR
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrUnaryOp {
    Neg,
    Not,
    BitNot,
}

/// Block terminators (control flow)
#[derive(Debug, Clone)]
pub enum IrTerminator {
    /// Unconditional jump
    Jump { target: usize },
    /// Conditional branch
    Branch {
        condition: IrValue,
        then_block: usize,
        else_block: usize,
    },
    /// Return from function
    Return { value: Option<IrValue> },
    /// Unreachable (for exhausted match arms)
    Unreachable,
}

impl IrModule {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            types: HashMap::new(),
            globals: Vec::new(),
            classes: Vec::new(),
            traits: Vec::new(),
            trait_impls: Vec::new(),
            specializations: Vec::new(),
            error_types: Vec::new(),
            iterator_traits: Vec::new(),
        }
    }

    pub fn add_function(&mut self, func: IrFunction) {
        self.functions.push(func);
    }

    pub fn add_type(&mut self, name: String, ty: IrType) {
        self.types.insert(name, ty);
    }

    pub fn add_global(&mut self, global: IrGlobal) {
        self.globals.push(global);
    }

    pub fn add_class(&mut self, class: IrClass) {
        self.classes.push(class);
    }

    pub fn get_class(&self, name: &str) -> Option<&IrClass> {
        self.classes.iter().find(|c| c.name == name)
    }

    pub fn specialize_type(&mut self, generic_name: &str, type_args: Vec<IrType>) -> String {
        // Generate specialized name: List__i64__ or Dict__str__i64__
        let mut specialized = generic_name.to_string();
        specialized.push_str("__");
        for (i, ty) in type_args.iter().enumerate() {
            if i > 0 { specialized.push_str("__"); }  // Double underscore between types
            specialized.push_str(&self.type_to_name(ty));
        }
        specialized.push_str("__");

        // Check if already specialized
        for spec in &self.specializations {
            if spec.specialized_name == specialized {
                return specialized;
            }
        }

        // Record new specialization
        self.specializations.push(TypeSpecialization {
            generic_name: generic_name.to_string(),
            type_args: type_args.clone(),
            specialized_name: specialized.clone(),
        });

        specialized
    }

    fn type_to_name(&self, ty: &IrType) -> String {
        match ty {
            IrType::I64 => "i64".to_string(),
            IrType::F64 => "f64".to_string(),
            IrType::Bool => "bool".to_string(),
            IrType::Str => "str".to_string(),
            IrType::Ptr => "ptr".to_string(),
            IrType::Named(n) => n.clone(),
            _ => "unknown".to_string(),
        }
    }

    /// Check if a type implements a specific trait
    pub fn type_implements_trait(&self, type_name: &str, trait_name: &str) -> bool {
        for impl_block in &self.trait_impls {
            if impl_block.impl_type == type_name && impl_block.trait_name == trait_name {
                return true;
            }
        }
        false
    }

    /// Get trait bounds for a generic parameter
    pub fn get_generic_param_bounds(&self, param_name: &str, functions: &[IrFunction]) -> Vec<String> {
        for func in functions {
            for param in &func.generic_params {
                if param.name == param_name {
                    return param.bounds.iter().map(|b| b.trait_name.clone()).collect();
                }
            }
        }
        Vec::new()
    }

    /// Validate that a type satisfies trait bounds
    pub fn type_satisfies_bounds(&self, type_name: &str, bounds: &[String]) -> bool {
        for bound in bounds {
            if !self.type_implements_trait(type_name, bound) {
                return false;
            }
        }
        true
    }

    /// Check if a type parameter's variance is sound in a given position
    /// position_is_output: true if parameter appears in output (return type, result)
    /// position_is_input: true if parameter appears in input (parameter, argument)
    pub fn check_variance_soundness(&self, variance: Variance, position_is_output: bool, position_is_input: bool) -> bool {
        match variance {
            Variance::Covariant => {
                // +K: produces values, cannot consume values
                // Valid ONLY if not used in input positions
                !position_is_input
            }
            Variance::Contravariant => {
                // -K: consumes values, cannot produce values
                // Valid ONLY if not used in output positions
                !position_is_output
            }
            Variance::Invariant => {
                // =K: can be anywhere, no restrictions
                true
            }
        }
    }

    /// Check if a generic substitution respects variance rules
    /// Returns true if declaring a subtype satisfies the type parameter's variance
    pub fn type_is_valid_substitution(&self, param_variance: Variance, actual_type: &str, expected_type: &str) -> bool {
        if actual_type == expected_type {
            return true;
        }

        match param_variance {
            Variance::Covariant => {
                // +K: actual type can be a subtype of expected
                // In this simple implementation, we treat direct equality as valid
                // A full implementation would check inheritance hierarchy
                actual_type == expected_type
            }
            Variance::Contravariant => {
                // -K: actual type can be a supertype of expected
                // For simplicity, require exact match
                actual_type == expected_type
            }
            Variance::Invariant => {
                // =K: must be exact type
                actual_type == expected_type
            }
        }
    }

    /// Check if a match expression on a Result type is exhaustive
    pub fn is_result_match_exhaustive(&self, arms: &[MatchArm]) -> bool {
        let mut has_ok = false;
        let mut has_err = false;
        let mut has_wildcard = false;

        for arm in arms {
            match &arm.pattern {
                Pattern::Variant(name, _) if name == "Ok" => has_ok = true,
                Pattern::Variant(name, _) if name == "Err" => has_err = true,
                Pattern::Wildcard => has_wildcard = true,
                _ => {}
            }
        }

        has_wildcard || (has_ok && has_err)
    }

    /// Check if a match expression on an error type is exhaustive
    pub fn is_error_match_exhaustive(&self, error_type_name: &str, arms: &[MatchArm]) -> bool {
        // Find the error type definition
        for error_type in &self.error_types {
            if error_type.name == error_type_name {
                let mut has_wildcard = false;
                let mut covered_variants = Vec::new();

                for arm in arms {
                    match &arm.pattern {
                        Pattern::Variant(name, _) => covered_variants.push(name.clone()),
                        Pattern::Wildcard => has_wildcard = true,
                        _ => {}
                    }
                }

                if has_wildcard {
                    return true;
                }

                // Check if all variants are covered
                for variant in &error_type.variants {
                    if !covered_variants.contains(&variant.name) {
                        return false;
                    }
                }

                return true;
            }
        }

        false
    }

    /// Get all variants of an enum type
    pub fn get_enum_variants(&self, enum_name: &str) -> Vec<String> {
        for error_type in &self.error_types {
            if error_type.name == enum_name {
                return error_type.variants.iter().map(|v| v.name.clone()).collect();
            }
        }
        Vec::new()
    }

    /// Check if pattern match covers all required cases
    pub fn validate_match_exhaustiveness(&self, type_name: &str, arms: &[MatchArm]) -> Result<(), String> {
        if type_name == "Result" {
            if !self.is_result_match_exhaustive(arms) {
                return Err("Match on Result must cover Ok and Err or use wildcard".to_string());
            }
        } else {
            // Check for error type
            if !self.is_error_match_exhaustive(type_name, arms) {
                let variants = self.get_enum_variants(type_name);
                return Err(format!("Match on {} must cover all variants: {:?}", type_name, variants));
            }
        }
        Ok(())
    }

    /// Register an iterator trait for a collection type
    pub fn register_iterator(&mut self, collection_type: String, item_type: IrType, key_type: Option<IrType>) -> IteratorTrait {
        let has_keys = key_type.is_some();
        let iterator = IteratorTrait {
            collection_type,
            item_type,
            key_type,
            has_keys_method: has_keys,
            has_values_method: has_keys,
            has_items_method: has_keys,
        };
        self.iterator_traits.push(iterator.clone());
        iterator
    }

    /// Get iterator for a collection type
    pub fn get_iterator(&self, collection_type: &str) -> Option<&IteratorTrait> {
        self.iterator_traits.iter().find(|it| it.collection_type == collection_type)
    }

    /// Check if a type supports iteration
    pub fn is_iterable(&self, type_name: &str) -> bool {
        self.iterator_traits.iter().any(|it| it.collection_type == type_name) ||
        type_name == "List" || type_name == "Dict" || type_name == "Range" ||
        type_name == "String"
    }

    /// Get the item type for iteration
    pub fn get_iteration_type(&self, type_name: &str) -> Option<IrType> {
        match type_name {
            "List" => Some(IrType::I64), // Default for now
            "Range" => Some(IrType::I64),
            "String" => Some(IrType::Str),
            _ => self.get_iterator(type_name).map(|it| it.item_type.clone()),
        }
    }

    /// Validate that a type satisfies all where clause predicates
    pub fn validate_where_clause(&self, where_clause: &WhereClause, type_name: &str) -> Result<(), String> {
        for predicate in &where_clause.predicates {
            if predicate.type_name == type_name {
                if !self.type_satisfies_bounds(type_name, &predicate.required_traits) {
                    return Err(format!(
                        "Type {} does not satisfy where clause constraints: {:?}",
                        type_name, predicate.required_traits
                    ));
                }
            }
        }
        Ok(())
    }

    /// Create a where clause for function
    pub fn create_where_clause(predicates: Vec<WhereClausePredicate>) -> WhereClause {
        WhereClause { predicates }
    }

    /// Add a predicate to a where clause
    pub fn add_where_predicate(where_clause: &mut WhereClause, type_name: String, traits: Vec<String>) {
        where_clause.predicates.push(WhereClausePredicate {
            type_name,
            required_traits: traits,
        });
    }

    /// Create pattern bindings for a match arm
    pub fn create_pattern_bindings(pattern: &Pattern) -> Vec<PatternBinding> {
        match pattern {
            Pattern::Variant(variant_name, captured) => {
                captured.iter().enumerate().map(|(i, name)| {
                    PatternBinding {
                        binding_name: name.clone(),
                        field_path: vec![variant_name.clone(), format!("field_{}", i)],
                    }
                }).collect()
            }
            Pattern::Tuple(patterns) => {
                let mut bindings = Vec::new();
                for (i, pat) in patterns.iter().enumerate() {
                    let sub_bindings = Self::create_pattern_bindings(pat);
                    for mut binding in sub_bindings {
                        binding.field_path.insert(0, format!("tuple_{}", i));
                        bindings.push(binding);
                    }
                }
                bindings
            }
            _ => Vec::new(),
        }
    }

    /// Create match arm with full pattern bindings
    pub fn create_match_arm_with_bindings(pattern: Pattern, target_block: usize) -> MatchArmWithBindings {
        let bindings = Self::create_pattern_bindings(&pattern);
        MatchArmWithBindings {
            pattern,
            bindings,
            target_block,
        }
    }

    /// Create error context for stack traces
    pub fn create_error_context(
        function_name: String,
        error_type: String,
        error_message: String,
    ) -> ErrorContext {
        ErrorContext {
            function_name,
            error_type,
            error_message,
            line_number: 0, // Set by codegen if source locations available
        }
    }

    /// Create new error stack
    pub fn create_error_stack() -> ErrorStack {
        ErrorStack {
            contexts: Vec::new(),
        }
    }

    /// Push error context onto stack
    pub fn push_error_context(stack: &mut ErrorStack, context: ErrorContext) {
        stack.contexts.push(context);
    }

    /// Get full error stack trace
    pub fn get_error_trace(stack: &ErrorStack) -> Vec<String> {
        stack.contexts.iter().enumerate().map(|(i, ctx)| {
            format!(
                "  {} in {}: {} ({})",
                i, ctx.function_name, ctx.error_message, ctx.error_type
            )
        }).collect()
    }

    /// Mark function for inlining optimization
    pub fn mark_for_inlining(func: &mut IrFunction) {
        func.is_inline = true;
    }

    /// Mark function as pure (no side effects)
    pub fn mark_as_pure(func: &mut IrFunction) {
        func.is_pure = true;
    }

    /// Count instructions in function for complexity analysis
    pub fn function_complexity(func: &IrFunction) -> usize {
        func.blocks.iter().map(|b| b.instructions.len()).sum()
    }

    /// Check if function is eligible for inlining based on size
    pub fn is_inlinable(func: &IrFunction, max_complexity: usize) -> bool {
        Self::function_complexity(func) <= max_complexity
    }
}

impl IrFunction {
    pub fn new(
        name: String,
        params: Vec<IrParam>,
        return_type: IrType,
    ) -> Self {
        Self {
            name,
            generic_params: Vec::new(),
            where_clause: None,
            params,
            return_type,
            blocks: vec![IrBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: IrTerminator::Unreachable,
            }],
            entry_block: 0,
            is_inline: false,
            is_pure: false,
        }
    }

    pub fn new_block(&mut self, label: String) -> usize {
        let id = self.blocks.len();
        self.blocks.push(IrBlock {
            id,
            label,
            instructions: Vec::new(),
            terminator: IrTerminator::Unreachable,
        });
        id
    }

    pub fn current_block_mut(&mut self) -> &mut IrBlock {
        let last_idx = self.blocks.len() - 1;
        &mut self.blocks[last_idx]
    }
}

impl IrBlock {
    pub fn add_instruction(&mut self, instr: IrInstruction) {
        self.instructions.push(instr);
    }

    pub fn set_terminator(&mut self, terminator: IrTerminator) {
        self.terminator = terminator;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ir_type_c_mapping() {
        assert_eq!(IrType::I64.c_type(), "int64_t");
        assert_eq!(IrType::F64.c_type(), "double");
        assert_eq!(IrType::Bool.c_type(), "bool");
        assert_eq!(IrType::Ptr.c_type(), "void*");
    }

    #[test]
    fn test_ir_function_creation() {
        let func = IrFunction::new(
            "test".to_string(),
            vec![IrParam {
                name: "x".to_string(),
                ty: IrType::I64,
            }],
            IrType::I64,
        );
        assert_eq!(func.name, "test");
        assert_eq!(func.blocks.len(), 1);
        assert_eq!(func.entry_block, 0);
    }

    #[test]
    fn test_ir_module_construction() {
        let mut module = IrModule::new();
        let func = IrFunction::new(
            "main".to_string(),
            Vec::new(),
            IrType::I64,
        );
        module.add_function(func);
        assert_eq!(module.functions.len(), 1);
    }
}
