//! The fold loop compiled with LLVM 21 through its C API: IR built from the
//! step, optimised by LLVM's `default<O3>` pipeline for the host CPU and
//! its features, and linked into the process by ORC's LLJIT.
//!
//! The IR carries no fast-math flags, so LLVM neither reassociates nor
//! contracts a product and a sum into a fused multiply-add: the target
//! machine fuses only operations marked contractable, and none are.

use std::ffi::{CStr, CString};
use std::ptr::null_mut;
use std::sync::{Mutex, OnceLock};

use llvm_sys::analysis::{LLVMVerifierFailureAction, LLVMVerifyModule};
use llvm_sys::core::*;
use llvm_sys::error::{LLVMDisposeErrorMessage, LLVMErrorRef, LLVMGetErrorMessage};
use llvm_sys::orc2::lljit::*;
use llvm_sys::orc2::*;
use llvm_sys::prelude::*;
use llvm_sys::target::*;
use llvm_sys::target_machine::*;
use llvm_sys::transforms::pass_builder::*;
use llvm_sys::{LLVMAttributeFunctionIndex, LLVMIntPredicate, LLVMRealPredicate};

use super::map::{Input, Map, MapRaw, Op};
use super::{Bin, Kernel, Node, Raw, Step, Un};
use crate::scalar;

/// Compile a step's loop. None where LLVM has no backend for the host or
/// any stage of the compilation fails.
pub(super) fn compile(step: &Step) -> Option<Kernel> {
    let jit = jit()?;
    let mut jit = jit.lock().ok()?;
    // SAFETY: the LLJIT instance and its target machine are used only
    // under this lock.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { jit.compile(step) }))
        .ok()
        .flatten()
}

/// The process's one LLJIT instance, made on first use; None where the
/// host's target cannot be initialised.
fn jit() -> Option<&'static Mutex<Jit>> {
    static JIT: OnceLock<Option<Mutex<Jit>>> = OnceLock::new();
    // SAFETY: LLVM's C API is called as its headers document.
    JIT.get_or_init(|| unsafe { Jit::new() }.map(Mutex::new)).as_ref()
}

/// An LLJIT instance targeting the host, and a target machine of the same
/// CPU, features and level for the IR passes.
struct Jit {
    jit: LLVMOrcLLJITRef,
    tm: LLVMTargetMachineRef,
    triple: CString,
    /// Loops compiled so far: the next one's symbol is numbered from it.
    count: u64,
}

// SAFETY: the raw LLVM handles are owned by this value alone and are used
// only through the mutex that holds it.
unsafe impl Send for Jit {}

/// Ok, or the error's message dropped and None.
unsafe fn ok(e: LLVMErrorRef) -> Option<()> {
    if e.is_null() {
        return Some(());
    }
    // SAFETY: a non-null error is consumed by taking its message.
    unsafe {
        let m = LLVMGetErrorMessage(e);
        LLVMDisposeErrorMessage(m);
    }
    None
}

/// The host's triple, CPU name and CPU features.
unsafe fn host() -> (CString, CString, CString) {
    unsafe {
        let take = |p: *mut std::ffi::c_char| {
            let s = CStr::from_ptr(p).to_owned();
            LLVMDisposeMessage(p);
            s
        };
        (take(LLVMGetDefaultTargetTriple()), take(LLVMGetHostCPUName()), take(LLVMGetHostCPUFeatures()))
    }
}

/// A target machine for the host CPU and all its features, optimising
/// aggressively.
unsafe fn target_machine() -> Option<LLVMTargetMachineRef> {
    unsafe {
        let (t, cpu, features) = host();
        let mut target = null_mut();
        let mut err = null_mut();
        if LLVMGetTargetFromTriple(t.as_ptr(), &mut target, &mut err) != 0 {
            LLVMDisposeMessage(err);
            return None;
        }
        let tm = LLVMCreateTargetMachine(
            target,
            t.as_ptr(),
            cpu.as_ptr(),
            features.as_ptr(),
            LLVMCodeGenOptLevel::LLVMCodeGenLevelAggressive,
            LLVMRelocMode::LLVMRelocDefault,
            LLVMCodeModel::LLVMCodeModelJITDefault,
        );
        (!tm.is_null()).then_some(tm)
    }
}

