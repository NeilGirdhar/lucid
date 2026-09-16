//! Stable ABI shared by generated native code and the Lucid runtime.

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeErrorCode {
    Ok = 0,
    DivisionByZero = 1,
    ArithmeticOverflow = 2,
    NegativeExponent = 3,
    InvalidOperation = 4,
    UnexpectedArgumentCount = 5,
    StepLimitExceeded = 6,
    RangeStepZero = 7,
}
impl NativeErrorCode {
    pub const fn as_raw(self) -> u32 {
        self as u32
    }
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Ok,
            1 => Self::DivisionByZero,
            2 => Self::ArithmeticOverflow,
            3 => Self::NegativeExponent,
            4 => Self::InvalidOperation,
            5 => Self::UnexpectedArgumentCount,
            6 => Self::StepLimitExceeded,
            7 => Self::RangeStepZero,
            _ => Self::InvalidOperation,
        }
    }
}
impl std::fmt::Display for NativeErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Ok => "ok",
            Self::DivisionByZero => "division by zero",
            Self::ArithmeticOverflow => "arithmetic overflow",
            Self::NegativeExponent => "negative exponent",
            Self::InvalidOperation => "invalid operation",
            Self::UnexpectedArgumentCount => "unexpected argument count",
            Self::StepLimitExceeded => "step limit exceeded",
            Self::RangeStepZero => "range step cannot be zero",
        })
    }
}
impl From<&lucid_cir::ExecuteError> for NativeErrorCode {
    fn from(error: &lucid_cir::ExecuteError) -> Self {
        match error {
            lucid_cir::ExecuteError::DivisionByZero => Self::DivisionByZero,
            lucid_cir::ExecuteError::ArithmeticOverflow => Self::ArithmeticOverflow,
            lucid_cir::ExecuteError::NegativeExponent => Self::NegativeExponent,
            lucid_cir::ExecuteError::UnexpectedArgumentCount { .. } => {
                Self::UnexpectedArgumentCount
            }
            lucid_cir::ExecuteError::StepLimitExceeded => Self::StepLimitExceeded,
            lucid_cir::ExecuteError::RangeStepZero => Self::RangeStepZero,
            _ => Self::InvalidOperation,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeResult {
    pub value: i64,
    pub error: NativeErrorCode,
}
impl NativeResult {
    pub const fn ok(value: i64) -> Self {
        Self {
            value,
            error: NativeErrorCode::Ok,
        }
    }
    pub const fn error(error: NativeErrorCode) -> Self {
        Self { value: 0, error }
    }
    pub const fn is_ok(self) -> bool {
        matches!(self.error, NativeErrorCode::Ok)
    }
    pub fn from_execute_error(error: &lucid_cir::ExecuteError) -> Self {
        Self::error(NativeErrorCode::from(error))
    }
    pub fn from_result(result: Result<i64, lucid_cir::ExecuteError>) -> Self {
        match result {
            Ok(value) => Self::ok(value),
            Err(error) => Self::from_execute_error(&error),
        }
    }
    pub const fn into_result(self) -> Result<i64, NativeErrorCode> {
        if self.is_ok() {
            Ok(self.value)
        } else {
            Err(self.error)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
}
pub fn checked_integer_binary(op: NativeBinaryOp, left: i64, right: i64) -> NativeResult {
    let result = match op {
        NativeBinaryOp::Add => left.checked_add(right),
        NativeBinaryOp::Sub => left.checked_sub(right),
        NativeBinaryOp::Mul => left.checked_mul(right),
        NativeBinaryOp::Div => {
            if right == 0 {
                return NativeResult::error(NativeErrorCode::DivisionByZero);
            }
            left.checked_div(right)
        }
        NativeBinaryOp::FloorDiv => {
            if right == 0 {
                return NativeResult::error(NativeErrorCode::DivisionByZero);
            }
            left.checked_div_euclid(right)
        }
        NativeBinaryOp::Mod => {
            if right == 0 {
                return NativeResult::error(NativeErrorCode::DivisionByZero);
            }
            match (left.checked_div(right), left.checked_rem(right)) {
                (Some(_), Some(rem)) if rem != 0 && (left < 0) != (right < 0) => {
                    rem.checked_add(right)
                }
                (_, Some(rem)) => Some(rem),
                _ => None,
            }
        }
        NativeBinaryOp::Pow => {
            let exponent = match u32::try_from(right) {
                Ok(value) => value,
                Err(_) => return NativeResult::error(NativeErrorCode::NegativeExponent),
            };
            left.checked_pow(exponent)
        }
    };
    result.map_or_else(
        || NativeResult::error(NativeErrorCode::ArithmeticOverflow),
        NativeResult::ok,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn lucid_checked_integer_binary(op: u32, left: i64, right: i64) -> NativeResult {
    let operation = match op {
        0 => NativeBinaryOp::Add,
        1 => NativeBinaryOp::Sub,
        2 => NativeBinaryOp::Mul,
        3 => NativeBinaryOp::Div,
        4 => NativeBinaryOp::FloorDiv,
        5 => NativeBinaryOp::Mod,
        6 => NativeBinaryOp::Pow,
        _ => return NativeResult::error(NativeErrorCode::InvalidOperation),
    };
    checked_integer_binary(operation, left, right)
}

/// # Safety
/// `out`, when non-null, must point to writable storage for one `NativeResult`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lucid_checked_integer_binary_into(
    op: u32,
    left: i64,
    right: i64,
    out: *mut NativeResult,
) -> bool {
    if out.is_null() {
        return false;
    }
    let result = lucid_checked_integer_binary(op, left, right);
    // SAFETY: checked non-null; caller supplies writable storage.
    unsafe { out.write(result) };
    result.is_ok()
}

pub fn execute_cir(function: &lucid_cir::Function) -> NativeResult {
    execute_cir_with_args(function, &[])
}

/// Execute a verified CIR function through the shared result ABI with
/// positional integer arguments. Keeping this conversion beside the
/// zero-argument bridge prevents interpreter/native callers from inventing
/// different handling for void returns and recoverable errors.
pub fn execute_cir_with_args(function: &lucid_cir::Function, args: &[i64]) -> NativeResult {
    execute_cir_with_args_and_step_limit(function, args, 1_000_000)
}

/// Execute parameterized CIR through the shared result ABI with an explicit
/// instruction-step budget. This keeps ABI callers from accidentally turning
/// a malformed or intentionally cyclic function into an unbounded process.
pub fn execute_cir_with_args_and_step_limit(
    function: &lucid_cir::Function,
    args: &[i64],
    step_limit: usize,
) -> NativeResult {
    match function.execute_with_args_and_step_limit(args, step_limit) {
        Ok(Some(value)) => NativeResult::ok(value),
        // A void CIR return is the successful zero-valued edge of the shared
        // result ABI.  It is not an invalid operation: mixed value/void CFGs
        // use the same convention when the void branch is selected.
        Ok(None) => NativeResult::ok(0),
        Err(error) => NativeResult::from_execute_error(&error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abi_contract_and_checked_operations() {
        assert_eq!(std::mem::size_of::<NativeResult>(), 16);
        assert_eq!(
            NativeErrorCode::from_raw(99),
            NativeErrorCode::InvalidOperation
        );
        assert_eq!(
            NativeErrorCode::DivisionByZero.to_string(),
            "division by zero"
        );
        assert_eq!(
            NativeErrorCode::from_raw(5),
            NativeErrorCode::UnexpectedArgumentCount
        );
        assert_eq!(
            NativeErrorCode::from_raw(6),
            NativeErrorCode::StepLimitExceeded
        );
        assert_eq!(NativeErrorCode::from_raw(7), NativeErrorCode::RangeStepZero);
        assert_eq!(
            NativeErrorCode::RangeStepZero.to_string(),
            "range step cannot be zero"
        );
        assert_eq!(
            checked_integer_binary(NativeBinaryOp::FloorDiv, -5, 2),
            NativeResult::ok(-3)
        );
        assert_eq!(
            checked_integer_binary(NativeBinaryOp::Mod, -5, 2),
            NativeResult::ok(1)
        );
        assert_eq!(
            checked_integer_binary(NativeBinaryOp::Div, 1, 0).error,
            NativeErrorCode::DivisionByZero
        );
        assert_eq!(
            checked_integer_binary(NativeBinaryOp::Pow, 2, -1).error,
            NativeErrorCode::NegativeExponent
        );
        let mut result = NativeResult::error(NativeErrorCode::InvalidOperation);
        assert!(unsafe { lucid_checked_integer_binary_into(0, 4, 5, &mut result) });
        assert_eq!(result, NativeResult::ok(9));
        assert!(!unsafe { lucid_checked_integer_binary_into(0, 4, 5, std::ptr::null_mut()) });
    }

    #[test]
    fn checked_abi_covers_successful_numeric_operations() {
        for (op, left, right, expected) in [
            (NativeBinaryOp::Add, 7, 5, 12),
            (NativeBinaryOp::Sub, 7, 5, 2),
            (NativeBinaryOp::Mul, 7, 5, 35),
            (NativeBinaryOp::Div, 7, 2, 3),
            (NativeBinaryOp::FloorDiv, -7, 2, -4),
            (NativeBinaryOp::Mod, -7, 2, 1),
            (NativeBinaryOp::Mod, 7, -2, -1),
            (NativeBinaryOp::Pow, 2, 8, 256),
        ] {
            assert_eq!(
                checked_integer_binary(op, left, right),
                NativeResult::ok(expected)
            );
        }
    }

    #[test]
    fn execute_cir_uses_the_same_result_contract() {
        let module = lucid_syntax::parse("answer = 6 * 7\n").unwrap();
        let function = lucid_cir::Function::from_module(&module).unwrap();
        assert_eq!(execute_cir(&function).into_result(), Ok(42));
        let module = lucid_syntax::parse("value = 6 * 7\n").unwrap();
        let function = lucid_cir::Function::from_module(&module).unwrap();
        assert_eq!(execute_cir_with_args(&function, &[]), NativeResult::ok(42));
        let module = lucid_syntax::parse("answer = 8 // 0\n").unwrap();
        let function = lucid_cir::Function::from_module(&module).unwrap();
        assert_eq!(
            execute_cir(&function).error,
            NativeErrorCode::DivisionByZero
        );
        let module = lucid_syntax::parse("pass\n").unwrap();
        let function = lucid_cir::Function::from_module(&module).unwrap();
        assert_eq!(execute_cir(&function), NativeResult::ok(0));
        let function = lucid_cir::Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    lucid_cir::Instruction::Param {
                        result: lucid_cir::ValueId(0),
                        index: 0,
                    },
                    lucid_cir::Instruction::ConstInt {
                        result: lucid_cir::ValueId(1),
                        value: 1,
                    },
                    lucid_cir::Instruction::Add {
                        result: lucid_cir::ValueId(2),
                        left: lucid_cir::ValueId(0),
                        right: lucid_cir::ValueId(1),
                    },
                ],
                terminator: lucid_cir::Terminator::Return(Some(lucid_cir::ValueId(2))),
            }],
        };
        assert_eq!(
            execute_cir_with_args(&function, &[41]),
            NativeResult::ok(42)
        );
        assert_eq!(
            execute_cir_with_args(&function, &[41, 99]).error,
            NativeErrorCode::UnexpectedArgumentCount
        );
        let cyclic = lucid_cir::Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![lucid_cir::Instruction::Param {
                    result: lucid_cir::ValueId(0),
                    index: 0,
                }],
                terminator: lucid_cir::Terminator::Jump(lucid_cir::BlockId(0)),
            }],
        };
        assert_eq!(
            execute_cir_with_args_and_step_limit(&cyclic, &[1], 2).error,
            NativeErrorCode::StepLimitExceeded
        );
    }

    #[test]
    fn result_conversion_preserves_success_and_recoverable_errors() {
        assert_eq!(NativeResult::from_result(Ok(42)).into_result(), Ok(42));
        assert_eq!(
            NativeResult::from_result(Err(lucid_cir::ExecuteError::DivisionByZero)).error,
            NativeErrorCode::DivisionByZero
        );
        assert_eq!(
            NativeResult::from_execute_error(&lucid_cir::ExecuteError::UnexpectedArgumentCount {
                expected: 1,
                provided: 2,
            })
            .error,
            NativeErrorCode::UnexpectedArgumentCount
        );
    }
}

/// Calling convention specification for method dispatch
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallingConvention {
    /// System V AMD64 (used on Linux/Unix)
    SystemVAmd64 = 1,
    /// Microsoft x64 (used on Windows)
    MicrosoftX64 = 2,
    /// ARM64 (used on Apple Silicon/ARM64 Linux)
    Arm64 = 3,
}

impl CallingConvention {
    /// Get the current platform's calling convention
    pub const fn current() -> Self {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            CallingConvention::SystemVAmd64
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            CallingConvention::MicrosoftX64
        }
        #[cfg(target_arch = "aarch64")]
        {
            CallingConvention::Arm64
        }
        #[cfg(not(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "windows", target_arch = "x86_64"),
            target_arch = "aarch64"
        )))]
        {
            // Default to System V for unknown platforms
            CallingConvention::SystemVAmd64
        }
    }
}

