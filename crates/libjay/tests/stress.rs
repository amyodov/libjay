//! Usage stress: what repeated, concurrent and failing use does to the engine.
//!
//! Every other battery asks what a sentence means. This one asks the same
//! question thousands of times, from several threads at once, with the pool
//! sized differently, and with refusals interleaved, and holds every answer
//! to being the answer the first cycle gave. The programs are the unit of
//! work — pipelines rather than single primitives, so one cycle touches the
//! parser, the fusion pass, the allocator and the reductions — and they end
//! in integers, so "the same answer" means exactly the same answer whatever
//! order a reduction was associated in.
//!
//! The one measurement here that is not an answer is resident memory. It is
//! compared as a RATIO against this process's own baseline rather than as a
//! megabyte figure: a byte count is a property of the machine and the
//! allocator, a ratio is a property of the code. The baseline is taken after
//! a warm-up, because the memory a first pass costs — the thread pool, the
//! lazily built tables — is not a leak.

use std::process::Command;
use std::sync::Arc;
use std::thread;

use jay::{Array, Dialect, ErrorKind, Lang, Program, compile};

// --- the work --------------------------------------------------------------

/// The programs every case here runs, each with the arguments it wants.
/// `{v}` is a parameter: the compile leaves a hole and the run fills it, so
/// a cycle exercises the data path and not only the source path.
fn work() -> Vec<(Lang, &'static str, Vec<Array>)> {
    let v: Vec<i64> = (0..20_000).collect();
    vec![
        (
            Lang::J,
            "n =. 4000\nv =. i. n\ns =. +/ v\nm =. >./ v\n(s , m) , +/ 0 = 7 | v",
            Vec::new(),
        ),
        (
            Lang::J,
            "s =. 1000 | i. 2000\ng =. /: s\nsrt =. g { s\n(3 {. srt) , (3 {. \\: s) , (+/ srt)",
            Vec::new(),
        ),
        (Lang::J, "s =. {v}\n(+/ s) , (>./ s) , (# s) , (+/ 0 = 3 | s)", vec![Array::from_i64(v.clone())]),
        (
            Lang::Apl,
            "N←4000\nV←¯1+⍳N\nS←+/V\nM←⌈/V\n(S,M),+/0=7|V",
            Vec::new(),
        ),
        (
            Lang::Apl,
            "S←1000|¯1+⍳2000\nG←⍋S\nSR←S[G]\n(3↑SR),(3↑S[⍒S]),(+/SR)",
            Vec::new(),
        ),
        (Lang::Apl, "S←{v}\n(+/S),(⌈/S),(⍴S),(+/0=3|S)", vec![Array::from_i64(v)]),
    ]
}

/// A program compiled once and run once, reported as text. Shape and values,
/// so a change of either is a change of the digest.
fn digest(a: &Array) -> String {
    let vals = a.to_f64_vec().expect("the stress programs answer with numbers");
    format!("{:?}:{:?}", a.shape, vals)
}

fn run_one(program: &Program, args: &[Array]) -> Array {
    let mut sink = |_: &str| {};
    program
        .run(args, &mut sink)
        .unwrap_or_else(|e| panic!("run failed:\n{}", program.render_error(&e)))
        .expect("the program yielded no value")
}

fn compile_one(lang: Lang, src: &str) -> Program {
    compile(lang, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)))
}

