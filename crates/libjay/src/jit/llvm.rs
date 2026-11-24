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
use super::{Bin, Cmp, Ins, Out, Prog, Repr, Shape, Un, CAP, WHOLE_BOUND};
use crate::scalar;

/// Compile a program's loop; the entry point's address. None where LLVM
/// has no backend for the host or any stage of the compilation fails.
pub(super) fn compile(shape: Shape, prog: &Prog) -> Option<usize> {
    let jit = jit()?;
    let mut jit = jit.lock().ok()?;
    // SAFETY: the LLJIT instance and its target machine are used only
    // under this lock.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { jit.compile(shape, prog) }))
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

    unsafe fn compile(&mut self, shape: Shape, prog: &Prog) -> Option<usize> {
        // SAFETY: the symbol is the function `build` defines, with the
        // signature its shape names under the C calling convention.
        unsafe { self.add("libjay_loop", |ctx, m, name| build(ctx, m, shape, prog, name)) }
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

/// What the loops are emitted with.
struct Em {
    b: LLVMBuilderRef,
    f64t: LLVMTypeRef,
    i64t: LLVMTypeRef,
    ptrt: LLVMTypeRef,
    fabs: (LLVMValueRef, LLVMTypeRef),
    sqrt: (LLVMValueRef, LLVMTypeRef),
    floor: (LLVMValueRef, LLVMTypeRef),
    ceil: (LLVMValueRef, LLVMTypeRef),
    unary: LLVMTypeRef,
    pure: Vec<LLVMAttributeRef>,
    /// The comparison tolerance, and whether it scales by the smaller
    /// magnitude.
    ct: f64,
    by_smaller: bool,
}

const NM: *const std::ffi::c_char = c"".as_ptr();

impl Em {
    /// An emitter writing with builder `b` into module `m`, comparing with
    /// tolerance `ct`.
    unsafe fn new(ctx: LLVMContextRef, m: LLVMModuleRef, b: LLVMBuilderRef, ct: f64, by_smaller: bool) -> Em {
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
                floor: intrinsic("llvm.floor"),
                ceil: intrinsic("llvm.ceil"),
                unary: LLVMFunctionType(f64t, unary_params.as_mut_ptr(), 1, 0),
                // memory(none) is the attribute "memory" with the value 0.
                pure: vec![attr("memory", 0), attr("nounwind", 0), attr("willreturn", 0)],
                ct,
                by_smaller,
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

    unsafe fn fcmp(&self, p: LLVMRealPredicate, a: LLVMValueRef, c: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildFCmp(self.b, p, a, c, NM) }
    }

    /// Whether `|v| < bound`: false for a NaN.
    unsafe fn below(&self, v: LLVMValueRef, bound: f64) -> LLVMValueRef {
        unsafe {
            let m = self.call(self.fabs.1, self.fabs.0, &mut [v]);
            self.fcmp(LLVMRealPredicate::LLVMRealOLT, m, self.cf(bound))
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

    /// J's tolerant equality: equal, or apart by less than the tolerance
    /// times the smaller magnitude (the larger, for APL's rule).
    unsafe fn teq(&self, a: LLVMValueRef, c: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            use LLVMRealPredicate::*;
            let b = self.b;
            let same = self.fcmp(LLVMRealOEQ, a, c);
            let ma = self.call(self.fabs.1, self.fabs.0, &mut [a]);
            let mc = self.call(self.fabs.1, self.fabs.0, &mut [c]);
            let pick = if self.by_smaller { LLVMRealOLT } else { LLVMRealOGT };
            let first = self.fcmp(pick, ma, mc);
            let s = LLVMBuildSelect(b, first, ma, mc, NM);
            let d = LLVMBuildFSub(b, a, c, NM);
            let d = self.call(self.fabs.1, self.fabs.0, &mut [d]);
            let lim = LLVMBuildFMul(b, self.cf(self.ct), s, NM);
            let near = self.fcmp(LLVMRealOLT, d, lim);
            LLVMBuildOr(b, same, near, NM)
        }
    }

    /// J's tolerant floor, or with `up` its ceiling.
    unsafe fn tfloor(&self, y: LLVMValueRef, up: bool) -> LLVMValueRef {
        unsafe {
            let fl = self.call(self.floor.1, self.floor.0, &mut [y]);
            let ce = self.call(self.ceil.1, self.ceil.0, &mut [y]);
            let (near, far) = if up { (fl, ce) } else { (ce, fl) };
            let e = self.teq(y, near);
            LLVMBuildSelect(self.b, e, near, far, NM)
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
                Un::Floor => self.tfloor(a, false),
                Un::Ceil => self.tfloor(a, true),
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
                Bin::Mul | Bin::And => LLVMBuildFMul(b, l, r, NM),
                // `scalar::div` differs from IEEE only at `0 % 0`, which
                // IEEE makes a NaN.
                Bin::Div => LLVMBuildFDiv(b, l, r, NM),
                // `scalar::min` and `scalar::max`: the left operand where
                // the strict comparison picks it, otherwise the right.
                Bin::Min => {
                    let lt = LLVMBuildFCmp(b, LLVMRealPredicate::LLVMRealOLT, l, r, NM);
                    LLVMBuildSelect(b, lt, l, r, NM)
                }
                Bin::Max | Bin::Or => {
                    let gt = LLVMBuildFCmp(b, LLVMRealPredicate::LLVMRealOGT, l, r, NM);
                    LLVMBuildSelect(b, gt, l, r, NM)
                }
                Bin::Residue => {
                    // `Tol::residue` over finite operands: y where x is
                    // zero, otherwise y - x * <. y % x, or 0 where the
                    // product is tolerantly y.
                    let q = LLVMBuildFDiv(b, r, l, NM);
                    let f = self.tfloor(q, false);
                    let p = LLVMBuildFMul(b, l, f, NM);
                    let e = self.teq(r, p);
                    let d = LLVMBuildFSub(b, r, p, NM);
                    let res = LLVMBuildSelect(b, e, self.cf(0.0), d, NM);
                    let zero = self.fcmp(LLVMRealPredicate::LLVMRealOEQ, l, self.cf(0.0));
                    LLVMBuildSelect(b, zero, r, res, NM)
                }
            }
        }
    }

    /// The tolerant comparison `op` of `a` and `c`, as 1 or 0.
    unsafe fn cmp(&self, op: Cmp, a: LLVMValueRef, c: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            use LLVMRealPredicate::*;
            let b = self.b;
            let t = match op {
                Cmp::Eq => self.teq(a, c),
                Cmp::Ne => LLVMBuildNot(b, self.teq(a, c), NM),
                Cmp::Lt | Cmp::Gt => {
                    let (p, q) = if op == Cmp::Lt { (a, c) } else { (c, a) };
                    let less = self.fcmp(LLVMRealOLT, p, q);
                    LLVMBuildAnd(b, less, LLVMBuildNot(b, self.teq(p, q), NM), NM)
                }
                Cmp::Le | Cmp::Ge => {
                    let (p, q) = if op == Cmp::Le { (a, c) } else { (c, a) };
                    let le = self.fcmp(LLVMRealOLE, p, q);
                    LLVMBuildOr(b, le, self.teq(p, q), NM)
                }
            };
            LLVMBuildSelect(b, t, self.cf(1.0), self.cf(0.0), NM)
        }
    }
}

/// A program's instructions emitted into a function: its variables are
/// stack slots, which LLVM's optimiser turns into registers.
struct Body<'e> {
    em: &'e Em,
    ctx: LLVMContextRef,
    f: LLVMValueRef,
    /// The builder positioned in the block that holds every stack slot.
    slots: LLVMBuilderRef,
    vars: Vec<LLVMValueRef>,
    /// Where a check that fails goes.
    bail: LLVMBasicBlockRef,
    /// The checks since the last branch, and-ed.
    ok: Option<LLVMValueRef>,
}

