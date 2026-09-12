//! Cranelift lowering for the backend-independent CIR.
//!
//! This module intentionally exposes a small, strict slice first.  It proves
//! that native code consumes verified CIR rather than walking the source AST;
//! unsupported instructions return an error instead of silently changing
//! semantics.

use cranelift_codegen::ir::{AbiParam, InstBuilder, UserFuncName, types};
use cranelift_entity::EntityRef;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module, default_libcall_names};
use lucid_cir::{Function, Instruction, Terminator, ValueId};

type PhiParam = (
    ValueId,
    cranelift_codegen::ir::Value,
    Vec<(lucid_cir::BlockId, ValueId)>,
);
type PhiParamMap = std::collections::HashMap<lucid_cir::BlockId, Vec<PhiParam>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CraneliftError {
    InvalidCIR(String),
    UnsupportedInstruction(String),
    UnsupportedControlFlow,
    Backend(String),
}

/// An owned JIT module and its exported entry point. Keeping the module alive
/// keeps the machine code mapped; dropping this value releases it normally.
pub struct CompiledFunction {
    module: JITModule,
    id: FuncId,
    parameter_count: usize,
    returns_value: bool,
    result_abi: bool,
}

impl CompiledFunction {
    /// Whether the compiled entry point returns an integer value.
    pub fn returns_value(&self) -> bool {
        self.returns_value
    }

    /// Number of positional integer arguments accepted by this entry point.
    /// Callers can inspect this before crossing the unsafe invocation methods.
    pub fn parameter_count(&self) -> usize {
        self.parameter_count
    }

    /// Invoke a zero-argument integer function produced from CIR.
    ///
    /// The CIR ABI currently exposes only this small entry signature. The
    /// method is unsafe because the generated code is outside Rust's memory
    /// safety model, even though the compiler validates its input first.
    ///
    /// # Safety
    ///
    /// The function must have been produced by [`compile_integer_function`]
    /// and must not be called concurrently with destruction of this handle.
    pub unsafe fn call(&self) -> i64 {
        assert!(
            self.parameter_count == 0 && self.returns_value && !self.result_abi,
            "called legacy integer ABI on an incompatible function"
        );
        let pointer = self.module.get_finalized_function(self.id);
        let function: unsafe extern "C" fn() -> i64 = unsafe { std::mem::transmute(pointer) };
        unsafe { function() }
    }

    /// Invoke a zero-result/value integer function with positional arguments.
    /// The generated entry point uses the C ABI and accepts one `i64` per CIR
    /// `Param` index.
    ///
    /// # Safety
    ///
    /// The argument count must match the compiled function's parameter count,
    /// and the handle must remain alive and not be concurrently destroyed
    /// while generated code executes.
    pub unsafe fn call_with_args(&self, args: &[i64]) -> i64 {
        assert!(
            self.parameter_count == args.len() && self.returns_value && !self.result_abi,
            "called integer ABI with an incompatible argument list"
        );
        let pointer = self.module.get_finalized_function(self.id);
        match args.len() {
            0 => unsafe {
                std::mem::transmute::<*const u8, unsafe extern "C" fn() -> i64>(pointer)()
            },
            1 => unsafe {
                std::mem::transmute::<*const u8, unsafe extern "C" fn(i64) -> i64>(pointer)(args[0])
            },
            2 => unsafe {
                std::mem::transmute::<*const u8, unsafe extern "C" fn(i64, i64) -> i64>(pointer)(
                    args[0], args[1],
                )
            },
            3 => unsafe {
                std::mem::transmute::<*const u8, unsafe extern "C" fn(i64, i64, i64) -> i64>(
                    pointer,
                )(args[0], args[1], args[2])
            },
            4 => unsafe {
                std::mem::transmute::<*const u8, unsafe extern "C" fn(i64, i64, i64, i64) -> i64>(
                    pointer,
                )(args[0], args[1], args[2], args[3])
            },
            5 => unsafe {
                std::mem::transmute::<*const u8, unsafe extern "C" fn(i64, i64, i64, i64, i64) -> i64>(
                    pointer,
                )(args[0], args[1], args[2], args[3], args[4])
            },
            6 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64,
                >(pointer)(args[0], args[1], args[2], args[3], args[4], args[5])
            },
            7 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64) -> i64,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6],
                )
            },
            8 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64) -> i64,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                )
            },
            9 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64, i64) -> i64,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], args[8],
                )
            },
            10 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) -> i64,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9],
                )
            },
            11 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> i64,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10],
                )
            },
            12 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> i64,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11],
                )
            },
            13 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> i64,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12],
                )
            },
            14 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> i64,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12], args[13],
                )
            },
            15 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> i64,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12], args[13], args[14],
                )
            },
            16 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> i64,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12], args[13], args[14], args[15],
                )
            },
            _ => {
                debug_assert!(
                    args.len() <= 16,
                    "call_with_args currently supports at most sixteen arguments"
                );
                0
            }
        }
    }

    /// Checked wrapper for [`Self::call_with_args`].  FFI callers can use this
    /// API to turn an invalid signature or argument vector into a recoverable
    /// ABI error before reaching the unsafe adapter.
    ///
    /// # Safety
    ///
    /// The generated handle must remain alive and must not be concurrently
    /// destroyed while generated code executes.
    pub unsafe fn try_call_with_args(
        &self,
        args: &[i64],
    ) -> Result<i64, crate::native_abi::NativeErrorCode> {
        if self.parameter_count != args.len() {
            return Err(crate::native_abi::NativeErrorCode::UnexpectedArgumentCount);
        }
        if args.len() > 16 {
            return Err(crate::native_abi::NativeErrorCode::InvalidOperation);
        }
        if !self.returns_value || self.result_abi {
            return Err(crate::native_abi::NativeErrorCode::InvalidOperation);
        }
        Ok(unsafe { self.call_with_args(args) })
    }

    /// Invoke the function and adapt its current integer ABI to the shared
    /// value/error result contract.
    ///
    /// # Safety
    ///
    /// The same conditions as [`Self::call`] apply.
    pub unsafe fn call_result(&self) -> crate::native_abi::NativeResult {
        // This method returns the shared recoverable ABI, so an argument
        // mismatch must remain a value-level error even when the caller came
        // through an FFI boundary.  The older integer/void helpers cannot
        // represent this condition and therefore retain their documented
        // assertions; NativeResult can represent it without panicking.
        if self.parameter_count != 0 {
            return crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::UnexpectedArgumentCount,
            );
        }
        if self.result_abi {
            let pointer = self.module.get_finalized_function(self.id);
            let function: unsafe extern "C" fn() -> crate::native_abi::NativeResult =
                unsafe { std::mem::transmute(pointer) };
            return unsafe { function() };
        }
        if !self.returns_value {
            // The shared value/result boundary represents a void return as a
            // successful zero, just like the CIR and exported ABI bridges.
            return crate::native_abi::NativeResult::ok(0);
        }
        let mut result = crate::native_abi::NativeResult::error(
            crate::native_abi::NativeErrorCode::InvalidOperation,
        );
        // SAFETY: `result` is valid caller-owned storage for the duration of
        // this call, and this handle was produced by the integer compiler.
        let wrote = unsafe { self.call_result_into(&mut result) };
        debug_assert!(wrote);
        result
    }

    /// Invoke a parameterized function through the shared recoverable
    /// `NativeResult` ABI. Dedicated result-ABI entry points are called
    /// directly; ordinary value and void entry points are adapted at this
    /// boundary. The argument count must match the compiled CIR signature.
    ///
    /// # Safety
    ///
    /// The generated handle must remain alive and must not be concurrently
    /// destroyed while generated code executes.
    pub unsafe fn call_result_with_args(&self, args: &[i64]) -> crate::native_abi::NativeResult {
        // Keep argument-count failures in the shared recoverable ABI.  This
        // entry point is commonly reached from FFI, where panicking across
        // the boundary is undefined behaviour and an assertion would turn a
        // normal caller error into process termination.
        if self.parameter_count != args.len() {
            return crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::UnexpectedArgumentCount,
            );
        }
        if args.len() > 16 {
            return crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::InvalidOperation,
            );
        }
        if !self.result_abi {
            if self.returns_value {
                return crate::native_abi::NativeResult::ok(unsafe { self.call_with_args(args) });
            }
            unsafe { self.call_void_with_args(args) };
            return crate::native_abi::NativeResult::ok(0);
        }
        let pointer = self.module.get_finalized_function(self.id);
        match args.len() {
            0 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn() -> crate::native_abi::NativeResult,
                >(pointer)()
            },
            1 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64) -> crate::native_abi::NativeResult,
                >(pointer)(args[0])
            },
            2 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64) -> crate::native_abi::NativeResult,
                >(pointer)(args[0], args[1])
            },
            3 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64, i64) -> crate::native_abi::NativeResult,
                >(pointer)(args[0], args[1], args[2])
            },
            4 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64, i64, i64) -> crate::native_abi::NativeResult,
                >(pointer)(args[0], args[1], args[2], args[3])
            },
            5 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> crate::native_abi::NativeResult,
                >(pointer)(args[0], args[1], args[2], args[3], args[4])
            },
            6 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> crate::native_abi::NativeResult,
                >(pointer)(args[0], args[1], args[2], args[3], args[4], args[5])
            },
            7 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> crate::native_abi::NativeResult,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6],
                )
            },
            8 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> crate::native_abi::NativeResult,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                )
            },
            9 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> crate::native_abi::NativeResult,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], args[8],
                )
            },
            10 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> crate::native_abi::NativeResult,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9],
                )
            },
            11 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> crate::native_abi::NativeResult,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10],
                )
            },
            12 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> crate::native_abi::NativeResult,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11],
                )
            },
            13 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> crate::native_abi::NativeResult,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12],
                )
            },
            14 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> crate::native_abi::NativeResult,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12], args[13],
                )
            },
            15 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> crate::native_abi::NativeResult,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12], args[13], args[14],
                )
            },
            16 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ) -> crate::native_abi::NativeResult,
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12], args[13], args[14], args[15],
                )
            },
            _ => crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::InvalidOperation,
            ),
        }
    }

    /// Invoke the integer entry point and write its shared ABI result into
    /// caller-owned storage.
    ///
    /// A null output pointer is rejected without entering generated code.
    /// This gives FFI callers the same pointer-based contract as the exported
    /// `lucid_checked_integer_binary_into` helper.
    ///
    /// # Safety
    ///
    /// `out`, when non-null, must point to writable storage for one
    /// [`crate::native_abi::NativeResult`]. The handle must remain alive and
    /// must not be concurrently destroyed while the call runs.
    pub unsafe fn call_result_into(&self, out: *mut crate::native_abi::NativeResult) -> bool {
        if out.is_null() || self.parameter_count != 0 {
            return false;
        }
        let result = if self.result_abi {
            unsafe { self.call_result() }
        } else if self.returns_value {
            crate::native_abi::NativeResult::ok(unsafe { self.call() })
        } else {
            unsafe { self.call_void() };
            crate::native_abi::NativeResult::ok(0)
        };
        // SAFETY: the caller guarantees writable storage when non-null.
        unsafe { out.write(result) };
        result.is_ok()
    }

    /// Invoke a parameterized recoverable-result entry point and write the
    /// result into caller-owned storage. This is the pointer-based companion
    /// to [`Self::call_result_with_args`], so FFI users do not need to rely on
    /// platform-specific aggregate-return conventions.
    ///
    /// # Safety
    ///
    /// `out`, when non-null, must point to writable storage for one
    /// [`crate::native_abi::NativeResult`]. The argument count must match the
    /// compiled function, and the handle must remain alive during the call.
    pub unsafe fn call_result_into_with_args(
        &self,
        args: &[i64],
        out: *mut crate::native_abi::NativeResult,
    ) -> bool {
        if out.is_null() {
            return false;
        }
        if self.parameter_count != args.len() {
            // SAFETY: `out` is non-null and the caller promises writable
            // storage for one NativeResult.
            unsafe {
                out.write(crate::native_abi::NativeResult::error(
                    crate::native_abi::NativeErrorCode::UnexpectedArgumentCount,
                ));
            }
            return false;
        }
        if args.len() > 16 {
            unsafe {
                out.write(crate::native_abi::NativeResult::error(
                    crate::native_abi::NativeErrorCode::InvalidOperation,
                ));
            }
            return false;
        }
        let result = unsafe { self.call_result_with_args(args) };
        // SAFETY: the caller guarantees writable storage when non-null.
        unsafe { out.write(result) };
        result.is_ok()
    }

    /// Invoke a CIR function whose terminators all return without a value.
    /// Calling the integer method on such a handle is rejected above rather
    /// than relying on an ABI mismatch.
    ///
    /// # Safety
    ///
    /// The function must have been produced by [`compile_integer_function`]
    /// and must not be called concurrently with destruction of this handle.
    pub unsafe fn call_void(&self) {
        assert!(
            !self.returns_value && !self.result_abi,
            "called void ABI on an integer function"
        );
        let pointer = self.module.get_finalized_function(self.id);
        let function: unsafe extern "C" fn() = unsafe { std::mem::transmute(pointer) };
        unsafe { function() }
    }

    /// Invoke a void function with positional integer arguments.
    ///
    /// # Safety
    ///
    /// The argument count must match the compiled function signature, and the
    /// handle must remain alive while generated code executes.
    pub unsafe fn call_void_with_args(&self, args: &[i64]) {
        assert!(
            !self.returns_value && !self.result_abi && self.parameter_count == args.len(),
            "called void ABI with an incompatible argument list"
        );
        let pointer = self.module.get_finalized_function(self.id);
        match args.len() {
            0 => unsafe { std::mem::transmute::<*const u8, unsafe extern "C" fn()>(pointer)() },
            1 => unsafe {
                std::mem::transmute::<*const u8, unsafe extern "C" fn(i64)>(pointer)(args[0])
            },
            2 => unsafe {
                std::mem::transmute::<*const u8, unsafe extern "C" fn(i64, i64)>(pointer)(
                    args[0], args[1],
                )
            },
            3 => unsafe {
                std::mem::transmute::<*const u8, unsafe extern "C" fn(i64, i64, i64)>(pointer)(
                    args[0], args[1], args[2],
                )
            },
            4 => unsafe {
                std::mem::transmute::<*const u8, unsafe extern "C" fn(i64, i64, i64, i64)>(pointer)(
                    args[0], args[1], args[2], args[3],
                )
            },
            5 => unsafe {
                std::mem::transmute::<*const u8, unsafe extern "C" fn(i64, i64, i64, i64, i64)>(
                    pointer,
                )(args[0], args[1], args[2], args[3], args[4])
            },
            6 => unsafe {
                std::mem::transmute::<*const u8, unsafe extern "C" fn(i64, i64, i64, i64, i64, i64)>(
                    pointer,
                )(args[0], args[1], args[2], args[3], args[4], args[5])
            },
            7 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64),
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6],
                )
            },
            8 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64),
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                )
            },
            9 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64, i64),
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], args[8],
                )
            },
            10 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64, i64, i64),
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9],
                )
            },
            11 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64),
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10],
                )
            },
            12 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ),
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11],
                )
            },
            13 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ),
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12],
                )
            },
            14 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ),
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12], args[13],
                )
            },
            15 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ),
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12], args[13], args[14],
                )
            },
            16 => unsafe {
                std::mem::transmute::<
                    *const u8,
                    unsafe extern "C" fn(
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                        i64,
                    ),
                >(pointer)(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12], args[13], args[14], args[15],
                )
            },
            _ => {
                debug_assert!(
                    args.len() <= 16,
                    "call_void_with_args currently supports at most sixteen arguments"
                );
            }
        }
    }

    /// Checked wrapper for [`Self::call_void_with_args`].
    ///
    /// # Safety
    ///
    /// The generated handle must remain alive and must not be concurrently
    /// destroyed while generated code executes.
    pub unsafe fn try_call_void_with_args(
        &self,
        args: &[i64],
    ) -> Result<(), crate::native_abi::NativeErrorCode> {
        if self.parameter_count != args.len() {
            return Err(crate::native_abi::NativeErrorCode::UnexpectedArgumentCount);
        }
        if args.len() > 16 {
            return Err(crate::native_abi::NativeErrorCode::InvalidOperation);
        }
        if self.returns_value || self.result_abi {
            return Err(crate::native_abi::NativeErrorCode::InvalidOperation);
        }
        unsafe { self.call_void_with_args(args) };
        Ok(())
    }
}

