//! Deterministic pseudo-random expressions.
//!
//! Breadth the fixed corpora do not have: sentences drawn from the
//! implemented surface by a seeded generator, so the same seed always
//! yields the same list. What it produces is appended to
//! `corpus/<lang>/generated.txt` as ordinary corpus lines — from there on
//! they are inputs like any other, and the generator plays no part in a
//! test run.

use libjay_testkit::Lang;

/// The seed the checked-in generated corpora were drawn with.
pub const DEFAULT_SEED: u64 = 0x9E3779B97F4A7C15;

/// Rounds behind the checked-in generated corpora. One round emits several
/// expressions: eight for J, twelve for APL.
pub const DEFAULT_ROUNDS: usize = 300;
pub const DEFAULT_ROUNDS_APL: usize = 25;

/// A small xorshift, so a run is reproducible without any clock access.
fn rng(seed: u64) -> impl FnMut() -> u64 {
    let mut state = seed;
    move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    }
}

pub fn generate(lang: Lang, rounds: usize, seed: u64) -> Vec<String> {
    match lang {
        Lang::J => generate_j(rounds, seed),
        Lang::Apl => generate_apl(rounds, seed),
    }
}

fn generate_j(rounds: usize, seed: u64) -> Vec<String> {
    let mut rng = rng(seed);
    // Verbs safe to fold over a vector of small integers.
    let dyads = ["+", "-", "*", "<.", ">.", "|", "=", "<", ">", "*.", "+."];
    // Verbs additionally safe with a scalar left argument. `{` is excluded:
    // its left argument has to index the right one, so it is generated on
    // its own below.
    let struct_dyads = ["|.", ",", "-:", "e.", ",:", "#.", "#:", "!"];
    let monads = [
        "-", "*", "|", "<.", ">.", "+/", ">./", "<./", "#", "$", ",", "|:", "|.", "~.", "{:", "}:",
        "<:", ">:", "+:", "*:", "-.", "-:", "/:", "\\:", ",:", "#.", "#:", "\":", "!",
    ];
    // Verbs whose reduction folds a window or a prefix without leaving the
    // reals, whatever small integers it is given.
    let folds = ["+", "*", "<.", ">."];
    let mut exprs = Vec::new();
    for _ in 0..rounds {
        let n = 1 + (rng() % 5) as usize;
        let vec: Vec<String> = (0..n)
            .map(|_| {
                let v = (rng() % 19) as i64 - 9;
                if v < 0 { format!("_{}", -v) } else { v.to_string() }
            })
            .collect();
        let noun = match rng() % 3 {
            0 => vec.join(" "),
            1 => format!("i. {} {}", 1 + rng() % 3, 1 + rng() % 4),
            _ => format!("{} 4 $ {}", 1 + rng() % 3, vec.join(" ")),
        };
        // A noun with at least five items, so a window of 1 to 4 always has
        // both the full and the short cases available.
        let long = match rng() % 3 {
            0 => format!("i. {}", 5 + rng() % 4),
            1 => format!("{} 3 $ i. 12", 5 + rng() % 3),
            _ => format!("{} 4 $ {}", 5 + rng() % 4, vec.join(" ")),
        };
        let fold = |r: u64| folds[(r % folds.len() as u64) as usize];
        exprs.push(format!("{} +/\\ {long}", 1 + rng() % 4));
        exprs.push(format!("{}/\\ {long}", fold(rng())));
        exprs.push(format!("{}/\\. {long}", fold(rng())));
        exprs.push(format!("{} {}/\\ {long}", 1 + rng() % 4, fold(rng())));
        exprs.push(format!("_{} {}/\\ {long}", 1 + rng() % 4, fold(rng())));
        exprs.push(format!("{}~ {noun}", dyads[(rng() % dyads.len() as u64) as usize]));
        // The table: every cell of the left argument against every cell of
        // the right one.
        exprs.push(format!("{} {}/ {noun}", vec.join(" "), fold(rng())));
        let expr = match rng() % 4 {
            0 => format!("{} {}", monads[(rng() % monads.len() as u64) as usize], noun),
            1 => {
                let atom = (rng() % 7) as i64 - 3;
                let atom = if atom < 0 { format!("_{}", -atom) } else { atom.to_string() };
                let all = dyads.len() + struct_dyads.len();
                let k = (rng() % all as u64) as usize;
                let verb = if k < dyads.len() { dyads[k] } else { struct_dyads[k - dyads.len()] };
                format!("{atom} {verb} {noun}")
            }
            // Every generated noun has at least one item, so index 0 is
            // always in range.
            2 => format!("0 {{ {noun}"),
            _ => format!("{}/ {}", dyads[(rng() % dyads.len() as u64) as usize], noun),
        };
        exprs.push(expr);
    }
    exprs
}

