//! Tests for the Chirp per-app FFI surface. Lives in `ffi/tests/` rather
//! than `ffi/mod.rs` so the test bulk doesn't reintroduce the V-09 LOC
//! violation that motivated this split, and split further into per-domain
//! sub-modules (V-09b) so each test file stays under the 500-LOC ceiling.

mod helpers;
#[cfg(feature = "marmot")]
mod identity;
mod nip17;
mod nip29;
mod nip57;
mod register;
mod social;
