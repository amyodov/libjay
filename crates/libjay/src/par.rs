//! The thread pool and the chunked primitives the fast paths run on.
//!
//! Running in parallel is part of what a compiled expression is, not an
//! option the caller switches on, so this module is part of the runtime and
//! not of any frontend. Every primitive in here produces exactly what the
//! sequential code would, with one contracted exception: a reduction may
//! regroup an associative float fold, which reorders the rounding.
//!
//! The pool is libjay's own rather than rayon's global one, so an embedding
//! host that also uses rayon keeps its own pool. Callers that install a pool
//! themselves (tests, hosts) are respected: work started inside a rayon
//! worker stays in that worker's pool.

use std::sync::OnceLock;

use rayon::iter::{
    IndexedParallelIterator, IntoParallelIterator, IntoParallelRefIterator, ParallelIterator,
};
use rayon::slice::{ParallelSlice, ParallelSliceMut};
use rayon::{ThreadPool, ThreadPoolBuilder};

/// Least amount of elementwise work worth splitting across threads.
///
/// Measured on a 4-core/8-thread Kaby Lake: a two-buffer f64 pass breaks
/// even somewhere around 30k elements and is clearly ahead by 64k, which is
/// also where a 20M-element argument still gets 300+ chunks to balance.
pub const MIN_WORK: usize = 65_536;

/// An item this wide or wider makes a reduction parallel across the item's
/// own elements: each thread folds a contiguous range of columns down all
/// the items, which preserves the fold order for any operation. Narrower
/// items do not give a thread enough contiguous elements to be worth it and
/// go the chunked-items way instead.
pub const WIDE_ITEM: usize = 256;

/// Threads the pool is built with: `LIBJAY_THREADS` when set to a positive
/// number, otherwise the machine's available parallelism.
fn configured_threads() -> usize {
    static N: OnceLock<usize> = OnceLock::new();
    *N.get_or_init(|| {
        match std::env::var("LIBJAY_THREADS").ok().and_then(|v| v.trim().parse::<usize>().ok()) {
            Some(n) if n > 0 => n,
            _ => std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        }
    })
}

fn pool() -> &'static ThreadPool {
    static POOL: OnceLock<ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        ThreadPoolBuilder::new()
            .num_threads(configured_threads())
            .thread_name(|i| format!("libjay-{i}"))
            .build()
            .expect("building the libjay thread pool")
    })
}

/// Threads the current work can actually spread over: the pool we are
/// already inside, or the one this module would use.
fn parallelism() -> usize {
    match rayon::current_thread_index() {
        Some(_) => rayon::current_num_threads(),
        None => configured_threads(),
    }
}

/// Run `f` on the libjay pool, or in place when already on a rayon worker
/// so that a pool the caller installed stays in force.
fn in_pool<R: Send>(f: impl FnOnce() -> R + Send) -> R {
    match rayon::current_thread_index() {
        Some(_) => f(),
        None => pool().install(f),
    }
}

/// Run `f` with a pool of exactly `threads` threads. Tests use it to take
/// the sequential path (`threads` of 1) as the reference for the parallel
/// one without touching the environment.
#[cfg(test)]
pub fn with_threads<R: Send>(threads: usize, f: impl FnOnce() -> R + Send) -> R {
    ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("building a thread pool")
        .install(f)
}

/// Is `n` element operations enough work to spread over threads?
pub fn worth_it(n: usize) -> bool {
    n >= MIN_WORK && parallelism() > 1
}

/// Elements per chunk for an elementwise pass over `n` of them: a few
/// chunks per thread, so work stealing can even out a ragged tail.
fn chunk_len(n: usize) -> usize {
    n.div_ceil(parallelism() * 4).max(4096)
}

/// How many chunks to split `n` units into when each unit costs more than
/// one operation and `work` operations are at stake overall. 1 means the
/// work is not worth splitting.
pub fn chunks(n: usize, work: usize) -> usize {
    if !worth_it(work) {
        return 1;
    }
    n.min(parallelism() * 4).max(1)
}

