//! Compatibility module for the shared native ABI.
//!
//! The ABI lives in `lucid-abi` so the runtime and every backend consume one
//! definition. This module remains as a stable import path for codegen users.
pub use lucid_abi::{
    NativeBinaryOp, NativeErrorCode, NativeResult, checked_integer_binary, execute_cir,
    execute_cir_with_args, execute_cir_with_args_and_step_limit, lucid_checked_integer_binary,
    lucid_checked_integer_binary_into,
};
