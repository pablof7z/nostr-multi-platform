//! Tests for the `resolved_profiles` snapshot projection.
//!
//! V-112 (ADR-0042): `author_view` / `thread_view` deleted from Kernel.
//! `mention_profiles()` now returns an empty map (its sources — open author
//! and thread view items — were deleted). `resolved_profiles` is built
//! exclusively from `claimed_profiles` (tier 1).
//!
//! Precedence under test (highest → lowest):
//!   1. `claimed_profiles` — full `ProfileCard` (carries `nip05`/`about`/`lnurl`)
//!   2. (was: `author_view.profile`) — DELETED in V-112
//!   3. (was: `mention_profiles`) — always empty since V-112

use super::nostr::NostrEvent;
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// Deliver a kind:0 profile carrying real metadata by calling `ingest_profile`
/// directly. `parse_profile` JSON-decodes only the `content` field; the ingest
/// method runs post-verification and never reads the signature.
fn ingest_profile_with(
    kernel: &mut Kernel,
    pubkey: &str,
    created_at: u64,
    display_name: &str,
    nip05: &str,
) {
    let content = serde_json::json!({
        "display_name": display_name,
        "nip05": nip05,
        "picture": "https://example.com/avatar.png",
    })
    .to_string();
    kernel.ingest_profile(NostrEvent {
        id: "0".repeat(64),
        pubkey: pubkey.to_string(),
        created_at,
        kind: 0,
        tags: Vec::new(),
        content,
        sig: String::new(),
    });
}

/// 1. Empty case — a fresh kernel with no claims emits `resolved_profiles` as a
/// present-but-empty object (D1: never absent).
#[test]
fn resolved_profiles_present_and_empty_on_fresh_kernel() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let snapshot = kernel.make_update_value_for_test(true);
    let resolved = &snapshot["projections"]["resolved_profiles"];
    assert!(
        resolved.is_object(),
        "resolved_profiles must always be present as an object (D1) — got {resolved:?}"
    );
    assert_eq!(
        resolved.as_object().map(serde_json::Map::len),
        Some(0),
        "resolved_profiles must be empty `{{}}` on a fresh kernel"
    );
}

/// 2. `claimed_profiles` fills `resolved_profiles` — a claimed pubkey with a
/// kind:0 profile appears in `resolved_profiles` carrying the full ProfileCard
/// with its `nip05` value.
#[test]
fn claimed_profiles_fills_resolved_profiles() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys = ::nostr::Keys::generate();
    let pk = keys.public_key().to_hex();

    ingest_profile_with(
        &mut kernel,
        &pk,
        1_000,
        "Claimed User",
        "claimed@nip05.example",
    );
    let _ = kernel.claim_profile(pk.clone(), "view-0".to_string(), true, false);

    let snapshot = kernel.make_update_value_for_test(true);

    assert!(
        snapshot["projections"]["claimed_profiles"][&pk].is_object(),
        "precondition: pk must be in claimed_profiles"
    );

    let entry = &snapshot["projections"]["resolved_profiles"][&pk];
    assert!(entry.is_object(), "resolved_profiles[pk] must be present");
    assert_eq!(
        entry["nip05"], "claimed@nip05.example",
        "resolved entry must carry the claimed card's nip05"
    );
    assert_eq!(
        entry["display_name"], "Claimed User",
        "resolved entry must carry the claimed card's display_name"
    );
}

/// 3. `mention_profiles` is always empty since V-112 (ADR-0042) — a pubkey that
/// is not claimed does NOT appear in `resolved_profiles` even if it authors a
/// note that was ingested.
#[test]
fn unclaimed_pubkey_absent_from_resolved_profiles() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys = ::nostr::Keys::generate();
    let pk = keys.public_key().to_hex();

    // Ingest a kind:0 profile (populates kernel.profiles cache) but do NOT claim.
    ingest_profile_with(&mut kernel, &pk, 1_000, "Unknown User", "");

    let snapshot = kernel.make_update_value_for_test(true);

    // V-112: mention_profiles is always empty — no view-item sources remain.
    assert_eq!(
        snapshot["projections"]["mention_profiles"]
            .as_object()
            .map(|m| m.len()),
        Some(0),
        "mention_profiles must be empty after V-112"
    );

    // Unclaimed pubkey cannot reach resolved_profiles anymore.
    assert!(
        snapshot["projections"]["resolved_profiles"][&pk].is_null(),
        "unclaimed pubkey must NOT appear in resolved_profiles after V-112"
    );
}

/// 4. Multiple claimed profiles all appear in `resolved_profiles`.
#[test]
fn multiple_claimed_profiles_all_appear_in_resolved_profiles() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys_a = ::nostr::Keys::generate();
    let pk_a = keys_a.public_key().to_hex();
    let keys_b = ::nostr::Keys::generate();
    let pk_b = keys_b.public_key().to_hex();

    ingest_profile_with(&mut kernel, &pk_a, 1_000, "User A", "a@example.com");
    ingest_profile_with(&mut kernel, &pk_b, 1_001, "User B", "b@example.com");
    let _ = kernel.claim_profile(pk_a.clone(), "view-a".to_string(), true, false);
    let _ = kernel.claim_profile(pk_b.clone(), "view-b".to_string(), true, false);

    let snapshot = kernel.make_update_value_for_test(true);

    let entry_a = &snapshot["projections"]["resolved_profiles"][&pk_a];
    let entry_b = &snapshot["projections"]["resolved_profiles"][&pk_b];
    assert!(entry_a.is_object(), "resolved_profiles must carry pk_a");
    assert!(entry_b.is_object(), "resolved_profiles must carry pk_b");
    assert_eq!(entry_a["nip05"], "a@example.com");
    assert_eq!(entry_b["nip05"], "b@example.com");
}