/// Compile a single-block integer CIR function and return its executable
/// entry point. The returned pointer stays valid until the module is dropped.
pub fn compile_integer_function(function: &Function) -> Result<CompiledFunction, CraneliftError> {
    compile_integer_function_impl(function, false)
}

/// Compile a single-block integer function using the recoverable `NativeResult`
/// ABI. This entry point supports verified integer CIR. Straight-line
/// constants and checked arithmetic preserve the first recoverable status.
/// Control flow without division-family status-merging restrictions is also
/// supported.
pub fn compile_integer_result_function(
    function: &Function,
) -> Result<CompiledFunction, CraneliftError> {
    function
        .verify()
        .map_err(|error| CraneliftError::InvalidCIR(format!("{error:?}")))?;
    let block = function
        .blocks
        .iter()
        .find(|block| block.id == function.entry)
        .ok_or(CraneliftError::UnsupportedControlFlow)?;
    let has_division_operations = function.has_division_operations();
    if !has_division_operations {
        // With no recoverable operation, the status is always success.  Let
        // the ordinary CIR backend handle diamonds, jumps, and Phi merges so
        // the result ABI is not artificially limited to one block.
        return compile_integer_function_impl(function, true);
    }
    // A recoverable operation in a branch can still use the result ABI when
    // its value is returned directly from that same block. The block-local
    // lowering records the corresponding status without pretending to
    // propagate an error through an unrelated merge.
    if has_division_operations && function.blocks.len() > 1 {
        let direct_branch_recoverable = function.blocks.iter().all(|block| {
            let recoverable_count = block
                .instructions
                .iter()
                .filter(|instruction| {
                    matches!(
                        instruction,
                        Instruction::Add { .. }
                            | Instruction::Sub { .. }
                            | Instruction::Mul { .. }
                            | Instruction::Pow { .. }
                            | Instruction::Neg { .. }
                            | Instruction::Shl { .. }
                            | Instruction::Shr { .. }
                            | Instruction::Div { .. }
                            | Instruction::FloorDiv { .. }
                            | Instruction::Mod { .. }
                    )
                })
                .count();
            if recoverable_count == 0 {
                return true;
            }
            match block.terminator {
                Terminator::Return(Some(returned)) => {
                    block.instructions.last().map(instruction_result) == Some(Some(returned))
                }
                _ => false,
            }
        });
        if direct_branch_recoverable {
            return compile_integer_function_impl(function, true);
        }
    }
    let Terminator::Return(Some(returned)) = block.terminator else {
        return Err(CraneliftError::UnsupportedControlFlow);
    };
    let final_is_returned =
        block.instructions.last().map(instruction_result) == Some(Some(returned));
    let straight_line_arithmetic =
        block
            .instructions
            .iter()
            .enumerate()
            .all(|(index, instruction)| {
                if index + 1 == block.instructions.len() {
                    matches!(
                        instruction,
                        Instruction::Div { .. }
                            | Instruction::FloorDiv { .. }
                            | Instruction::Mod { .. }
                            | Instruction::ConstInt { .. }
                            | Instruction::ConstBool { .. }
                            | Instruction::Add { .. }
                            | Instruction::Sub { .. }
                            | Instruction::Mul { .. }
                            | Instruction::Pow { .. }
                            | Instruction::Neg { .. }
                            | Instruction::Shl { .. }
                            | Instruction::Shr { .. }
                            | Instruction::BitAnd { .. }
                            | Instruction::BitOr { .. }
                            | Instruction::BitXor { .. }
                            | Instruction::BitNot { .. }
                            | Instruction::Not { .. }
                            | Instruction::And { .. }
                            | Instruction::Or { .. }
                            | Instruction::CmpEq { .. }
                            | Instruction::CmpNe { .. }
                            | Instruction::CmpLe { .. }
                            | Instruction::CmpGt { .. }
                            | Instruction::CmpGe { .. }
                            | Instruction::CmpLt { .. }
                            | Instruction::Param { .. }
                    )
                } else {
                    matches!(
                        instruction,
                        Instruction::Param { .. }
                            | Instruction::ConstInt { .. }
                            | Instruction::ConstBool { .. }
                            | Instruction::Add { .. }
                            | Instruction::Sub { .. }
                            | Instruction::Mul { .. }
                            | Instruction::Pow { .. }
                            | Instruction::Neg { .. }
                            | Instruction::Shl { .. }
                            | Instruction::Shr { .. }
                            | Instruction::Div { .. }
                            | Instruction::FloorDiv { .. }
                            | Instruction::Mod { .. }
                            | Instruction::BitAnd { .. }
                            | Instruction::BitOr { .. }
                            | Instruction::BitXor { .. }
                            | Instruction::BitNot { .. }
                            | Instruction::Not { .. }
                            | Instruction::And { .. }
                            | Instruction::Or { .. }
                            | Instruction::CmpEq { .. }
                            | Instruction::CmpNe { .. }
                            | Instruction::CmpLe { .. }
                            | Instruction::CmpGt { .. }
                            | Instruction::CmpGe { .. }
                            | Instruction::CmpLt { .. }
                    )
                }
            });
    // A result ABI is useful for successful arithmetic too: zero divisions
    // returns status 0, while division-family operations carry their
    // recoverable error status. In either case the returned value must be the
    // final instruction; the status keeps the first error in the block.
    if !final_is_returned || !straight_line_arithmetic {
        return Err(CraneliftError::UnsupportedInstruction(
            "result ABI requires constants and checked straight-line arithmetic".into(),
        ));
    }
    compile_integer_function_impl(function, true)
}