impl Jit {
    unsafe fn new() -> Option<Jit> {
        unsafe {
            if LLVM_InitializeNativeTarget() != 0 || LLVM_InitializeNativeAsmPrinter() != 0 {
                return None;
            }
            let tm = target_machine()?;
            // The builder takes ownership of the machine it is made from.
            let jtmb = LLVMOrcJITTargetMachineBuilderCreateFromTargetMachine(target_machine()?);
            let b = LLVMOrcCreateLLJITBuilder();
            LLVMOrcLLJITBuilderSetJITTargetMachineBuilder(b, jtmb);
            let mut jit = null_mut();
            // Takes ownership of the builder, success or not.
            if ok(LLVMOrcCreateLLJIT(&mut jit, b)).is_none() {
                LLVMDisposeTargetMachine(tm);
                return None;
            }
            Some(Jit { jit, tm, triple: host().0, count: 0 })
        }
    }

    unsafe fn compile(&mut self, step: &Step) -> Option<Kernel> {
        // SAFETY: the symbol is the function `build` defines with exactly
        // [`Raw`]'s signature under the C calling convention.
        unsafe {
            let addr = self.add("libjay_fold", |ctx, m, name| build(ctx, m, step, name))?;
            Some(Kernel(std::mem::transmute::<usize, Raw>(addr)))
        }
    }

    unsafe fn compile_map(&mut self, map: &Map) -> Option<MapRaw> {
        // SAFETY: the symbol is the function `build_map` defines with
        // exactly [`MapRaw`]'s signature under the C calling convention.
        unsafe {
            let addr = self.add("libjay_map", |ctx, m, name| build_map(ctx, m, map, name))?;
            Some(std::mem::transmute::<usize, MapRaw>(addr))
        }
    }

    /// A fresh module for the host, holding the one function `define` adds
    /// under the name it is handed, verified and optimised. None where
    /// either fails.
    unsafe fn module(
        &mut self,
        prefix: &str,
        define: impl FnOnce(LLVMContextRef, LLVMModuleRef, &CStr),
    ) -> Option<(LLVMContextRef, LLVMModuleRef, CString)> {
        unsafe {
            self.count += 1;
            let name = CString::new(format!("{prefix}_{}", self.count)).ok()?;
            let ctx = LLVMContextCreate();
            let m = LLVMModuleCreateWithNameInContext(c"libjay".as_ptr(), ctx);
            LLVMSetTarget(m, self.triple.as_ptr());
            let dl = LLVMCreateTargetDataLayout(self.tm);
            LLVMSetModuleDataLayout(m, dl);
            LLVMDisposeTargetData(dl);
            define(ctx, m, &name);
            let mut msg = null_mut();
            let broken = LLVMVerifyModule(
                m,
                LLVMVerifierFailureAction::LLVMReturnStatusAction,
                &mut msg,
            ) != 0;
            if !msg.is_null() {
                LLVMDisposeMessage(msg);
            }
            let passed = !broken && {
                let opts = LLVMCreatePassBuilderOptions();
                let r = ok(LLVMRunPasses(m, c"default<O3>".as_ptr(), self.tm, opts));
                LLVMDisposePassBuilderOptions(opts);
                r.is_some()
            };
            if !passed {
                LLVMDisposeModule(m);
                LLVMContextDispose(ctx);
                return None;
            }
            Some((ctx, m, name))
        }
    }