/// Memory layout specification for Lucid objects
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectLayout {
    /// Name of the class this layout describes
    pub class_name: String,
    /// Offset (in bytes) of each field from object base pointer
    pub field_offsets: Vec<(String, usize)>,
    /// Total object size in bytes (including header/metadata)
    pub total_size: usize,
    /// Alignment requirement in bytes
    pub alignment: usize,
}

impl ObjectLayout {
    pub fn new(class_name: String, alignment: usize) -> Self {
        Self {
            class_name,
            field_offsets: Vec::new(),
            total_size: 0,
            alignment,
        }
    }

    /// Add a field to the layout with its byte offset
    pub fn add_field(&mut self, name: String, offset: usize) {
        self.field_offsets.push((name, offset));
    }

    /// Get the offset of a field by name
    pub fn field_offset(&self, name: &str) -> Option<usize> {
        self.field_offsets
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, offset)| *offset)
    }
}

/// C interop type specifications
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CInteropType {
    /// Lucid int maps to C int64_t
    Int = 1,
    /// Lucid float maps to C double
    Float = 2,
    /// Lucid bool maps to C bool
    Bool = 3,
    /// Lucid str maps to C const char* (null-terminated UTF-8)
    String = 4,
    /// Lucid object maps to opaque C void*
    Object = 5,
    /// Lucid list maps to C LucidList* struct
    List = 6,
    /// Lucid dict maps to C LucidDict* struct
    Dict = 7,
}