fn compile_integer_function_impl(
    function: &Function,
    result_abi: bool,
) -> Result<CompiledFunction, CraneliftError> {
    function
        .verify()
        .map_err(|error| CraneliftError::InvalidCIR(format!("{error:?}")))?;

    // Division-family operations are recoverable errors in Lucid.  The
    // legacy integer entry point returns only an i64, so letting these
    // instructions reach the lowering below would turn `DivisionByZero`
    // (and the MIN / -1 overflow case) into a machine trap.  Reject them at
    // the backend boundary until the result ABI is used by generated code.
    // This is deliberately a validation error rather than an implicit
    // change from recoverable errors to process-level traps.
    if !result_abi
        && function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Div { .. }
                        | Instruction::FloorDiv { .. }
                        | Instruction::Mod { .. }
                )
            })
    {
        return Err(CraneliftError::UnsupportedInstruction(
            "division-family instructions require the recoverable NativeResult ABI".into(),
        ));
    }

    let returns_value = function
        .blocks
        .iter()
        .filter_map(|block| match block.terminator {
            Terminator::Return(value) => Some(value.is_some()),
            _ => None,
        })
        .try_fold(None, |state, value| match (state, value) {
            (None, value) => Ok(Some(value)),
            (Some(previous), value) if previous == value => Ok(Some(previous)),
            (Some(_), _) if result_abi => Ok(Some(true)),
            _ => Err(CraneliftError::UnsupportedControlFlow),
        })?
        .unwrap_or(true);
    let parameter_count = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Param { index, .. } => Some(*index as usize + 1),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    if parameter_count > 16 {
        return Err(CraneliftError::UnsupportedInstruction(
            "native integer invocation currently supports at most sixteen parameters".into(),
        ));
    }
    let isa_builder =
        cranelift_native::builder().map_err(|error| CraneliftError::Backend(error.to_string()))?;
    let isa = isa_builder
        .finish(cranelift_codegen::settings::Flags::new(
            cranelift_codegen::settings::builder(),
        ))
        .map_err(|error| CraneliftError::Backend(error.to_string()))?;
    let mut module = JITModule::new(JITBuilder::with_isa(isa, default_libcall_names()));
    let mut context = module.make_context();
    context.func.name = UserFuncName::user(0, 0);
    if returns_value || result_abi {
        context
            .func
            .signature
            .returns
            .push(AbiParam::new(types::I64));
        if result_abi {
            context
                .func
                .signature
                .returns
                .push(AbiParam::new(types::I32));
        }
    }
    for _ in 0..parameter_count {
        context
            .func
            .signature
            .params
            .push(AbiParam::new(types::I64));
    }
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
    let mut block_ids = std::collections::HashMap::new();
    for block in &function.blocks {
        block_ids.insert(block.id, builder.create_block());
    }
    let entry = *block_ids
        .get(&function.entry)
        .ok_or(CraneliftError::UnsupportedControlFlow)?;
    builder.switch_to_block(entry);
    builder.append_block_params_for_function_params(entry);
    let function_params = builder.block_params(entry).to_vec();

    let mut phi_params = PhiParamMap::new();
    for block in &function.blocks {
        let params = block
            .instructions
            .iter()
            .filter_map(|instruction| match instruction {
                Instruction::Phi { result, incomings } => Some((
                    *result,
                    builder.append_block_param(block_ids[&block.id], types::I64),
                    incomings.clone(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        phi_params.insert(block.id, params);
    }

    let mut values = std::collections::HashMap::<ValueId, Variable>::new();
    let mut constant_values = std::collections::HashMap::<ValueId, i64>::new();
    macro_rules! binary {
        ($method:ident, $left:expr, $right:expr) => {{
            let left = load(&mut builder, &values, *$left)?;
            let right = load(&mut builder, &values, *$right)?;
            builder.ins().$method(left, right)
        }};
    }
    macro_rules! unary {
        ($method:ident, $operand:expr) => {{
            let operand = load(&mut builder, &values, *$operand)?;
            builder.ins().$method(operand)
        }};
    }
    macro_rules! compare {
        ($condition:expr, $left:expr, $right:expr) => {{
            let left = load(&mut builder, &values, *$left)?;
            let right = load(&mut builder, &values, *$right)?;
            let comparison = builder.ins().icmp($condition, left, right);
            builder.ins().uextend(types::I64, comparison)
        }};
    }
    for block in &function.blocks {
        let cranelift_block = block_ids[&block.id];
        builder.switch_to_block(cranelift_block);
        let mut block_error = None;
        for (result, parameter, _) in &phi_params[&block.id] {
            let variable = Variable::new(values.len());
            builder.declare_var(variable, types::I64);
            builder.def_var(variable, *parameter);
            values.insert(*result, variable);
        }
        for instruction in &block.instructions {
            if matches!(instruction, Instruction::Phi { .. }) {
                continue;
            }
            let result = instruction_result(instruction).ok_or_else(|| {
                CraneliftError::UnsupportedInstruction(format!(
                    "instruction has no result: {instruction:?}"
                ))
            })?;
            let value = match instruction {
                Instruction::Param { index, .. } => {
                    *function_params.get(*index as usize).ok_or_else(|| {
                        CraneliftError::InvalidCIR("parameter index exceeds signature".into())
                    })?
                }
                Instruction::ConstInt { value, .. } => builder.ins().iconst(types::I64, *value),
                Instruction::ConstBool { value, .. } => {
                    builder.ins().iconst(types::I64, i64::from(*value))
                }
                Instruction::Add { left, right, .. } => {
                    let left_value = load(&mut builder, &values, *left)?;
                    let right_value = load(&mut builder, &values, *right)?;
                    let sum = builder.ins().iadd(left_value, right_value);
                    let zero = builder.ins().iconst(types::I64, 0);
                    let left_negative = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                        left_value,
                        zero,
                    );
                    let right_negative = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                        right_value,
                        zero,
                    );
                    let sum_negative = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                        sum,
                        zero,
                    );
                    let same_sign = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        left_negative,
                        right_negative,
                    );
                    let sign_changed = builder.ins().bxor(left_negative, sum_negative);
                    let overflow = builder.ins().band(same_sign, sign_changed);
                    if result_abi {
                        let code = builder.ins().iconst(
                            types::I32,
                            lucid_abi::NativeErrorCode::ArithmeticOverflow.as_raw() as i64,
                        );
                        let zero_error = builder.ins().iconst(types::I32, 0);
                        let current_error = builder.ins().select(overflow, code, zero_error);
                        block_error = Some(match block_error {
                            Some(previous) => {
                                let previous_failed = builder.ins().icmp(
                                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                    previous,
                                    zero_error,
                                );
                                builder
                                    .ins()
                                    .select(previous_failed, previous, current_error)
                            }
                            None => current_error,
                        });
                    } else {
                        builder
                            .ins()
                            .trapnz(overflow, cranelift_codegen::ir::TrapCode::INTEGER_OVERFLOW);
                    }
                    sum
                }
                Instruction::Sub { left, right, .. } => {
                    let left_value = load(&mut builder, &values, *left)?;
                    let right_value = load(&mut builder, &values, *right)?;
                    let difference = builder.ins().isub(left_value, right_value);
                    let zero = builder.ins().iconst(types::I64, 0);
                    let left_negative = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                        left_value,
                        zero,
                    );
                    let right_negative = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                        right_value,
                        zero,
                    );
                    let difference_negative = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                        difference,
                        zero,
                    );
                    let differing_signs = builder.ins().bxor(left_negative, right_negative);
                    let changed_from_left = builder.ins().bxor(left_negative, difference_negative);
                    let overflow = builder.ins().band(differing_signs, changed_from_left);
                    if result_abi {
                        let code = builder.ins().iconst(
                            types::I32,
                            lucid_abi::NativeErrorCode::ArithmeticOverflow.as_raw() as i64,
                        );
                        let zero_error = builder.ins().iconst(types::I32, 0);
                        let current_error = builder.ins().select(overflow, code, zero_error);
                        block_error = Some(match block_error {
                            Some(previous) => {
                                let previous_failed = builder.ins().icmp(
                                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                    previous,
                                    zero_error,
                                );
                                builder
                                    .ins()
                                    .select(previous_failed, previous, current_error)
                            }
                            None => current_error,
                        });
                    } else {
                        builder
                            .ins()
                            .trapnz(overflow, cranelift_codegen::ir::TrapCode::INTEGER_OVERFLOW);
                    }
                    difference
                }
                Instruction::Mul { left, right, .. } => {
                    let left_value = load(&mut builder, &values, *left)?;
                    let right_value = load(&mut builder, &values, *right)?;
                    let product = builder.ins().imul(left_value, right_value);
                    let zero = builder.ins().iconst(types::I64, 0);
                    let minimum = builder.ins().iconst(types::I64, i64::MIN);
                    let minus_one = builder.ins().iconst(types::I64, -1);
                    let left_is_min = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        left_value,
                        minimum,
                    );
                    let right_is_minus_one = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        right_value,
                        minus_one,
                    );
                    let left_is_minus_one = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        left_value,
                        minus_one,
                    );
                    let right_is_min = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        right_value,
                        minimum,
                    );
                    let edge_left = builder.ins().band(left_is_min, right_is_minus_one);
                    let edge_right = builder.ins().band(left_is_minus_one, right_is_min);
                    let edge_overflow = builder.ins().bor(edge_left, edge_right);
                    if result_abi {
                        let code = builder.ins().iconst(
                            types::I32,
                            lucid_abi::NativeErrorCode::ArithmeticOverflow.as_raw() as i64,
                        );
                        let zero_error = builder.ins().iconst(types::I32, 0);
                        let current_error = builder.ins().select(edge_overflow, code, zero_error);
                        block_error = Some(match block_error {
                            Some(previous) => {
                                let previous_failed = builder.ins().icmp(
                                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                    previous,
                                    zero_error,
                                );
                                builder
                                    .ins()
                                    .select(previous_failed, previous, current_error)
                            }
                            None => current_error,
                        });
                    } else {
                        builder.ins().trapnz(
                            edge_overflow,
                            cranelift_codegen::ir::TrapCode::INTEGER_OVERFLOW,
                        );
                    }
                    let right_is_zero = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        right_value,
                        zero,
                    );
                    let unsafe_right = builder.ins().bor(right_is_zero, edge_overflow);
                    let one = builder.ins().iconst(types::I64, 1);
                    let safe_right = builder.ins().select(unsafe_right, one, right_value);
                    let quotient = builder.ins().sdiv(product, safe_right);
                    let mismatch = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                        quotient,
                        left_value,
                    );
                    let right_nonzero = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                        right_value,
                        zero,
                    );
                    let overflow = builder.ins().band(right_nonzero, mismatch);
                    if result_abi {
                        let code = builder.ins().iconst(
                            types::I32,
                            lucid_abi::NativeErrorCode::ArithmeticOverflow.as_raw() as i64,
                        );
                        let zero_error = builder.ins().iconst(types::I32, 0);
                        let current_error = builder.ins().select(overflow, code, zero_error);
                        block_error = Some(match block_error {
                            Some(previous) => {
                                let previous_failed = builder.ins().icmp(
                                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                    previous,
                                    zero_error,
                                );
                                builder
                                    .ins()
                                    .select(previous_failed, previous, current_error)
                            }
                            None => current_error,
                        });
                    } else {
                        builder
                            .ins()
                            .trapnz(overflow, cranelift_codegen::ir::TrapCode::INTEGER_OVERFLOW);
                    }
                    product
                }
                Instruction::Div { left, right, .. }
                | Instruction::FloorDiv { left, right, .. }
                | Instruction::Mod { left, right, .. } => {
                    let left_value = load(&mut builder, &values, *left)?;
                    let right_value = load(&mut builder, &values, *right)?;
                    let zero = builder.ins().iconst(types::I64, 0);
                    let denominator_zero = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        right_value,
                        zero,
                    );
                    if !result_abi {
                        builder.ins().trapnz(
                            denominator_zero,
                            cranelift_codegen::ir::TrapCode::INTEGER_DIVISION_BY_ZERO,
                        );
                    }
                    let min_value = builder.ins().iconst(types::I64, i64::MIN);
                    let minus_one = builder.ins().iconst(types::I64, -1);
                    let left_is_min = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        left_value,
                        min_value,
                    );
                    let right_is_minus_one = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        right_value,
                        minus_one,
                    );
                    let min_overflow = builder.ins().band(left_is_min, right_is_minus_one);
                    if !result_abi {
                        builder.ins().trapnz(
                            min_overflow,
                            cranelift_codegen::ir::TrapCode::INTEGER_OVERFLOW,
                        );
                    }
                    let invalid = builder.ins().bor(denominator_zero, min_overflow);
                    let safe_left = builder.ins().select(invalid, zero, left_value);
                    let one = builder.ins().iconst(types::I64, 1);
                    let safe_right = builder.ins().select(denominator_zero, one, right_value);
                    let quotient = builder.ins().sdiv(safe_left, safe_right);
                    if result_abi {
                        let div_zero = builder.ins().iconst(
                            types::I32,
                            lucid_abi::NativeErrorCode::DivisionByZero.as_raw() as i64,
                        );
                        let overflow = builder.ins().iconst(
                            types::I32,
                            lucid_abi::NativeErrorCode::ArithmeticOverflow.as_raw() as i64,
                        );
                        let ok = builder.ins().iconst(types::I32, 0);
                        let nonzero_error = builder.ins().select(min_overflow, overflow, ok);
                        let current_error =
                            builder
                                .ins()
                                .select(denominator_zero, div_zero, nonzero_error);
                        block_error = Some(match block_error {
                            Some(previous) => {
                                let previous_failed = builder.ins().icmp(
                                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                    previous,
                                    ok,
                                );
                                builder
                                    .ins()
                                    .select(previous_failed, previous, current_error)
                            }
                            None => current_error,
                        });
                    }
                    if matches!(instruction, Instruction::Div { .. }) {
                        quotient
                    } else {
                        let quotient_product = builder.ins().imul(quotient, right_value);
                        let remainder = builder.ins().isub(left_value, quotient_product);
                        let remainder_nonzero = builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                            remainder,
                            zero,
                        );
                        let right_negative = builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                            right_value,
                            zero,
                        );
                        if matches!(instruction, Instruction::FloorDiv { .. }) {
                            let remainder_negative = builder.ins().icmp(
                                cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                                remainder,
                                zero,
                            );
                            let adjust = builder.ins().band(remainder_nonzero, remainder_negative);
                            let one = builder.ins().iconst(types::I64, 1);
                            let direction = builder.ins().select(right_negative, one, zero);
                            let delta = builder.ins().isub(direction, one);
                            let adjusted = builder.ins().iadd(quotient, delta);
                            builder.ins().select(adjust, adjusted, quotient)
                        } else {
                            let left_negative = builder.ins().icmp(
                                cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                                left_value,
                                zero,
                            );
                            let signs_differ = builder.ins().bxor(left_negative, right_negative);
                            let adjust = builder.ins().band(remainder_nonzero, signs_differ);
                            let adjusted = builder.ins().iadd(remainder, right_value);
                            builder.ins().select(adjust, adjusted, remainder)
                        }
                    }
                }
                Instruction::Pow { left, right, .. } => {
                    let exponent = constant_values.get(right).copied();
                    if exponent.is_none() {
                        if !result_abi {
                            return Err(CraneliftError::UnsupportedInstruction(
                                "dynamic integer exponent requires the recoverable ABI".into(),
                            ));
                        }
                        let exponent_value = load(&mut builder, &values, *right)?;
                        let zero = builder.ins().iconst(types::I64, 0);
                        let negative = builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                            exponent_value,
                            zero,
                        );
                        let limit = builder.ins().iconst(types::I64, 1024);
                        let oversized = builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThan,
                            exponent_value,
                            limit,
                        );
                        let negative_code = builder.ins().iconst(
                            types::I32,
                            lucid_abi::NativeErrorCode::NegativeExponent.as_raw() as i64,
                        );
                        let invalid_code = builder.ins().iconst(
                            types::I32,
                            lucid_abi::NativeErrorCode::InvalidOperation.as_raw() as i64,
                        );
                        let zero_error = builder.ins().iconst(types::I32, 0);
                        let oversized_error =
                            builder.ins().select(oversized, invalid_code, zero_error);
                        let exponent_error =
                            builder
                                .ins()
                                .select(negative, negative_code, oversized_error);
                        block_error = Some(match block_error {
                            Some(previous) => {
                                let previous_failed = builder.ins().icmp(
                                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                    previous,
                                    zero_error,
                                );
                                builder
                                    .ins()
                                    .select(previous_failed, previous, exponent_error)
                            }
                            None => exponent_error,
                        });
                        let base = load(&mut builder, &values, *left)?;
                        let mut result = builder.ins().iconst(types::I64, 1);
                        for index in 0..1024i64 {
                            let threshold = builder.ins().iconst(types::I64, index);
                            let active = builder.ins().icmp(
                                cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThan,
                                exponent_value,
                                threshold,
                            );
                            let product = builder.ins().imul(result, base);
                            let minimum = builder.ins().iconst(types::I64, i64::MIN);
                            let minus_one = builder.ins().iconst(types::I64, -1);
                            let result_is_min = builder.ins().icmp(
                                cranelift_codegen::ir::condcodes::IntCC::Equal,
                                result,
                                minimum,
                            );
                            let base_is_minus_one = builder.ins().icmp(
                                cranelift_codegen::ir::condcodes::IntCC::Equal,
                                base,
                                minus_one,
                            );
                            let result_is_minus_one = builder.ins().icmp(
                                cranelift_codegen::ir::condcodes::IntCC::Equal,
                                result,
                                minus_one,
                            );
                            let base_is_min = builder.ins().icmp(
                                cranelift_codegen::ir::condcodes::IntCC::Equal,
                                base,
                                minimum,
                            );
                            let edge_left = builder.ins().band(result_is_min, base_is_minus_one);
                            let edge_right = builder.ins().band(result_is_minus_one, base_is_min);
                            let edge_overflow = builder.ins().bor(edge_left, edge_right);
                            let base_is_zero = builder.ins().icmp(
                                cranelift_codegen::ir::condcodes::IntCC::Equal,
                                base,
                                zero,
                            );
                            let one = builder.ins().iconst(types::I64, 1);
                            let unsafe_base = builder.ins().bor(base_is_zero, edge_overflow);
                            let safe_base = builder.ins().select(unsafe_base, one, base);
                            let quotient = builder.ins().sdiv(product, safe_base);
                            let mismatch = builder.ins().icmp(
                                cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                quotient,
                                result,
                            );
                            let base_nonzero = builder.ins().icmp(
                                cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                base,
                                zero,
                            );
                            let mismatch_overflow = builder.ins().band(base_nonzero, mismatch);
                            let total_overflow =
                                builder.ins().bor(edge_overflow, mismatch_overflow);
                            let arithmetic_overflow = builder.ins().band(active, total_overflow);
                            let arithmetic_code = builder.ins().iconst(
                                types::I32,
                                lucid_abi::NativeErrorCode::ArithmeticOverflow.as_raw() as i64,
                            );
                            let current_error = builder.ins().select(
                                arithmetic_overflow,
                                arithmetic_code,
                                zero_error,
                            );
                            block_error = Some(match block_error {
                                Some(previous) => {
                                    let previous_failed = builder.ins().icmp(
                                        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                        previous,
                                        zero_error,
                                    );
                                    builder
                                        .ins()
                                        .select(previous_failed, previous, current_error)
                                }
                                None => current_error,
                            });
                            result = builder.ins().select(active, product, result);
                        }
                        result
                    } else {
                        let exponent = match exponent {
                            Some(value) => value,
                            None => {
                                return Err(CraneliftError::UnsupportedInstruction(
                                    "constant exponent is missing".into(),
                                ));
                            }
                        };
                        if exponent < 0 {
                            if !result_abi {
                                return Err(CraneliftError::UnsupportedInstruction(
                                    "negative integer exponent requires the recoverable ABI".into(),
                                ));
                            }
                            let code = builder.ins().iconst(
                                types::I32,
                                lucid_abi::NativeErrorCode::NegativeExponent.as_raw() as i64,
                            );
                            let zero_error = builder.ins().iconst(types::I32, 0);
                            let previous = block_error.unwrap_or(zero_error);
                            let previous_failed = builder.ins().icmp(
                                cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                previous,
                                zero_error,
                            );
                            block_error =
                                Some(builder.ins().select(previous_failed, previous, code));
                            builder.ins().iconst(types::I64, 0)
                        } else {
                            if exponent > 1024 {
                                if !result_abi {
                                    return Err(CraneliftError::UnsupportedInstruction(
                                        "oversized integer exponent requires the recoverable ABI"
                                            .into(),
                                    ));
                                }
                                let code = builder.ins().iconst(
                                    types::I32,
                                    lucid_abi::NativeErrorCode::InvalidOperation.as_raw() as i64,
                                );
                                let zero_error = builder.ins().iconst(types::I32, 0);
                                let previous = block_error.unwrap_or(zero_error);
                                let previous_failed = builder.ins().icmp(
                                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                    previous,
                                    zero_error,
                                );
                                block_error =
                                    Some(builder.ins().select(previous_failed, previous, code));
                                builder.ins().iconst(types::I64, 0)
                            } else {
                                let base = load(&mut builder, &values, *left)?;
                                let mut result = builder.ins().iconst(types::I64, 1);
                                for _ in 0..exponent {
                                    let product = builder.ins().imul(result, base);
                                    let minimum = builder.ins().iconst(types::I64, i64::MIN);
                                    let minus_one = builder.ins().iconst(types::I64, -1);
                                    let result_is_min = builder.ins().icmp(
                                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                                        result,
                                        minimum,
                                    );
                                    let base_is_minus_one = builder.ins().icmp(
                                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                                        base,
                                        minus_one,
                                    );
                                    let result_is_minus_one = builder.ins().icmp(
                                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                                        result,
                                        minus_one,
                                    );
                                    let base_is_min = builder.ins().icmp(
                                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                                        base,
                                        minimum,
                                    );
                                    let edge_left =
                                        builder.ins().band(result_is_min, base_is_minus_one);
                                    let edge_right =
                                        builder.ins().band(result_is_minus_one, base_is_min);
                                    let edge_overflow = builder.ins().bor(edge_left, edge_right);
                                    let zero = builder.ins().iconst(types::I64, 0);
                                    let base_is_zero = builder.ins().icmp(
                                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                                        base,
                                        zero,
                                    );
                                    let one = builder.ins().iconst(types::I64, 1);
                                    let unsafe_base =
                                        builder.ins().bor(base_is_zero, edge_overflow);
                                    let safe_base = builder.ins().select(unsafe_base, one, base);
                                    let quotient = builder.ins().sdiv(product, safe_base);
                                    let mismatch = builder.ins().icmp(
                                        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                        quotient,
                                        result,
                                    );
                                    let base_nonzero = builder.ins().icmp(
                                        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                        base,
                                        zero,
                                    );
                                    let overflow = builder.ins().band(base_nonzero, mismatch);
                                    let total_overflow = builder.ins().bor(edge_overflow, overflow);
                                    if result_abi {
                                        let code = builder.ins().iconst(
                                            types::I32,
                                            lucid_abi::NativeErrorCode::ArithmeticOverflow.as_raw()
                                                as i64,
                                        );
                                        let zero_error = builder.ins().iconst(types::I32, 0);
                                        let current_error =
                                            builder.ins().select(total_overflow, code, zero_error);
                                        block_error = Some(match block_error {
                                            Some(previous) => {
                                                let previous_failed = builder.ins().icmp(
                                        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                        previous,
                                        zero_error,
                                    );
                                                builder.ins().select(
                                                    previous_failed,
                                                    previous,
                                                    current_error,
                                                )
                                            }
                                            None => current_error,
                                        });
                                    } else {
                                        builder.ins().trapnz(
                                            total_overflow,
                                            cranelift_codegen::ir::TrapCode::INTEGER_OVERFLOW,
                                        );
                                    }
                                    result = product;
                                }
                                result
                            }
                        }
                    }
                }
                Instruction::BitAnd { left, right, .. } => binary!(band, left, right),
                Instruction::BitOr { left, right, .. } => binary!(bor, left, right),
                Instruction::BitXor { left, right, .. } => binary!(bxor, left, right),
                Instruction::Shl { left, right, .. } | Instruction::Shr { left, right, .. } => {
                    let left_value = load(&mut builder, &values, *left)?;
                    let right_value = load(&mut builder, &values, *right)?;
                    let zero = builder.ins().iconst(types::I64, 0);
                    let negative = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                        right_value,
                        zero,
                    );
                    let limit = builder.ins().iconst(types::I64, 64);
                    let too_large = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThanOrEqual,
                        right_value,
                        limit,
                    );
                    let invalid = builder.ins().bor(negative, too_large);
                    if result_abi {
                        let code = builder.ins().iconst(
                            types::I32,
                            lucid_abi::NativeErrorCode::ArithmeticOverflow.as_raw() as i64,
                        );
                        let zero_error = builder.ins().iconst(types::I32, 0);
                        let current_error = builder.ins().select(invalid, code, zero_error);
                        block_error = Some(match block_error {
                            Some(previous) => {
                                let previous_failed = builder.ins().icmp(
                                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                    previous,
                                    zero_error,
                                );
                                builder
                                    .ins()
                                    .select(previous_failed, previous, current_error)
                            }
                            None => current_error,
                        });
                    } else {
                        builder
                            .ins()
                            .trapnz(invalid, cranelift_codegen::ir::TrapCode::INTEGER_OVERFLOW);
                    }
                    let safe_right = builder.ins().select(invalid, zero, right_value);
                    if matches!(instruction, Instruction::Shl { .. }) {
                        let shifted = builder.ins().ishl(left_value, safe_right);
                        let restored = builder.ins().sshr(shifted, safe_right);
                        let overflow = builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                            restored,
                            left_value,
                        );
                        if result_abi {
                            let code = builder.ins().iconst(
                                types::I32,
                                lucid_abi::NativeErrorCode::ArithmeticOverflow.as_raw() as i64,
                            );
                            let zero_error = builder.ins().iconst(types::I32, 0);
                            let current_error = builder.ins().select(overflow, code, zero_error);
                            block_error = Some(match block_error {
                                Some(previous) => {
                                    let previous_failed = builder.ins().icmp(
                                        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                        previous,
                                        zero_error,
                                    );
                                    builder
                                        .ins()
                                        .select(previous_failed, previous, current_error)
                                }
                                None => current_error,
                            });
                        } else {
                            builder.ins().trapnz(
                                overflow,
                                cranelift_codegen::ir::TrapCode::INTEGER_OVERFLOW,
                            );
                        }
                        shifted
                    } else {
                        builder.ins().sshr(left_value, safe_right)
                    }
                }
                Instruction::And { left, right, .. } => binary!(band, left, right),
                Instruction::Or { left, right, .. } => binary!(bor, left, right),
                Instruction::Not { operand, .. } => {
                    let operand = load(&mut builder, &values, *operand)?;
                    let one = builder.ins().iconst(types::I64, 1);
                    builder.ins().bxor(operand, one)
                }
                Instruction::Neg { operand, .. } => {
                    let value = load(&mut builder, &values, *operand)?;
                    let minimum = builder.ins().iconst(types::I64, i64::MIN);
                    let overflow = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        value,
                        minimum,
                    );
                    if result_abi {
                        let code = builder.ins().iconst(
                            types::I32,
                            lucid_abi::NativeErrorCode::ArithmeticOverflow.as_raw() as i64,
                        );
                        let zero = builder.ins().iconst(types::I32, 0);
                        let current_error = builder.ins().select(overflow, code, zero);
                        block_error = Some(match block_error {
                            Some(previous) => {
                                let previous_failed = builder.ins().icmp(
                                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                                    previous,
                                    zero,
                                );
                                builder
                                    .ins()
                                    .select(previous_failed, previous, current_error)
                            }
                            None => current_error,
                        });
                    } else {
                        builder
                            .ins()
                            .trapnz(overflow, cranelift_codegen::ir::TrapCode::INTEGER_OVERFLOW);
                    }
                    builder.ins().ineg(value)
                }
                Instruction::BitNot { operand, .. } => unary!(bnot, operand),
                Instruction::CmpEq { left, right, .. } => {
                    compare!(cranelift_codegen::ir::condcodes::IntCC::Equal, left, right)
                }
                Instruction::CmpNe { left, right, .. } => compare!(
                    cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                    left,
                    right
                ),
                Instruction::CmpLe { left, right, .. } => compare!(
                    cranelift_codegen::ir::condcodes::IntCC::SignedLessThanOrEqual,
                    left,
                    right
                ),
                Instruction::CmpLt { left, right, .. } => compare!(
                    cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                    left,
                    right
                ),
                Instruction::CmpGe { left, right, .. } => compare!(
                    cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThanOrEqual,
                    left,
                    right
                ),
                Instruction::CmpGt { left, right, .. } => compare!(
                    cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThan,
                    left,
                    right
                ),
                other => return Err(CraneliftError::UnsupportedInstruction(format!("{other:?}"))),
            };
            let variable = Variable::new(values.len());
            builder.declare_var(variable, types::I64);
            builder.def_var(variable, value);
            values.insert(result, variable);
            let constant = match instruction {
                Instruction::ConstInt { value, .. } => Some(*value),
                Instruction::ConstBool { value, .. } => Some(i64::from(*value)),
                Instruction::Add { left, right, .. } => constant_values
                    .get(left)
                    .zip(constant_values.get(right))
                    .and_then(|(left, right)| left.checked_add(*right)),
                Instruction::Sub { left, right, .. } => constant_values
                    .get(left)
                    .zip(constant_values.get(right))
                    .and_then(|(left, right)| left.checked_sub(*right)),
                Instruction::Mul { left, right, .. } => constant_values
                    .get(left)
                    .zip(constant_values.get(right))
                    .and_then(|(left, right)| left.checked_mul(*right)),
                Instruction::Neg { operand, .. } => constant_values
                    .get(operand)
                    .and_then(|value| value.checked_neg()),
                Instruction::Pow { left, right, .. } => constant_values
                    .get(left)
                    .zip(constant_values.get(right))
                    .and_then(|(left, right)| {
                        u32::try_from(*right)
                            .ok()
                            .and_then(|exponent| left.checked_pow(exponent))
                    }),
                _ => None,
            };
            if let Some(value) = constant {
                constant_values.insert(result, value);
            }
        }
        match &block.terminator {
            Terminator::Return(Some(result)) => {
                let return_value = load(&mut builder, &values, *result)?;
                if result_abi {
                    let error = block_error.unwrap_or_else(|| builder.ins().iconst(types::I32, 0));
                    let zero_error = builder.ins().iconst(types::I32, 0);
                    let failed = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                        error,
                        zero_error,
                    );
                    let zero_value = builder.ins().iconst(types::I64, 0);
                    let returned_value = builder.ins().select(failed, zero_value, return_value);
                    builder.ins().return_(&[returned_value, error]);
                } else {
                    builder.ins().return_(&[return_value]);
                }
            }
            Terminator::Return(None) => {
                if result_abi {
                    let value = builder.ins().iconst(types::I64, 0);
                    let error = builder.ins().iconst(types::I32, 0);
                    builder.ins().return_(&[value, error]);
                } else {
                    builder.ins().return_(&[]);
                }
            }
            Terminator::Jump(target) => {
                let target_id = *target;
                let target = *block_ids
                    .get(&target_id)
                    .ok_or(CraneliftError::UnsupportedControlFlow)?;
                let args = phi_arguments(block.id, target_id, &mut builder, &values, &phi_params)?;
                builder.ins().jump(target, &args);
            }
            Terminator::Branch {
                condition,
                then_block,
                else_block,
            } => {
                let condition = load(&mut builder, &values, *condition)?;
                let then_id = *then_block;
                let else_id = *else_block;
                let then_block = *block_ids
                    .get(&then_id)
                    .ok_or(CraneliftError::UnsupportedControlFlow)?;
                let else_block = *block_ids
                    .get(&else_id)
                    .ok_or(CraneliftError::UnsupportedControlFlow)?;
                let then_args =
                    phi_arguments(block.id, then_id, &mut builder, &values, &phi_params)?;
                let else_args =
                    phi_arguments(block.id, else_id, &mut builder, &values, &phi_params)?;
                builder
                    .ins()
                    .brif(condition, then_block, &then_args, else_block, &else_args);
            }
        }
    }
    builder.seal_all_blocks();
    builder.finalize();

    let id = module
        .declare_function("lucid_entry", Linkage::Export, &context.func.signature)
        .map_err(|error| CraneliftError::Backend(error.to_string()))?;
    module
        .define_function(id, &mut context)
        .map_err(|error| CraneliftError::Backend(error.to_string()))?;
    module.clear_context(&mut context);
    module
        .finalize_definitions()
        .map_err(|error| CraneliftError::Backend(error.to_string()))?;
    Ok(CompiledFunction {
        module,
        id,
        parameter_count,
        returns_value,
        result_abi,
    })
}