    /// Compile the function `define` adds and link it into the process;
    /// its address.
    unsafe fn add(
        &mut self,
        prefix: &str,
        define: impl FnOnce(LLVMContextRef, LLVMModuleRef, &CStr),
    ) -> Option<usize> {
        unsafe {
            let (ctx, m, name) = self.module(prefix, define)?;
            let tsctx = LLVMOrcCreateNewThreadSafeContextFromLLVMContext(ctx);
            let tsm = LLVMOrcCreateNewThreadSafeModule(m, tsctx);
            LLVMOrcDisposeThreadSafeContext(tsctx);
            // Takes ownership of the module, success or not. The code lives
            // as long as the instance, which is never disposed: the cache
            // hands the loop out for the rest of the process.
            ok(LLVMOrcLLJITAddLLVMIRModule(self.jit, LLVMOrcLLJITGetMainJITDylib(self.jit), tsm))?;
            let mut addr = 0;
            ok(LLVMOrcLLJITLookup(self.jit, &mut addr, name.as_ptr()))?;
            (addr != 0).then_some(addr as usize)
        }
    }

    /// A map's loop, optimised, as IR and as the host's assembly, without
    /// linking it.
    #[cfg(test)]
    unsafe fn map_listing(&mut self, map: &Map) -> Option<(String, String)> {
        unsafe {
            let (ctx, m, _) =
                self.module("libjay_map", |ctx, m, name| build_map(ctx, m, map, name))?;
            let p = LLVMPrintModuleToString(m);
            let ir = CStr::from_ptr(p).to_string_lossy().into_owned();
            LLVMDisposeMessage(p);
            let mut buf = null_mut();
            let mut err = null_mut();
            let asm = if LLVMTargetMachineEmitToMemoryBuffer(
                self.tm,
                m,
                LLVMCodeGenFileType::LLVMAssemblyFile,
                &mut err,
                &mut buf,
            ) == 0
            {
                let start = llvm_sys::core::LLVMGetBufferStart(buf).cast::<u8>();
                let len = llvm_sys::core::LLVMGetBufferSize(buf);
                let s = String::from_utf8_lossy(std::slice::from_raw_parts(start, len)).into_owned();
                llvm_sys::core::LLVMDisposeMemoryBuffer(buf);
                s
            } else {
                LLVMDisposeMessage(err);
                String::new()
            };
            LLVMDisposeModule(m);
            LLVMContextDispose(ctx);
            Some((ir, asm))
        }
    }
}

/// What the loop body is emitted with.
struct Em {
    b: LLVMBuilderRef,
    f64t: LLVMTypeRef,
    i64t: LLVMTypeRef,
    ptrt: LLVMTypeRef,
    fabs: (LLVMValueRef, LLVMTypeRef),
    sqrt: (LLVMValueRef, LLVMTypeRef),
    unary: LLVMTypeRef,
    pure: Vec<LLVMAttributeRef>,
}

const NM: *const std::ffi::c_char = c"".as_ptr();

impl Em {
    /// An emitter writing with builder `b` into module `m`.
    unsafe fn new(ctx: LLVMContextRef, m: LLVMModuleRef, b: LLVMBuilderRef) -> Em {
        unsafe {
            let f64t = LLVMDoubleTypeInContext(ctx);
            let attr = |nm: &str, v: u64| {
                let kind = LLVMGetEnumAttributeKindForName(nm.as_ptr().cast(), nm.len());
                LLVMCreateEnumAttribute(ctx, kind, v)
            };
            let intrinsic = |nm: &str| {
                let id = LLVMLookupIntrinsicID(nm.as_ptr().cast(), nm.len());
                let mut tys = [f64t];
                (
                    LLVMGetIntrinsicDeclaration(m, id, tys.as_mut_ptr(), 1),
                    LLVMIntrinsicGetType(ctx, id, tys.as_mut_ptr(), 1),
                )
            };
            let mut unary_params = [f64t];
            Em {
                b,
                f64t,
                i64t: LLVMInt64TypeInContext(ctx),
                ptrt: LLVMPointerTypeInContext(ctx, 0),
                fabs: intrinsic("llvm.fabs"),
                sqrt: intrinsic("llvm.sqrt"),
                unary: LLVMFunctionType(f64t, unary_params.as_mut_ptr(), 1, 0),
                // memory(none) is the attribute "memory" with the value 0.
                pure: vec![attr("memory", 0), attr("nounwind", 0), attr("willreturn", 0)],
            }
        }
    }