/// ABI compatibility information
#[derive(Debug, Clone)]
pub struct AbiInfo {
    /// Calling convention used on this platform
    pub calling_convention: CallingConvention,
    /// Pointer size in bytes (8 on 64-bit, 4 on 32-bit)
    pub pointer_size: usize,
    /// Object layouts for all classes in the module
    pub layouts: Vec<ObjectLayout>,
}

impl AbiInfo {
    pub fn new(calling_convention: CallingConvention, pointer_size: usize) -> Self {
        Self {
            calling_convention,
            pointer_size,
            layouts: Vec::new(),
        }
    }

    /// Add a class layout to ABI info
    pub fn add_layout(&mut self, layout: ObjectLayout) {
        self.layouts.push(layout);
    }

    /// Get layout for a specific class
    pub fn layout(&self, class_name: &str) -> Option<&ObjectLayout> {
        self.layouts.iter().find(|l| l.class_name == class_name)
    }

    /// Generate layout for a class given field types
    pub fn generate_layout(&mut self, class_name: String, fields: Vec<(String, CInteropType)>) {
        let mut layout = ObjectLayout::new(class_name, self.pointer_size);
        let mut current_offset = 0;

        // Add object header (vtable pointer)
        layout.add_field("__vtable".to_string(), current_offset);
        current_offset += self.pointer_size;

        // Add fields with proper alignment
        for (field_name, field_type) in fields {
            let field_size = self.size_of_cinterop_type(field_type);
            current_offset = self.align_offset(current_offset, field_size);
            layout.add_field(field_name, current_offset);
            current_offset += field_size;
        }

        layout.total_size = self.align_offset(current_offset, layout.alignment);
        self.add_layout(layout);
    }

