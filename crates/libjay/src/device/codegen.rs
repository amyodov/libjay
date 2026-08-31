//! WGSL for a fused kernel, generated at run time.
//!
//! The fusion pass already reduced a chain of scalar verbs to a postfix
//! program over a stack ([`crate::fuse::Instr`]). That program is the kernel
//! description, and it is the only one: this module walks it and writes
//! shader text, exactly as [`crate::fuse`]'s block executor walks it and
//! calls block loops. Nothing here knows what J or APL primitive a step came
//! from, and there is no per-primitive shader anywhere — adding a verb to
//! the fusable set adds one arm to the two `expr` functions below and
//! nothing else.
//!
//! Shaders are compiled by the driver when a program first runs on a
//! device. The build produces no shader and does not know what adapters
//! exist, which is what keeps compilation hermetic.

use crate::fuse::{FusedKernel, Instr};
use crate::verb::{ScalarDyad, ScalarMonad, Tol};

/// Threads per workgroup. 256 is the size every current adapter runs at
/// full occupancy; nothing here depends on the number beyond the workgroup
/// array the reduction declares, which is sized from it.
pub(crate) const WORKGROUP: usize = 256;

/// Most workgroups one reduction dispatches. The partials come back to the
/// host and are folded there, so this bounds that readback at a few kB.
const MAX_GROUPS: usize = 1024;

/// The entry point that writes one output per element.
pub(crate) const MAP: &str = "map";
/// The entry point that folds the mapped values, one partial per workgroup.
pub(crate) const REDUCE: &str = "reduce";

/// The type a device kernel computes in.
///
/// libjay's own arithmetic is f64. A device that has f64 in its shaders
/// computes what the CPU computes; one that has not runs nothing unless the
/// caller asks for [`Precision::F32`] in so many words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    F64,
    F32,
}

impl Precision {
    /// Bytes one element takes on the device.
    pub fn size(self) -> usize {
        match self {
            Precision::F64 => 8,
            Precision::F32 => 4,
        }
    }

    /// The name `deploy(precision=...)` takes.
    pub fn name(self) -> &'static str {
        match self {
            Precision::F64 => "f64",
            Precision::F32 => "f32",
        }
    }

    pub fn from_name(s: &str) -> Option<Precision> {
        match s.trim().to_ascii_lowercase().as_str() {
            "f64" | "double" => Some(Precision::F64),
            "f32" | "single" | "float" => Some(Precision::F32),
            _ => None,
        }
    }

    fn ty(self) -> &'static str {
        self.name()
    }

    /// WGSL's suffix for a literal of this type.
    fn suffix(self) -> &'static str {
        match self {
            Precision::F64 => "lf",
            Precision::F32 => "f",
        }
    }
}

/// Workgroups a reduction over `n` elements dispatches.
///
/// Never more threads than there are elements: every thread then starts its
/// grid-stride loop with a value of its own, so the fold needs no identity
/// element — which for `>./` would have to be an infinity WGSL cannot
/// spell.
pub(crate) fn groups_for(n: usize) -> usize {
    (n / WORKGROUP).clamp(1, MAX_GROUPS)
}

// ------------------------------------------------------------------ buffers

/// Host floats as the element bytes a device buffer holds them in.
///
/// An iterator rather than a filled buffer: mapped device memory is written
/// through a write-only reference that hands out one slot at a time, and the
/// arrays this uploads are tens of megabytes, so an intermediate `Vec` would
/// be one more pass over all of them for nothing.
pub(crate) fn byte_iter(v: &[f64], p: Precision) -> impl Iterator<Item = u8> {
    v.iter().flat_map(move |&x| {
        let mut b = [0u8; 8];
        match p {
            Precision::F64 => b.copy_from_slice(&x.to_ne_bytes()),
            Precision::F32 => b[..4].copy_from_slice(&(x as f32).to_ne_bytes()),
        }
        b.into_iter().take(p.size())
    })
}

/// `n` device elements as host floats.
pub(crate) fn from_bytes(b: &[u8], p: Precision, n: usize) -> Vec<f64> {
    let w = p.size();
    (0..n)
        .map(|i| {
            let s = &b[i * w..(i + 1) * w];
            match p {
                Precision::F64 => f64::from_ne_bytes(s.try_into().expect("8 bytes")),
                Precision::F32 => f32::from_ne_bytes(s.try_into().expect("4 bytes")) as f64,
            }
        })
        .collect()
}

// ---------------------------------------------------------------- generation

