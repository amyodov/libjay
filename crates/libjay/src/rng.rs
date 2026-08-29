//! The random source behind `?` and `?.` (J) and `?` (APL).
//!
//! The generator is MT19937, the published Mersenne Twister, with libjay's
//! own seeding: neither reference publishes the stream it draws from, so
//! the numbers libjay rolls are its own and the differential suites leave
//! both spellings alone. What is reproduced is the *behaviour* the
//! references define — `?.` restarts from a fixed seed on every invocation,
//! so the same sentence always answers the same way, while `?` is seeded
//! once per process and moves on.

use std::sync::Mutex;

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER: u32 = 0x8000_0000;
const LOWER: u32 = 0x7fff_ffff;

/// The seed `?.` restarts from. Any constant would do; this one is the
/// Mersenne Twister reference implementation's own default.
const FIXED_SEED: u32 = 5489;

pub struct Mt19937 {
    state: [u32; N],
    at: usize,
}

impl Mt19937 {
    pub fn new(seed: u32) -> Mt19937 {
        let mut state = [0u32; N];
        state[0] = seed;
        for i in 1..N {
            let prev = state[i - 1];
            state[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        Mt19937 { state, at: N }
    }

    fn twist(&mut self) {
        for i in 0..N {
            let y = (self.state[i] & UPPER) | (self.state[(i + 1) % N] & LOWER);
            let mut next = self.state[(i + M) % N] ^ (y >> 1);
            if y & 1 != 0 {
                next ^= MATRIX_A;
            }
            self.state[i] = next;
        }
        self.at = 0;
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.at >= N {
            self.twist();
        }
        let mut y = self.state[self.at];
        self.at += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    fn next_u64(&mut self) -> u64 {
        (u64::from(self.next_u32()) << 32) | u64::from(self.next_u32())
    }

    /// A uniform integer in `0 .. bound`. Rejection sampling, so no value
    /// is more likely than any other.
    pub fn below(&mut self, bound: u64) -> u64 {
        debug_assert!(bound > 0);
        if bound == 1 {
            return 0;
        }
        // The largest multiple of `bound` that fits: everything at or above
        // it is drawn again.
        let limit = u64::MAX - (u64::MAX % bound) - (bound - 1);
        loop {
            let v = self.next_u64();
            if v <= limit {
                return v % bound;
            }
        }
    }

    /// A uniform double in `[0, 1)`, with 53 bits of resolution.
    pub fn unit(&mut self) -> f64 {
        let a = u64::from(self.next_u32() >> 5);
        let b = u64::from(self.next_u32() >> 6);
        ((a << 26) | b) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// `x` distinct values from `0 .. bound`, in draw order. Partial
    /// Fisher–Yates over the values actually touched, so the cost follows
    /// how many are asked for rather than how large the range is.
    pub fn deal(&mut self, x: usize, bound: u64) -> Vec<i64> {
        use std::collections::HashMap;
        let mut moved: HashMap<u64, u64> = HashMap::with_capacity(x);
        let mut out = Vec::with_capacity(x);
        for i in 0..x as u64 {
            let j = i + self.below(bound - i);
            let at_j = moved.get(&j).copied().unwrap_or(j);
            let at_i = moved.get(&i).copied().unwrap_or(i);
            moved.insert(j, at_i);
            out.push(at_j as i64);
        }
        out
    }
}

/// The generator `?` draws from: one per process, seeded once.
fn shared() -> &'static Mutex<Mt19937> {
    static SHARED: std::sync::OnceLock<Mutex<Mt19937>> = std::sync::OnceLock::new();
    SHARED.get_or_init(|| Mutex::new(Mt19937::new(os_seed())))
}

/// A seed for the process. The sandbox reaches no file and no device, so
/// this is the clock and one address the allocator chose, mixed.
fn os_seed() -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let here = Box::new(0u8);
    let addr = std::ptr::from_ref::<u8>(&*here) as u64;
    let mixed = nanos ^ addr.rotate_left(21) ^ (nanos >> 32);
    (mixed as u32) ^ ((mixed >> 32) as u32)
}

thread_local! {
    /// The generator a program asked for by name, if one did. APL's `⎕RL`
    /// sets it; it lives for the run that set it and no longer, so one
    /// program's seed never reaches the next.
    static LINK: std::cell::RefCell<Option<Mt19937>> = const { std::cell::RefCell::new(None) };
}

/// Start a random stream at `seed`, for this thread, until [`clear_link`].
///
/// The stream is libjay's own: the seed reproduces libjay's sequence, and
/// no other implementation's.
pub fn set_link(seed: i64) {
    let g = Mt19937::new(seed as u32);
    LINK.with(|l| *l.borrow_mut() = Some(g));
}

/// Forget any stream a program started, so the next run begins where an
/// unseeded run begins.
pub fn clear_link() {
    LINK.with(|l| *l.borrow_mut() = None);
}

/// Clears the thread's random link when it goes out of scope, whatever
/// ended the run that installed it.
pub struct LinkGuard;

impl Drop for LinkGuard {
    fn drop(&mut self) {
        clear_link();
    }
}

/// Run `f` over the generator this spelling draws from: a fresh one at the
/// fixed seed for `?.`, the one a `⎕RL` started if there is one, and
/// otherwise the process's own.
pub fn with<R>(fixed: bool, f: impl FnOnce(&mut Mt19937) -> R) -> R {
    if fixed {
        return f(&mut Mt19937::new(FIXED_SEED));
    }
    let linked = LINK.with(|l| l.borrow().is_some());
    if linked {
        return LINK.with(|l| {
            let mut slot = l.borrow_mut();
            f(slot.as_mut().expect("the link was there a moment ago"))
        });
    }
    // A poisoned lock would mean a panic inside a roll; the state is a
    // plain array, so carrying on with it is safe.
    let mut g = shared().lock().unwrap_or_else(|e| e.into_inner());
    f(&mut g)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published test vector: MT19937 seeded with 5489 starts here.
    #[test]
    fn the_reference_seed_gives_the_published_stream() {
        let mut g = Mt19937::new(5489);
        let got: Vec<u32> = (0..5).map(|_| g.next_u32()).collect();
        assert_eq!(got, vec![3_499_211_612, 581_869_302, 3_890_346_734, 3_586_334_585, 545_404_204]);
    }

    #[test]
    fn a_deal_draws_every_value_once() {
        let mut g = Mt19937::new(1);
        let mut v = g.deal(10, 10);
        v.sort_unstable();
        assert_eq!(v, (0..10).collect::<Vec<i64>>());
    }

    #[test]
    fn bounded_draws_stay_in_range() {
        let mut g = Mt19937::new(7);
        for _ in 0..1000 {
            assert!(g.below(6) < 6);
        }
    }
}
