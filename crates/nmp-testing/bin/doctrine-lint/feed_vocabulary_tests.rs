//! Smoke tests for the feed-vocabulary ratchet (#2508, #2783): "session" is
//! not public feed vocabulary on the native/UniFFI/browser-runtime/codegen
//! facade surfaces.

use std::path::Path;

use super::{fixture_path, run_lint, workspace_root};

#[path = "rules/feed_vocabulary.rs"]
mod feed_vocabulary;

#[test]
fn feed_vocabulary_positive_fixture_fires() {
    let workspace = workspace_root();
    let tmp = workspace
        .join("target")
        .join("doctrine_lint_feed_vocabulary_pos")
        .join("crates")
        .join("nmp-native-runtime")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_feed_vocabulary_pos"),
    );
    std::fs::create_dir_all(&tmp).expect("create fake nmp-native-runtime src dir");
    let pos_src = workspace.join(fixture_path("feed_vocabulary/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("feed_facade.rs")).expect("copy positive fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 1,
        "feed_vocabulary positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains(&format!("error[{}]", feed_vocabulary::ID)),
        "positive fixture must emit feed_vocabulary finding; stdout:\n{}",
        stdout
    );
    for token in ["FeedSessions", "FeedSessionHandle", "close_feed_session"] {
        assert!(
            stdout.contains(token),
            "positive fixture must flag `{token}`; stdout:\n{}",
            stdout
        );
    }
}

#[test]
fn feed_vocabulary_negative_fixture_is_clean() {
    let workspace = workspace_root();
    let tmp = workspace
        .join("target")
        .join("doctrine_lint_feed_vocabulary_neg")
        .join("crates")
        .join("nmp-native-runtime")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_feed_vocabulary_neg"),
    );
    std::fs::create_dir_all(&tmp).expect("create fake nmp-native-runtime src dir");
    let neg_src = workspace.join(fixture_path("feed_vocabulary/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("feed_facade.rs")).expect("copy negative fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 0,
        "feed_vocabulary negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains(&format!("error[{}]", feed_vocabulary::ID)),
        "negative fixture (incl. untouched internal session vocabulary) must \
         produce no feed_vocabulary finding; stdout:\n{}",
        stdout
    );
}

#[test]
fn feed_vocabulary_scope_covers_the_facade_surfaces_only() {
    assert!(feed_vocabulary::file_in_scope(Path::new(
        "crates/nmp-native-runtime/src/feed_facade.rs"
    )));
    assert!(feed_vocabulary::file_in_scope(Path::new(
        "crates/nmp-uniffi-support/src/sessions.rs"
    )));
    assert!(feed_vocabulary::file_in_scope(Path::new(
        "crates/nmp-browser-runtime/src/runtime/feed_lifecycle.rs"
    )));
    assert!(feed_vocabulary::file_in_scope(Path::new(
        "crates/nmp-codegen/src/feed_helpers/ts.rs"
    )));
    // Internal/other-domain session machinery stays out of scope — it is
    // legitimate vocabulary this ratchet must never touch.
    assert!(!feed_vocabulary::file_in_scope(Path::new(
        "crates/nmp-feed/src/params.rs"
    )));
    assert!(!feed_vocabulary::file_in_scope(Path::new(
        "crates/nmp-feed-session/src/session_engine.rs"
    )));
    assert!(!feed_vocabulary::file_in_scope(Path::new(
        "crates/nmp-uniffi/src/sessions/feed.rs"
    )));
}