/// Helper functions the chain turned out to need. Emitting only these keeps
/// a shader to what it uses, which matters on a driver that type-checks
/// every function it is handed whether or not anything calls it.
#[derive(Default)]
struct Needs {
    tol_eq: bool,
    tol_lt: bool,
    tol_le: bool,
    recip: bool,
    divj: bool,
    residue: bool,
}

/// The shader for this kernel, or the name of the operation that has no
/// shader form.
pub(crate) fn wgsl(
    k: &FusedKernel,
    splat: &[bool],
    p: Precision,
) -> Result<String, &'static str> {
    let mut needs = Needs::default();
    let body = chain_body(k, splat, p, &mut needs)?;
    let reduce = match k.reduce() {
        None => None,
        Some(op) => Some(fold_expr(op)?),
    };

    let t = p.ty();
    let mut s = String::new();
    s.push_str("// generated by libjay from a fused kernel\n");
    s.push_str("struct JayGrid { n: u32, stride: u32 };\n");
    s.push_str("@group(0) @binding(0) var<uniform> jg : JayGrid;\n");
    s.push_str(&format!(
        "@group(0) @binding(1) var<storage, read_write> jay_out : array<{t}>;\n"
    ));
    for i in 0..splat.len() {
        s.push_str(&format!(
            "@group(0) @binding({}) var<storage, read> jay_in{i} : array<{t}>;\n",
            i + 2
        ));
    }
    s.push('\n');
    s.push_str(&helpers(&needs, k.tol(), p));
    s.push_str(&format!("fn jay_chain(i: u32) -> {t} {{\n{body}}}\n\n"));

    s.push_str(&format!("@compute @workgroup_size({WORKGROUP})\n"));
    s.push_str(&format!("fn {MAP}(@builtin(global_invocation_id) gid: vec3<u32>) {{\n"));
    s.push_str("  let i = gid.x;\n  if (i >= jg.n) { return; }\n");
    s.push_str("  jay_out[i] = jay_chain(i);\n}\n");

    if let Some(fold) = reduce {
        s.push_str(&format!("\nvar<workgroup> lane : array<{t}, {WORKGROUP}>;\n\n"));
        s.push_str(&format!("@compute @workgroup_size({WORKGROUP})\n"));
        s.push_str(&format!("fn {REDUCE}(\n"));
        s.push_str("  @builtin(global_invocation_id) gid: vec3<u32>,\n");
        s.push_str("  @builtin(local_invocation_id) lid: vec3<u32>,\n");
        s.push_str("  @builtin(workgroup_id) wid: vec3<u32>,\n");
        s.push_str(") {\n");
        // The grid never holds more threads than there are elements, so the
        // first value needs no test and the fold needs no identity.
        s.push_str("  var acc = jay_chain(gid.x);\n");
        s.push_str("  var i = gid.x + jg.stride;\n");
        s.push_str("  loop {\n    if (i >= jg.n) { break; }\n");
        s.push_str(&format!("    acc = {};\n", fold("acc", "jay_chain(i)")));
        s.push_str("    i = i + jg.stride;\n  }\n");
        s.push_str("  lane[lid.x] = acc;\n  workgroupBarrier();\n");
        // A tree over the workgroup. `s` is the same for every lane, so the
        // barrier is reached uniformly, which WGSL requires.
        s.push_str(&format!("  var s = {}u;\n", WORKGROUP / 2));
        s.push_str("  loop {\n    if (s == 0u) { break; }\n");
        s.push_str(&format!(
            "    if (lid.x < s) {{ lane[lid.x] = {}; }}\n",
            fold("lane[lid.x]", "lane[lid.x + s]")
        ));
        s.push_str("    workgroupBarrier();\n    s = s >> 1u;\n  }\n");
        s.push_str("  if (lid.x == 0u) { jay_out[wid.x] = lane[0]; }\n}\n");
    }
    Ok(s)
}