impl Body<'_> {
    unsafe fn block(&self, name: &CStr) -> LLVMBasicBlockRef {
        unsafe { LLVMAppendBasicBlockInContext(self.ctx, self.f, name.as_ptr()) }
    }

    unsafe fn slot(&self, ty: LLVMTypeRef) -> LLVMValueRef {
        unsafe { LLVMBuildAlloca(self.slots, ty, NM) }
    }

    unsafe fn get(&self, v: u32) -> LLVMValueRef {
        unsafe { LLVMBuildLoad2(self.em.b, self.em.f64t, self.vars[v as usize], NM) }
    }

    unsafe fn set(&self, v: u32, x: LLVMValueRef) {
        unsafe {
            LLVMBuildStore(self.em.b, x, self.vars[v as usize]);
        }
    }

    unsafe fn check(&mut self, c: LLVMValueRef) {
        unsafe {
            self.ok = Some(match self.ok {
                None => c,
                Some(o) => LLVMBuildAnd(self.em.b, o, c, NM),
            });
        }
    }

    /// Branch on the pending checks: to the bail block where one failed.
    unsafe fn flush(&mut self) {
        unsafe {
            if let Some(ok) = self.ok.take() {
                let cont = self.block(c"cont");
                LLVMBuildCondBr(self.em.b, ok, cont, self.bail);
                LLVMPositionBuilderAtEnd(self.em.b, cont);
            }
        }
    }

    /// Emit `code`, `arg` supplying the arguments.
    unsafe fn run(&mut self, code: &[Ins], arg: &mut dyn FnMut(&mut Self, u32) -> LLVMValueRef) {
        unsafe {
            use LLVMRealPredicate::*;
            let em = self.em;
            let b = em.b;
            for ins in code {
                match ins {
                    Ins::Arg(v, k) => {
                        let x = arg(self, *k);
                        self.set(*v, x);
                    }
                    Ins::Const(v, bits) => self.set(*v, em.cf(f64::from_bits(*bits))),
                    Ins::Copy(d, s) => self.set(*d, self.get(*s)),
                    Ins::Un(d, op, a) => self.set(*d, em.un(*op, self.get(*a))),
                    Ins::Bin(d, op, l, r) => self.set(*d, em.bin(*op, self.get(*l), self.get(*r))),
                    Ins::Cmp(d, op, l, r) => self.set(*d, em.cmp(*op, self.get(*l), self.get(*r))),
                    Ins::Select(d, c, a, e) => {
                        let holds = em.fcmp(LLVMRealUNE, self.get(*c), em.cf(0.0));
                        self.set(*d, LLVMBuildSelect(b, holds, self.get(*a), self.get(*e), NM));
                    }
                    Ins::Finite(v) => {
                        let c = em.below(self.get(*v), f64::INFINITY);
                        self.check(c);
                    }
                    Ins::Whole(v) => {
                        let c = em.below(self.get(*v), WHOLE_BOUND);
                        self.check(c);
                    }
                    Ins::If(c, then, other) => {
                        self.flush();
                        let holds = em.fcmp(LLVMRealUNE, self.get(*c), em.cf(0.0));
                        let (tb, eb, join) = (self.block(c"then"), self.block(c"else"), self.block(c"join"));
                        LLVMBuildCondBr(b, holds, tb, eb);
                        for (blk, code) in [(tb, then), (eb, other)] {
                            LLVMPositionBuilderAtEnd(b, blk);
                            self.run(code, arg);
                            self.flush();
                            LLVMBuildBr(b, join);
                        }
                        LLVMPositionBuilderAtEnd(b, join);
                    }
                    Ins::Switch(sel, arms) => {
                        self.flush();
                        let s = self.get(*sel);
                        let n = arms.len() as f64;
                        let join = self.block(c"join");
                        for (k, code) in arms.iter().enumerate() {
                            let (here, next) = (self.block(c"arm"), self.block(c"next"));
                            let k = k as f64;
                            let e1 = em.fcmp(LLVMRealOEQ, s, em.cf(k));
                            let e2 = em.fcmp(LLVMRealOEQ, s, em.cf(k - n));
                            LLVMBuildCondBr(b, LLVMBuildOr(b, e1, e2, NM), here, next);
                            LLVMPositionBuilderAtEnd(b, here);
                            self.run(code, arg);
                            self.flush();
                            LLVMBuildBr(b, join);
                            LLVMPositionBuilderAtEnd(b, next);
                        }
                        // An index that names no verb of the gerund.
                        LLVMBuildBr(b, self.bail);
                        LLVMPositionBuilderAtEnd(b, join);
                    }
                    Ins::While(test, cond, body) => {
                        self.flush();
                        let count = self.slot(em.i64t);
                        LLVMBuildStore(b, em.ci(0), count);
                        let (head, blk, exit) = (self.block(c"while"), self.block(c"do"), self.block(c"end"));
                        LLVMBuildBr(b, head);
                        LLVMPositionBuilderAtEnd(b, head);
                        self.run(test, arg);
                        self.flush();
                        let holds = em.fcmp(LLVMRealUNE, self.get(*cond), em.cf(0.0));
                        LLVMBuildCondBr(b, holds, blk, exit);
                        LLVMPositionBuilderAtEnd(b, blk);
                        self.run(body, arg);
                        self.flush();
                        let i = LLVMBuildLoad2(b, em.i64t, count, NM);
                        let i1 = LLVMBuildNSWAdd(b, i, em.ci(1), NM);
                        LLVMBuildStore(b, i1, count);
                        let more = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntSLT, i1, em.ci(CAP as i64), NM);
                        LLVMBuildCondBr(b, more, head, self.bail);
                        LLVMPositionBuilderAtEnd(b, exit);
                    }
                    Ins::Repeat { count, index, body } => {
                        self.flush();
                        let n = self.get(*count);
                        // A whole number from 0 to CAP, or the loop stops.
                        let lo = em.fcmp(LLVMRealOGE, n, em.cf(0.0));
                        let hi = em.fcmp(LLVMRealOLE, n, em.cf(CAP as f64));
                        let fl = em.call(em.floor.1, em.floor.0, &mut [n]);
                        let whole = em.fcmp(LLVMRealOEQ, fl, n);
                        let ok = LLVMBuildAnd(b, LLVMBuildAnd(b, lo, hi, NM), whole, NM);
                        self.check(ok);
                        self.flush();
                        let total = LLVMBuildFPToSI(b, n, em.i64t, NM);
                        let i = self.slot(em.i64t);
                        LLVMBuildStore(b, em.ci(0), i);
                        let (head, blk, exit) = (self.block(c"repeat"), self.block(c"do"), self.block(c"end"));
                        LLVMBuildBr(b, head);
                        LLVMPositionBuilderAtEnd(b, head);
                        let iv = LLVMBuildLoad2(b, em.i64t, i, NM);
                        let more = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntSLT, iv, total, NM);
                        LLVMBuildCondBr(b, more, blk, exit);
                        LLVMPositionBuilderAtEnd(b, blk);
                        let iv = LLVMBuildLoad2(b, em.i64t, i, NM);
                        self.set(*index, LLVMBuildSIToFP(b, iv, em.f64t, NM));
                        self.run(body, arg);
                        self.flush();
                        let iv = LLVMBuildLoad2(b, em.i64t, i, NM);
                        LLVMBuildStore(b, LLVMBuildNSWAdd(b, iv, em.ci(1), NM), i);
                        LLVMBuildBr(b, head);
                        LLVMPositionBuilderAtEnd(b, exit);
                    }
                }
            }
        }
    }
}

