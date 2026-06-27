//! Tests for follow/unfollow, relay CRUD + URL normalization, profile
//! metadata updates, bunker sign-in, and the relay+profile projection shape.

use super::*;

// ── follow: unfollow / idempotency / account / pubkey-validation gaps ───────
//
// `follow_publishes_kind3_with_p_tag` above covers only the first add against
// an empty contact list. These pin the rest of `publish::follow`: removal
// from an existing kind:3, idempotent re-add (no duplicate `p` tag), the
// no-account D6 toast for both add and remove, and the malformed-pubkey toast.

#[test]
fn unfollow_removes_pubkey_from_contact_list() {
    // Seed a kind:3 that already follows two pubkeys, then unfollow one.
    // The re-published kind:3 must drop exactly that pubkey and keep the other.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let author = id.active_pubkey().unwrap();
    let keep = "c".repeat(64);
    let drop = "d".repeat(64);
    seed_contact_list(&mut kernel, &author, &[&keep, &drop]);

    let outbound = follow(
        &id,
        &mut kernel,
        &drop,
        false,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(!outbound.is_empty(), "unfollow must re-publish the kind:3");
    let event = last_published_event_json(&outbound);
    assert_eq!(event["kind"], 3);
    let p_pubkeys: Vec<String> = tags_of(&event)
        .into_iter()
        .filter(|t| t.first().map(String::as_str) == Some("p"))
        .filter_map(|t| t.get(1).cloned())
        .collect();
    assert!(
        p_pubkeys.contains(&keep),
        "unfollowed list must still contain the kept pubkey"
    );
    assert!(
        !p_pubkeys.contains(&drop),
        "unfollowed pubkey must be gone from the contact list"
    );
    assert_eq!(p_pubkeys.len(), 1, "exactly one follow must remain");
}

#[test]
fn unfollow_same_second_baseline_stamps_strict_replacement() {
    let (mut id, mut kernel) = fresh();
    kernel.set_clock(std::sync::Arc::new(crate::kernel::clock::FixedClock(
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
    )));
    sign_in_with_nip65(&mut id, &mut kernel);
    let author = id.active_pubkey().unwrap();
    let keep = "c".repeat(64);
    let drop = "d".repeat(64);
    kernel.inject_replaceable_event(
        &"3".repeat(64),
        &author,
        1_700_000_000,
        3,
        vec![
            vec!["p".to_string(), keep.clone()],
            vec!["p".to_string(), drop.clone()],
        ],
        "wss://seed-relay.test",
        1,
    );

    let outbound = follow(
        &id,
        &mut kernel,
        &drop,
        false,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );

    assert!(!outbound.is_empty(), "unfollow must re-publish the kind:3");
    let event = last_published_event_json(&outbound);
    assert_eq!(
        event["created_at"].as_u64(),
        Some(1_700_000_001),
        "same-second contact edits must stamp after the baseline they replace"
    );
}

#[test]
fn follow_already_followed_is_idempotent_no_duplicate() {
    // Re-following a pubkey already in the kind:3 must not append a duplicate
    // `p` tag (publish.rs:308-311 — the `!any(|p| p == pubkey)` guard).
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let author = id.active_pubkey().unwrap();
    let already = "e".repeat(64);
    seed_contact_list(&mut kernel, &author, &[&already]);

    let outbound = follow(
        &id,
        &mut kernel,
        &already,
        true,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(!outbound.is_empty(), "follow must re-publish the kind:3");
    let event = last_published_event_json(&outbound);
    let p_pubkeys: Vec<String> = tags_of(&event)
        .into_iter()
        .filter(|t| t.first().map(String::as_str) == Some("p"))
        .filter_map(|t| t.get(1).cloned())
        .collect();
    assert_eq!(
        p_pubkeys,
        vec![already],
        "re-following an existing pubkey must not duplicate the `p` tag"
    );
}

#[test]
fn follow_without_account_toasts_and_no_outbound() {
    // D6: follow with no active account → toast naming the `follow` action.
    let (id, mut kernel) = fresh();
    let target = "f".repeat(64);
    let outbound = follow(
        &id,
        &mut kernel,
        &target,
        true,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(
        outbound.is_empty(),
        "follow with no active account must produce no outbound frames"
    );
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("follow") && t.contains("no active account")));
}

#[test]
fn unfollow_without_account_toasts_with_unfollow_action() {
    // D6: the no-account toast distinguishes add (`follow`) from remove
    // (`unfollow`) — publish.rs:301 picks the action string off `add`.
    let (id, mut kernel) = fresh();
    let target = "f".repeat(64);
    let outbound = follow(
        &id,
        &mut kernel,
        &target,
        false,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(outbound.is_empty());
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("unfollow") && t.contains("no active account")));
}

#[test]
fn follow_malformed_pubkey_toasts_and_refuses() {
    // The follow target must be a 64-hex pubkey. A malformed value is a
    // user-visible error (D6 toast), not a silent no-op — and must not panic.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let outbound = follow(
        &id,
        &mut kernel,
        "xyz",
        true,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(
        outbound.is_empty(),
        "follow with a malformed pubkey must produce no outbound frames"
    );
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("follow") && t.contains("64-hex")));
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "follow with a malformed pubkey must not enqueue a publish"
    );
}

