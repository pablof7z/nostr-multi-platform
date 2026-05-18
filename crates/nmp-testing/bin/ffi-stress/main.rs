//! FFI stress harness — stub for M10.5 implementation.
//!
//! The full implementation drives the nmp_app_* C symbols via extern "C"
//! declarations. This stub allows the workspace to compile while the M10.5
//! task is in progress.

fn main() {
    eprintln!("ffi-stress: not yet implemented (M10.5 task)");
    std::process::exit(1);
}