/// Compile and run every program once, and refuse two sentences on the way.
/// The refusals belong in the cycle rather than beside it: what is being
/// asked is that a failed compile and a failed run cost nothing that a
/// successful one does not.
fn cycle(work: &[(Lang, &'static str, Vec<Array>)]) -> Vec<String> {
    let mut out = Vec::with_capacity(work.len());
    for (lang, src, args) in work {
        let program = compile_one(*lang, src);
        out.push(digest(&run_one(&program, args)));
    }
    let refused = compile(Lang::J, "1 + ", &Dialect::default()).expect_err("an unfinished sentence");
    assert_eq!(refused.kind, ErrorKind::Parse);
    let mismatched = compile_one(Lang::J, "1 2 3 + 4 5");
    let mut sink = |_: &str| {};
    assert!(mismatched.run(&[], &mut sink).is_err(), "lengths 3 and 2 do not agree");
    out
}

// --- resident memory -------------------------------------------------------

/// This process's resident set in kibibytes, or `None` where it cannot be
/// asked for. `ps` is the one reader available on every platform the suite
/// runs on without a new dependency; where it is missing or answers
/// something unparseable the case reports and passes rather than failing on
/// the environment.
fn rss_kib() -> Option<u64> {
    let pid = std::process::id().to_string();
    let out = Command::new("ps").args(["-o", "rss=", "-p", &pid]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

#[test]
fn repeated_compile_and_run_holds_its_memory() {
    let work = work();
    let expected = cycle(&work);
    for _ in 0..10 {
        assert_eq!(cycle(&work), expected, "an answer moved during the warm-up");
    }
    let before = rss_kib();
    for _ in 0..150 {
        assert_eq!(cycle(&work), expected, "an answer moved under repetition");
    }
    let after = rss_kib();
    let (Some(before), Some(after)) = (before, after) else {
        println!("resident memory could not be read here; the answers were still checked");
        return;
    };
    // A ratio, with a small additive slack so that a baseline of a few
    // megabytes cannot make ordinary allocator noise look like a leak. The
    // measured cycles hand the same borrowed vectors in and build several
    // hundred megabytes of intermediates between them; retaining any
    // fraction of that would clear this ceiling by a wide margin.
    let ceiling = (before as f64 * 1.5) + 16_384.0;
    assert!(
        (after as f64) <= ceiling,
        "resident memory grew from {before} KiB to {after} KiB over 150 compile-and-run cycles \
         (ceiling {ceiling:.0} KiB)"
    );
}

// --- the pool --------------------------------------------------------------

/// Set in the child so it knows to answer rather than to ask.
const WORKER: &str = "LIBJAY_STRESS_WORKER";

#[test]
fn the_thread_count_does_not_change_the_answer() {
    // `LIBJAY_THREADS` is read once per process and frozen, so the sweep is
    // three child processes of this same binary rather than three loops.
    if std::env::var_os(WORKER).is_some() {
        let work = work();
        println!("DIGEST {}", cycle(&work).join(" | "));
        return;
    }
    let exe = std::env::current_exe().expect("the test binary's own path");
    let mut answers: Vec<(usize, String)> = Vec::new();
    for threads in [1usize, 2, 4] {
        let out = Command::new(&exe)
            .args(["--exact", "the_thread_count_does_not_change_the_answer", "--nocapture"])
            .env(WORKER, "1")
            .env("LIBJAY_THREADS", threads.to_string())
            .output()
            .expect("running this binary again");
        assert!(
            out.status.success(),
            "the worker failed with LIBJAY_THREADS={threads}:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let line = text
            .lines()
            .find_map(|l| l.strip_prefix("DIGEST "))
            .unwrap_or_else(|| panic!("the worker printed no digest:\n{text}"))
            .to_string();
        answers.push((threads, line));
    }
    let (first_n, first) = &answers[0];
    for (threads, answer) in &answers[1..] {
        assert_eq!(
            answer, first,
            "LIBJAY_THREADS={threads} answered differently from LIBJAY_THREADS={first_n}"
        );
    }
}

#[test]
fn one_program_answers_the_same_from_many_threads() {
    let work = work();
    let programs: Vec<(Arc<Program>, Arc<Vec<Array>>, String)> = work
        .iter()
        .map(|(lang, src, args)| {
            let p = Arc::new(compile_one(*lang, src));
            let want = digest(&run_one(&p, args));
            (p, Arc::new(args.clone()), want)
        })
        .collect();
    // One compiled program per source, shared by every thread: what is being
    // asked is that a Program is a read-only thing and that two runs of it at
    // once cannot see each other.
    let mut handles = Vec::new();
    for _ in 0..8 {
        let programs = programs.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..25 {
                for (program, args, want) in &programs {
                    assert_eq!(&digest(&run_one(program, args)), want);
                }
            }
        }));
    }
    for h in handles {
        h.join().expect("a worker thread panicked");
    }
}

// --- refusals --------------------------------------------------------------

/// Every way of being refused that the surface offers, and the kind each one
/// must report. A refusal is contract, so the loop below holds the kind as
/// well as the fact.
const REFUSALS: &[(Lang, &str, ErrorKind, bool)] = &[
    // (language, source, kind, whether the refusal comes from the run)
    (Lang::J, "1 + ", ErrorKind::Parse, false),
    (Lang::J, "+/ (", ErrorKind::Parse, false),
    (Lang::Apl, "1 2 3 +", ErrorKind::Parse, false),
    (Lang::J, "1 2 3 + 4 5", ErrorKind::Length, true),
    (Lang::J, "(i. 2 3) + i. 3 2", ErrorKind::Shape, true),
    (Lang::Apl, "1 2 3+4 5", ErrorKind::Length, true),
    (Lang::J, "'abc' + 1", ErrorKind::Type, true),
    (Lang::Apl, "1÷0", ErrorKind::Domain, true),
];

#[test]
fn refusals_do_not_poison_what_comes_after() {
    let work = work();
    let good: Vec<(Program, Vec<Array>, String)> = work
        .iter()
        .map(|(lang, src, args)| {
            let p = compile_one(*lang, src);
            let want = digest(&run_one(&p, args));
            (p, args.clone(), want)
        })
        .collect();
    let mut kinds = Vec::new();
    for round in 0..100 {
        for (lang, src, kind, at_run) in REFUSALS {
            let err = if *at_run {
                let program = compile(*lang, src, &Dialect::default())
                    .unwrap_or_else(|e| panic!("{src:?} should compile:\n{}", e.render(src)));
                let mut sink = |_: &str| {};
                match program.run(&[], &mut sink) {
                    Err(e) => e,
                    // A sentence libjay refuses at compile time instead is
                    // still a refusal; what must not happen is an answer.
                    Ok(v) => panic!("{src:?} answered {v:?} rather than being refused"),
                }
            } else {
                compile(*lang, src, &Dialect::default())
                    .err()
                    .unwrap_or_else(|| panic!("{src:?} should not compile"))
            };
            if round == 0 {
                kinds.push((src, err.kind));
            }
            assert_eq!(err.kind, *kind, "{src:?} reported the wrong kind of refusal");
            assert!(!err.msg.is_empty(), "{src:?} was refused without a message");
        }
        // The engine is still the engine afterwards.
        for (program, args, want) in &good {
            assert_eq!(&digest(&run_one(program, args)), want);
        }
    }
    assert_eq!(kinds.len(), REFUSALS.len());
}

#[test]
fn a_program_survives_being_run_with_the_wrong_data() {
    let program = compile_one(Lang::J, "s =. {v}\n(+/ s) , (# s)");
    let good = vec![Array::from_i64((0..1000).collect::<Vec<_>>())];
    let want = digest(&run_one(&program, &good));
    let mut sink = |_: &str| {};
    for _ in 0..200 {
        // No data at all where the program asked for some.
        assert!(program.run(&[], &mut sink).is_err(), "a missing argument must be refused");
        // Too much of it.
        let extra = vec![good[0].clone(), Array::scalar_i64(1)];
        assert!(program.run(&extra, &mut sink).is_err(), "a surplus argument must be refused");
        assert_eq!(digest(&run_one(&program, &good)), want);
    }
}