/// Define `name` in `m` as the loop of `shape` over `prog`: `Raw`'s
/// contract for a fold, `CellsRaw`'s for a rank-0 application.
unsafe fn build(ctx: LLVMContextRef, m: LLVMModuleRef, shape: Shape, prog: &Prog, name: &CStr) {
    unsafe {
        let b = LLVMCreateBuilderInContext(ctx);
        let em = Em::new(ctx, m, b, f64::from_bits(prog.ct), prog.by_smaller);
        let (f64t, i64t, ptrt) = (em.f64t, em.i64t, em.ptrt);

        let mut params = match shape {
            // (xs, xstride, n, seed, out, ostride, last) -> steps taken
            Shape::Fold { .. } => vec![ptrt, i64t, i64t, f64t, ptrt, i64t, ptrt],
            // (ys, ystride, xs, xstride, n, out, kind) -> items computed
            Shape::Cells { .. } => vec![ptrt, i64t, ptrt, i64t, i64t, ptrt, ptrt],
        };
        let fty = LLVMFunctionType(i64t, params.as_mut_ptr(), params.len() as u32, 0);
        let f = LLVMAddFunction(m, name.as_ptr(), fty);
        let attr = |nm: &str, v: u64| {
            let kind = LLVMGetEnumAttributeKindForName(nm.as_ptr().cast(), nm.len());
            LLVMCreateEnumAttribute(ctx, kind, v)
        };
        // The pointers are separate buffers: the items, the values written
        // out and the one value written at the end.
        let noalias: &[u32] = match shape {
            Shape::Fold { .. } => &[1, 5, 7],
            Shape::Cells { .. } => &[6, 7],
        };
        for &i in noalias {
            LLVMAddAttributeAtIndex(f, i, attr("noalias", 0));
        }
        LLVMAddAttributeAtIndex(f, LLVMAttributeFunctionIndex, attr("nounwind", 0));
        let p = |i| LLVMGetParam(f, i);

        let slots_block = LLVMAppendBasicBlockInContext(ctx, f, c"slots".as_ptr());
        let entry = LLVMAppendBasicBlockInContext(ctx, f, c"entry".as_ptr());
        let head = LLVMAppendBasicBlockInContext(ctx, f, c"head".as_ptr());
        let body = LLVMAppendBasicBlockInContext(ctx, f, c"body".as_ptr());
        let exit = LLVMAppendBasicBlockInContext(ctx, f, c"exit".as_ptr());
        let slots = LLVMCreateBuilderInContext(ctx);
        LLVMPositionBuilderAtEnd(slots, slots_block);
        let vars = (0..prog.vars).map(|_| LLVMBuildAlloca(slots, f64t, NM)).collect();
        let mut g = Body { em: &em, ctx, f, slots, vars, bail: exit, ok: None };
        let i = g.slot(i64t);

        LLVMPositionBuilderAtEnd(b, entry);
        LLVMBuildStore(b, em.ci(0), i);
        let more = |n: LLVMValueRef| {
            let iv = LLVMBuildLoad2(b, i64t, i, NM);
            LLVMBuildCondBr(b, LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntSLT, iv, n, NM), body, exit);
        };
        let gep = |base: LLVMValueRef, ty: LLVMTypeRef, at: LLVMValueRef| {
            let mut ix = [at];
            LLVMBuildInBoundsGEP2(b, ty, base, ix.as_mut_ptr(), 1, NM)
        };
        match shape {
            Shape::Fold { store } => {
                let (xs, xstride, n, seed, out, ostride, last) = (p(0), p(1), p(2), p(3), p(4), p(5), p(6));
                let acc = g.slot(f64t);
                LLVMBuildStore(b, seed, acc);
                LLVMBuildBr(b, head);

                LLVMPositionBuilderAtEnd(b, head);
                more(n);

                LLVMPositionBuilderAtEnd(b, body);
                let iv = LLVMBuildLoad2(b, i64t, i, NM);
                let base = LLVMBuildNSWMul(b, iv, xstride, NM);
                g.run(&prog.code, &mut |g, k| {
                    if k == 0 {
                        return LLVMBuildLoad2(b, f64t, acc, NM);
                    }
                    let at = LLVMBuildNSWAdd(b, base, em.ci(i64::from(k) - 1), NM);
                    let v = LLVMBuildLoad2(b, f64t, gep(xs, f64t, at), NM);
                    g.check(em.below(v, f64::INFINITY));
                    v
                });
                g.flush();
                let new = g.get(prog.result);
                if store {
                    let iv = LLVMBuildLoad2(b, i64t, i, NM);
                    LLVMBuildStore(b, new, gep(out, f64t, LLVMBuildNSWMul(b, iv, ostride, NM)));
                }
                LLVMBuildStore(b, new, acc);
                let iv = LLVMBuildLoad2(b, i64t, i, NM);
                LLVMBuildStore(b, LLVMBuildNSWAdd(b, iv, em.ci(1), NM), i);
                LLVMBuildBr(b, head);

                // exit: from the head when every step is taken, from the
                // body at the first step that could not be completed.
                LLVMPositionBuilderAtEnd(b, exit);
                LLVMBuildStore(b, LLVMBuildLoad2(b, f64t, acc, NM), last);
                LLVMBuildRet(b, LLVMBuildLoad2(b, i64t, i, NM));
            }
            Shape::Cells { y, x } => {
                let (ys, ystride, xs, xstride, n, out, kind_out) = (p(0), p(1), p(2), p(3), p(4), p(5), p(6));
                let top = g.slot(f64t);
                LLVMBuildStore(b, em.cf(0.0), top);
                LLVMBuildBr(b, head);

                LLVMPositionBuilderAtEnd(b, head);
                more(n);

                LLVMPositionBuilderAtEnd(b, body);
                let iv = LLVMBuildLoad2(b, i64t, i, NM);
                let atom = |g: &mut Body<'_>, base: LLVMValueRef, stride: LLVMValueRef, repr: Repr| {
                    let at = LLVMBuildNSWMul(b, iv, stride, NM);
                    match repr {
                        Repr::F64 => {
                            let v = LLVMBuildLoad2(b, f64t, gep(base, f64t, at), NM);
                            g.check(em.below(v, f64::INFINITY));
                            v
                        }
                        Repr::I64 => {
                            let w = LLVMBuildLoad2(b, i64t, gep(base, i64t, at), NM);
                            let v = LLVMBuildSIToFP(b, w, f64t, NM);
                            g.check(em.below(v, WHOLE_BOUND));
                            v
                        }
                        Repr::Bool => {
                            let i8t = LLVMInt8TypeInContext(ctx);
                            let w = LLVMBuildLoad2(b, i8t, gep(base, i8t, at), NM);
                            LLVMBuildUIToFP(b, w, f64t, NM)
                        }
                    }
                };
                let yv = atom(&mut g, ys, ystride, y);
                let xv = x.map(|r| atom(&mut g, xs, xstride, r));
                g.run(&prog.code, &mut |_, k| if k == 0 { yv } else { xv.expect("a dyad's left argument") });
                g.flush();
                let r = g.get(prog.result);
                let iv = LLVMBuildLoad2(b, i64t, i, NM);
                LLVMBuildStore(b, r, gep(out, f64t, iv));
                let k = match prog.out {
                    Out::Static(k) => em.cf(k.code()),
                    Out::Dyn(v) => g.get(v),
                };
                let t = LLVMBuildLoad2(b, f64t, top, NM);
                let gt = em.fcmp(LLVMRealPredicate::LLVMRealOGT, k, t);
                LLVMBuildStore(b, LLVMBuildSelect(b, gt, k, t, NM), top);
                LLVMBuildStore(b, LLVMBuildNSWAdd(b, iv, em.ci(1), NM), i);
                LLVMBuildBr(b, head);

                LLVMPositionBuilderAtEnd(b, exit);
                LLVMBuildStore(b, LLVMBuildLoad2(b, f64t, top, NM), kind_out);
                LLVMBuildRet(b, LLVMBuildLoad2(b, i64t, i, NM));
            }
        }
        // Every stack slot is made; the slots block falls into the entry.
        LLVMBuildBr(slots, entry);
        LLVMDisposeBuilder(slots);
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
        let em = Em::new(ctx, m, b, 0.0, true);
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