// ── profile update (kind:0 metadata) via the generic publish path ──────────
//
// There is no dedicated profile-update command handler; profile metadata
// updates flow through `publish_unsigned_event` as a generic kind:0 event.

#[test]
fn profile_update_publishes_kind0_metadata_event() {
    // Updating a display name builds a kind:0 metadata event whose JSON
    // content carries the new profile fields; the signer overwrites the
    // pubkey with the active identity's key.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let active_pubkey = id.active_pubkey().unwrap();
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: "ignored-by-signer".into(),
        kind: 0,
        tags: Vec::new(),
        content: r#"{"name":"marcus","display_name":"Marcus Webb"}"#.into(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event(
        &id,
        &mut kernel,
        unsigned,
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(
        !outbound.is_empty(),
        "kind:0 update must produce an EVENT frame"
    );
    let event = last_published_event_json(&outbound);
    assert_eq!(event["kind"], 0, "profile metadata must be kind:0");
    assert_eq!(
        event["pubkey"], active_pubkey,
        "signer must stamp the active identity's pubkey, not the caller's"
    );
    assert!(
        event["content"]
            .as_str()
            .is_some_and(|c| c.contains("Marcus Webb")),
        "kind:0 content must carry the updated display name"
    );
    assert_eq!(kernel.publish_queue_snapshot().last().unwrap().kind, 0);
}

#[test]
fn publish_profile_merges_edits_onto_cached_kind0_fields() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let active_pubkey = id.active_pubkey().unwrap();
    kernel.seed_profile_kind0_for_test(
        &active_pubkey,
        "kind0-current",
        1_700_000_000,
        r#"{"name":"marcus","display_name":"Marcus Webb","banner":"https://example.com/banner.png","website":"https://example.com","third_party":{"keep":true}}"#,
    );
    let mut fields = serde_json::Map::new();
    fields.insert(
        "display_name".to_string(),
        serde_json::Value::String("Marcus Updated".to_string()),
    );

    let outbound = publish_profile(
        &id,
        &mut kernel,
        fields,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(
        !outbound.is_empty(),
        "PublishProfile must produce an EVENT frame"
    );

    let event = last_published_event_json(&outbound);
    assert_eq!(event["kind"], 0);
    let content: serde_json::Value =
        serde_json::from_str(event["content"].as_str().expect("content string")).unwrap();
    assert_eq!(content["display_name"], "Marcus Updated");
    assert_eq!(content["name"], "marcus");
    assert_eq!(content["banner"], "https://example.com/banner.png");
    assert_eq!(content["website"], "https://example.com");
    assert_eq!(content["third_party"], serde_json::json!({"keep": true}));
}

#[test]
fn profile_update_without_account_toasts_and_no_outbound() {
    // D6: a kind:0 metadata update with no active account is a toast, never
    // an exception — the generic publish path can't sign without an identity.
    let (id, mut kernel) = fresh();
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: "ignored".into(),
        kind: 0,
        tags: Vec::new(),
        content: r#"{"display_name":"Nobody"}"#.into(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event(
        &id,
        &mut kernel,
        unsigned,
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(
        outbound.is_empty(),
        "profile update with no active account must produce no outbound frames"
    );
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("publish") && t.contains("no active account")));
}

// ── relay CRUD ───────────────────────────────────────────────────────────────

#[test]
fn add_and_remove_relay_edits_projection() {
    let (_id, mut kernel) = fresh();
    // T158: add_relay returns Some(url) on success, None on failure.
    let result = add_relay(&mut kernel, "wss://relay.damus.io", "both");
    assert_eq!(result, Some("wss://relay.damus.io".to_string()));
    let result2 = add_relay(&mut kernel, "wss://nos.lol", "write");
    assert_eq!(result2, Some("wss://nos.lol".to_string()));
    assert_eq!(kernel.configured_relays_snapshot().len(), 2);
    // Invalid URL scheme — returns None and sets a toast.
    let bad = add_relay(&mut kernel, "http://bad", "read");
    assert_eq!(bad, None);
    assert_eq!(kernel.configured_relays_snapshot().len(), 2);
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("invalid relay URL")));
    // Invalid role — returns None.
    let bad_role = add_relay(&mut kernel, "wss://nos.lol", "superwrite");
    assert_eq!(bad_role, None);
    remove_relay(&mut kernel, "wss://nos.lol");
    assert_eq!(kernel.configured_relays_snapshot().len(), 1);
    assert_eq!(
        kernel.configured_relays_snapshot()[0].url,
        "wss://relay.damus.io"
    );
}

// ── T-relay-url-normalize — add_relay canonicalization ───────────────────────

