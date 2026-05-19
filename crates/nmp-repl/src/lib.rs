//! Library surface for `nmp-repl`.
//!
//! The design plan (§2) calls for "binary only, no lib.rs". We add a thin
//! library here purely so `cargo test -p nmp-repl --lib` can run the parser
//! unit tests — the design doc's §14 acceptance criteria explicitly require
//! this. The library has no embedded-use cases; promote modules to `pub`
//! at this seam only.

pub mod ast;
pub mod parser;
pub mod error;
pub mod session;
pub mod nip05;
pub mod ws;
pub mod discovery;
pub mod plan;
pub mod fanout;
pub mod render;
pub mod publish;
pub mod commands;