/// No primitive whose two dialects are known to part ways appears here:
/// what diverges on purpose is in `corpus/apl/divergences.txt`.
fn generate_apl(rounds: usize, seed: u64) -> Vec<String> {
    let mut rng = rng(seed);
    // Scalar dyads, safe on any pair of small integers. `÷` is left out: its
    // zero divisor is a divergence of its own.
    let dyads = ["+", "-", "×", "⌈", "⌊", "|", "=", "≠", "<", "≤", ">", "≥"];
    // Monads safe on any small-integer array of rank 1 or 2.
    let monads = ["-", "+", "×", "|", "⌈", "⌊", "⍴", ",", "⍉", "⌽", "⊖", "≢", "⍕"];
    // Verbs that fold a whole axis without leaving the integers.
    let folds = ["+", "×", "⌈", "⌊"];
    let mut exprs = Vec::new();
    for _ in 0..rounds {
        let n = 1 + (rng() % 5) as usize;
        let vec: Vec<String> = (0..n)
            .map(|_| {
                let v = (rng() % 19) as i64 - 9;
                if v < 0 { format!("¯{}", -v) } else { v.to_string() }
            })
            .collect();
        let vec = vec.join(" ");
        let noun = match rng() % 3 {
            0 => vec.clone(),
            1 => format!("{} {}⍴⍳{}", 1 + rng() % 3, 1 + rng() % 4, 1 + rng() % 12),
            _ => format!("{} 4⍴{vec}", 1 + rng() % 3),
        };
        // Scans only get arguments whose every axis is at least 2 long: a
        // length-1 axis is where GNU APL's scan loses the axis, which is
        // its bug and is pinned in the divergence corpus rather than
        // generated over and over here.
        let dense = match rng() % 2 {
            0 => format!("{} {}⍴⍳{}", 2 + rng() % 2, 2 + rng() % 3, 6 + rng() % 12),
            _ => format!("{}⍴{}", 2 + rng() % 4, vec),
        };
        let fold = |r: u64| folds[(r % folds.len() as u64) as usize];
        let dyad = |r: u64| dyads[(r % dyads.len() as u64) as usize];
        let monad = |r: u64| monads[(r % monads.len() as u64) as usize];
        exprs.push(format!("{}/{noun}", fold(rng())));
        exprs.push(format!("{}⌿{noun}", fold(rng())));
        exprs.push(format!("{}\\{dense}", fold(rng())));
        exprs.push(format!("{}⍀{dense}", fold(rng())));
        exprs.push(format!("{} {noun}", monad(rng())));
        let atom = (rng() % 7) as i64 - 3;
        let atom = if atom < 0 { format!("¯{}", -atom) } else { atom.to_string() };
        exprs.push(format!("{atom}{}{noun}", dyad(rng())));
        exprs.push(format!("{}⍨{noun}", dyad(rng())));
        exprs.push(format!("{vec}∘.{}{vec}", dyad(rng())));
        // A rotation amount is always legal, whatever the shape.
        exprs.push(format!("{atom}⌽{noun}"));
        exprs.push(format!("{atom}⊖{noun}"));
        // The vector is its own index domain, so `⍳` always has an answer.
        exprs.push(format!("{vec}⍳{atom}"));
        exprs.push(format!("⍴{noun}"));
    }
    exprs
}