/// Lower the first supported initializer in a parsed module and compile it
/// through the same verified CIR path used by [`compile_integer_function`].
pub fn compile_integer_module(
    module: &lucid_syntax::Module,
) -> Result<CompiledFunction, CraneliftError> {
    let function = Function::from_module(module)
        .map_err(|error| CraneliftError::InvalidCIR(format!("{error:?}")))?;
    compile_integer_function(&function)
}

fn instruction_result(instruction: &Instruction) -> Option<ValueId> {
    match instruction {
        Instruction::Param { result, .. }
        | Instruction::ConstInt { result, .. }
        | Instruction::ConstBool { result, .. }
        | Instruction::Add { result, .. }
        | Instruction::Sub { result, .. }
        | Instruction::Mul { result, .. }
        | Instruction::Pow { result, .. }
        | Instruction::Div { result, .. }
        | Instruction::FloorDiv { result, .. }
        | Instruction::Mod { result, .. }
        | Instruction::BitAnd { result, .. }
        | Instruction::BitOr { result, .. }
        | Instruction::BitXor { result, .. }
        | Instruction::Shl { result, .. }
        | Instruction::Shr { result, .. }
        | Instruction::And { result, .. }
        | Instruction::Or { result, .. }
        | Instruction::Not { result, .. }
        | Instruction::Neg { result, .. }
        | Instruction::BitNot { result, .. }
        | Instruction::CmpEq { result, .. }
        | Instruction::CmpNe { result, .. }
        | Instruction::CmpLe { result, .. }
        | Instruction::CmpLt { result, .. }
        | Instruction::CmpGe { result, .. }
        | Instruction::CmpGt { result, .. } => Some(*result),
        _ => None,
    }
}

