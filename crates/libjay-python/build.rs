//! Says so when a release build of the extension leaves LLVM out.
//!
//! The published wheels are built with the `llvm` feature. A build from the
//! source distribution is not, unless asked: it then compiles with Cranelift
//! alone, which is correct and only slower on long loops, and this warning is
//! how the person building it learns how to get the rest.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let llvm = std::env::var_os("CARGO_FEATURE_LLVM").is_some();
    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");
    if release && !llvm {
        println!(
            "cargo:warning=libjay: built without LLVM, so Cranelift is the only code generator. \
             To include LLVM 21, point LLVM_SYS_211_PREFIX at an installation and build with \
             MATURIN_PEP517_ARGS=\"--features llvm\" (pip/uv) or `maturin build --features llvm`."
        );
    }
}