/// The straight-line body of `chain`: one `let` per step of the postfix
/// program, in the order the program performs them.
fn chain_body(
    k: &FusedKernel,
    splat: &[bool],
    p: Precision,
    needs: &mut Needs,
) -> Result<String, &'static str> {
    let mut out = String::new();
    let mut stack: Vec<String> = Vec::new();
    let mut lets: Vec<String> = Vec::new();
    let mut temp = 0usize;
    for ins in k.code() {
        match ins {
            Instr::Load(j) => {
                let at = if *splat.get(*j).unwrap_or(&false) { "0u" } else { "i" };
                stack.push(format!("jay_in{j}[{at}]"));
            }
            Instr::Let(j) => stack.push(lets.get(*j).ok_or("let")?.clone()),
            Instr::Store(j) => {
                let v = stack.pop().ok_or("store")?;
                let name = format!("l{j}");
                out.push_str(&format!("  let {name} = {v};\n"));
                lets.push(name);
            }
            Instr::Monad(op) => {
                let a = stack.pop().ok_or("monad")?;
                let e = monad_expr(*op, &a, p, needs)?;
                let name = format!("t{temp}");
                temp += 1;
                out.push_str(&format!("  let {name} = {e};\n"));
                stack.push(name);
            }
            // A window step and a running fold read items the shader's own
            // element does not: they stay on the CPU.
            Instr::Window(..) => return Err("a moving window"),
            Instr::Scan(_) => return Err("a running fold"),
            Instr::Dyad(op) => {
                let b = stack.pop().ok_or("dyad")?;
                let a = stack.pop().ok_or("dyad")?;
                let e = dyad_expr(*op, &a, &b, p, needs)?;
                let name = format!("t{temp}");
                temp += 1;
                out.push_str(&format!("  let {name} = {e};\n"));
                stack.push(name);
            }
        }
    }
    let root = stack.pop().ok_or("empty kernel")?;
    out.push_str(&format!("  return {root};\n"));
    Ok(out)
}

/// A literal of the shader's element type.
fn lit(v: f64, p: Precision) -> String {
    let mut s = format!("{v:?}");
    if !s.contains('.') && !s.contains('e') {
        s.push_str(".0");
    }
    s.push_str(p.suffix());
    s
}

fn monad_expr(
    op: ScalarMonad,
    a: &str,
    p: Precision,
    needs: &mut Needs,
) -> Result<String, &'static str> {
    use ScalarMonad::*;
    let one = lit(1.0, p);
    Ok(match op {
        Conj => format!("({a})"),
        Neg => format!("-({a})"),
        Abs => format!("abs({a})"),
        Signum => format!("sign({a})"),
        Recip => {
            needs.recip = true;
            format!("recip({a})")
        }
        Floor => format!("floor({a})"),
        Ceil => format!("ceil({a})"),
        Inc => format!("({a}) + {one}"),
        Dec => format!("({a}) - {one}"),
        Double => format!("({a}) + ({a})"),
        Halve => format!("({a}) / {}", lit(2.0, p)),
        Square => format!("({a}) * ({a})"),
        OneMinus => format!("{one} - ({a})"),
        // The exponential is a 32-bit builtin: SPIR-V's extended
        // instruction set and MSL both define it for single precision only,
        // so an f64 chain that reaches one stays on the CPU.
        Exp if p == Precision::F32 => format!("exp({a})"),
        Exp => return Err("^"),
        _ => return Err("this monad"),
    })
}

fn dyad_expr(
    op: ScalarDyad,
    a: &str,
    b: &str,
    p: Precision,
    needs: &mut Needs,
) -> Result<String, &'static str> {
    use ScalarDyad::*;
    // A comparison is a number inside a kernel, as it is in J; the dtype of
    // a result made from one is the caller's business.
    let bool_to_num =
        |c: String| format!("select({}, {}, {c})", lit(0.0, p), lit(1.0, p));
    Ok(match op {
        Add => format!("({a}) + ({b})"),
        Sub => format!("({a}) - ({b})"),
        Mul => format!("({a}) * ({b})"),
        Min => format!("min({a}, {b})"),
        Max => format!("max({a}, {b})"),
        DivJ => {
            needs.divj = true;
            format!("divj({a}, {b})")
        }
        Residue => {
            needs.residue = true;
            format!("residue({a}, {b})")
        }
        Eq => {
            needs.tol_eq = true;
            bool_to_num(format!("teq({a}, {b})"))
        }
        Ne => {
            needs.tol_eq = true;
            bool_to_num(format!("!teq({a}, {b})"))
        }
        Lt => {
            needs.tol_lt = true;
            bool_to_num(format!("tlt({a}, {b})"))
        }
        Le => {
            needs.tol_le = true;
            bool_to_num(format!("tle({a}, {b})"))
        }
        Gt => {
            needs.tol_lt = true;
            bool_to_num(format!("tlt({b}, {a})"))
        }
        Ge => {
            needs.tol_le = true;
            bool_to_num(format!("tle({b}, {a})"))
        }
        _ => return Err("this dyad"),
    })
}