    unsafe fn cf(&self, v: f64) -> LLVMValueRef {
        unsafe { LLVMConstReal(self.f64t, v) }
    }

    unsafe fn ci(&self, v: i64) -> LLVMValueRef {
        unsafe { LLVMConstInt(self.i64t, v as u64, 1) }
    }

    unsafe fn call(&self, ty: LLVMTypeRef, f: LLVMValueRef, args: &mut [LLVMValueRef]) -> LLVMValueRef {
        unsafe { LLVMBuildCall2(self.b, ty, f, args.as_mut_ptr(), args.len() as u32, NM) }
    }

    /// The flag that `v` is finite, and-ed into `ok`.
    unsafe fn finite(&self, v: LLVMValueRef, ok: &mut Option<LLVMValueRef>) {
        unsafe {
            let m = self.call(self.fabs.1, self.fabs.0, &mut [v]);
            let f = LLVMBuildFCmp(self.b, LLVMRealPredicate::LLVMRealOLT, m, self.cf(f64::INFINITY), NM);
            *ok = Some(match *ok {
                None => f,
                Some(o) => LLVMBuildAnd(self.b, o, f, NM),
            });
        }
    }

    /// A call of `f`, one float to one float, by its address. The call is
    /// marked as touching no memory, never unwinding and always returning,
    /// which the definitions in `scalar` satisfy.
    unsafe fn call_by_address(&self, f: extern "C" fn(f64) -> f64, a: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            let callee = LLVMConstIntToPtr(self.ci(f as usize as i64), self.ptrt);
            let c = self.call(self.unary, callee, &mut [a]);
            for &at in &self.pure {
                LLVMAddCallSiteAttribute(c, LLVMAttributeFunctionIndex, at);
            }
            c
        }
    }

    /// The monad `op` of `a`, as `scalar` defines it.
    unsafe fn un(&self, op: Un, a: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            let b = self.b;
            match op {
                Un::Neg => LLVMBuildFNeg(b, a, NM),
                Un::Abs => {
                    // `scalar::abs`: the negative of a value below zero and
                    // the value itself otherwise.
                    let below = LLVMBuildFCmp(b, LLVMRealPredicate::LLVMRealOLT, a, self.cf(0.0), NM);
                    let neg = LLVMBuildFNeg(b, a, NM);
                    LLVMBuildSelect(b, below, neg, a, NM)
                }
                Un::Inc => LLVMBuildFAdd(b, a, self.cf(1.0), NM),
                Un::Dec => LLVMBuildFSub(b, a, self.cf(1.0), NM),
                Un::Double => LLVMBuildFAdd(b, a, a, NM),
                Un::Halve => LLVMBuildFDiv(b, a, self.cf(2.0), NM),
                Un::Square => LLVMBuildFMul(b, a, a, NM),
                Un::OneMinus => LLVMBuildFSub(b, self.cf(1.0), a, NM),
                Un::Recip => LLVMBuildFDiv(b, self.cf(1.0), a, NM),
                Un::Sqrt => self.call(self.sqrt.1, self.sqrt.0, &mut [a]),
                Un::Exp => self.call_by_address(scalar::exp, a),
                Un::Ln => self.call_by_address(scalar::ln, a),
            }
        }
    }

    /// The dyad `op` of `l` and `r`, as `scalar` defines it wherever no
    /// operand is a NaN and the operation is not `0 % 0`. A caller makes
    /// sure neither case reaches an answer it keeps.
    unsafe fn bin(&self, op: Bin, l: LLVMValueRef, r: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            let b = self.b;
            match op {
                Bin::Add => LLVMBuildFAdd(b, l, r, NM),
                Bin::Sub => LLVMBuildFSub(b, l, r, NM),
                Bin::Mul => LLVMBuildFMul(b, l, r, NM),
                // `scalar::div` differs from IEEE only at `0 % 0`, which
                // IEEE makes a NaN.
                Bin::Div => LLVMBuildFDiv(b, l, r, NM),
                // `scalar::min` and `scalar::max`: the left operand where
                // the strict comparison picks it, otherwise the right.
                Bin::Min => {
                    let lt = LLVMBuildFCmp(b, LLVMRealPredicate::LLVMRealOLT, l, r, NM);
                    LLVMBuildSelect(b, lt, l, r, NM)
                }
                Bin::Max => {
                    let gt = LLVMBuildFCmp(b, LLVMRealPredicate::LLVMRealOGT, l, r, NM);
                    LLVMBuildSelect(b, gt, l, r, NM)
                }
            }
        }
    }

    /// The step's nodes, the item read from `xs` at element `base`, the
    /// running value `acc`. Answers the value of every node.
    unsafe fn step(
        &self,
        step: &Step,
        xs: LLVMValueRef,
        base: LLVMValueRef,
        acc: LLVMValueRef,
        ok: &mut Option<LLVMValueRef>,
    ) -> Vec<LLVMValueRef> {
        unsafe {
            let b = self.b;
            let mut vals: Vec<LLVMValueRef> = Vec::with_capacity(step.nodes.len());
            for node in &step.nodes {
                let v = match *node {
                    Node::X(k) => {
                        let mut ix = [LLVMBuildNSWAdd(b, base, self.ci(i64::from(k)), NM)];
                        let p = LLVMBuildInBoundsGEP2(b, self.f64t, xs, ix.as_mut_ptr(), 1, NM);
                        let v = LLVMBuildLoad2(b, self.f64t, p, NM);
                        self.finite(v, ok);
                        v
                    }
                    Node::Y => acc,
                    Node::Const(bits) => self.cf(f64::from_bits(bits)),
                    Node::Un(op, a) => {
                        let v = self.un(op, vals[a as usize]);
                        self.finite(v, ok);
                        v
                    }
                    Node::Bin(op, l, r) => {
                        // A NaN operand of an extremum stops the loop before
                        // the answer is used: every operand was checked.
                        let v = self.bin(op, vals[l as usize], vals[r as usize]);
                        self.finite(v, ok);
                        v
                    }
                };
                vals.push(v);
            }
            vals
        }
    }
}

