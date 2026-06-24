use super::*;
use crate::relay::{DEFAULT_VISIBLE_LIMIT};
use nmp_network::role::RelayRole;
use crate::store::{RawEvent, VerifiedEvent};

const TEST_AUTHOR_HEX: &str = "abababababababababababababababababababababababababababababababab";

fn hex64(prefix: &str) -> String {
    let mut s = prefix.to_string();
    while s.len() < 64 {
        s.push('0');
    }
    s.chars().take(64).collect()
}

fn inject_note(kernel: &mut Kernel, id: &str, author: &str, content: &str) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at: 1_700_000_000,
        kind: 1,
        tags: vec![],
        content: content.to_string(),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        RelayRole::Content,
        "claimed-events-raw-author",
        VerifiedEvent::from_raw_unchecked(raw),
    );
}

#[test]
fn claimed_events_carries_raw_author_pubkey_without_profile_enrichment() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let id = hex64("9");
    inject_note(
        &mut kernel,
        &id,
        TEST_AUTHOR_HEX,
        "profile should not enrich this",
    );
    kernel.inject_profile(NostrEvent {
        id: hex64("8"),
        pubkey: TEST_AUTHOR_HEX.to_string(),
        created_at: 1_700_000_001,
        kind: 0,
        tags: vec![],
        content: r#"{"display_name":"Alice","picture":"https://example.com/alice.png"}"#
            .to_string(),
        sig: "a".repeat(128),
    });

    let _ = kernel.resolve_ref(
        RefNamespace::Event,
        id.clone(),
        "view-0".to_string(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );

    let snapshot = kernel.make_update_value_for_test(true);
    let entry = &snapshot["projections"]["claimed_events"][&id];
    assert_eq!(entry["author_pubkey"], TEST_AUTHOR_HEX);
    assert!(
        entry["author_display_name"].is_null(),
        "claimed_events must not duplicate kind:0 display names"
    );
    assert!(
        entry["author_picture_url"].is_null(),
        "claimed_events must not duplicate kind:0 avatar URLs"
    );
}