/// Fill `n` outputs from contiguous chunks. `f(start, chunk)` writes the
/// outputs `start .. start + chunk.len()` and returns false to report that
/// its chunk failed; the returned flag is the conjunction over all chunks,
/// so a failure anywhere is visible whatever the split was.
fn fill_chunks<U, F>(n: usize, chunk: usize, parallel: bool, f: F) -> (Vec<U>, bool)
where
    U: Copy + Default + Send,
    F: Fn(usize, &mut [U]) -> bool + Sync + Send,
{
    let mut out = vec![U::default(); n];
    let ok = if parallel {
        in_pool(|| {
            out.par_chunks_mut(chunk)
                .enumerate()
                .map(|(k, part)| f(k * chunk, part))
                .reduce(|| true, |a, b| a && b)
        })
    } else {
        f(0, &mut out)
    };
    (out, ok)
}


/// Fill `n` outputs that cost one operation each.
pub fn fill<U, F>(n: usize, f: F) -> (Vec<U>, bool)
where
    U: Copy + Default + Send,
    F: Fn(usize, &mut [U]) -> bool + Sync + Send,
{
    fill_chunks(n, chunk_len(n), worth_it(n), f)
}

/// Fill `n` outputs that each cost far more than one operation — a column
/// of a reduction, say. `work` is the total number of operations; the
/// output is split once per thread, since the chunks all cost the same and
/// each one wants as many contiguous elements as it can get.
pub fn fill_wide<U, F>(n: usize, work: usize, f: F) -> (Vec<U>, bool)
where
    U: Copy + Default + Send,
    F: Fn(usize, &mut [U]) -> bool + Sync + Send,
{
    let threads = parallelism();
    let parallel = worth_it(work) && n >= threads;
    fill_chunks(n, n.div_ceil(threads), parallel, f)
}

/// Fill `rows` rows of `width` outputs each, split by WHOLE rows: `f` is
/// handed the index of the first row of its block and that block's outputs.
/// A kernel that walks a row at a time — the matrix product's inner pass —
/// needs the split to fall on a row boundary, which the elementwise
/// splitters do not promise.
pub fn fill_rows<U, F>(rows: usize, width: usize, work: usize, f: F) -> Vec<U>
where
    U: Copy + Default + Send,
    F: Fn(usize, &mut [U]) + Sync + Send,
{
    let mut out = vec![U::default(); rows * width];
    let threads = parallelism();
    if width > 0 && rows >= threads && worth_it(work) {
        let per = rows.div_ceil(threads);
        in_pool(|| {
            out.par_chunks_mut(per * width)
                .enumerate()
                .for_each(|(k, part)| f(k * per, part));
        });
    } else if rows > 0 {
        f(0, &mut out);
    }
    out
}

/// Fill `n` outputs whose computation can fail.
///
/// Several chunks may fail at once and rayon reports one of them. Every
/// error raised on these paths is decided by the operation and the span
/// alone, never by which element hit it, so which one wins is not
/// observable.
pub fn try_fill<U, E, F>(n: usize, f: F) -> Result<Vec<U>, E>
where
    U: Copy + Default + Send,
    E: Send,
    F: Fn(usize, &mut [U]) -> Result<(), E> + Sync + Send,
{
    let mut out = vec![U::default(); n];
    if worth_it(n) {
        let chunk = chunk_len(n);
        in_pool(|| {
            out.par_chunks_mut(chunk)
                .enumerate()
                .try_for_each(|(k, part)| f(k * chunk, part))
        })?;
    } else {
        f(0, &mut out)?;
    }
    Ok(out)
}

/// Map a slice elementwise.
pub fn map<T, U, F>(src: &[T], f: F) -> Vec<U>
where
    T: Sync,
    U: Send,
    F: Fn(&T) -> U + Sync + Send,
{
    if worth_it(src.len()) {
        in_pool(|| src.par_iter().map(&f).collect())
    } else {
        src.iter().map(&f).collect()
    }
}

