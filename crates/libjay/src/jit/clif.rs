//! The compiled loops built with Cranelift.

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    types, AbiParam, Block, InstBuilder, MemFlags, SigRef, Signature, Type, Value,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};

use super::{Bin, Cmp, Ins, Out, Prog, Repr, Shape, Un, CAP, WHOLE_BOUND};
use crate::scalar;

/// Compile a program's loop; the entry point's address. None where the
/// host has no Cranelift backend or the compilation fails for any reason.
pub(super) fn compile(shape: Shape, prog: &Prog) -> Option<usize> {
    std::panic::catch_unwind(|| compile_inner(shape, prog)).ok().flatten()
}

/// What the program's instructions are emitted with.
struct Gen<'a, 'f> {
    b: &'a mut FunctionBuilder<'f>,
    vars: Vec<Variable>,
    /// Where a check that fails goes.
    bail: Block,
    /// The checks since the last branch, and-ed: whether everything is in
    /// bounds so far.
    ok: Option<Value>,
    unary: SigRef,
    ptr: Type,
    ct: f64,
    by_smaller: bool,
}

impl Gen<'_, '_> {
    fn fc(&mut self, v: f64) -> Value {
        self.b.ins().f64const(v)
    }

    fn get(&mut self, v: u32) -> Value {
        self.b.use_var(self.vars[v as usize])
    }

    fn set(&mut self, v: u32, x: Value) {
        self.b.def_var(self.vars[v as usize], x);
    }

    /// And `c` into the pending checks.
    fn check(&mut self, c: Value) {
        self.ok = Some(match self.ok {
            None => c,
            Some(o) => self.b.ins().band(o, c),
        });
    }

    /// Branch on the pending checks: to the bail block where one failed.
    fn flush(&mut self) {
        if let Some(ok) = self.ok.take() {
            let cont = self.b.create_block();
            self.b.ins().brif(ok, cont, &[], self.bail, &[]);
            self.b.switch_to_block(cont);
        }
    }

    /// Whether `|v| < bound`: false for a NaN.
    fn below(&mut self, v: Value, bound: f64) -> Value {
        let m = self.b.ins().fabs(v);
        let k = self.fc(bound);
        self.b.ins().fcmp(FloatCC::LessThan, m, k)
    }

    /// J's tolerant equality: equal, or apart by less than the tolerance
    /// times the smaller magnitude (the larger, for APL's rule).
    fn teq(&mut self, a: Value, c: Value) -> Value {
        let b = &mut *self.b;
        let same = b.ins().fcmp(FloatCC::Equal, a, c);
        let ma = b.ins().fabs(a);
        let mc = b.ins().fabs(c);
        let pick = if self.by_smaller { FloatCC::LessThan } else { FloatCC::GreaterThan };
        let first = b.ins().fcmp(pick, ma, mc);
        let s = b.ins().select(first, ma, mc);
        let d = b.ins().fsub(a, c);
        let d = b.ins().fabs(d);
        let ct = b.ins().f64const(self.ct);
        let lim = b.ins().fmul(ct, s);
        let near = b.ins().fcmp(FloatCC::LessThan, d, lim);
        b.ins().bor(same, near)
    }

    /// J's tolerant floor, or with `up` its ceiling.
    fn tfloor(&mut self, y: Value, up: bool) -> Value {
        let (near, far) = if up {
            (self.b.ins().floor(y), self.b.ins().ceil(y))
        } else {
            (self.b.ins().ceil(y), self.b.ins().floor(y))
        };
        let e = self.teq(y, near);
        self.b.ins().select(e, near, far)
    }

    fn call(&mut self, f: extern "C" fn(f64) -> f64, a: Value) -> Value {
        let callee = self.b.ins().iconst(self.ptr, f as usize as i64);
        let call = self.b.ins().call_indirect(self.unary, callee, &[a]);
        self.b.inst_results(call)[0]
    }