/// Define `name` in `m` as the loop of [`Raw`]'s contract over `step`.
unsafe fn build(ctx: LLVMContextRef, m: LLVMModuleRef, step: &Step, name: &CStr) {
    unsafe {
        let b = LLVMCreateBuilderInContext(ctx);
        let f64t = LLVMDoubleTypeInContext(ctx);
        let i64t = LLVMInt64TypeInContext(ctx);
        let ptrt = LLVMPointerTypeInContext(ctx, 0);

        // (xs, xstride, n, seed, out, ostride, last) -> steps taken
        let mut params = [ptrt, i64t, i64t, f64t, ptrt, i64t, ptrt];
        let fty = LLVMFunctionType(i64t, params.as_mut_ptr(), params.len() as u32, 0);
        let f = LLVMAddFunction(m, name.as_ptr(), fty);
        let attr = |nm: &str, v: u64| {
            let kind = LLVMGetEnumAttributeKindForName(nm.as_ptr().cast(), nm.len());
            LLVMCreateEnumAttribute(ctx, kind, v)
        };
        // The items, the running values written out and the last value
        // reached are three separate buffers.
        for i in [1, 5, 7] {
            LLVMAddAttributeAtIndex(f, i, attr("noalias", 0));
        }
        LLVMAddAttributeAtIndex(f, LLVMAttributeFunctionIndex, attr("nounwind", 0));

        let em = Em::new(ctx, m, b);
        let p = |i| LLVMGetParam(f, i);
        let (xs, xstride, n, seed, out, ostride, last) = (p(0), p(1), p(2), p(3), p(4), p(5), p(6));

        let entry = LLVMAppendBasicBlockInContext(ctx, f, c"entry".as_ptr());
        let head = LLVMAppendBasicBlockInContext(ctx, f, c"head".as_ptr());
        let body = LLVMAppendBasicBlockInContext(ctx, f, c"body".as_ptr());
        let cont = LLVMAppendBasicBlockInContext(ctx, f, c"cont".as_ptr());
        let exit = LLVMAppendBasicBlockInContext(ctx, f, c"exit".as_ptr());

        LLVMPositionBuilderAtEnd(b, entry);
        LLVMBuildBr(b, head);

        // head: the step index and the running value.
        LLVMPositionBuilderAtEnd(b, head);
        let i = LLVMBuildPhi(b, i64t, NM);
        let acc = LLVMBuildPhi(b, f64t, NM);
        let more = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntSLT, i, n, NM);
        LLVMBuildCondBr(b, more, body, exit);

        LLVMPositionBuilderAtEnd(b, body);
        let base = LLVMBuildNSWMul(b, i, xstride, NM);
        let mut ok = None;
        let vals = em.step(step, xs, base, acc, &mut ok);
        let new = vals[step.root as usize];
        match ok {
            Some(ok) => LLVMBuildCondBr(b, ok, cont, exit),
            None => LLVMBuildBr(b, cont),
        };

        LLVMPositionBuilderAtEnd(b, cont);
        if step.store {
            let mut ix = [LLVMBuildNSWMul(b, i, ostride, NM)];
            let at = LLVMBuildInBoundsGEP2(b, f64t, out, ix.as_mut_ptr(), 1, NM);
            LLVMBuildStore(b, new, at);
        }
        let i1 = LLVMBuildNSWAdd(b, i, em.ci(1), NM);
        LLVMBuildBr(b, head);

        let add = |phi: LLVMValueRef, v: LLVMValueRef, from: LLVMBasicBlockRef| {
            let (mut vs, mut bs) = ([v], [from]);
            LLVMAddIncoming(phi, vs.as_mut_ptr(), bs.as_mut_ptr(), 1);
        };
        add(i, em.ci(0), entry);
        add(i, i1, cont);
        add(acc, seed, entry);
        add(acc, new, cont);

        // exit: from the head when every step is taken, from the body at
        // the first step that made a value that is not finite.
        LLVMPositionBuilderAtEnd(b, exit);
        LLVMBuildStore(b, acc, last);
        LLVMBuildRet(b, i);
        LLVMDisposeBuilder(b);
    }
}

