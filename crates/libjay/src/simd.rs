//! Runtime dispatch over CPU feature levels.
//!
//! One artifact per platform, several compilations of every hot loop. Each
//! loop covered here is compiled once for each level below, by the
//! `multiversion` crate, which attaches the level's `target_feature` set to
//! a clone of the same generic Rust source; the compiler's autovectoriser
//! is what turns each clone into vector code. Nothing in libjay writes SIMD
//! intrinsics, and nothing may start: vectorisation is the backend's job.
//!
//! Which clone runs is decided once per process. `LIBJAY_CPU_LEVEL` pins it
//! — `baseline`, `v2`, `v3`, `v4`, or `native` for what the machine offers
//! — and a level the CPU cannot run is clamped down to the one it can, so a
//! pinned level is always a level that actually executes.
//!
//! A covered loop may still decline the vector clone: where the loop that
//! would widen is only a few elements long, entering a vector body costs
//! more than the width gives back, so the loop takes the baseline
//! compilation whatever the machine can run. `verb::VECTOR_COLUMNS` is that
//! rule and carries the measurement behind it.
//!
//! An elementwise pass computes the same values whatever clone runs it:
//! vectorising `dst[i] = a[i] + b[i]` reorders nothing. A reduction is
//! another matter — the levels agree there only to the tolerance the float
//! contract already allows for regrouping an associative fold (§5.9).

use std::sync::atomic::{AtomicU8, Ordering};

/// A set of CPU features the hot loops are compiled for.
///
/// The names are the x86-64 microarchitecture levels: `V2` is SSE4.2 and
/// its neighbours, `V3` adds AVX2 and FMA, `V4` adds the AVX-512 subsets
/// the level names (`f`, `bw`, `cd`, `dq`, `vl`). On aarch64 the ladder has
/// two rungs — `Baseline` and `V3`, which stands for NEON. NEON is also in
/// the aarch64 baseline, so those two compile to the same code and exist to
/// keep the dispatch the same shape on both architectures; `V4` is x86-64
/// only and is never detected elsewhere.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Baseline = 0,
    V2 = 1,
    V3 = 2,
    V4 = 3,
}

impl Level {
    /// The name `LIBJAY_CPU_LEVEL` takes for this level.
    pub fn name(self) -> &'static str {
        match self {
            Level::Baseline => "baseline",
            Level::V2 => "v2",
            Level::V3 => "v3",
            Level::V4 => "v4",
        }
    }

    fn from_u8(v: u8) -> Level {
        match v {
            1 => Level::V2,
            2 => Level::V3,
            3 => Level::V4,
            _ => Level::Baseline,
        }
    }

    /// The level a `LIBJAY_CPU_LEVEL` value names, or None for one that
    /// names none. `native` and `auto` name whatever the machine offers.
    fn from_name(s: &str) -> Option<Level> {
        match s.trim().to_ascii_lowercase().as_str() {
            "baseline" | "v1" | "none" => Some(Level::Baseline),
            "v2" => Some(Level::V2),
            "v3" => Some(Level::V3),
            "v4" => Some(Level::V4),
            "native" | "auto" | "max" => Some(detected()),
            _ => None,
        }
    }
}

/// The highest level this machine can run.
///
/// The levels nest, so each test includes the one below it: a machine that
/// reports V4 is a machine every clone below V4 also runs, which is what
/// lets [`available`] hand a test the whole ladder up to this point.
#[cfg(target_arch = "x86_64")]
pub fn detected() -> Level {
    let v2 = is_x86_feature_detected!("sse4.2") && is_x86_feature_detected!("popcnt");
    let v3 = v2
        && is_x86_feature_detected!("avx2")
        && is_x86_feature_detected!("fma")
        && is_x86_feature_detected!("bmi1")
        && is_x86_feature_detected!("bmi2")
        && is_x86_feature_detected!("f16c")
        && is_x86_feature_detected!("lzcnt");
    let v4 = v3
        && is_x86_feature_detected!("avx512f")
        && is_x86_feature_detected!("avx512bw")
        && is_x86_feature_detected!("avx512cd")
        && is_x86_feature_detected!("avx512dq")
        && is_x86_feature_detected!("avx512vl");
    if v4 {
        Level::V4
    } else if v3 {
        Level::V3
    } else if v2 {
        Level::V2
    } else {
        Level::Baseline
    }
}

/// The highest level this machine can run. NEON is in every aarch64
/// baseline, so the top rung is always reachable.
#[cfg(target_arch = "aarch64")]
pub fn detected() -> Level {
    if std::arch::is_aarch64_feature_detected!("neon") { Level::V3 } else { Level::Baseline }
}

/// The highest level this machine can run. An architecture with no levels
/// of its own runs the one compilation there is.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub fn detected() -> Level {
    Level::Baseline
}

/// Every level this machine can run, lowest first. A test that wants to
/// compare the levels against each other iterates this; asking for one
/// that is not in it would only get the highest one that is.
pub fn available() -> Vec<Level> {
    let top = detected();
    [Level::Baseline, Level::V2, Level::V3, Level::V4].into_iter().filter(|&l| l <= top).collect()
}

