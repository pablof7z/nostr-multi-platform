//! Shared fixtures for the state-projection suite: fixed 64-char hex
//! pubkeys/ids, the `make_update` snapshot driver, and a timeline-note
//! ingest helper.

use crate::kernel::Kernel;
use crate::store::{RawEvent, VerifiedEvent};
use nmp_network::role::RelayRole;

// 64-char hex pubkeys / ids — the kernel's `is_hex_pubkey` / `is_hex_id`
// gates require exactly 64 ascii hex digits.
pub(super) const ACCOUNT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const FOLLOW_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
pub(super) const FOLLOW_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";
pub(super) const NOTE_ID: &str = "e1e2e3e4e5e6e7e8e9eae1e2e3e4e5e6e7e8e9eae1e2e3e4e5e6e7e8e9eae1e2";

/// Drive `make_update` and parse the emitted JSON snapshot.
pub(super) fn snapshot(kernel: &mut Kernel) -> serde_json::Value {
    let json = kernel.make_update_json_for_test(true);
    serde_json::from_str(&json).expect("kernel snapshot must be valid JSON")
}

/// Ingest a kind:1 note through the `diag-firehose-` test path so it lands in
/// both the `events` read-cache and the `timeline` ordering projection without
/// needing the author to be a followed `timeline_authors` member.
pub(super) fn ingest_note(kernel: &mut Kernel, id: &str, author: &str, created_at: u64, content: &str) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at,
        kind: 1,
        tags: vec![],
        content: content.to_string(),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        RelayRole::Content,
        "diag-firehose-stress",
        VerifiedEvent::from_raw_unchecked(raw),
    );
    kernel.sort_timeline_deferred();
}
