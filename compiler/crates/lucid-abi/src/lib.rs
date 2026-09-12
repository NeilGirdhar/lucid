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