/// Not yet resolved: no level has this value.
const UNSET: u8 = u8::MAX;

static LEVEL: AtomicU8 = AtomicU8::new(UNSET);

/// `LIBJAY_CPU_LEVEL`, clamped to what the machine can run; the machine's
/// own level when the variable is unset or names nothing.
fn from_env() -> Level {
    let asked = std::env::var("LIBJAY_CPU_LEVEL").ok().and_then(|v| Level::from_name(&v));
    match asked {
        Some(l) => l.min(detected()),
        None => detected(),
    }
}

/// The level the hot loops dispatch to. Resolved once, then an atomic load.
#[inline]
pub fn level() -> Level {
    let v = LEVEL.load(Ordering::Relaxed);
    if v != UNSET {
        return Level::from_u8(v);
    }
    let l = from_env();
    LEVEL.store(l as u8, Ordering::Relaxed);
    l
}

/// Dispatch to `l` from here on, clamped to what the machine can run.
/// Returns the level that took effect.
///
/// This is the same knob `LIBJAY_CPU_LEVEL` turns, for a caller that wants
/// to turn it more than once — a test comparing the levels against each
/// other, or a benchmark. Values already computed are not affected.
pub fn set_level(l: Level) -> Level {
    let l = l.min(detected());
    LEVEL.store(l as u8, Ordering::Relaxed);
    l
}

/// Compile a hot loop once per CPU feature level and dispatch on [`level`].
///
/// The loop itself is written once, as an ordinary function; this generates
/// the clones and the dispatch:
///
/// ```ignore
/// #[inline(always)]
/// fn add_body(a: &[f64], b: &[f64], dst: &mut [f64]) { … }
///
/// multiversioned! {
///     /// The doc comment the dispatching function carries.
///     fn add(a: &[f64], b: &[f64], dst: &mut [f64]) -> () = add_body;
/// }
/// ```
///
/// Generic loops name their parameters, with bounds inline, in brackets —
/// `fn fold[T: Copy](…)` — since angle brackets are not a group a macro can
/// match. The body must be `#[inline(always)]`: it is what carries the
/// arithmetic into each clone, where the clone's features apply to it.
macro_rules! multiversioned {
    (
        $(#[$attr:meta])*
        fn $name:ident $([$($gen:tt)*])? ($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty = $body:ident;
    ) => {
        mod $name {
            // The clones only forward, so they inherit the shape of the
            // loop they wrap, argument count and all.
            #![allow(clippy::too_many_arguments)]

            #[allow(unused_imports)]
            use super::*;

            pub(super) fn baseline $(<$($gen)*>)? ($($arg: $ty),*) -> $ret {
                $body($($arg),*)
            }

            #[::multiversion::multiversion(targets("x86_64+sse3+ssse3+sse4.1+sse4.2+popcnt"))]
            pub(super) fn v2 $(<$($gen)*>)? ($($arg: $ty),*) -> $ret {
                $body($($arg),*)
            }

            #[::multiversion::multiversion(targets(
                "x86_64+avx+avx2+fma+bmi1+bmi2+lzcnt+f16c",
                "aarch64+neon",
            ))]
            pub(super) fn v3 $(<$($gen)*>)? ($($arg: $ty),*) -> $ret {
                $body($($arg),*)
            }

            // x86-64 only: no other architecture has a rung above v3, and
            // on those this clone is the unversioned body, never dispatched
            // to because `detected` cannot return V4 there.
            #[::multiversion::multiversion(targets(
                "x86_64+avx512f+avx512bw+avx512cd+avx512dq+avx512vl",
            ))]
            pub(super) fn v4 $(<$($gen)*>)? ($($arg: $ty),*) -> $ret {
                $body($($arg),*)
            }
        }

        $(#[$attr])*
        #[inline]
        fn $name $(<$($gen)*>)? ($($arg: $ty),*) -> $ret {
            match $crate::simd::level() {
                $crate::simd::Level::Baseline => $name::baseline($($arg),*),
                $crate::simd::Level::V2 => $name::v2($($arg),*),
                $crate::simd::Level::V3 => $name::v3($($arg),*),
                $crate::simd::Level::V4 => $name::v4($($arg),*),
            }
        }
    };
}

pub(crate) use multiversioned;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_machines_own_level_is_available() {
        let all = available();
        assert_eq!(all.last().copied(), Some(detected()));
        assert!(all.contains(&Level::Baseline));
    }

    #[test]
    fn a_level_the_machine_lacks_clamps_to_one_it_has() {
        assert!(set_level(Level::V4) <= detected());
        assert_eq!(set_level(Level::Baseline), Level::Baseline);
        assert_eq!(level(), Level::Baseline);
        set_level(detected());
    }

    #[test]
    fn every_level_has_a_name_and_reads_back() {
        for l in [Level::Baseline, Level::V2, Level::V3, Level::V4] {
            assert_eq!(Level::from_name(l.name()), Some(l));
        }
        assert_eq!(Level::from_name("nonsense"), None);
        assert_eq!(Level::from_name("native"), Some(detected()));
    }
}
