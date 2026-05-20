//! Standalone Chirp REPL library surface.

pub mod actions;
pub mod command;
pub mod render;
pub mod session;
pub mod wire;

pub type Result<T> = std::result::Result<T, String>;
