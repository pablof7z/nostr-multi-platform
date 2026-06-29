// build.rs — nmp-testing
//
// Captures build metadata and re-emits it as compile-time env vars for the
// ffi-transport-bench binary so that committed reports are reproducible.
//
// The three vars emitted are:
//   FFI_BENCH_OPT_LEVEL  — cargo optimization level ("0", "1", "2", "3", "s", "z")
//   FFI_BENCH_TARGET     — target triple (e.g. "aarch64-apple-darwin")
//   FFI_BENCH_RUSTC_VERSION — `rustc --version` string (e.g. "rustc 1.82.0 (f6e511...)")

fn main() {
    let opt_level = std::env::var("OPT_LEVEL").unwrap_or_else(|_| "unknown".to_string());
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());

    // Invoke the compiler that cargo selected for this build to get its version.
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let rustc_version = std::process::Command::new(&rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=FFI_BENCH_OPT_LEVEL={opt_level}");
    println!("cargo:rustc-env=FFI_BENCH_TARGET={target}");
    println!("cargo:rustc-env=FFI_BENCH_RUSTC_VERSION={rustc_version}");

    // Re-run if the build environment changes (cargo handles most cases, but
    // be explicit so the bench binary is re-stamped on toolchain upgrades).
    println!("cargo:rerun-if-env-changed=OPT_LEVEL");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=RUSTC");
}