fn load(
    builder: &mut FunctionBuilder<'_>,
    values: &std::collections::HashMap<ValueId, Variable>,
    id: ValueId,
) -> Result<cranelift_codegen::ir::Value, CraneliftError> {
    values
        .get(&id)
        .map(|variable| builder.use_var(*variable))
        .ok_or_else(|| CraneliftError::InvalidCIR(format!("missing value {id:?}")))
}

fn phi_arguments(
    predecessor: lucid_cir::BlockId,
    target: lucid_cir::BlockId,
    builder: &mut FunctionBuilder<'_>,
    values: &std::collections::HashMap<ValueId, Variable>,
    phi_params: &PhiParamMap,
) -> Result<Vec<cranelift_codegen::ir::Value>, CraneliftError> {
    phi_params[&target]
        .iter()
        .map(|(_, _, incomings)| {
            let (_, incoming) = incomings
                .iter()
                .find(|(block, _)| *block == predecessor)
                .ok_or_else(|| {
                    CraneliftError::InvalidCIR(format!("missing Phi value on {predecessor:?}"))
                })?;
            values
                .get(incoming)
                .map(|variable| builder.use_var(*variable))
                .ok_or_else(|| {
                    CraneliftError::InvalidCIR(format!("missing incoming value {incoming:?}"))
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_abi_executes_range_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
for i in range(n):
    total += i
return total
"#,
        )
        .expect("range accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("range accumulation should lower");
        let compiled =
            compile_integer_result_function(&function).expect("result ABI should compile loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[5]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 10);
    }

    #[test]
    fn result_abi_executes_commuted_range_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
for i in range(n):
    total = i + total
return total
"#,
        )
        .expect("commuted range accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("commuted range accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile commuted range loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[5]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 10);
    }

    #[test]
    fn result_abi_executes_literal_step_range_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
for i in range(n):
    total += 2
return total
"#,
        )
        .expect("literal-step range accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("literal-step range accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile literal-step range loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[5]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 10);
    }

    #[test]
    fn result_abi_executes_signed_literal_step_range_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
for i in range(n):
    total += -2
return total
"#,
        )
        .expect("signed-literal range accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("signed-literal range accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile signed-literal range loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[5]) };
        assert!(result.is_ok());
        assert_eq!(result.value, -10);
    }

    #[test]
    fn result_abi_executes_range_accumulation_with_local_step_alias_cfg() {
        let module = lucid_syntax::parse(
            r#"inc = step
total = 0
for i in range(n):
    total += inc
return total
"#,
        )
        .expect("range local step alias accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "step".into()],
        )
        .expect("range local step alias accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile range local step alias loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[5, 3]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 15);
    }

    #[test]
    fn result_abi_executes_range_accumulation_with_local_bound_alias_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
limit = n
for i in range(limit):
    total += i
return total
"#,
        )
        .expect("range alias accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("range alias accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile range alias loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[5]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 10);
    }

    #[test]
    fn result_abi_executes_range_accumulation_with_local_seed_alias_cfg() {
        let module = lucid_syntax::parse(
            r#"seeded = seed
total = seeded
for i in range(n):
    total += i
return total
"#,
        )
        .expect("range seed alias accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "seed".into()],
        )
        .expect("range seed alias accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile range seed alias loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[5, 7]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 17);
    }

    #[test]
    fn result_abi_executes_range_accumulation_with_local_start_alias_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
begin = seed
for i in range(begin, n):
    total += i
return total
"#,
        )
        .expect("range start alias accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "seed".into()],
        )
        .expect("range start alias accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile range start alias loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[5, 2]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 9);
    }

    #[test]
    fn result_abi_executes_range_accumulation_with_chained_local_bound_alias_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
start = seed
begin = start
for i in range(begin, n):
    total += i
return total
"#,
        )
        .expect("range chained start alias accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "seed".into()],
        )
        .expect("range chained start alias accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile range chained start alias loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[5, 2]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 9);
    }

    #[test]
    fn result_abi_executes_range_accumulation_with_long_chained_local_bound_alias_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
start = seed
middle = start
begin = middle
for i in range(begin, n):
    total += i
return total
"#,
        )
        .expect("range long chained start alias accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "seed".into()],
        )
        .expect("range long chained start alias accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile range long chained start alias loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[5, 2]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 9);
    }

    #[test]
    fn result_abi_executes_range_accumulation_with_two_local_bound_aliases_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
begin = seed
stop = limit
for i in range(begin, stop):
    total += i
return total
"#,
        )
        .expect("range two-alias accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["seed".into(), "limit".into()],
        )
        .expect("range two-alias accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile range two-alias loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[2, 6]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 14);
    }

    #[test]
    fn result_abi_executes_range_accumulation_with_reordered_local_bound_aliases_cfg() {
        let module = lucid_syntax::parse(
            r#"begin = seed
stop = limit
total = 0
for i in range(begin, stop):
    total += i
return total
"#,
        )
        .expect("range reordered two-alias accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["seed".into(), "limit".into()],
        )
        .expect("range reordered two-alias accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile range reordered two-alias loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[2, 6]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 14);
    }

    #[test]
    fn result_abi_executes_descending_range_accumulation_with_local_bound_alias_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
stop = limit
for i in range(n, stop, -1):
    total += i
return total
"#,
        )
        .expect("descending range alias accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "limit".into()],
        )
        .expect("descending range alias accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile descending range alias loop CFG");
        let result = unsafe { compiled.call_result_with_args(&[5, 2]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 12);
    }

    #[test]
    fn result_abi_executes_while_induction_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
while n > 0:
    total += n
    n -= 1
return total
"#,
        )
        .expect("while induction accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("while induction accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile while accumulation CFG");
        let result = unsafe { compiled.call_result_with_args(&[5]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 15);
    }

    #[test]
    fn result_abi_executes_signed_update_while_induction_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"total = -1
while n > 0:
    total += n
    n += -1
return total
"#,
        )
        .expect("signed-update while induction accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("signed-update while induction accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile signed-update while accumulation CFG");
        let result = unsafe { compiled.call_result_with_args(&[4]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 9);
    }

    #[test]
    fn result_abi_executes_commuted_update_while_induction_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"total = -1
while n > 0:
    total += n
    n = -1 + n
return total
"#,
        )
        .expect("commuted-update while induction accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("commuted-update while induction accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile commuted-update while accumulation CFG");
        let result = unsafe { compiled.call_result_with_args(&[4]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 9);
    }

    #[test]
    fn result_abi_executes_parameter_bound_while_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
while n > limit:
    total += n
    n -= 1
return total
"#,
        )
        .expect("parameter-bound while accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "limit".into()],
        )
        .expect("parameter-bound while accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile parameter-bound while accumulation CFG");
        let result = unsafe { compiled.call_result_with_args(&[5, 2]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 12);
    }

    #[test]
    fn result_abi_executes_parameter_step_while_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
while n > 0:
    total += step
    n -= tick
return total
"#,
        )
        .expect("parameter-step while accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "step".into(), "tick".into()],
        )
        .expect("parameter-step while accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile parameter-step while accumulation CFG");
        let result = unsafe { compiled.call_result_with_args(&[10, 4, 3]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 16);
    }

    #[test]
    fn result_abi_executes_local_step_while_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"tick = step
total = 0
while n > 0:
    total += tick
    n -= 1
return total
"#,
        )
        .expect("local-step while accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "step".into()],
        )
        .expect("local-step while accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile local-step while accumulation CFG");
        let result = unsafe { compiled.call_result_with_args(&[5, 3]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 15);
    }

    #[test]
    fn result_abi_executes_local_init_local_step_while_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"value = n
