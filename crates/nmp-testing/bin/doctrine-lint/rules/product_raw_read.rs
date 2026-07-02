//! Product raw-read ratchet — first typed-read-session guard.
//!
//! Product shells and starter templates must not grow new raw `open_interest`
//! or observed-projection hooks. Those are substrate/runtime/protocol seams; user-
//! facing read models should be typed sessions, typed projections, or the
//! external pull cursor.
//!
//! Current classification:
//! - allowed: `nmp-core`, `nmp-ffi`, native-runtime internals, protocol crates,
//!   diagnostics, tests, and export/pull consumers;
//! - denied: Rust app shells under `apps/**/src/**` and starter templates under
//!   `crates/nmp-cli/templates/**`;
//! - denied: the worked example product crate under
//!   `crates/nmp-example-login-timeline/src/**`.
//!
//! This deliberately does not retire the existing low-level runtime behavior,
//! but native `NmpApp` must not expose public raw `open_interest` /
//! `close_interest` methods.

use std::path::Path;

pub const ID: &str = "product_raw_read";

const BANNED_TOKENS: &[&str] = &[
    "nmp_app_open_interest",
    "nmp_app_close_interest",
    "nmp.kernel.open_interest",
    "nmp.kernel.close_interest",
    "open_interest(",
    ".open_interest(",
    "close_interest(",
    ".close_interest(",
    "open_observed_projection(",
    ".open_observed_projection(",
    "ObservedProjectionSink",
    "ObservedProjection",
    "KernelEventObserver",
    "register_event_observer",
    "register_live_event_tap",
    "register_snapshot_tick_observer",
    "register_raw_event_observer",
    "RawEventObserver",
];

pub fn check(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    for token in BANNED_TOKENS {
        if let Some(pos) = line.find(token) {
            return vec![(
                pos + 1,
                format!(
                    "`{token}` is a raw read/session hook in product-facing code. \
                     Product shells and starter templates must use typed read \
                     sessions/projections instead of raw `open_interest`, \
                     raw `close_interest`, observed-projection handles, raw \
                     event observers, or snapshot tick observers"
                ),
                "move the raw interest/observer work behind a typed session or \
                 typed projection owned by Rust; external mirrors should use a \
                 `GlobalLog` cursor plus UniFFI `NmpApp::mirror_pull_page`"
                    .to_string(),
            )];
        }
    }
    Vec::new()
}

pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.contains("/crates/nmp-cli/templates/") || s.starts_with("crates/nmp-cli/templates/") {
        return true;
    }
    if s.contains("/crates/nmp-example-login-timeline/src/")
        || s.starts_with("crates/nmp-example-login-timeline/src/")
    {
        return true;
    }
    let under_apps = s.contains("/apps/") || s.starts_with("apps/");
    under_apps && s.contains("/src/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_open_interest_in_product_shell() {
        let hits = check(
            "    app.open_interest(filter_json, consumer_id, scope);",
            false,
            false,
        );
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("typed read sessions"));
    }

    #[test]
    fn flags_snapshot_tick_observer_in_product_shell() {
        let hits = check(
            "    host.register_snapshot_tick_observer(|| {});",
            false,
            false,
        );
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn flags_close_interest_in_product_shell() {
        let hits = check("    app.close_interest(interest_id);", false, false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("raw `close_interest`"));
    }

    #[test]
    fn flags_observed_projection_in_product_shell() {
        let hits = check(
            "    let sink: ObservedProjectionSink = app.open_observed_projection(shape);",
            false,
            false,
        );
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("observed-projection"));
    }

    #[test]
    fn ignores_comments_and_test_cfg() {
        assert!(check("// app.open_interest(...)", true, false).is_empty());
        assert!(check("app.open_interest(filter, id, scope);", false, true).is_empty());
        assert!(check("let _: ObservedProjection = projection;", false, true).is_empty());
    }

    #[test]
    fn scope_is_product_example_and_templates_only() {
        assert!(file_in_scope(Path::new("apps/demo/src/main.rs")));
        assert!(file_in_scope(Path::new(
            "apps/demo/crates/nmp-app-demo/src/lib.rs"
        )));
        assert!(file_in_scope(Path::new(
            "crates/nmp-cli/templates/lib.rs.tmpl"
        )));
        assert!(file_in_scope(Path::new(
            "crates/nmp-example-login-timeline/src/lib.rs"
        )));
        assert!(!file_in_scope(Path::new("crates/nmp-core/src/lib.rs")));
        assert!(!file_in_scope(Path::new("crates/nmp-ffi/src/timeline.rs")));
        assert!(!file_in_scope(Path::new(
            "crates/nmp-native-runtime/src/app_impl_feeds.rs"
        )));
        assert!(!file_in_scope(Path::new("crates/nmp-testing/tests/foo.rs")));
    }
}
