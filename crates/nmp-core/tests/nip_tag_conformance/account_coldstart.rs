//! `create_account` cold-start publishing (NIP-01 kind:0, NIP-02 kind:3).
//!
//! A brand-new account has no kind:10002 of its own yet, so routing any of
//! these kinds through the fail-closed outbox resolver (`PublishTarget::Auto`)
//! would resolve `NoTargets` and the publish engine would silently drop them.
//! `create_account` closes that gap by publishing through the cold-start target
//! seam. These tests pin that core-owned kinds remain observable on cold-start
//! and keep their NIP-mandated tag shape.
//!
//! This applies only to cold-start. A user updating an *existing* kind:0/3/
//! Later profile/contact updates publish through the `Auto` outbox path, which
//! routes to already-declared write relays.

use std::collections::HashMap;

use nmp_core::testing::ConformanceHarness;

use crate::support::*;

/// This test pins that the kind:0 IS observable in the publish store (a
/// `NoTargets` drop never persists) and carries the profile JSON in `content`
/// with no tags (NIP-01).
#[test]
fn create_account_publishes_kind0_to_coldstart_relays() {
    let mut h = ConformanceHarness::new();
    let relays: Vec<(String, String)> = vec![
        ("wss://nip65-write.test".to_string(), "write".to_string()),
        ("wss://nip65-read.test".to_string(), "read".to_string()),
    ];
    let mut profile = HashMap::new();
    profile.insert("display_name".to_string(), "Marcus Webb".to_string());
    h.create_account(profile, &relays, &[]);

    let event = h
        .published_event_of_kind(0)
        .expect("create_account must emit an observable kind:0 on cold-start");
    assert_eq!(event["kind"], 0, "profile metadata must be kind:0");
    // NIP-01: kind:0 carries the profile JSON in `content`, no tags.
    assert!(
        tags_of(&event).is_empty(),
        "NIP-01 kind:0 metadata must carry no tags, got: {:?}",
        tags_of(&event)
    );
    assert!(
        event["content"]
            .as_str()
            .is_some_and(|c| c.contains("Marcus Webb")),
        "cold-start kind:0 metadata content must carry the profile JSON"
    );
    // No D6 toast — the cold-start publish had targets and succeeded.
    assert_eq!(
        h.last_error_toast(),
        None,
        "cold-start kind:0 publish must not surface an error toast"
    );
}

/// This test pins that the kind:3 IS observable in the publish store and
/// carries one `p` tag per followed pubkey (NIP-02) and nothing else. The
/// `initial_follows` seed set is the app-supplied cold-start follow list
/// (#1493).
#[test]
fn create_account_publishes_kind3_to_coldstart_relays() {
    let mut h = ConformanceHarness::new();
    let relays: Vec<(String, String)> = vec![
        ("wss://nip65-write.test".to_string(), "write".to_string()),
        ("wss://nip65-read.test".to_string(), "read".to_string()),
    ];
    let mut profile = HashMap::new();
    profile.insert("display_name".to_string(), "Marcus Webb".to_string());
    let follows = vec![hex64('b'), hex64('c')]; // app-supplied seed set (#1493)
    h.create_account(profile, &relays, &follows);

    let event = h
        .published_event_of_kind(3)
        .expect("create_account must emit an observable kind:3 on cold-start");
    assert_eq!(event["kind"], 3, "contacts list must be kind:3");
    // NIP-02: at least one `p` tag (the cold-start follow seed), every value a
    // well-formed 64-hex pubkey, and no tag keys besides `p`.
    let p_values = values_for_key(&event, "p");
    assert!(
        !p_values.is_empty(),
        "cold-start kind:3 must carry the app-supplied initial_follows `p` tags"
    );
    for pubkey in &p_values {
        assert!(
            is_hex64(pubkey),
            "every NIP-02 `p` value must be a 64-hex pubkey, got: {pubkey:?}"
        );
    }
    assert_only_keys(&event, &["p"], "cold-start NIP-02 contact list");
    // No D6 toast — the cold-start publish had targets and succeeded.
    assert_eq!(
        h.last_error_toast(),
        None,
        "cold-start kind:3 publish must not surface an error toast"
    );
}