    /// Calculate size of C interop type
    fn size_of_cinterop_type(&self, ty: CInteropType) -> usize {
        match ty {
            CInteropType::Int => 8,                    // int64_t
            CInteropType::Float => 8,                  // double
            CInteropType::Bool => 1,                   // bool
            CInteropType::String => self.pointer_size, // const char*
            CInteropType::Object => self.pointer_size, // void*
            CInteropType::List => self.pointer_size,   // LucidList*
            CInteropType::Dict => self.pointer_size,   // LucidDict*
        }
    }

    /// Align offset to next boundary
    fn align_offset(&self, offset: usize, alignment: usize) -> usize {
        if alignment == 0 {
            return offset;
        }
        offset.div_ceil(alignment) * alignment
    }

    /// Validate that a field access is within bounds
    pub fn validate_field_access(&self, class_name: &str, field_name: &str, size: usize) -> bool {
        if let Some(layout) = self.layout(class_name)
            && let Some(offset) = layout.field_offset(field_name)
        {
            return offset + size <= layout.total_size;
        }
        false
    }
}

/// Stack frame layout for function calls
#[derive(Debug, Clone)]
pub struct StackFrame {
    /// Return address storage (8 bytes on 64-bit)
    pub return_addr_offset: usize,
    /// Previous frame pointer (8 bytes on 64-bit)
    pub prev_frame_ptr_offset: usize,
    /// Local variables offset
    pub locals_offset: usize,
    /// Total frame size
    pub frame_size: usize,
}

impl StackFrame {
    /// Create a new stack frame layout
    pub fn new(pointer_size: usize) -> Self {
        Self {
            return_addr_offset: 0,
            prev_frame_ptr_offset: pointer_size,
            locals_offset: pointer_size * 2,
            frame_size: pointer_size * 2,
        }
    }

    /// Add a local variable to the frame
    pub fn add_local(&mut self, size: usize) -> usize {
        let offset = self.frame_size;
        self.frame_size += size;
        offset
    }

    /// Align frame size to boundary (typically 16 bytes for System V AMD64)
    pub fn align_frame(&mut self, alignment: usize) {
        if alignment > 0 {
            self.frame_size = self.frame_size.div_ceil(alignment) * alignment;
        }
    }

    /// Get the total stack frame size including alignment
    pub fn total_size(&self) -> usize {
        self.frame_size
    }
}
