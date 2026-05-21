//! Standalone Chirp REPL library surface.

pub mod actions;
pub mod app;
pub mod command;
pub mod marmot;
pub mod render;
pub mod session;

pub type Result<T> = std::result::Result<T, String>;
