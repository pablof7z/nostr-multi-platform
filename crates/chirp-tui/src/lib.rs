//! `chirp-tui` library surface.

pub mod app;
pub mod bridge;
pub mod commands;
pub mod feature_snapshot;
pub mod features;
pub mod input;
pub mod render_intents;
pub mod runtime;
pub mod runtime_commands;
pub mod snapshot;
pub mod timeline;
pub mod ui;

pub type Result<T> = std::result::Result<T, String>;