tick = step
total = 0
while value > 0:
    total += tick
    value -= 1
return total
"#,
        )
        .expect("local-init local-step while accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "step".into()],
        )
        .expect("local-init local-step while accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile local-init local-step while accumulation CFG");
        let result = unsafe { compiled.call_result_with_args(&[5, 3]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 15);
    }

    #[test]
    fn result_abi_executes_local_bound_counted_while_cfg() {
        let module = lucid_syntax::parse(
            r#"value = n
stop = limit
while value > stop:
    value -= 1
return value
"#,
        )
        .expect("local-bound counted while fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "limit".into()],
        )
        .expect("local-bound counted while should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile local-bound counted while CFG");
        let result = unsafe { compiled.call_result_with_args(&[5, 2]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 2);
    }

    #[test]
    fn result_abi_executes_counted_while_with_repeated_pass_tail() {
        let module = lucid_syntax::parse(
            r#"while n > 0:
    n -= 1
    pass
    pass
return n
"#,
        )
        .expect("repeated-pass counted while fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("repeated-pass counted while should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile repeated-pass counted while CFG");
        let result = unsafe { compiled.call_result_with_args(&[4]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 0);
    }

    #[test]
    fn result_abi_executes_signed_update_counted_while_cfg() {
        let module = lucid_syntax::parse(
            r#"while n > 0:
    n += -1
return n
"#,
        )
        .expect("signed-update counted while fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("signed-update counted while should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile signed-update counted while CFG");
        let result = unsafe { compiled.call_result_with_args(&[4]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 0);
    }

    #[test]
    fn result_abi_executes_parameter_step_counted_while_cfg() {
        let module = lucid_syntax::parse(
            r#"while n > 0:
    n -= step
return n
"#,
        )
        .expect("parameter-step counted while fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "step".into()],
        )
        .expect("parameter-step counted while should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile parameter-step counted while CFG");
        let result = unsafe { compiled.call_result_with_args(&[10, 3]) };
        assert!(result.is_ok());
        assert_eq!(result.value, -2);
    }

    #[test]
    fn result_abi_executes_local_step_counted_while_cfg() {
        let module = lucid_syntax::parse(
            r#"tick = step
while n > 0:
    n -= tick
return n
"#,
        )
        .expect("local-step counted while fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "step".into()],
        )
        .expect("local-step counted while should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile local-step counted while CFG");
        let result = unsafe { compiled.call_result_with_args(&[10, 3]) };
        assert!(result.is_ok());
        assert_eq!(result.value, -2);
    }

    #[test]
    fn result_abi_executes_local_init_local_step_counted_while_cfg() {
        let module = lucid_syntax::parse(
            r#"value = n
tick = step
while value > 0:
    value -= tick
return value
"#,
        )
        .expect("local-init local-step counted while fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "step".into()],
        )
        .expect("local-init local-step counted while should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile local-init local-step counted while CFG");
        let result = unsafe { compiled.call_result_with_args(&[10, 3]) };
        assert!(result.is_ok());
        assert_eq!(result.value, -2);
    }

    #[test]
    fn result_abi_executes_void_local_bound_counted_while_cfg() {
        let module = lucid_syntax::parse(
            r#"value = n
stop = limit
while value > stop:
    value -= 1
"#,
        )
        .expect("void local-bound counted while fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "limit".into()],
        )
        .expect("void local-bound counted while should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile void local-bound counted while CFG");
        let result = unsafe { compiled.call_result_with_args(&[5, 2]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 0);
    }

    #[test]
    fn result_abi_executes_local_induction_while_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
value = n
while value > 0:
    total += value
    value -= 1
return total
"#,
        )
        .expect("local-induction while accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("local-induction while accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile local-induction while accumulation CFG");
        let result = unsafe { compiled.call_result_with_args(&[5]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 15);
    }

    #[test]
    fn result_abi_executes_local_bound_while_accumulation_cfg() {
        let module = lucid_syntax::parse(
            r#"total = 0
value = n
stop = limit
while value > stop:
    total += value
    value -= 1
return total
"#,
        )
        .expect("local-bound while accumulation fixture should parse");
        let function = lucid_cir::Function::from_module_linear_with_params(
            &module,
            &["n".into(), "limit".into()],
        )
        .expect("local-bound while accumulation should lower");
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should compile local-bound while accumulation CFG");
        let result = unsafe { compiled.call_result_with_args(&[5, 2]) };
        assert!(result.is_ok());
        assert_eq!(result.value, 12);
    }

    #[test]
    fn lowers_verified_integer_cir_to_machine_code() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 40,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 2,
                    },
                    Instruction::Add {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        let entry = compile_integer_function(&function).expect("CIR should compile");
        assert_eq!(unsafe { entry.call() }, 42);
        assert_eq!(
            unsafe { entry.call_result() },
            crate::native_abi::NativeResult::ok(42)
        );
        let mut result = crate::native_abi::NativeResult::error(
            crate::native_abi::NativeErrorCode::InvalidOperation,
        );
        assert!(unsafe { entry.call_result_into(&mut result) });
        assert_eq!(result, crate::native_abi::NativeResult::ok(42));
        assert!(!unsafe { entry.call_result_into(std::ptr::null_mut()) });
    }

    #[test]
    fn lowers_void_cir_return_with_void_abi() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![],
                terminator: Terminator::Return(None),
            }],
        };
        let entry = compile_integer_function(&function).expect("void CIR should compile");
        unsafe { entry.call_void() };
        assert_eq!(
            unsafe { entry.call_result() },
            crate::native_abi::NativeResult::ok(0)
        );
        let mut result = crate::native_abi::NativeResult::ok(1);
        assert!(unsafe { entry.call_result_into(&mut result) });
        assert_eq!(result, crate::native_abi::NativeResult::ok(0));
    }

    #[test]
    fn lowers_bitwise_and_shift_cir_operations() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 5,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 3,
                    },
                    Instruction::BitXor {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                    Instruction::ConstInt {
                        result: ValueId(3),
                        value: 2,
                    },
                    Instruction::Shl {
                        result: ValueId(4),
                        left: ValueId(2),
                        right: ValueId(3),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(4))),
            }],
        };
        let entry = compile_integer_function(&function).expect("bitwise CIR should compile");
        assert_eq!(unsafe { entry.call() }, 24);
    }

    #[test]
    fn lowers_canonical_boolean_logic() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstBool {
                        result: ValueId(0),
                        value: true,
                    },
                    Instruction::ConstBool {
                        result: ValueId(1),
                        value: false,
                    },
                    Instruction::And {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                    Instruction::Not {
                        result: ValueId(3),
                        operand: ValueId(2),
                    },
                    Instruction::Or {
                        result: ValueId(4),
                        left: ValueId(2),
                        right: ValueId(3),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(4))),
            }],
        };
        let entry = compile_integer_function(&function).expect("boolean CIR should compile");
        assert_eq!(unsafe { entry.call() }, 1);
    }

    #[test]
    fn native_results_match_cir_reference_interpreter() {
        for (source, expected) in [
            ("x = 7 + 5 * 2", 17),
            ("x = (9 ^ 3) << 1", 20),
            ("x = 4 < 9", 1),
            ("x = 3 is 3", 1),
            ("x = 3 is not 4", 1),
            ("x = 7 + 5", 12),
            ("x = 7 - 12", -5),
            ("x = 7 * -3", -21),
            ("x = 1 << 3", 8),
            ("x = -8 >> 1", -4),
            ("x = -7", -7),
            ("x = not false", 1),
            ("x = true and false", 0),
            ("x = 2 ** 10", 1024),
        ] {
            let module = lucid_syntax::parse(source).expect("expression should parse");
            let cir = lucid_cir::Function::from_module(&module).expect("expression should lower");
            assert_eq!(cir.execute(), Ok(Some(expected)));
            let native = compile_integer_function(&cir).expect("expression should compile");
            assert_eq!(unsafe { native.call() }, expected);
        }
    }

    #[test]
    fn compiles_a_parsed_module_through_cir() {
        let module = lucid_syntax::parse("answer = 6 * 7\n").expect("module should parse");
        let native = compile_integer_module(&module).expect("module should compile");
        assert_eq!(unsafe { native.call() }, 42);
    }

    #[test]
    fn compiles_linear_bindings_through_shared_cir() {
        let module = lucid_syntax::parse("x = 6\ny = x * 7\n").expect("module should parse");
        let native = compile_integer_module(&module).expect("module should compile");
        assert_eq!(unsafe { native.call() }, 42);
    }

    #[test]
    fn compiles_dynamic_if_phi_module() {
        let module = lucid_syntax::parse("if 1 < 2:\n    x = 7 + 3\nelse:\n    x = 9\n")
            .expect("module should parse");
        let native = compile_integer_module(&module).expect("if module should compile");
        assert_eq!(unsafe { native.call() }, 10);
        let module =
            lucid_syntax::parse("if 1 < 0:\n    x = 1\nelif true:\n    x = 6\nelse:\n    x = 9\n")
                .expect("module should parse");
        let native = compile_integer_module(&module).expect("elif module should compile");
        assert_eq!(unsafe { native.call() }, 6);
        let module =
            lucid_syntax::parse("if 1 < 2:\n    x = 1 if false else 6\nelse:\n    x = 0\n")
                .expect("nested conditional module should parse");
        let native = compile_integer_module(&module).expect("nested conditional should compile");
        assert_eq!(unsafe { native.call() }, 6);
    }

    #[test]
    fn lowers_jump_control_flow() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![
                lucid_cir::Block {
                    id: lucid_cir::BlockId(0),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(0),
                        value: 1,
                    }],
                    terminator: Terminator::Jump(lucid_cir::BlockId(1)),
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(1),
                    instructions: vec![],
                    terminator: Terminator::Return(Some(ValueId(0))),
                },
            ],
        };
        let entry = compile_integer_function(&function).expect("jump CIR should compile");
        assert_eq!(unsafe { entry.call() }, 1);
    }

    #[test]
    fn lowers_branch_control_flow() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![
                lucid_cir::Block {
                    id: lucid_cir::BlockId(0),
                    instructions: vec![Instruction::ConstBool {
                        result: ValueId(0),
                        value: true,
                    }],
                    terminator: Terminator::Branch {
                        condition: ValueId(0),
                        then_block: lucid_cir::BlockId(1),
                        else_block: lucid_cir::BlockId(2),
                    },
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(1),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(1),
                        value: 7,
                    }],
                    terminator: Terminator::Return(Some(ValueId(1))),
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(2),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(2),
                        value: 9,
                    }],
                    terminator: Terminator::Return(Some(ValueId(2))),
                },
            ],
        };
        let entry = compile_integer_function(&function).expect("branch CIR should compile");
        assert_eq!(unsafe { entry.call() }, 7);
    }

    #[test]
    fn lowers_phi_merge_values() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![
                lucid_cir::Block {
                    id: lucid_cir::BlockId(0),
                    instructions: vec![Instruction::ConstBool {
                        result: ValueId(0),
                        value: true,
                    }],
                    terminator: Terminator::Branch {
                        condition: ValueId(0),
                        then_block: lucid_cir::BlockId(1),
                        else_block: lucid_cir::BlockId(2),
                    },
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(1),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(1),
                        value: 7,
                    }],
                    terminator: Terminator::Jump(lucid_cir::BlockId(3)),
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(2),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(2),
                        value: 9,
                    }],
                    terminator: Terminator::Jump(lucid_cir::BlockId(3)),
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(3),
                    instructions: vec![Instruction::Phi {
                        result: ValueId(3),
                        incomings: vec![
                            (lucid_cir::BlockId(1), ValueId(1)),
                            (lucid_cir::BlockId(2), ValueId(2)),
                        ],
                    }],
                    terminator: Terminator::Return(Some(ValueId(3))),
                },
            ],
        };
        let entry = compile_integer_function(&function).expect("Phi CIR should compile");
        assert_eq!(unsafe { entry.call() }, 7);
    }

    #[test]
    fn rejects_integer_division_without_error_abi() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 1,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 2,
                    },
                    Instruction::Div {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        assert!(matches!(
            compile_integer_function(&function),
            Err(CraneliftError::UnsupportedInstruction(message))
                if message.contains("NativeResult ABI")
        ));
    }

    #[test]
    fn result_abi_returns_value_and_recoverable_division_error() {
        let make = |right: i64| Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 8,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: right,
                    },
                    Instruction::Div {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        let success = compile_integer_result_function(&make(2)).expect("result ABI should compile");
        assert_eq!(
            unsafe { success.call_result() },
            crate::native_abi::NativeResult::ok(4)
        );
        let failure = compile_integer_result_function(&make(0)).expect("result ABI should compile");
        assert_eq!(
            unsafe { failure.call_result() }.error,
            crate::native_abi::NativeErrorCode::DivisionByZero
        );

        let overflow = compile_integer_result_function(&Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: i64::MIN,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: -1,
                    },
                    Instruction::Div {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        })
        .expect("overflowing division should use the recoverable ABI");
        assert_eq!(
            unsafe { overflow.call_result() }.error,
            crate::native_abi::NativeErrorCode::ArithmeticOverflow
        );
    }

    #[test]
    fn result_abi_compiles_checked_straight_line_arithmetic_chain() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 10,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 2,
                    },
                    Instruction::Add {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                    Instruction::ConstInt {
                        result: ValueId(3),
                        value: 3,
                    },
                    Instruction::Mul {
                        result: ValueId(4),
                        left: ValueId(2),
                        right: ValueId(3),
                    },
                    Instruction::ConstInt {
                        result: ValueId(5),
                        value: 4,
                    },
                    Instruction::FloorDiv {
                        result: ValueId(6),
                        left: ValueId(4),
                        right: ValueId(5),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(6))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("straight-line arithmetic should compile");
        assert_eq!(
            unsafe { compiled.call_result() },
            crate::native_abi::NativeResult::ok(9)
        );
    }

    #[test]
    fn result_abi_returns_success_for_non_division_chain() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 6,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 7,
                    },
                    Instruction::Mul {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("successful arithmetic should use the result ABI");
        assert_eq!(
            unsafe { compiled.call_result() },
            crate::native_abi::NativeResult::ok(42)
        );
    }

    #[test]
    fn result_abi_returns_arithmetic_overflow_instead_of_trapping() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: i64::MAX,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 1,
                    },
                    Instruction::Add {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("overflowing arithmetic should use the recoverable ABI");
        assert_eq!(
            unsafe { compiled.call_result() },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::ArithmeticOverflow
            )
        );
    }

    #[test]
    fn result_abi_reports_overflow_in_a_selected_cfg_branch() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![
                lucid_cir::Block {
                    id: lucid_cir::BlockId(0),
                    instructions: vec![Instruction::ConstBool {
                        result: ValueId(0),
                        value: true,
                    }],
                    terminator: Terminator::Branch {
                        condition: ValueId(0),
                        then_block: lucid_cir::BlockId(1),
                        else_block: lucid_cir::BlockId(2),
                    },
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(1),
                    instructions: vec![
                        Instruction::ConstInt {
                            result: ValueId(1),
                            value: i64::MAX,
                        },
                        Instruction::ConstInt {
                            result: ValueId(2),
                            value: 1,
                        },
                        Instruction::Add {
                            result: ValueId(3),
                            left: ValueId(1),
                            right: ValueId(2),
                        },
                    ],
                    terminator: Terminator::Return(Some(ValueId(3))),
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(2),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(4),
                        value: 5,
                    }],
                    terminator: Terminator::Return(Some(ValueId(4))),
                },
            ],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("checked overflow branch should use the result ABI");
        assert_eq!(
            unsafe { compiled.call_result() },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::ArithmeticOverflow
            )
        );
    }

    #[test]
    fn result_abi_returns_power_overflow_instead_of_trapping() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 2,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 63,
                    },
                    Instruction::Pow {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("overflowing power should use the recoverable ABI");
        assert_eq!(
            unsafe { compiled.call_result() },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::ArithmeticOverflow
            )
        );
    }

    #[test]
    fn result_abi_returns_negative_exponent_instead_of_rejecting() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 2,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: -1,
                    },
                    Instruction::Pow {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("negative exponent should use the recoverable ABI");
        assert_eq!(
            unsafe { compiled.call_result() },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::NegativeExponent
            )
        );
    }

    #[test]
    fn result_abi_returns_invalid_operation_for_oversized_constant_power() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 2,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 1025,
                    },
                    Instruction::Pow {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("oversized power should use the recoverable ABI");
        assert_eq!(
            unsafe { compiled.call_result() },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::InvalidOperation
            )
        );
    }

    #[test]
    fn result_abi_supports_dynamic_integer_power() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::Param {
                        result: ValueId(0),
                        index: 0,
                    },
                    Instruction::Param {
                        result: ValueId(1),
                        index: 1,
                    },
                    Instruction::Pow {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("dynamic integer power should use the recoverable ABI");
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[2, 10]) },
            crate::native_abi::NativeResult::ok(1024)
        );
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[2, -1]) },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::NegativeExponent
            )
        );
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[2, 1025]) },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::InvalidOperation
            )
        );
    }

    #[test]
    fn result_abi_returns_negation_overflow_instead_of_trapping() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: i64::MIN,
                    },
                    Instruction::Neg {
                        result: ValueId(1),
                        operand: ValueId(0),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(1))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("overflowing negation should use the recoverable ABI");
        assert_eq!(
            unsafe { compiled.call_result() },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::ArithmeticOverflow
            )
        );
    }

    #[test]
    fn result_abi_returns_shift_errors_instead_of_trapping() {
        for (operation, expected) in [
            (
                Instruction::Shl {
                    result: ValueId(2),
                    left: ValueId(0),
                    right: ValueId(1),
                },
                crate::native_abi::NativeErrorCode::ArithmeticOverflow,
            ),
            (
                Instruction::Shr {
                    result: ValueId(2),
                    left: ValueId(0),
                    right: ValueId(1),
                },
                crate::native_abi::NativeErrorCode::ArithmeticOverflow,
            ),
        ] {
            let right = if matches!(operation, Instruction::Shl { .. }) {
                63
            } else {
                64
            };
            let function = Function {
                entry: lucid_cir::BlockId(0),
                blocks: vec![lucid_cir::Block {
                    id: lucid_cir::BlockId(0),
                    instructions: vec![
                        Instruction::ConstInt {
                            result: ValueId(0),
                            value: 1,
                        },
                        Instruction::ConstInt {
                            result: ValueId(1),
                            value: right,
                        },
                        operation,
                    ],
                    terminator: Terminator::Return(Some(ValueId(2))),
                }],
            };
            let compiled = compile_integer_result_function(&function)
                .expect("checked shifts should use the recoverable ABI");
            assert_eq!(
                unsafe { compiled.call_result() },
                crate::native_abi::NativeResult::error(expected)
            );
        }
    }

    #[test]
    fn result_abi_preserves_first_error_across_later_operations() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: i64::MAX,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 1,
                    },
                    Instruction::Add {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                    Instruction::ConstInt {
                        result: ValueId(3),
                        value: 2,
                    },
                    Instruction::Div {
                        result: ValueId(4),
                        left: ValueId(2),
                        right: ValueId(3),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(4))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("checked arithmetic chain should compile");
        assert_eq!(
            unsafe { compiled.call_result() },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::ArithmeticOverflow
            )
        );
    }

    #[test]
    fn result_abi_preserves_first_error_before_multiplication_edge_case() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: i64::MAX,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 1,
                    },
                    Instruction::Add {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                    Instruction::ConstInt {
                        result: ValueId(3),
                        value: i64::MIN,
                    },
                    Instruction::ConstInt {
                        result: ValueId(4),
                        value: -1,
                    },
                    Instruction::Mul {
                        result: ValueId(5),
                        left: ValueId(3),
                        right: ValueId(4),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(5))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("checked multiplication chain should compile");
        assert_eq!(
            unsafe { compiled.call_result() },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::ArithmeticOverflow
            )
        );
    }

    #[test]
    fn result_abi_reports_sub_and_mul_overflow() {
        for (operation, expected) in [
            (
                Instruction::Sub {
                    result: ValueId(2),
                    left: ValueId(0),
                    right: ValueId(1),
                },
                crate::native_abi::NativeErrorCode::ArithmeticOverflow,
            ),
            (
                Instruction::Mul {
                    result: ValueId(2),
                    left: ValueId(0),
                    right: ValueId(1),
                },
                crate::native_abi::NativeErrorCode::ArithmeticOverflow,
            ),
        ] {
            let (left, right) = if matches!(&operation, Instruction::Sub { .. }) {
                (i64::MIN, 1)
            } else {
                (i64::MAX, 2)
            };
            let function = Function {
                entry: lucid_cir::BlockId(0),
                blocks: vec![lucid_cir::Block {
                    id: lucid_cir::BlockId(0),
                    instructions: vec![
                        Instruction::ConstInt {
                            result: ValueId(0),
                            value: left,
                        },
                        Instruction::ConstInt {
                            result: ValueId(1),
                            value: right,
                        },
                        operation,
                    ],
                    terminator: Terminator::Return(Some(ValueId(2))),
                }],
            };
            let compiled = compile_integer_result_function(&function)
                .expect("checked subtraction/multiplication should compile");
            assert_eq!(
                unsafe { compiled.call_result() },
                crate::native_abi::NativeResult::error(expected)
            );
        }
    }

    #[test]
    fn result_abi_supports_non_division_control_flow() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![
                lucid_cir::Block {
                    id: lucid_cir::BlockId(0),
                    instructions: vec![Instruction::ConstBool {
                        result: ValueId(0),
                        value: true,
                    }],
                    terminator: Terminator::Branch {
                        condition: ValueId(0),
                        then_block: lucid_cir::BlockId(1),
                        else_block: lucid_cir::BlockId(2),
                    },
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(1),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(1),
                        value: 7,
                    }],
                    terminator: Terminator::Jump(lucid_cir::BlockId(3)),
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(2),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(2),
                        value: 9,
                    }],
                    terminator: Terminator::Jump(lucid_cir::BlockId(3)),
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(3),
                    instructions: vec![Instruction::Phi {
                        result: ValueId(3),
                        incomings: vec![
                            (lucid_cir::BlockId(1), ValueId(1)),
                            (lucid_cir::BlockId(2), ValueId(2)),
                        ],
                    }],
                    terminator: Terminator::Return(Some(ValueId(3))),
                },
            ],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should support control flow without recoverable ops");
        assert_eq!(
            unsafe { compiled.call_result() },
            crate::native_abi::NativeResult::ok(7)
        );
    }

    #[test]
    fn result_abi_supports_mixed_value_and_void_returns() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![
                lucid_cir::Block {
                    id: lucid_cir::BlockId(0),
                    instructions: vec![Instruction::Param {
                        result: ValueId(0),
                        index: 0,
                    }],
                    terminator: Terminator::Branch {
                        condition: ValueId(0),
                        then_block: lucid_cir::BlockId(1),
                        else_block: lucid_cir::BlockId(2),
                    },
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(1),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(1),
                        value: 42,
                    }],
                    terminator: Terminator::Return(Some(ValueId(1))),
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(2),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(None),
                },
            ],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("result ABI should support mixed return edges");
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[1]) },
            crate::native_abi::NativeResult::ok(42)
        );
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[0]) },
            crate::native_abi::NativeResult::ok(0)
        );
    }

    #[test]
    fn result_abi_propagates_division_errors_from_a_returning_branch() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![
                lucid_cir::Block {
                    id: lucid_cir::BlockId(0),
                    instructions: vec![
                        Instruction::Param {
                            result: ValueId(0),
                            index: 0,
                        },
                        Instruction::Param {
                            result: ValueId(1),
                            index: 1,
                        },
                        Instruction::Param {
                            result: ValueId(2),
                            index: 2,
                        },
                    ],
                    terminator: Terminator::Branch {
                        condition: ValueId(0),
                        then_block: lucid_cir::BlockId(1),
                        else_block: lucid_cir::BlockId(2),
                    },
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(1),
                    instructions: vec![Instruction::Div {
                        result: ValueId(3),
                        left: ValueId(1),
                        right: ValueId(2),
                    }],
                    terminator: Terminator::Return(Some(ValueId(3))),
                },
                lucid_cir::Block {
                    id: lucid_cir::BlockId(2),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(4),
                        value: 7,
                    }],
                    terminator: Terminator::Return(Some(ValueId(4))),
                },
            ],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("a directly returning division branch should use the result ABI");
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[1, 42, 2]) },
            crate::native_abi::NativeResult::ok(21)
        );
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[1, 42, 0]) },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::DivisionByZero
            )
        );
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[0, 42, 0]) },
            crate::native_abi::NativeResult::ok(7)
        );
    }

    #[test]
    fn result_abi_preserves_errors_across_multiple_divisions_in_one_block() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::Param {
                        result: ValueId(0),
                        index: 0,
                    },
                    Instruction::Param {
                        result: ValueId(1),
                        index: 1,
                    },
                    Instruction::Div {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                    Instruction::Div {
                        result: ValueId(3),
                        left: ValueId(2),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(3))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("multiple checked divisions should use the result ABI");
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[42, 0]) },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::DivisionByZero
            )
        );
    }

    #[test]
    fn native_backend_executes_parameterized_cir() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::Param {
                        result: ValueId(0),
                        index: 0,
                    },
                    Instruction::Param {
                        result: ValueId(1),
                        index: 1,
                    },
                    Instruction::Add {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        let compiled =
            compile_integer_function(&function).expect("parameterized CIR should compile");
        assert_eq!(unsafe { compiled.call_with_args(&[20, 22]) }, 42);
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[20, 22]) },
            crate::native_abi::NativeResult::ok(42)
        );
        let mut result = crate::native_abi::NativeResult::error(
            crate::native_abi::NativeErrorCode::InvalidOperation,
        );
        assert!(unsafe { compiled.call_result_into_with_args(&[20, 22], &mut result) });
        assert_eq!(result, crate::native_abi::NativeResult::ok(42));
    }

    #[test]
    fn native_backend_invokes_parameterized_void_cir() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![Instruction::Param {
                    result: ValueId(0),
                    index: 0,
                }],
                terminator: Terminator::Return(None),
            }],
        };
        let compiled = compile_integer_function(&function).expect("void CIR should compile");
        assert!(!compiled.returns_value());
        unsafe { compiled.call_void_with_args(&[42]) };
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[42]) },
            crate::native_abi::NativeResult::ok(0)
        );
        let mut result = crate::native_abi::NativeResult::error(
            crate::native_abi::NativeErrorCode::InvalidOperation,
        );
        assert!(unsafe { compiled.call_result_into_with_args(&[42], &mut result) });
        assert_eq!(result, crate::native_abi::NativeResult::ok(0));
    }

    #[test]
    fn native_backend_executes_three_parameter_cir() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::Param {
                        result: ValueId(0),
                        index: 0,
                    },
                    Instruction::Param {
                        result: ValueId(1),
                        index: 1,
                    },
                    Instruction::Param {
                        result: ValueId(2),
                        index: 2,
                    },
                    Instruction::Add {
                        result: ValueId(3),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                    Instruction::Add {
                        result: ValueId(4),
                        left: ValueId(3),
                        right: ValueId(2),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(4))),
            }],
        };
        let compiled =
            compile_integer_function(&function).expect("three-parameter CIR should compile");
        assert_eq!(unsafe { compiled.call_with_args(&[10, 20, 12]) }, 42);
    }

    #[test]
    fn result_abi_executes_parameterized_cir_with_arguments() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::Param {
                        result: ValueId(0),
                        index: 0,
                    },
                    Instruction::Param {
                        result: ValueId(1),
                        index: 1,
                    },
                    Instruction::Div {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("parameterized result ABI should compile");
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[84, 2]) },
            crate::native_abi::NativeResult::ok(42)
        );
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[84, 0]) }.error,
            crate::native_abi::NativeErrorCode::DivisionByZero
        );
    }

    #[test]
    fn result_abi_returns_success_for_parameterized_arithmetic() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::Param {
                        result: ValueId(0),
                        index: 0,
                    },
                    Instruction::Param {
                        result: ValueId(1),
                        index: 1,
                    },
                    Instruction::Mul {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("parameterized successful result ABI should compile");
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[6, 7]) },
            crate::native_abi::NativeResult::ok(42)
        );
    }

    #[test]
    fn checked_legacy_adapters_report_incompatible_signatures() {
        let value = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![Instruction::ConstInt {
                    result: ValueId(0),
                    value: 7,
                }],
                terminator: Terminator::Return(Some(ValueId(0))),
            }],
        };
        let compiled = compile_integer_function(&value).expect("value function should compile");
        assert_eq!(unsafe { compiled.try_call_with_args(&[]) }, Ok(7));
        assert_eq!(
            unsafe { compiled.try_call_void_with_args(&[]) },
            Err(crate::native_abi::NativeErrorCode::InvalidOperation)
        );
    }

    #[test]
    fn result_adapter_reports_argument_count_without_panicking() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![Instruction::Param {
                    result: ValueId(0),
                    index: 0,
                }],
                terminator: Terminator::Return(Some(ValueId(0))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("parameterized result ABI should compile");
        let result = unsafe { compiled.call_result_with_args(&[]) };
        assert_eq!(
            result.error,
            crate::native_abi::NativeErrorCode::UnexpectedArgumentCount
        );
        assert_eq!(
            unsafe { compiled.call_result() }.error,
            crate::native_abi::NativeErrorCode::UnexpectedArgumentCount
        );
        let mut written = crate::native_abi::NativeResult::ok(99);
        let success = unsafe { compiled.call_result_into_with_args(&[], &mut written) };
        assert!(!success);
        assert_eq!(
            written.error,
            crate::native_abi::NativeErrorCode::UnexpectedArgumentCount
        );
    }

    #[test]
    fn native_backend_executes_eight_parameter_cir() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::Param {
                        result: ValueId(0),
                        index: 0,
                    },
                    Instruction::Param {
                        result: ValueId(1),
                        index: 1,
                    },
                    Instruction::Param {
                        result: ValueId(2),
                        index: 2,
                    },
                    Instruction::Param {
                        result: ValueId(3),
                        index: 3,
                    },
                    Instruction::Param {
                        result: ValueId(4),
                        index: 4,
                    },
                    Instruction::Param {
                        result: ValueId(5),
                        index: 5,
                    },
                    Instruction::Param {
                        result: ValueId(6),
                        index: 6,
                    },
                    Instruction::Param {
                        result: ValueId(7),
                        index: 7,
                    },
                    Instruction::Add {
                        result: ValueId(8),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                    Instruction::Add {
                        result: ValueId(9),
                        left: ValueId(8),
                        right: ValueId(2),
                    },
                    Instruction::Add {
                        result: ValueId(10),
                        left: ValueId(9),
                        right: ValueId(3),
                    },
                    Instruction::Add {
                        result: ValueId(11),
                        left: ValueId(10),
                        right: ValueId(4),
                    },
                    Instruction::Add {
                        result: ValueId(12),
                        left: ValueId(11),
                        right: ValueId(5),
                    },
                    Instruction::Add {
                        result: ValueId(13),
                        left: ValueId(12),
                        right: ValueId(6),
                    },
                    Instruction::Add {
                        result: ValueId(14),
                        left: ValueId(13),
                        right: ValueId(7),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(14))),
            }],
        };
        let compiled =
            compile_integer_function(&function).expect("eight-parameter CIR should compile");
        assert_eq!(
            unsafe { compiled.call_with_args(&[1, 2, 3, 4, 5, 6, 7, 14]) },
            42
        );
    }

    #[test]
    fn native_backend_executes_sixteen_parameter_abi_boundary() {
        let mut instructions = Vec::new();
        for index in 0..16u32 {
            instructions.push(Instruction::Param {
                result: ValueId(index),
                index,
            });
        }
        let mut accumulator = ValueId(0);
        for index in 1..16u32 {
            let result = ValueId(16 + index - 1);
            instructions.push(Instruction::Add {
                result,
                left: accumulator,
                right: ValueId(index),
            });
            accumulator = result;
        }
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions,
                terminator: Terminator::Return(Some(accumulator)),
            }],
        };
        let compiled =
            compile_integer_function(&function).expect("sixteen-parameter CIR should compile");
        assert_eq!(compiled.parameter_count(), 16);
        let args = (1..=16).collect::<Vec<_>>();
        assert_eq!(unsafe { compiled.call_with_args(&args) }, 136);
    }

    #[test]
    fn native_backend_executes_void_sixteen_parameter_abi_boundary() {
        let mut instructions = Vec::new();
        for index in 0..16u32 {
            instructions.push(Instruction::Param {
                result: ValueId(index),
                index,
            });
        }
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions,
                terminator: Terminator::Return(None),
            }],
        };
        let compiled =
            compile_integer_function(&function).expect("sixteen-parameter void CIR should compile");
        assert_eq!(compiled.parameter_count(), 16);
        let args = (1..=16).collect::<Vec<_>>();
        unsafe { compiled.call_void_with_args(&args) };
        assert_eq!(unsafe { compiled.try_call_void_with_args(&args) }, Ok(()));
        assert_eq!(
            unsafe { compiled.try_call_with_args(&args) },
            Err(crate::native_abi::NativeErrorCode::InvalidOperation)
        );
        assert_eq!(
            unsafe { compiled.try_call_void_with_args(&[1, 2]) },
            Err(crate::native_abi::NativeErrorCode::UnexpectedArgumentCount)
        );
        assert_eq!(
            unsafe { compiled.try_call_void_with_args(&(1..=17).collect::<Vec<_>>()) },
            Err(crate::native_abi::NativeErrorCode::UnexpectedArgumentCount)
        );
    }

    #[test]
    fn native_backend_rejects_seventeen_parameter_abi_boundary() {
        let mut instructions = Vec::new();
        for index in 0..17u32 {
            instructions.push(Instruction::Param {
                result: ValueId(index),
                index,
            });
        }
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions,
                terminator: Terminator::Return(Some(ValueId(0))),
            }],
        };
        match compile_integer_function(&function) {
            Err(CraneliftError::UnsupportedInstruction(message)) => assert_eq!(
                message,
                "native integer invocation currently supports at most sixteen parameters"
                    .to_string()
            ),
            Ok(_) => panic!("expected compile to be rejected"),
            Err(other) => panic!("expected unsupported-parameter error, got {other:?}"),
        }
    }

    #[test]
    fn native_result_abi_executes_sixteen_parameter_boundary() {
        let mut instructions = Vec::new();
        for index in 0..16u32 {
            instructions.push(Instruction::Param {
                result: ValueId(index),
                index,
            });
        }
        let mut accumulator = ValueId(0);
        for index in 1..16u32 {
            let result = ValueId(16 + index - 1);
            instructions.push(Instruction::Add {
                result,
                left: accumulator,
                right: ValueId(index),
            });
            accumulator = result;
        }
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions,
                terminator: Terminator::Return(Some(accumulator)),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("sixteen-parameter result CIR should compile");
        let args = (1..=16).collect::<Vec<_>>();
        assert_eq!(
            unsafe { compiled.call_result_with_args(&args) },
            crate::native_abi::NativeResult::ok(136)
        );
        let mut out = crate::native_abi::NativeResult::error(
            crate::native_abi::NativeErrorCode::InvalidOperation,
        );
        assert!(unsafe { compiled.call_result_into_with_args(&args, &mut out) });
        assert_eq!(out, crate::native_abi::NativeResult::ok(136));
        assert!(!unsafe { compiled.call_result_into_with_args(&args, std::ptr::null_mut()) });
    }

    #[test]
    fn native_result_abi_rejects_seventeen_parameter_abi_boundary() {
        let mut instructions = Vec::new();
        for index in 0..17u32 {
            instructions.push(Instruction::Param {
                result: ValueId(index),
                index,
            });
        }
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions,
                terminator: Terminator::Return(Some(ValueId(0))),
            }],
        };
        match compile_integer_result_function(&function) {
            Err(CraneliftError::UnsupportedInstruction(message)) => assert_eq!(
                message,
                "native integer invocation currently supports at most sixteen parameters"
                    .to_string()
            ),
            Ok(_) => panic!("expected result compile to be rejected"),
            Err(other) => panic!("expected unsupported-parameter error, got {other:?}"),
        }
    }

    #[test]
    fn result_abi_allows_division_before_later_checked_arithmetic() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 8,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 2,
                    },
                    Instruction::Div {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                    Instruction::ConstInt {
                        result: ValueId(3),
                        value: 1,
                    },
                    Instruction::Add {
                        result: ValueId(4),
                        left: ValueId(2),
                        right: ValueId(3),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(4))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("division followed by checked arithmetic should compile");
        assert_eq!(
            unsafe { compiled.call_result() },
            crate::native_abi::NativeResult::ok(5)
        );
    }

    #[test]
    fn result_abi_allows_bitwise_operations_around_division() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::Param {
                        result: ValueId(0),
                        index: 0,
                    },
                    Instruction::Param {
                        result: ValueId(1),
                        index: 1,
                    },
                    Instruction::Div {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                    Instruction::ConstInt {
                        result: ValueId(3),
                        value: 1,
                    },
                    Instruction::BitXor {
                        result: ValueId(4),
                        left: ValueId(2),
                        right: ValueId(3),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(4))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("bitwise operations around division should compile");
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[8, 2]) },
            crate::native_abi::NativeResult::ok(5)
        );
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[8, 0]) },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::DivisionByZero
            )
        );
    }

    #[test]
    fn result_abi_allows_comparisons_after_division() {
        let function = Function {
            entry: lucid_cir::BlockId(0),
            blocks: vec![lucid_cir::Block {
                id: lucid_cir::BlockId(0),
                instructions: vec![
                    Instruction::Param {
                        result: ValueId(0),
                        index: 0,
                    },
                    Instruction::Param {
                        result: ValueId(1),
                        index: 1,
                    },
                    Instruction::Div {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                    Instruction::ConstInt {
                        result: ValueId(3),
                        value: 4,
                    },
                    Instruction::CmpEq {
                        result: ValueId(4),
                        left: ValueId(2),
                        right: ValueId(3),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(4))),
            }],
        };
        let compiled = compile_integer_result_function(&function)
            .expect("comparisons after division should compile");
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[8, 2]) },
            crate::native_abi::NativeResult::ok(1)
        );
        assert_eq!(
            unsafe { compiled.call_result_with_args(&[8, 0]) },
            crate::native_abi::NativeResult::error(
                crate::native_abi::NativeErrorCode::DivisionByZero
            )
        );
    }
}