/// T-normalize-cmd-1: `add_relay` with uppercase + trailing slash must return
/// the canonical (lowercased, slash-stripped) URL.
#[test]
fn add_relay_canonicalizes_url() {
    let (_id, mut kernel) = fresh();
    let result = add_relay(&mut kernel, "WSS://Relay.Damus.IO/", "both");
    assert_eq!(
        result,
        Some("wss://relay.damus.io".to_string()),
        "T-normalize-cmd-1: add_relay must return canonical URL (lowercase scheme+host, no empty-path slash)"
    );
    let rows = kernel.configured_relays_snapshot();
    assert_eq!(rows.len(), 1, "exactly one row added");
    assert_eq!(
        rows[0].url, "wss://relay.damus.io",
        "AppRelay must store the canonical URL"
    );
}

/// T-normalize-cmd-2: adding the same relay via two URL-equivalent forms must
/// dedup to a single `AppRelay` (not two rows).
#[test]
fn add_relay_case_slash_variants_dedup_to_one_row() {
    let (_id, mut kernel) = fresh();
    let r1 = add_relay(&mut kernel, "WSS://R.Ex/", "both");
    let r2 = add_relay(&mut kernel, "wss://r.ex", "read");
    assert!(r1.is_some(), "first add must succeed");
    assert!(r2.is_some(), "second add must succeed (role update)");
    let rows = kernel.configured_relays_snapshot();
    assert_eq!(
        rows.len(),
        1,
        "T-normalize-cmd-2: URL-equivalent adds must dedup to one AppRelay, got {:?}",
        rows
    );
    assert_eq!(rows[0].url, "wss://r.ex");
    assert_eq!(rows[0].role, "read", "second add must update the role");
}

/// T-normalize-cmd-3: `remove_relay` with a URL-variant that differs from the
/// add form (trailing slash vs not) must still remove the row.
#[test]
fn remove_relay_canonical_matches_add_form() {
    let (_id, mut kernel) = fresh();
    add_relay(&mut kernel, "wss://r.ex", "both");
    assert_eq!(
        kernel.configured_relays_snapshot().len(),
        1,
        "row must exist after add"
    );
    // Remove with trailing slash (different bytes, same canonical form).
    remove_relay(&mut kernel, "wss://r.ex/");
    assert_eq!(
        kernel.configured_relays_snapshot().len(),
        0,
        "T-normalize-cmd-3: remove_relay with trailing-slash variant must remove the row"
    );
}

// ── bunker sign-in ────────────────────────────────────────────────────────────

#[test]
fn sign_in_bunker_seeds_handshake_progress() {
    // Stage 3 of NIP-46 wiring: a shape-valid bunker:// URI seeds the
    // snapshot with `"connecting"` so the SwiftUI sign-in flow can render
    // progress immediately. The broker (Stage 4) drives the real handshake
    // and pushes subsequent progress via `BunkerHandshakeProgress`.
    //
    // Stage 4 also added a fallback: if no broker hook is installed, the
    // actor clears the seeded "connecting" stage and surfaces a toast.
    // ADR-0052 §D3 — install a no-op hook into THIS runtime's per-app slot so
    // the test exercises the happy path (no process-global state).
    use std::sync::Arc;

    let (mut id, mut kernel) = fresh();
    id.install_bunker_hook_for_test(Arc::new(|_req| {}));
    let pk = "c".repeat(64);
    sign_in_bunker(
        &mut id,
        &mut kernel,
        &format!("bunker://{pk}?relay=wss://r.example"),
    );
    // D0: handshake state is an app noun — it is written to the identity
    // runtime's shared slot (read by the `"bunker_handshake"` projection),
    // not a typed kernel field.
    let handshake = id.bunker_handshake_for_test().expect("handshake seeded");
    assert_eq!(handshake.stage, "connecting");
    assert!(handshake.message.is_some());
    // No toast on the happy path — the seeded progress is the UX signal.
    assert!(kernel.last_error_toast_snapshot().is_none());
}

#[test]
fn sign_in_bunker_rejects_malformed_uri() {
    let (mut id, mut kernel) = fresh();
    sign_in_bunker(&mut id, &mut kernel, "bunker://nope");
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("invalid bunker")));
}

#[test]
fn sign_in_bunker_without_broker_clears_progress_and_toasts() {
    // Stage 4: if no broker hook is installed when a URI arrives, the actor
    // clears the seeded "connecting" stage and surfaces a toast so the user
    // knows the bunker subsystem is missing. In normal flow the broker installs
    // its hook at startup, before any URI can be submitted.
    //
    // ADR-0052 §D3 — the hook is now a PER-APP slot (no process-global), so
    // this test can exercise the *no-hook* path deterministically: a fresh
    // `IdentityRuntime` starts with an empty bunker hook slot. (The old global
    // design could not — its `OnceLock` stayed fired from a sibling test.)
    // Deliberately install NO hook.
    let (mut id, mut kernel) = fresh();
    let pk = "d".repeat(64);
    sign_in_bunker(
        &mut id,
        &mut kernel,
        &format!("bunker://{pk}?relay=wss://r.example"),
    );
    // No broker installed → the seeded "connecting" stage is cleared and a
    // toast naming the missing init call is surfaced (D6: error becomes state).
    assert!(
        id.bunker_handshake_for_test().is_none(),
        "no-hook path must clear the seeded handshake progress"
    );
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("broker not initialised")));
}
