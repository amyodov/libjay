//! The fold loop compiled with Cranelift.

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{types, AbiParam, InstBuilder, MemFlags, Signature, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};

use super::{Bin, Kernel, Node, Raw, Step, Un};
use crate::scalar;

/// Compile a step's loop. None where the host has no Cranelift backend or
/// the compilation fails for any reason.
pub(super) fn compile(step: &Step) -> Option<Kernel> {
    std::panic::catch_unwind(|| compile_inner(step)).ok().flatten()
}

fn compile_inner(step: &Step) -> Option<Kernel> {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").ok()?;
    flags.set("use_colocated_libcalls", "false").ok()?;
    flags.set("is_pic", "false").ok()?;
    let isa = cranelift_native::builder().ok()?.finish(settings::Flags::new(flags)).ok()?;
    let ptr = isa.pointer_type();
    let call_conv = isa.default_call_conv();
    let mut module = JITModule::new(JITBuilder::with_isa(isa, default_libcall_names()));

    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(ptr)); // xs
    sig.params.push(AbiParam::new(types::I64)); // xstride
    sig.params.push(AbiParam::new(types::I64)); // n
    sig.params.push(AbiParam::new(types::F64)); // seed
    sig.params.push(AbiParam::new(ptr)); // out
    sig.params.push(AbiParam::new(types::I64)); // ostride
    sig.params.push(AbiParam::new(ptr)); // last
    sig.returns.push(AbiParam::new(types::I64));

    let id = module.declare_function("fold", Linkage::Local, &sig).ok()?;
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

        let entry = b.create_block();
        let head = b.create_block();
        let body = b.create_block();
        let exit = b.create_block();
        b.append_block_params_for_function_params(entry);
        // head(i, acc, xp, op)
        let hi = b.append_block_param(head, types::I64);
        let hacc = b.append_block_param(head, types::F64);
        let hxp = b.append_block_param(head, ptr);
        let hop = b.append_block_param(head, ptr);
        // exit(i, acc)
        let ei = b.append_block_param(exit, types::I64);
        let eacc = b.append_block_param(exit, types::F64);

        b.switch_to_block(entry);
        let p = b.block_params(entry).to_vec();
        let (xs, xstride, n, seed, out, ostride, last) = (p[0], p[1], p[2], p[3], p[4], p[5], p[6]);
        let xstep = b.ins().ishl_imm(xstride, 3);
        let ostep = b.ins().ishl_imm(ostride, 3);
        let zero = b.ins().iconst(types::I64, 0);
        b.ins().jump(head, &[zero.into(), seed.into(), xs.into(), out.into()]);

        b.switch_to_block(head);
        let more = b.ins().icmp(IntCC::SignedLessThan, hi, n);
        b.ins().brif(more, body, &[], exit, &[hi.into(), hacc.into()]);

        b.switch_to_block(body);
        let inf = b.ins().f64const(f64::INFINITY);
        let mut vals: Vec<Value> = Vec::with_capacity(step.nodes.len());
        let mut ok: Option<Value> = None;
        let finite = |b: &mut FunctionBuilder, v: Value, ok: &mut Option<Value>| {
            let m = b.ins().fabs(v);
            let f = b.ins().fcmp(FloatCC::LessThan, m, inf);
            *ok = Some(match *ok {
                None => f,
                Some(o) => b.ins().band(o, f),
            });
        };
        for node in &step.nodes {
            let v = match *node {
                Node::X(k) => {
                    let v = b.ins().load(types::F64, MemFlags::trusted(), hxp, (k * 8) as i32);
                    finite(&mut b, v, &mut ok);
                    v
                }
                Node::Y => hacc,
                Node::Const(bits) => b.ins().f64const(f64::from_bits(bits)),
                Node::Un(op, a) => {
                    let a = vals[a as usize];
                    let v = match op {
                        Un::Neg => b.ins().fneg(a),
                        Un::Abs => {
                            // `scalar::abs`: the negative of a value below
                            // zero and the value itself otherwise.
                            let z = b.ins().f64const(0.0);
                            let below = b.ins().fcmp(FloatCC::LessThan, a, z);
                            let neg = b.ins().fneg(a);
                            b.ins().select(below, neg, a)
                        }
                        Un::Inc => {
                            let one = b.ins().f64const(1.0);
                            b.ins().fadd(a, one)
                        }
                        Un::Dec => {
                            let one = b.ins().f64const(1.0);
                            b.ins().fsub(a, one)
                        }
                        Un::Double => b.ins().fadd(a, a),
                        Un::Halve => {
                            let two = b.ins().f64const(2.0);
                            b.ins().fdiv(a, two)
                        }
                        Un::Square => b.ins().fmul(a, a),
                        Un::OneMinus => {
                            let one = b.ins().f64const(1.0);
                            b.ins().fsub(one, a)
                        }
                        Un::Recip => {
                            let one = b.ins().f64const(1.0);
                            b.ins().fdiv(one, a)
                        }
                        Un::Sqrt => b.ins().sqrt(a),
                        Un::Exp | Un::Ln => {
                            let f: extern "C" fn(f64) -> f64 =
                                if op == Un::Exp { scalar::exp } else { scalar::ln };
                            let callee = b.ins().iconst(ptr, f as usize as i64);
                            let call = b.ins().call_indirect(unary, callee, &[a]);
                            b.inst_results(call)[0]
                        }
                    };
                    finite(&mut b, v, &mut ok);
                    v
                }
                Node::Bin(op, l, r) => {
                    let (l, r) = (vals[l as usize], vals[r as usize]);
                    let v = match op {
                        Bin::Add => b.ins().fadd(l, r),
                        Bin::Sub => b.ins().fsub(l, r),
                        Bin::Mul => b.ins().fmul(l, r),
                        // `scalar::div` differs from IEEE only at `0 % 0`,
                        // whose NaN stops the loop.
                        Bin::Div => b.ins().fdiv(l, r),
                        // `scalar::min` and `scalar::max`: the left operand
                        // where the strict comparison picks it, otherwise
                        // the right, which is how the interpreter reads one
                        // pair of atoms. A NaN operand stops the loop
                        // before the answer is used.
                        Bin::Min => {
                            let lt = b.ins().fcmp(FloatCC::LessThan, l, r);
                            b.ins().select(lt, l, r)
                        }
                        Bin::Max => {
                            let gt = b.ins().fcmp(FloatCC::GreaterThan, l, r);
                            b.ins().select(gt, l, r)
                        }
                    };
                    finite(&mut b, v, &mut ok);
                    v
                }
            };
            vals.push(v);
        }
        let new = vals[step.root as usize];
        let cont = b.create_block();
        match ok {
            Some(ok) => {
                b.ins().brif(ok, cont, &[], exit, &[hi.into(), hacc.into()]);
            }
            None => {
                b.ins().jump(cont, &[]);
            }
        }
        b.switch_to_block(cont);
        if step.store {
            b.ins().store(MemFlags::trusted(), new, hop, 0);
        }
        let i1 = b.ins().iadd_imm(hi, 1);
        let xp1 = b.ins().iadd(hxp, xstep);
        let op1 = b.ins().iadd(hop, ostep);
        b.ins().jump(head, &[i1.into(), new.into(), xp1.into(), op1.into()]);

        b.switch_to_block(exit);
        b.ins().store(MemFlags::trusted(), eacc, last, 0);
        b.ins().return_(&[ei]);

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
    // SAFETY: the function was declared with exactly this signature, under
    // the host's default calling convention, which `extern "C"` is.
    Some(Kernel(unsafe { std::mem::transmute::<*const u8, Raw>(code) }))
}