/// Compile a map's loop. None where LLVM has no backend for the host or any
/// stage of the compilation fails.
pub(super) fn compile_map(map: &Map) -> Option<MapRaw> {
    let jit = jit()?;
    let mut jit = jit.lock().ok()?;
    // SAFETY: as in `compile`.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { jit.compile_map(map) }))
        .ok()
        .flatten()
}

/// Define `name` in `m` as the loop of [`MapRaw`]'s contract over `map`.
///
/// The flag that an item is not to be trusted is or-ed into a value the
/// loop carries rather than branched on, so the loop has one exit and
/// LLVM's vectoriser can take it: the flag becomes a vector of flags,
/// reduced once after the loop.
unsafe fn build_map(ctx: LLVMContextRef, m: LLVMModuleRef, map: &Map, name: &CStr) {
    unsafe {
        let b = LLVMCreateBuilderInContext(ctx);
        let em = Em::new(ctx, m, b);
        let (f64t, i64t, ptrt) = (em.f64t, em.i64t, em.ptrt);
        let i1t = LLVMInt1TypeInContext(ctx);
        let i8t = LLVMInt8TypeInContext(ctx);

        // (ptrs, splats, n, out) -> untrusted
        let mut params = [ptrt, ptrt, i64t, ptrt];
        let fty = LLVMFunctionType(i64t, params.as_mut_ptr(), params.len() as u32, 0);
        let f = LLVMAddFunction(m, name.as_ptr(), fty);
        let attr = |nm: &str, v: u64| {
            let kind = LLVMGetEnumAttributeKindForName(nm.as_ptr().cast(), nm.len());
            LLVMCreateEnumAttribute(ctx, kind, v)
        };
        // The result is a fresh buffer nothing else reads or writes during
        // the call, and the two tables are only read.
        for i in [1, 2, 4] {
            LLVMAddAttributeAtIndex(f, i, attr("noalias", 0));
        }
        for i in [1, 2] {
            LLVMAddAttributeAtIndex(f, i, attr("readonly", 0));
        }
        LLVMAddAttributeAtIndex(f, LLVMAttributeFunctionIndex, attr("nounwind", 0));
        let p = |i| LLVMGetParam(f, i);
        let (ptrs, splats, n, out) = (p(0), p(1), p(2), p(3));

        let entry = LLVMAppendBasicBlockInContext(ctx, f, c"entry".as_ptr());
        let head = LLVMAppendBasicBlockInContext(ctx, f, c"head".as_ptr());
        let body = LLVMAppendBasicBlockInContext(ctx, f, c"body".as_ptr());
        let exit = LLVMAppendBasicBlockInContext(ctx, f, c"exit".as_ptr());

        // entry: each input's buffer, or its one value, read once.
        LLVMPositionBuilderAtEnd(b, entry);
        let at = |base: LLVMValueRef, ty: LLVMTypeRef, i: LLVMValueRef| {
            let mut ix = [i];
            LLVMBuildInBoundsGEP2(b, ty, base, ix.as_mut_ptr(), 1, NM)
        };
        let cols: Vec<LLVMValueRef> = map
            .inputs
            .iter()
            .enumerate()
            .map(|(j, kind)| {
                let j = em.ci(j as i64);
                match kind {
                    Input::Splat => LLVMBuildLoad2(b, f64t, at(splats, f64t, j), NM),
                    _ => LLVMBuildLoad2(b, ptrt, at(ptrs, ptrt, j), NM),
                }
            })
            .collect();
        LLVMBuildBr(b, head);

        // head: the item index and the flag so far.
        LLVMPositionBuilderAtEnd(b, head);
        let i = LLVMBuildPhi(b, i64t, NM);
        let bad = LLVMBuildPhi(b, i1t, NM);
        let more = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntSLT, i, n, NM);
        LLVMBuildCondBr(b, more, body, exit);

        LLVMPositionBuilderAtEnd(b, body);
        let mut flag = bad;
        let mut raise = |c: LLVMValueRef| flag = LLVMBuildOr(b, flag, c, NM);
        let not_finite = |v: LLVMValueRef| {
            let mag = em.call(em.fabs.1, em.fabs.0, &mut [v]);
            // Unordered or equal: a NaN, or an infinity.
            LLVMBuildFCmp(b, LLVMRealPredicate::LLVMRealUGE, mag, em.cf(f64::INFINITY), NM)
        };
        let mut stack: Vec<LLVMValueRef> = Vec::new();
        let mut lets: Vec<LLVMValueRef> = Vec::new();
        for op in &map.ops {
            match *op {
                Op::Load(j) => {
                    let col = cols[j as usize];
                    let v = match map.inputs[j as usize] {
                        Input::Splat => col,
                        Input::F64 => LLVMBuildLoad2(b, f64t, at(col, f64t, i), NM),
                        Input::I64 => {
                            let x = LLVMBuildLoad2(b, i64t, at(col, i64t, i), NM);
                            LLVMBuildSIToFP(b, x, f64t, NM)
                        }
                        Input::Bool => {
                            let x = LLVMBuildLoad2(b, i8t, at(col, i8t, i), NM);
                            LLVMBuildUIToFP(b, x, f64t, NM)
                        }
                    };
                    stack.push(v);
                }
                Op::Un(u) => {
                    let a = stack.pop().expect("a lowered map is well formed");
                    let v = em.un(u, a);
                    if u == Un::Recip {
                        raise(not_finite(v));
                    }
                    stack.push(v);
                }
                Op::Bin(o) => {
                    let r = stack.pop().expect("a lowered map is well formed");
                    let l = stack.pop().expect("a lowered map is well formed");
                    if matches!(o, Bin::Min | Bin::Max) {
                        // A NaN on either side, or the two zeros: equal
                        // values whose bits differ.
                        let uno = LLVMBuildFCmp(b, LLVMRealPredicate::LLVMRealUNO, l, r, NM);
                        let eq = LLVMBuildFCmp(b, LLVMRealPredicate::LLVMRealOEQ, l, r, NM);
                        let lb = LLVMBuildBitCast(b, l, i64t, NM);
                        let rb = LLVMBuildBitCast(b, r, i64t, NM);
                        let differ = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntNE, lb, rb, NM);
                        raise(uno);
                        raise(LLVMBuildAnd(b, eq, differ, NM));
                    }
                    stack.push(em.bin(o, l, r));
                }
                Op::Store(_) => lets.push(stack.pop().expect("a lowered map is well formed")),
                Op::Let(k) => stack.push(lets[k as usize]),
            }
        }
        let v = stack.pop().expect("a lowered map is well formed");
        raise(not_finite(v));
        LLVMBuildStore(b, v, at(out, f64t, i));
        let i1 = LLVMBuildNSWAdd(b, i, em.ci(1), NM);
        let body_end = LLVMGetInsertBlock(b);
        LLVMBuildBr(b, head);

        let add = |phi: LLVMValueRef, v: LLVMValueRef, from: LLVMBasicBlockRef| {
            let (mut vs, mut bs) = ([v], [from]);
            LLVMAddIncoming(phi, vs.as_mut_ptr(), bs.as_mut_ptr(), 1);
        };
        add(i, em.ci(0), entry);
        add(i, i1, body_end);
        add(bad, LLVMConstInt(i1t, 0, 0), entry);
        add(bad, flag, body_end);

        LLVMPositionBuilderAtEnd(b, exit);
        LLVMBuildRet(b, LLVMBuildZExt(b, bad, i64t, NM));
        LLVMDisposeBuilder(b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The map loop vectorises: LLVM's optimised IR computes on vectors of
    /// floats, and the host's assembly holds packed instructions.
    #[test]
    fn a_map_loop_vectorises() {
        let map = Map {
            // ((x * w) + 0.5) <. 3, a product, a sum and an extremum.
            ops: vec![
                Op::Load(0),
                Op::Load(1),
                Op::Bin(Bin::Mul),
                Op::Load(2),
                Op::Bin(Bin::Add),
                Op::Load(3),
                Op::Bin(Bin::Min),
            ],
            inputs: vec![Input::F64, Input::F64, Input::Splat, Input::Splat],
        };
        let jit = jit().expect("the host has a backend");
        let mut jit = jit.lock().unwrap();
        let (ir, asm) = unsafe { jit.map_listing(&map) }.expect("the map compiles");
        // Shown with `--nocapture`.
        eprintln!("{ir}\n{asm}");
        assert!(ir.contains(" x double>"), "the loop was not vectorised:\n{ir}");
        #[cfg(target_arch = "x86_64")]
        assert!(asm.contains("mulpd"), "no packed multiply in the loop:\n{asm}");
    }
}
