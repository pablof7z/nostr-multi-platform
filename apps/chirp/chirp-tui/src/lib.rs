//! `chirp-tui` library surface.

pub mod app;
pub mod bridge;
pub mod commands;
pub mod feature_snapshot;
pub mod feature_snapshot_json;
pub mod feature_snapshot_typed;
pub mod features;
pub mod input;
pub(crate) mod keyring;
pub mod media_cache;
pub mod render_intents;
pub mod runtime;
pub mod runtime_commands;
pub mod snapshot;
pub mod timeline;
pub mod ui;

pub type Result<T> = std::result::Result<T, String>;

/// Abbreviated hex identifier: `<first8>...<last6>` for TUI status lines.
/// Uses ASCII `"..."` not Unicode `"…"` — safer for terminal fonts.
pub(crate) fn short_id(value: &str) -> String {
    if value.len() <= 16 {
        value.to_string()
    } else {
        format!("{}...{}", &value[..8], &value[value.len() - 6..])
    }
}
