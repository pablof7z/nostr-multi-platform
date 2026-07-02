//! Feed vocabulary ratchet — "session" is not public feed vocabulary (#2783).
//!
//! Owner decision (#2508, #2783): "session" is internal runtime bookkeeping.
//! It must never resurface on the app-facing feed surfaces: the native
//! `app.feeds()` facade (`nmp-native-runtime`), the UniFFI bridge mechanics
//! (`nmp-uniffi-support`), the browser-runtime `runtime.feeds()` facade and its
//! crate-root exports, or the feed-helper codegen templates that render
//! Swift/Kotlin/TypeScript bindings for those surfaces.
//!
//! This is a ratchet, not a blanket "session" ban: `nmp-feed-session`,
//! `session_engine.rs`, `FeedSessionRegistry`, `FeedSessionId`, and NIP-50
//! search / NIP-29 group session vocabulary (`Nip50SearchSession`,
//! `Nip29GroupDiscoverySession`, `BrowserGroupDiscoverySessionHandle`, …) are
//! explicitly untouched internal-runtime or separate-domain vocabulary and
//! must not be flagged. The banned list below names only the exact retired
//! feed-facade identifiers so unrelated "session" vocabulary in the same
//! crates never false-positives.
//!
//! ## Allowed exemptions
//!
//! - Comment lines (any of `//`, `///`, `//!`, inside `/* */`).
//! - Per-line `// doctrine-allow: feed_vocabulary — reason` opt-out (reason
//!   REQUIRED, like the other ownership ratchets).

use std::path::Path;

pub const ID: &str = "feed_vocabulary";

/// The exact retired feed-facade identifiers. Each is distinctive enough that
/// it cannot appear as a substring of a legitimate, still-current identifier
/// (`FeedSessionRegistry`, `FeedSessionId`, `FeedSessionHost`, `Nip50SearchSession`,
/// `ActiveFollowsOpFeedSession`, `BrowserGroupDiscoverySessionHandle`, …) — see
/// the module doc for why those stay unbanned.
const BANNED_TOKENS: &[&str] = &[
    "FeedSessions",
    "FeedSessionHandle",
    "FeedSessionError",
    "close_feed_session",
    "open_feed_session",
    "load_older_feed_session",
    "reopen_feed_session",
];

/// True iff `path` is one of the app-facing feed-facade surfaces this ratchet
/// covers: the native `app.feeds()` facade, the UniFFI bridge mechanics crate,
/// browser-runtime, and the feed-helper codegen templates (the source of
/// truth for the generated Swift/Kotlin/TypeScript helpers — generated-file
/// freshness against these templates is separately enforced by
/// `nmp-codegen`'s own `checked_fixtures_are_current` test).
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    let in_tree = |root: &str| s.contains(root) || s.starts_with(root.trim_start_matches('/'));
    in_tree("/crates/nmp-native-runtime/src/")
        || in_tree("/crates/nmp-uniffi-support/src/")
        || in_tree("/crates/nmp-browser-runtime/src/")
        || in_tree("/crates/nmp-codegen/src/feed_helpers/")
}

pub fn check(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    for token in BANNED_TOKENS {
        if let Some(pos) = line.find(token) {
            return vec![(
                pos + 1,
                format!(
                    "`{token}` is banned: \"session\" is not public feed vocabulary (#2508, \
                     #2783) — the app-facing feed facade and its FFI/codegen mirrors spell \
                     `Feeds`/`FeedHandle`/`open_feed`/`close_feed`/`reopen_feed`"
                ),
                "rename to the Feeds/FeedHandle vocabulary (`Feeds`, `BrowserFeeds`, \
                 `FeedHandle`, `FeedError`, `open_feed`, `close_feed`, `load_older_feed`, \
                 `reopen_feed`); internal-only session vocabulary (nmp-feed-session, \
                 FeedSessionRegistry, search/group sessions) is unaffected and lives outside \
                 this rule's scope"
                    .to_string(),
            )];
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_retired_facade_type_names() {
        let hits = check("pub struct FeedSessions<'a> {", false, false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("FeedSessions"));

        let hits = check("pub struct FeedSessionHandle {", false, false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("FeedSessionHandle"));
    }

    #[test]
    fn flags_retired_fn_and_error_names() {
        for line in [
            "pub fn close_feed_session(app: &NmpApp, opened: &OpenedFeed) -> bool {",
            "pub fn open_feed_session(app: &NmpApp, params_json: &str) {",
            "pub fn load_older_feed_session(app: &NmpApp, opened: &OpenedFeed) -> bool {",
            "pub fn reopen_feed_session(app: &NmpApp, old: &OpenedFeed, p: &str) {",
            "pub enum FeedSessionError {",
        ] {
            let hits = check(line, false, false);
            assert_eq!(hits.len(), 1, "expected a hit for: {line}");
        }
    }

    #[test]
    fn does_not_flag_untouched_internal_session_vocabulary() {
        // Explicitly-not-renamed internal/search/group session vocabulary must
        // never trip this ratchet — it is a different, still-legitimate domain.
        for line in [
            "pub struct FeedSessionRegistry {",
            "pub struct FeedSessionId(pub u64);",
            "use nmp_feed_session::{FeedSessionHost, IdentityChangeObserverId};",
            "pub struct Nip50SearchSession {",
            "pub fn open_search_session(&self, descriptor: Nip50SearchSession) {}",
            "pub struct ActiveFollowsOpFeedSession {",
            "pub struct BrowserGroupDiscoverySessionHandle {",
            "pub fn feed_session_is_open(&self, handle: &FeedHandle) -> bool {",
            "pub fn live_feed_session_count(&self) -> usize {",
        ] {
            let hits = check(line, false, false);
            assert!(hits.is_empty(), "unexpected hit for: {line}");
        }
    }

    #[test]
    fn ignores_comments_and_test_cfg() {
        assert!(check("// pub struct FeedSessions<'a> {}", true, false).is_empty());
        assert!(check("pub struct FeedSessionHandle {}", false, true).is_empty());
    }

    #[test]
    fn scope_covers_the_four_facade_surfaces() {
        assert!(file_in_scope(Path::new(
            "crates/nmp-native-runtime/src/feed_facade.rs"
        )));
        assert!(file_in_scope(Path::new(
            "crates/nmp-uniffi-support/src/sessions.rs"
        )));
        assert!(file_in_scope(Path::new(
            "crates/nmp-browser-runtime/src/runtime/feed_lifecycle.rs"
        )));
        assert!(file_in_scope(Path::new(
            "crates/nmp-codegen/src/feed_helpers/ts.rs"
        )));
        assert!(!file_in_scope(Path::new("crates/nmp-feed/src/params.rs")));
        assert!(!file_in_scope(Path::new(
            "crates/nmp-feed-session/src/session_engine.rs"
        )));
        assert!(!file_in_scope(Path::new("crates/nmp-uniffi/src/sessions/feed.rs")));
    }
}
