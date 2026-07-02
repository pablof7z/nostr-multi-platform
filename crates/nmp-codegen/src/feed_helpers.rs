//! Generated app-facing feed helpers over the canonical `FeedParams` JSON seam.
//!
//! These helpers are host-language convenience only. They build the same
//! serializable declaration that Rust app code builds with `FeedKey::app(...)`,
//! `feed::events()`, `source::active_user().follows()`, and
//! `app.feeds().open_spec(...)`, then call the host platform's feed-opening
//! door.
//! They do not introduce another feed runtime or expose Trellis/session
//! compiler internals.

use std::path::Path;

pub mod kotlin;
pub mod swift;
pub mod ts;

/// Which host language to emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Emit `FeedHelpers.generated.swift`.
    Swift,
    /// Emit `FeedHelpers.kt`.
    Kotlin,
    /// Emit `feedHelpers.generated.ts`.
    Ts,
}

impl Platform {
    /// Parse the `--platform` argument.
    ///
    /// # Errors
    /// An unrecognised platform string.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "swift" => Ok(Self::Swift),
            "kotlin" => Ok(Self::Kotlin),
            "ts" => Ok(Self::Ts),
            other => Err(format!(
                "unknown --platform `{other}` (expected swift|kotlin|ts)"
            )),
        }
    }
}

/// Outcome of a `--check` run.
#[derive(Debug)]
pub struct FeedHelpersCheckOutcome {
    /// `true` when the on-disk file matches the freshly-rendered output.
    pub up_to_date: bool,
    /// First differing line (1-based) when stale; `None` when up-to-date or missing.
    pub first_diff_line: Option<usize>,
}

/// Render generated feed helpers for one host language.
#[must_use]
pub fn render_feed_helpers(platform: Platform) -> String {
    match platform {
        Platform::Swift => swift::render(),
        Platform::Kotlin => kotlin::render(),
        Platform::Ts => ts::render(),
    }
}

/// Write generated feed helpers to `out_path`.
///
/// # Errors
/// Filesystem I/O failures.
pub fn generate_feed_helpers(platform: Platform, out_path: &Path) -> std::io::Result<()> {
    let rendered = render_feed_helpers(platform);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, rendered)
}

/// Diff freshly-rendered helpers against `out_path`.
///
/// # Errors
/// Filesystem I/O failures other than NotFound.
pub fn check_feed_helpers(
    platform: Platform,
    out_path: &Path,
) -> std::io::Result<FeedHelpersCheckOutcome> {
    let rendered = render_feed_helpers(platform);
    let actual = match std::fs::read_to_string(out_path) {
        Ok(value) => value,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FeedHelpersCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(err),
    };
    if actual == rendered {
        return Ok(FeedHelpersCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    Ok(FeedHelpersCheckOutcome {
        up_to_date: false,
        first_diff_line: crate::diff_report::first_diff_or_length(&actual, &rendered),
    })
}

#[cfg(test)]
#[path = "feed_helpers/tests.rs"]
mod tests;
