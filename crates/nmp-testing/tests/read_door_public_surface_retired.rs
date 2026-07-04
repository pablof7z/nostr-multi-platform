//! #2418 — search/notification/pointer-source read doors are typed or internal.
//!
//! This grep gate protects the slice boundary: the app-facing runtime surface
//! must not re-expose the old raw read doors after typed descriptor/handle
//! wrappers have taken ownership of the lifecycle.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root is two levels above crates/nmp-testing")
        .to_path_buf()
}

fn code_only(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
        return "";
    }
    line
}

#[test]
fn retired_app_visible_read_doors_are_not_public_defs() {
    let root = repo_root();
    let absent_files = [
        "crates/nmp-native-runtime/src/plain_reactions.rs",
        "crates/nmp-native-runtime/src/plain_reactions/types.rs",
        "crates/nmp-native-runtime/src/plain_reactions_tests.rs",
        "crates/nmp-native-runtime/src/group_feed/feed.rs",
        "crates/nmp-native-runtime/src/group_feed/mod.rs",
        "crates/nmp-native-runtime/src/group_feed/reactions.rs",
        "crates/nmp-native-runtime/src/group_feed/roster.rs",
        "crates/nmp-native-runtime/src/group_feed/types.rs",
        "crates/nmp-browser-runtime/src/runtime/group_events.rs",
        "crates/nmp-browser-runtime/src/runtime/group_discovery.rs",
        "crates/nmp-native-runtime/src/op_pointer_source/mod.rs",
        "crates/nmp-native-runtime/src/op_pointer_source/shapes.rs",
        "crates/nmp-native-runtime/src/op_pointer_source/tests.rs",
        "crates/nmp-content/src/pointer_source/composition.rs",
        "crates/nmp-content/src/pointer_source/composition_tests.rs",
        "crates/nmp-content/src/pointer_source/shapes.rs",
    ];
    let checks = [
        (
            "crates/nmp-native-runtime/src/search.rs",
            &[
                "pub fn open_search(",
                "pub fn open_search_session(",
                "pub fn close_search(",
                "pub fn close_search_session(",
                "pub fn search_snapshot_bytes(",
                "pub fn search_session_snapshot_bytes(",
                "pub fn parse_search_request(",
                "pub struct Nip50SearchHandle",
                "pub struct Nip50SearchSession",
            ][..],
        ),
        (
            "crates/nmp-browser-runtime/src/runtime/search.rs",
            &[
                "pub fn open_search(",
                "pub(crate) fn open_search(",
                "pub(crate) fn open_search_session(",
                "pub(crate) fn close_search_session(",
                "pub(crate) fn open_search_for_key(",
                "pub(crate) struct BrowserSearchSessionDescriptor",
                "pub(crate) struct BrowserSearchSessionHandle",
            ][..],
        ),
        (
            "crates/nmp-browser-runtime/src/runtime.rs",
            &[
                "BrowserSearchSessionDescriptor",
                "BrowserSearchSessionHandle",
                "BrowserGroupEventsSession",
                "BrowserGroupEventsSessionDescriptor",
                "BrowserGroupEventsSessionHandle",
                "BrowserGroupDiscoverySession",
                "BrowserGroupDiscoverySessionDescriptor",
                "BrowserGroupDiscoverySessionHandle",
            ][..],
        ),
        (
            "crates/nmp-browser-runtime/src/runtime/handle.rs",
            &[
                "BrowserGroupEventsSession",
                "BrowserGroupDiscoverySession",
                "group_events_sessions",
                "group_discovery_sessions",
            ][..],
        ),
        (
            "crates/nmp-browser-runtime/src/lib.rs",
            &[
                "BrowserGroupEventsSessionHandle",
                "BrowserGroupDiscoverySessionHandle",
            ][..],
        ),
        (
            "crates/nmp-browser-runtime/src/runtime/notifications/mod.rs",
            &[
                "pub fn open_notifications(",
                "pub(crate) fn open_notifications(",
                "pub fn mark_notifications_read(",
                "pub(crate) fn mark_notifications_read(",
                "pub fn close_notifications(",
                "pub(crate) fn close_notifications(",
            ][..],
        ),
        (
            "crates/nmp-native-runtime/src/lib.rs",
            &[
                "pub mod op_pointer_source",
                "pub use nmp_nip50::SearchRequest",
                "Nip50SearchHandle",
                "Nip50SearchSession",
                "parse_search_request",
                "Nip29GroupDiscoveryHandle",
                "Nip29GroupDiscoverySession",
                "Nip29GroupEventsHandle",
                "Nip29GroupEventsSession",
                "Nip29GroupRosterHandle",
                "Nip29GroupRosterSession",
                "Nip29JoinedGroupsHandle",
                "Nip29JoinedGroupsSession",
                "DISCOVERED_GROUPS_KEY",
                "GROUP_EVENTS_KEY",
                "GROUP_ROSTER_KEY",
                "JOINED_GROUPS_KEY",
                "Nip25ReactionsHandle",
                "mod group_feed;",
                "Nip25GroupReactionsHandle",
                "Nip25GroupReactionsSession",
                "GROUP_REACTIONS_KEY",
            ][..],
        ),
        (
            "crates/nmp-native-runtime/src/app_struct.rs",
            &["reaction_read_sessions"][..],
        ),
        (
            "crates/nmp-content/src/lib.rs",
            &["open_pointer_source", "register_pointer_source"][..],
        ),
        (
            "crates/nmp-content/src/pointer_source/mod.rs",
            &["open_pointer_source", "register_pointer_source"][..],
        ),
    ];

    let mut violations = Vec::new();
    for relative in absent_files {
        let path = root.join(relative);
        if path.exists() {
            violations.push(format!("{relative}: retired read-door file reappeared"));
        }
    }
    for (relative, needles) in checks {
        let path = root.join(relative);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{} must be readable: {err}", path.display()));
        for (line_number, line) in text.lines().enumerate() {
            let code = code_only(line);
            for needle in needles {
                if code.contains(needle) {
                    violations.push(format!("{}:{}: {}", relative, line_number + 1, line.trim()));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "retired app-visible read-door definitions reappeared:\n{}",
        violations.join("\n")
    );
}
