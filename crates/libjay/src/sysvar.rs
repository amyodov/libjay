//! APL's system variables: the `⎕`-names a running program may read, and
//! the two it may also set.
//!
//! Most of libjay's `⎕`-names are constants the compiler folds into the
//! program before it runs — `⎕A`, `⎕AV`, `⎕IO`. The names here are
//! different: a program can change them part-way through, so they live in
//! the run's own name table under their full spelling (`⎕PP`, `⎕RL`), are
//! read like any other name, and go through [`apply`] when they are
//! assigned so that whatever they control follows.
//!
//! `⎕` is not a character either language allows in a name a user writes,
//! so a system variable can never collide with one.

use crate::array::Array;
use crate::error::{Error, ErrorKind, Result, Span};
use crate::fmt::{DEFAULT_PRECISION, MAX_PRECISION, MIN_PRECISION};
use crate::verb::{Ctx, Env};

/// Print precision: how many significant digits a displayed float keeps.
pub const PP: &str = "⎕PP";
/// Random link: the seed libjay's random stream was last set from.
pub const RL: &str = "⎕RL";

/// The link's starting value. It is the classic APL seed, and libjay
/// agrees on the number even though the stream it starts is its own.
pub const RL_DEFAULT: i64 = 16807;

/// Whether a name is one of the system variables a program may set, by
/// its full spelling.
pub fn is_settable(name: &str) -> bool {
    matches!(name, PP | RL)
}

/// [`is_settable`], for a name the lexer has read without its `⎕` and
/// upper-cased.
pub fn is_settable_bare(bare: &str) -> bool {
    matches!(bare, "PP" | "RL")
}

/// The upper half of the atomic vector: the 128 characters `⎕AV` holds
/// above the ASCII ones, in their order.
///
/// The standard leaves the atomic vector to the implementation, so there
/// is no language fact to derive it from. This is the order libjay
/// measured from its GNU APL oracle and adopted, so that `⎕AV⍳c` and
/// `⎕AV[i]` answer the same on both.
#[rustfmt::skip]
const AV_UPPER: [u32; 128] = [
    0x00A5, 0x20AC, 0x21C4, 0x2227, 0x223C, 0x00AB, 0x22C6, 0x22F8,
    0x2338, 0x00B2, 0x233C, 0x03BC, 0x2341, 0x00BB, 0x2363, 0x2345,
    0x2395, 0x235E, 0x2339, 0x2346, 0x2364, 0x2347, 0x2348, 0x234A,
    0x22A4, 0x03BB, 0x234D, 0x234F, 0x00A3, 0x22A5, 0x2376, 0x2336,
    0x2350, 0x2351, 0x03C7, 0x2262, 0x2356, 0x2357, 0x2358, 0x235A,
    0x235B, 0x2308, 0x235C, 0x2362, 0x222A, 0x2368, 0x2355, 0x234E,
    0x236C, 0x236A, 0x2223, 0x2502, 0x2524, 0x235F, 0x2206, 0x2207,
    0x2192, 0x2563, 0x2551, 0x2557, 0x255D, 0x2190, 0x230A, 0x2510,
    0x2514, 0x2534, 0x252C, 0x251C, 0x2500, 0x253C, 0x2191, 0x2193,
    0x2554, 0x255A, 0x2569, 0x2566, 0x2560, 0x2550, 0x256C, 0x2261,
    0x2378, 0x2377, 0x2235, 0x2337, 0x2342, 0x233B, 0x22A2, 0x22A3,
    0x25CA, 0x2518, 0x250C, 0x2588, 0x2584, 0x258C, 0x2590, 0x2580,
    0x237A, 0x2379, 0x2282, 0x2283, 0x235D, 0x2372, 0x2374, 0x2371,
    0x233D, 0x2296, 0x25CB, 0x2228, 0x2373, 0x2349, 0x03F5, 0x2229,
    0x233F, 0x2340, 0x2265, 0x2264, 0x2260, 0x00D7, 0x00F7, 0x2359,
    0x2218, 0x2375, 0x236B, 0x234B, 0x2352, 0x00AF, 0x00A8, 0x00A0,
];

/// `⎕AV`: the atomic vector, 256 characters. The first 128 are the code
/// points 0 to 127 in order; the rest are the repertoire above them.
pub fn atomic_vector() -> Vec<char> {
    (0u32..128)
        .chain(AV_UPPER)
        .map(|c| char::from_u32(c).expect("the atomic vector holds real characters"))
        .collect()
}

/// The `⎕`-names that are system VARIABLES: the ones `⎕NC` calls a system
/// name. A system FUNCTION — `⎕FX`, `⎕CC`, `⎕NC` itself — is not one, and
/// neither is a name libjay has never heard of; both are "not a name".
///
/// The name arrives without its `⎕`, in upper case.
pub fn is_system_variable(bare: &str) -> bool {
    matches!(
        bare,
        // Answered as values.
        "A" | "AV" | "CT" | "D" | "EM" | "ET" | "IO" | "LX" | "PP" | "RL"
        // Named as gaps, and system variables all the same.
        | "PW" | "SYL"
        // Closed by the sandbox, and system variables all the same.
        | "AI" | "LC" | "TC" | "TS" | "TZ" | "WA"
    )
}

/// Give an APL run its system variables. Called once, before the first
/// sentence; J has no `⎕`-names and is not given any.
pub fn seed(env: &mut Env) {
    env.set_global(PP.to_string(), Array::scalar_i64(i64::from(DEFAULT_PRECISION)));
    env.set_global(RL.to_string(), Array::scalar_i64(RL_DEFAULT));
}

/// Check an assignment to a system variable and make what it controls
/// follow. The value is stored by the ordinary assignment that calls this.
///
/// A name that is not a system variable is left alone: a program cannot
/// invent one, because the lexer answers every other `⎕`-name itself.
///
/// Kept out of the evaluator's own frame: `eval_node` recurses, so every
/// local an arm of it holds is stack paid for at each level, and the
/// recursion limit is set by exactly that cost.
#[inline(never)]
pub fn apply(name: &str, value: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<()> {
    match name {
        PP => {
            let n = one_whole(value, name, span)?;
            if n < i64::from(MIN_PRECISION) {
                return Err(Error::domain(
                    format!(
                        "{PP} is a print precision: at least {MIN_PRECISION} \
                         significant digit, and {n} is fewer"
                    ),
                    span,
                ));
            }
            ctx.cfg.fmt.precision = n.min(i64::from(MAX_PRECISION)) as u8;
            Ok(())
        }
        RL => {
            let n = one_whole(value, name, span)?;
            crate::rng::set_link(n);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The one whole number a system variable takes, with this language's
/// spelling of what went wrong.
fn one_whole(value: &Array, name: &str, span: Span) -> Result<i64> {
    if value.count() != 1 {
        return Err(Error::new(
            ErrorKind::Length,
            format!("{name} takes one number, and was given {}", value.count()),
            Some(span),
        ));
    }
    value
        .to_i64_vec()
        .and_then(|v| v.first().copied())
        .ok_or_else(|| Error::domain(format!("{name} takes a whole number"), span))
}