    fn un(&mut self, op: Un, a: Value) -> Value {
        match op {
            Un::Neg => self.b.ins().fneg(a),
            Un::Abs => {
                // `scalar::abs`: the negative of a value below zero and the
                // value itself otherwise.
                let z = self.fc(0.0);
                let below = self.b.ins().fcmp(FloatCC::LessThan, a, z);
                let neg = self.b.ins().fneg(a);
                self.b.ins().select(below, neg, a)
            }
            Un::Inc => {
                let one = self.fc(1.0);
                self.b.ins().fadd(a, one)
            }
            Un::Dec => {
                let one = self.fc(1.0);
                self.b.ins().fsub(a, one)
            }
            Un::Double => self.b.ins().fadd(a, a),
            Un::Halve => {
                let two = self.fc(2.0);
                self.b.ins().fdiv(a, two)
            }
            Un::Square => self.b.ins().fmul(a, a),
            Un::OneMinus => {
                let one = self.fc(1.0);
                self.b.ins().fsub(one, a)
            }
            Un::Recip => {
                let one = self.fc(1.0);
                self.b.ins().fdiv(one, a)
            }
            Un::Sqrt => self.b.ins().sqrt(a),
            Un::Exp => self.call(scalar::exp, a),
            Un::Ln => self.call(scalar::ln, a),
            Un::Floor => self.tfloor(a, false),
            Un::Ceil => self.tfloor(a, true),
        }
    }

    fn bin(&mut self, op: Bin, l: Value, r: Value) -> Value {
        match op {
            Bin::Add => self.b.ins().fadd(l, r),
            Bin::Sub => self.b.ins().fsub(l, r),
            Bin::Mul | Bin::And => self.b.ins().fmul(l, r),
            // `scalar::div` differs from IEEE only at `0 % 0`, whose NaN
            // stops the loop.
            Bin::Div => self.b.ins().fdiv(l, r),
            // `scalar::min` and `scalar::max`: the left operand where the
            // strict comparison picks it, otherwise the right. A NaN
            // operand stops the loop before the answer is used.
            Bin::Min => {
                let lt = self.b.ins().fcmp(FloatCC::LessThan, l, r);
                self.b.ins().select(lt, l, r)
            }
            Bin::Max | Bin::Or => {
                let gt = self.b.ins().fcmp(FloatCC::GreaterThan, l, r);
                self.b.ins().select(gt, l, r)
            }
            Bin::Residue => {
                // `Tol::residue` over finite operands: y where x is zero,
                // otherwise y - x * <. y % x, or 0 where the product is
                // tolerantly y.
                let q = self.b.ins().fdiv(r, l);
                let f = self.tfloor(q, false);
                let p = self.b.ins().fmul(l, f);
                let e = self.teq(r, p);
                let d = self.b.ins().fsub(r, p);
                let z = self.fc(0.0);
                let res = self.b.ins().select(e, z, d);
                let zero = self.b.ins().fcmp(FloatCC::Equal, l, z);
                self.b.ins().select(zero, r, res)
            }
        }
    }

    fn cmp(&mut self, op: Cmp, a: Value, c: Value) -> Value {
        let b = |g: &mut Self, cc, p, q| g.b.ins().fcmp(cc, p, q);
        let t = match op {
            Cmp::Eq => self.teq(a, c),
            Cmp::Ne => {
                let e = self.teq(a, c);
                self.b.ins().bxor_imm(e, 1)
            }
            Cmp::Lt | Cmp::Gt => {
                let (p, q) = if op == Cmp::Lt { (a, c) } else { (c, a) };
                let less = b(self, FloatCC::LessThan, p, q);
                let e = self.teq(p, q);
                let ne = self.b.ins().bxor_imm(e, 1);
                self.b.ins().band(less, ne)
            }
            Cmp::Le | Cmp::Ge => {
                let (p, q) = if op == Cmp::Le { (a, c) } else { (c, a) };
                let le = b(self, FloatCC::LessThanOrEqual, p, q);
                let e = self.teq(p, q);
                self.b.ins().bor(le, e)
            }
        };
        let one = self.fc(1.0);
        let zero = self.fc(0.0);
        self.b.ins().select(t, one, zero)
    }

