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
}

/// An IR function with control flow graph
#[derive(Debug, Clone)]
pub struct IrFunction {
    pub name: String,
    pub params: Vec<IrParam>,
    pub return_type: IrType,
    pub blocks: Vec<IrBlock>,
    pub entry_block: usize,
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

/// Class definition in IR
#[derive(Debug, Clone)]
pub struct IrClass {
    pub name: String,
    pub fields: Vec<(String, IrType)>,
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
}

impl IrFunction {
    pub fn new(
        name: String,
        params: Vec<IrParam>,
        return_type: IrType,
    ) -> Self {
        Self {
            name,
            params,
            return_type,
            blocks: vec![IrBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: IrTerminator::Unreachable,
            }],
            entry_block: 0,
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