/// Map a slice elementwise, failing as a whole when any element fails.
pub fn try_map<T, U, F>(src: &[T], f: F) -> Option<Vec<U>>
where
    T: Copy + Sync,
    U: Copy + Default + Send,
    F: Fn(T) -> Option<U> + Sync + Send,
{
    let (out, ok) = fill(src.len(), |start, part| {
        for (k, slot) in part.iter_mut().enumerate() {
            match f(src[start + k]) {
                Some(v) => *slot = v,
                None => return false,
            }
        }
        true
    });
    ok.then_some(out)
}

/// True when any element satisfies `f`.
///
/// Every element is read: no short circuit. That is what makes the scan one
/// branch-free pass the compiler can widen and the pool can split, and the
/// answer is the same either way. The callers are the checks that ask
/// whether a whole buffer will leave the reals, where the usual answer is
/// no and the whole buffer is read regardless.
pub fn any<T, F>(v: &[T], f: F) -> bool
where
    T: Sync,
    F: Fn(&T) -> bool + Sync + Send,
{
    let scan = |part: &[T]| part.iter().fold(false, |a, x| a | f(x));
    if worth_it(v.len()) {
        in_pool(|| v.par_chunks(chunk_len(v.len())).map(scan).reduce(|| false, |a, b| a | b))
    } else {
        scan(v)
    }
}

/// Map `0 .. n` in parallel, in order. The caller has already decided that
/// the work is worth splitting and that `f` is safe to run off the main
/// thread.
pub fn map_indexed<U, F>(n: usize, f: F) -> Vec<U>
where
    U: Send,
    F: Fn(usize) -> U + Sync + Send,
{
    in_pool(|| (0..n).into_par_iter().map(&f).collect())
}

/// Fold `v` right to left in chunks combined right to left, each chunk
/// folded by `seq`. `step` must be associative: the regrouping is the whole
/// point, and it is what lets `seq` regroup inside its own chunk too. None
/// when a step left its type (integer overflow) anywhere.
///
/// The element type read and the type accumulated are separate, so a fold
/// that promotes as it reads — a boolean buffer summed as integers — needs
/// no widened copy of its argument.
pub fn try_fold_chunks<S, T, C, F>(v: &[S], seq: C, step: F) -> Option<T>
where
    S: Copy + Send + Sync,
    T: Copy + Send + Sync,
    C: Fn(&[S]) -> Option<T> + Sync + Send,
    F: Fn(T, T) -> Option<T> + Sync + Send,
{
    if !worth_it(v.len()) {
        return seq(v);
    }
    let chunk = chunk_len(v.len());
    let parts: Vec<Option<T>> =
        in_pool(|| v.par_chunks(chunk).map(&seq).collect::<Vec<Option<T>>>());
    let parts: Option<Vec<T>> = parts.into_iter().collect();
    let parts = parts?;
    let mut acc = parts[parts.len() - 1];
    for &x in parts[..parts.len() - 1].iter().rev() {
        acc = step(x, acc)?;
    }
    Some(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_pass_stays_on_one_thread() {
        assert!(!worth_it(MIN_WORK - 1));
    }

    #[test]
    fn one_thread_never_splits() {
        assert!(with_threads(1, || !worth_it(MIN_WORK * 16)));
        assert!(with_threads(4, || worth_it(MIN_WORK * 16)));
    }

    #[test]
    fn a_split_fill_writes_every_element_once() {
        let n = MIN_WORK * 4;
        let run = |threads: usize| {
            with_threads(threads, || {
                fill(n, |start, part: &mut [i64]| {
                    for (k, slot) in part.iter_mut().enumerate() {
                        *slot = (start + k) as i64;
                    }
                    true
                })
            })
        };
        let (a, ok_a) = run(1);
        let (b, ok_b) = run(4);
        assert!(ok_a && ok_b);
        assert_eq!(a, b);
        assert_eq!(b[n - 1], (n - 1) as i64);
    }

    #[test]
    fn a_failure_in_one_chunk_fails_the_whole_fill() {
        let n = MIN_WORK * 4;
        let (_, ok) = with_threads(4, || {
            fill(n, |start, part: &mut [i64]| {
                // Only the chunk holding element 0 reports failure.
                !(start..start + part.len()).contains(&0)
            })
        });
        assert!(!ok);
    }
}