    /// Emit `code`, `arg` supplying the arguments.
    fn run(&mut self, code: &[Ins], arg: &mut dyn FnMut(&mut Self, u32) -> Value) {
        for ins in code {
            match ins {
                Ins::Arg(v, k) => {
                    let x = arg(self, *k);
                    self.set(*v, x);
                }
                Ins::Const(v, bits) => {
                    let x = self.fc(f64::from_bits(*bits));
                    self.set(*v, x);
                }
                Ins::Copy(d, s) => {
                    let x = self.get(*s);
                    self.set(*d, x);
                }
                Ins::Un(d, op, a) => {
                    let a = self.get(*a);
                    let x = self.un(*op, a);
                    self.set(*d, x);
                }
                Ins::Bin(d, op, l, r) => {
                    let (l, r) = (self.get(*l), self.get(*r));
                    let x = self.bin(*op, l, r);
                    self.set(*d, x);
                }
                Ins::Cmp(d, op, l, r) => {
                    let (l, r) = (self.get(*l), self.get(*r));
                    let x = self.cmp(*op, l, r);
                    self.set(*d, x);
                }
                Ins::Select(d, c, a, e) => {
                    let (c, a, e) = (self.get(*c), self.get(*a), self.get(*e));
                    let z = self.fc(0.0);
                    let holds = self.b.ins().fcmp(FloatCC::NotEqual, c, z);
                    let x = self.b.ins().select(holds, a, e);
                    self.set(*d, x);
                }
                Ins::Finite(v) => {
                    let x = self.get(*v);
                    let c = self.below(x, f64::INFINITY);
                    self.check(c);
                }
                Ins::Whole(v) => {
                    let x = self.get(*v);
                    let c = self.below(x, WHOLE_BOUND);
                    self.check(c);
                }
                Ins::If(c, then, other) => {
                    self.flush();
                    let c = self.get(*c);
                    let z = self.fc(0.0);
                    let holds = self.b.ins().fcmp(FloatCC::NotEqual, c, z);
                    let (tb, eb, join) =
                        (self.b.create_block(), self.b.create_block(), self.b.create_block());
                    self.b.ins().brif(holds, tb, &[], eb, &[]);
                    for (blk, code) in [(tb, then), (eb, other)] {
                        self.b.switch_to_block(blk);
                        self.run(code, arg);
                        self.flush();
                        self.b.ins().jump(join, &[]);
                    }
                    self.b.switch_to_block(join);
                }
                Ins::Switch(sel, arms) => {
                    self.flush();
                    let s = self.get(*sel);
                    let n = arms.len() as f64;
                    let join = self.b.create_block();
                    for (k, code) in arms.iter().enumerate() {
                        let (here, next) = (self.b.create_block(), self.b.create_block());
                        let k = k as f64;
                        let a = self.fc(k);
                        let c = self.fc(k - n);
                        let e1 = self.b.ins().fcmp(FloatCC::Equal, s, a);
                        let e2 = self.b.ins().fcmp(FloatCC::Equal, s, c);
                        let e = self.b.ins().bor(e1, e2);
                        self.b.ins().brif(e, here, &[], next, &[]);
                        self.b.switch_to_block(here);
                        self.run(code, arg);
                        self.flush();
                        self.b.ins().jump(join, &[]);
                        self.b.switch_to_block(next);
                    }
                    // An index that names no verb of the gerund.
                    self.b.ins().jump(self.bail, &[]);
                    self.b.switch_to_block(join);
                }
                Ins::While(test, cond, body) => {
                    self.flush();
                    let count = self.b.declare_var(types::I64);
                    let zero = self.b.ins().iconst(types::I64, 0);
                    self.b.def_var(count, zero);
                    let (head, blk, exit) =
                        (self.b.create_block(), self.b.create_block(), self.b.create_block());
                    self.b.ins().jump(head, &[]);
                    self.b.switch_to_block(head);
                    self.run(test, arg);
                    self.flush();
                    let c = self.get(*cond);
                    let z = self.fc(0.0);
                    let holds = self.b.ins().fcmp(FloatCC::NotEqual, c, z);
                    self.b.ins().brif(holds, blk, &[], exit, &[]);
                    self.b.switch_to_block(blk);
                    self.run(body, arg);
                    self.flush();
                    let i = self.b.use_var(count);
                    let i1 = self.b.ins().iadd_imm(i, 1);
                    self.b.def_var(count, i1);
                    let more = self.b.ins().icmp_imm(IntCC::SignedLessThan, i1, CAP as i64);
                    self.b.ins().brif(more, head, &[], self.bail, &[]);
                    self.b.switch_to_block(exit);
                }
                Ins::Repeat { count, index, body } => {
                    self.flush();
                    let n = self.get(*count);
                    // A whole number from 0 to CAP, or the loop stops.
                    let z = self.fc(0.0);
                    let cap = self.fc(CAP as f64);
                    let lo = self.b.ins().fcmp(FloatCC::GreaterThanOrEqual, n, z);
                    let hi = self.b.ins().fcmp(FloatCC::LessThanOrEqual, n, cap);
                    let fl = self.b.ins().floor(n);
                    let whole = self.b.ins().fcmp(FloatCC::Equal, fl, n);
                    let ok = self.b.ins().band(lo, hi);
                    let ok = self.b.ins().band(ok, whole);
                    self.check(ok);
                    self.flush();
                    let total = self.b.ins().fcvt_to_sint(types::I64, n);
                    let i = self.b.declare_var(types::I64);
                    let zero = self.b.ins().iconst(types::I64, 0);
                    self.b.def_var(i, zero);
                    let (head, blk, exit) =
                        (self.b.create_block(), self.b.create_block(), self.b.create_block());
                    self.b.ins().jump(head, &[]);
                    self.b.switch_to_block(head);
                    let iv = self.b.use_var(i);
                    let more = self.b.ins().icmp(IntCC::SignedLessThan, iv, total);
                    self.b.ins().brif(more, blk, &[], exit, &[]);
                    self.b.switch_to_block(blk);
                    let iv = self.b.use_var(i);
                    let fi = self.b.ins().fcvt_from_sint(types::F64, iv);
                    self.set(*index, fi);
                    self.run(body, arg);
                    self.flush();
                    let iv = self.b.use_var(i);
                    let i1 = self.b.ins().iadd_imm(iv, 1);
                    self.b.def_var(i, i1);
                    self.b.ins().jump(head, &[]);
                    self.b.switch_to_block(exit);
                }
            }
        }
    }
}