/// How an absorbed reduction combines two values.
fn fold_expr(op: ScalarDyad) -> Result<fn(&str, &str) -> String, &'static str> {
    use ScalarDyad::*;
    Ok(match op {
        Add => |a: &str, b: &str| format!("{a} + {b}"),
        Mul => |a: &str, b: &str| format!("{a} * {b}"),
        Min => |a: &str, b: &str| format!("min({a}, {b})"),
        Max => |a: &str, b: &str| format!("max({a}, {b})"),
        _ => return Err("this reduction"),
    })
}

/// The helper functions the chain used, with the dialect's comparison
/// tolerance compiled into them, so that a comparison on the device answers
/// as the same comparison does anywhere else.
fn helpers(needs: &Needs, tol: Tol, p: Precision) -> String {
    let t = p.ty();
    let zero = lit(0.0, p);
    let one = lit(1.0, p);
    let mut s = String::new();
    if needs.tol_eq || needs.tol_lt || needs.tol_le {
        let scale = if tol.by_smaller { "min" } else { "max" };
        s.push_str(&format!("fn teq(a: {t}, b: {t}) -> bool {{\n"));
        s.push_str("  if (a == b) { return true; }\n");
        s.push_str(&format!("  let s = {scale}(abs(a), abs(b));\n"));
        s.push_str(&format!("  return abs(a - b) < {} * s;\n}}\n", lit(tol.ct, p)));
    }
    if needs.tol_lt {
        s.push_str(&format!(
            "fn tlt(a: {t}, b: {t}) -> bool {{ return a < b && !teq(a, b); }}\n"
        ));
    }
    if needs.tol_le {
        s.push_str(&format!(
            "fn tle(a: {t}, b: {t}) -> bool {{ return a <= b || teq(a, b); }}\n"
        ));
    }
    if needs.recip {
        // `% 0` is infinity, as it is unfused. Dividing by the magnitude
        // rather than by the value keeps that out of the shader compiler's
        // constant folding, and gives -0 the same +infinity J gives it.
        s.push_str(&format!("fn recip(x: {t}) -> {t} {{\n"));
        s.push_str(&format!("  if (x == {zero}) {{ return {one} / abs(x); }}\n"));
        s.push_str(&format!("  return {one} / x;\n}}\n"));
    }
    if needs.divj {
        s.push_str(&format!("fn divj(x: {t}, y: {t}) -> {t} {{\n"));
        s.push_str(&format!("  if (y == {zero}) {{\n"));
        s.push_str(&format!("    if (x == {zero}) {{ return {zero}; }}\n"));
        // A NEGATIVE zero divisor turns the infinity over, as it does
        // on the host: `1 % (% __)` is `__`.
        s.push_str("    return sign(x) / y;\n  }\n");
        s.push_str("  return x / y;\n}\n");
    }
    if needs.residue {
        // The quotient is rounded with the dialect's tolerance, as it is on
        // the host: J takes the tolerant floor and answers an exact zero
        // when the product is tolerantly the dividend, GNU APL shifts the
        // quotient by `⎕CT` and reads the remainder against the modulus.
        let ct = lit(tol.ct, p);
        s.push_str(&format!("fn residue(x: {t}, y: {t}) -> {t} {{\n"));
        s.push_str(&format!("  if (x == {zero}) {{ return y; }}\n"));
        if tol.by_smaller {
            s.push_str("  let q = y / x;\n");
            s.push_str("  let c = ceil(q);\n");
            s.push_str(&format!(
                "  let k = select(floor(q), c, abs(q - c) < {ct} * min(abs(q), abs(c)));\n"
            ));
            s.push_str("  let d = x * k;\n");
            s.push_str(&format!(
                "  if (abs(y - d) < {ct} * min(abs(y), abs(d))) {{ return {zero}; }}\n"
            ));
            s.push_str("  return y - d;\n}\n");
        } else {
            s.push_str("  let q = y / x;\n  let c = ceil(q);\n  let gap = c - q;\n");
            s.push_str(&format!(
                "  let k = select(floor(q), c, gap <= {ct} || gap < {ct} * max(abs(q), abs(c)));\n"
            ));
            s.push_str("  let r = y - x * k;\n");
            s.push_str(&format!("  if (abs(r) < {ct} * abs(x)) {{ return {zero}; }}\n"));
            s.push_str(&format!(
                "  if (r != {zero} && (r < {zero}) != (x < {zero})) {{ return r + x; }}\n"
            ));
            s.push_str("  return r;\n}\n");
        }
    }
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{compile, Dialect, Lang};
    use crate::ir::Expr;

    /// The first fused node the program holds, wherever it sits.
    fn kernel(src: &str) -> FusedKernel {
        fn find(e: &Expr) -> Option<FusedKernel> {
            match e {
                Expr::Fused { kernel, .. } => Some(kernel.clone()),
                Expr::Assign { value, .. } | Expr::PrintPass { value, .. } => find(value),
                Expr::Monad { y, .. } => find(y),
                Expr::Dyad { x, y, .. } => find(x).or_else(|| find(y)),
                _ => None,
            }
        }
        let p = compile(Lang::J, src, &Dialect::default()).expect("compile");
        p.stmts.iter().find_map(find).unwrap_or_else(|| panic!("{src} did not fuse"))
    }

    /// Parse and type-check generated WGSL the way a driver would, without
    /// an adapter. The f64 path cannot be executed on a Metal machine; this
    /// is what holds it to being valid all the same.
    fn validate(src: &str, p: Precision) {
        let module = naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("{}\n\n{src}", e.emit_to_string(src)));
        let caps = match p {
            Precision::F64 => naga::valid::Capabilities::FLOAT64,
            Precision::F32 => naga::valid::Capabilities::empty(),
        };
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps)
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}\n\n{src}"));
    }

    const CHAINS: &[&str] = &[
        "+/ {w} * {x}",
        "1 + 2 * {x}",
        "+/ ({x} - 1) * ({x} - 1)",
        "{w} - {x} - 1",
        "%: 1 + 2 * {x}",
        ">./ {w} * {x}",
        "<./ {w} + {x}",
        "*/ 1 + {x}",
        "+/ ({x} > 1) * {x}",
        "+/ ({x} <: 1) * {x}",
        "+/ (2 | {x}) * {x}",
        "+/ ({w} % {x}) + 1",
        "+/ (% {x}) + 1",
        "+/ (| {x}) * -: {x}",
        "+/ (* {x}) + >: {x}",
    ];

    #[test]
    fn every_chain_generates_valid_f32_wgsl() {
        for src in CHAINS {
            let k = kernel(src);
            let splat = vec![false; 4];
            let s = wgsl(&k, &splat, Precision::F32).unwrap_or_else(|e| panic!("{src}: {e}"));
            validate(&s, Precision::F32);
        }
    }

    #[test]
    fn every_chain_generates_valid_f64_wgsl() {
        for src in CHAINS {
            let k = kernel(src);
            let splat = vec![false; 4];
            match wgsl(&k, &splat, Precision::F64) {
                Ok(s) => validate(&s, Precision::F64),
                // The exponential has no f64 form; that is the only thing
                // the generator is allowed to turn away here.
                Err(op) => assert_eq!(op, "^", "{src}"),
            }
        }
    }

    #[test]
    fn the_exponential_declines_in_f64_and_runs_in_f32() {
        let k = kernel("+/ ^ {x}");
        assert_eq!(wgsl(&k, &[false], Precision::F64), Err("^"));
        let s = wgsl(&k, &[false], Precision::F32).expect("f32");
        validate(&s, Precision::F32);
        assert!(s.contains("exp("));
    }

    #[test]
    fn a_scalar_input_is_read_at_zero() {
        let k = kernel("+/ 2 * {x}");
        let s = wgsl(&k, &[true, false], Precision::F32).expect("wgsl");
        assert!(s.contains("jay_in0[0u]"), "{s}");
        assert!(s.contains("jay_in1[i]"), "{s}");
    }

    #[test]
    fn the_grid_never_outnumbers_the_elements() {
        for n in [1 << 19, 1 << 20, 1 << 24, 3_000_000] {
            assert!(groups_for(n) * WORKGROUP <= n, "{n}");
            assert!(groups_for(n) >= 1);
        }
    }

    #[test]
    fn elements_survive_the_round_trip() {
        let v = vec![1.0, -2.5, 1e300, 0.0];
        let bytes = |p: Precision| byte_iter(&v, p).collect::<Vec<u8>>();
        let b = bytes(Precision::F64);
        assert_eq!(from_bytes(&b, Precision::F64, v.len()), v);
        let b = bytes(Precision::F32);
        let back = from_bytes(&b, Precision::F32, v.len());
        assert_eq!(back[0], 1.0);
        assert_eq!(back[1], -2.5);
        assert!(back[2].is_infinite());
    }

    #[test]
    fn precision_names_read_back() {
        for p in [Precision::F64, Precision::F32] {
            assert_eq!(Precision::from_name(p.name()), Some(p));
        }
        assert_eq!(Precision::from_name("f16"), None);
    }
}
