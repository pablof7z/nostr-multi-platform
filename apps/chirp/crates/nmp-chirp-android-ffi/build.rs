// Force the Android linker (lld) to pull rlib archive members that define the
// `#[no_mangle] extern "C"` kernel symbols from `nmp-core`. Without `-u`, the
// cdylib references them only via our `extern "C"` declarations (a *use* the
// archive linker treats as undefined dynamic, not as a pull request), so the
// rlib object defining them is dead-stripped before reaching the .so — see
// dlopen "cannot locate symbol" at runtime. `-u sym` marks each one as
// initially-undefined, which forces lld to look for a definition (the rlib
// has it), pulls the object, and the symbol becomes defined in the .so.
//
// nmp-core's `ffi` module is `mod ffi;` (private) so we cannot reach these
// through a Rust path — and per D0 the `test-support` cfg-gated re-exports
// must NOT be enabled in production builds. `-u` is the production-safe
// path: it changes only the linker invocation, not the kernel's source.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        println!("cargo:rerun-if-changed=build.rs");
        return;
    }

    let production_symbols = [
        // Lifecycle/read-side symbols. Android calls the wider FFI surface
        // through Rust paths in `src/lib.rs`; these `-u` entries are kept for
        // the startup symbols that must survive linker dead stripping.
        "nmp_app_new",
        "nmp_app_free",
        "nmp_app_set_update_callback",
        "nmp_app_start",
        "nmp_app_stop",
    ];
    for sym in production_symbols {
        println!("cargo:rustc-link-arg=-Wl,-u,{sym}");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