/// A float read from `p + off` as `repr` stores it.
fn load(b: &mut FunctionBuilder<'_>, repr: Repr, p: Value, off: i32) -> Value {
    match repr {
        Repr::F64 => b.ins().load(types::F64, MemFlags::trusted(), p, off),
        Repr::I64 => {
            let i = b.ins().load(types::I64, MemFlags::trusted(), p, off);
            b.ins().fcvt_from_sint(types::F64, i)
        }
        Repr::Bool => {
            let i = b.ins().uload8(types::I64, MemFlags::trusted(), p, off);
            b.ins().fcvt_from_uint(types::F64, i)
        }
    }
}

fn compile_inner(shape: Shape, prog: &Prog) -> Option<usize> {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").ok()?;
    flags.set("use_colocated_libcalls", "false").ok()?;
    flags.set("is_pic", "false").ok()?;
    let isa = cranelift_native::builder().ok()?.finish(settings::Flags::new(flags)).ok()?;
    let ptr = isa.pointer_type();
    let call_conv = isa.default_call_conv();
    let mut module = JITModule::new(JITBuilder::with_isa(isa, default_libcall_names()));

    let mut sig = module.make_signature();
    let params: &[Type] = match shape {
        // (xs, xstride, n, seed, out, ostride, last) -> steps taken
        Shape::Fold { .. } => &[ptr, types::I64, types::I64, types::F64, ptr, types::I64, ptr],
        // (ys, ystride, xs, xstride, n, out, kind) -> items computed
        Shape::Cells { .. } => &[ptr, types::I64, ptr, types::I64, types::I64, ptr, ptr],
    };
    for &t in params {
        sig.params.push(AbiParam::new(t));
    }
    sig.returns.push(AbiParam::new(types::I64));

    let id = module.declare_function("loop", Linkage::Local, &sig).ok()?;
    let mut ctx = module.make_context();
    ctx.func.signature = sig;
    let mut fctx = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let unary = {
            let mut s = Signature::new(call_conv);
            s.params.push(AbiParam::new(types::F64));
            s.returns.push(AbiParam::new(types::F64));
            b.import_signature(s)
        };
        let vars = (0..prog.vars).map(|_| b.declare_var(types::F64)).collect();
        let entry = b.create_block();
        let head = b.create_block();
        let body = b.create_block();
        let exit = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let p = b.block_params(entry).to_vec();
        let i = b.declare_var(types::I64);
        let zero = b.ins().iconst(types::I64, 0);
        b.def_var(i, zero);
        let mut g = Gen {
            b: &mut b,
            vars,
            bail: exit,
            ok: None,
            unary,
            ptr,
            ct: f64::from_bits(prog.ct),
            by_smaller: prog.by_smaller,
        };
        match shape {
            Shape::Fold { store } => {
                let (xs, xstride, n, seed, out, ostride, last) =
                    (p[0], p[1], p[2], p[3], p[4], p[5], p[6]);
                let acc = g.b.declare_var(types::F64);
                g.b.def_var(acc, seed);
                g.b.ins().jump(head, &[]);

                g.b.switch_to_block(head);
                let iv = g.b.use_var(i);
                let more = g.b.ins().icmp(IntCC::SignedLessThan, iv, n);
                g.b.ins().brif(more, body, &[], exit, &[]);

                g.b.switch_to_block(body);
                let iv = g.b.use_var(i);
                let xstep = g.b.ins().imul(iv, xstride);
                let xoff = g.b.ins().ishl_imm(xstep, 3);
                let xp = g.b.ins().iadd(xs, xoff);
                g.run(&prog.code, &mut |g, k| {
                    if k == 0 {
                        return g.b.use_var(acc);
                    }
                    let v = g.b.ins().load(types::F64, MemFlags::trusted(), xp, ((k - 1) * 8) as i32);
                    let c = g.below(v, f64::INFINITY);
                    g.check(c);
                    v
                });
                g.flush();
                let new = g.get(prog.result);
                if store {
                    let iv = g.b.use_var(i);
                    let ostep = g.b.ins().imul(iv, ostride);
                    let ooff = g.b.ins().ishl_imm(ostep, 3);
                    let op = g.b.ins().iadd(out, ooff);
                    g.b.ins().store(MemFlags::trusted(), new, op, 0);
                }
                g.b.def_var(acc, new);
                let iv = g.b.use_var(i);
                let i1 = g.b.ins().iadd_imm(iv, 1);
                g.b.def_var(i, i1);
                g.b.ins().jump(head, &[]);

                g.b.switch_to_block(exit);
                let a = g.b.use_var(acc);
                g.b.ins().store(MemFlags::trusted(), a, last, 0);
                let iv = g.b.use_var(i);
                g.b.ins().return_(&[iv]);
            }
            Shape::Cells { y, x } => {
                let (ys, ystride, xs, xstride, n, out, kind_out) =
                    (p[0], p[1], p[2], p[3], p[4], p[5], p[6]);
                let top = g.b.declare_var(types::F64);
                let z = g.b.ins().f64const(0.0);
                g.b.def_var(top, z);
                g.b.ins().jump(head, &[]);

                g.b.switch_to_block(head);
                let iv = g.b.use_var(i);
                let more = g.b.ins().icmp(IntCC::SignedLessThan, iv, n);
                g.b.ins().brif(more, body, &[], exit, &[]);

                g.b.switch_to_block(body);
                let iv = g.b.use_var(i);
                let atom = |g: &mut Gen<'_, '_>, base: Value, stride: Value, repr: Repr| {
                    let at = g.b.ins().imul(iv, stride);
                    let off = g.b.ins().imul_imm(at, repr_size(repr));
                    let p = g.b.ins().iadd(base, off);
                    let v = load(g.b, repr, p, 0);
                    match repr {
                        Repr::F64 => {
                            let c = g.below(v, f64::INFINITY);
                            g.check(c);
                        }
                        Repr::I64 => {
                            let c = g.below(v, WHOLE_BOUND);
                            g.check(c);
                        }
                        Repr::Bool => {}
                    }
                    v
                };
                let yv = atom(&mut g, ys, ystride, y);
                let xv = x.map(|r| atom(&mut g, xs, xstride, r));
                g.run(&prog.code, &mut |_, k| if k == 0 { yv } else { xv.expect("a dyad's left argument") });
                g.flush();
                let r = g.get(prog.result);
                let iv = g.b.use_var(i);
                let ooff = g.b.ins().ishl_imm(iv, 3);
                let op = g.b.ins().iadd(out, ooff);
                g.b.ins().store(MemFlags::trusted(), r, op, 0);
                let k = match prog.out {
                    Out::Static(k) => g.fc(k.code()),
                    Out::Dyn(v) => g.get(v),
                };
                let t = g.b.use_var(top);
                let gt = g.b.ins().fcmp(FloatCC::GreaterThan, k, t);
                let t1 = g.b.ins().select(gt, k, t);
                g.b.def_var(top, t1);
                let i1 = g.b.ins().iadd_imm(iv, 1);
                g.b.def_var(i, i1);
                g.b.ins().jump(head, &[]);

                g.b.switch_to_block(exit);
                let t = g.b.use_var(top);
                g.b.ins().store(MemFlags::trusted(), t, kind_out, 0);
                let iv = g.b.use_var(i);
                g.b.ins().return_(&[iv]);
            }
        }
        b.seal_all_blocks();
        b.finalize();
    }
    module.define_function(id, &mut ctx).ok()?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().ok()?;
    let code = module.get_finalized_function(id);
    // The code lives as long as the module's memory, which is never freed:
    // the cache hands the loop out for the rest of the process.
    std::mem::forget(module);
    Some(code as usize)
}

fn repr_size(r: Repr) -> i64 {
    match r {
        Repr::F64 | Repr::I64 => 8,
        Repr::Bool => 1,
    }
}
