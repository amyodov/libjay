//! Builds tests/csmoke.c against the real shared library and runs it.
//!
//! This is the test that proves the header, the ABI and the artifact name
//! agree: a plain C compiler, `-ljay`, and nothing Rust-specific. It skips
//! (loudly, but without failing) when there is no C compiler, or when the
//! shared library has not been built yet — run `cargo build -p libjay-capi`
//! first.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The profile directory holding the uplifted artifacts: the test binary
/// lives in `<target>/<profile>/deps/`.
fn artifact_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary path");
    exe.parent().and_then(Path::parent).expect("<target>/<profile>").to_path_buf()
}

/// The platform's shared-library file name for `-ljay`.
fn shared_library(dir: &Path) -> Option<PathBuf> {
    let names = ["libjay.dylib", "libjay.so", "jay.dll", "libjay.a"];
    names.iter().map(|n| dir.join(n)).find(|p| p.exists())
}

fn find_cc() -> Option<String> {
    let from_env = std::env::var("CC").ok();
    let candidates: Vec<String> =
        from_env.into_iter().chain(["cc", "clang", "gcc"].iter().map(|s| s.to_string())).collect();
    candidates.into_iter().find(|cc| {
        Command::new(cc).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    })
}

#[test]
fn a_c_program_links_against_ljay_and_runs() {
    let Some(cc) = find_cc() else {
        eprintln!("skipping: no C compiler (tried $CC, cc, clang, gcc)");
        return;
    };
    let dir = artifact_dir();
    let Some(lib) = shared_library(&dir) else {
        eprintln!(
            "skipping: no libjay shared library in {}; run `cargo build -p libjay-capi` first",
            dir.display()
        );
        return;
    };

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = manifest.join("tests/csmoke.c");
    let include = manifest.join("include");
    let binary = dir.join(if cfg!(windows) { "csmoke.exe" } else { "csmoke" });

    let mut build = Command::new(&cc);
    build
        .arg(&source)
        .arg("-I")
        .arg(&include)
        .arg("-L")
        .arg(&dir)
        .arg("-ljay")
        .arg("-o")
        .arg(&binary);
    if !cfg!(windows) {
        // So the child finds the library without an environment variable.
        build.arg(format!("-Wl,-rpath,{}", dir.display()));
    }
    let built = build.output().expect("running the C compiler");
    assert!(
        built.status.success(),
        "compiling {} against {} failed:\n{}",
        source.display(),
        lib.display(),
        String::from_utf8_lossy(&built.stderr)
    );

    let mut run = Command::new(&binary);
    // Belt and braces for platforms where the rpath is not honoured.
    run.env("DYLD_LIBRARY_PATH", &dir).env("LD_LIBRARY_PATH", &dir);
    let out = run.output().expect("running the C program");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "csmoke exited with {:?}\nstdout:\n{stdout}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );

    let expected = format!(
        "mean=2.5\napl=15\necho=hello from j\ncaret=ok\nversion={}\n",
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(stdout, expected);
}
