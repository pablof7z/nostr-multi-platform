//! Tests for `publish_unsigned_event_to_relays` — sign + EXPLICIT relay pin.
//!
//! The host-pinned twin of `publish_unsigned_event`: it SIGNS with the active
//! account (unlike `publish_signed_event` which carries an already-signed
//! event) and ROUTES to an explicit relay set (unlike `publish_unsigned_event`
//! which routes via the NIP-65 outbox). This is the path a NIP-29 group action
//! needs — a join request must reach the group's host relay, not the author's
//! kind:10002 outbox.

use super::*;

#[test]
fn publish_unsigned_event_to_relays_signs_and_routes_to_exactly_those() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let active_pubkey = id.active_pubkey().unwrap();

    // A kind:9021 NIP-29 join-request-shaped unsigned event. `pubkey` is a
    // placeholder — the signer derives it from the active identity.
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 9021,
        tags: vec![vec!["h".into(), "rust-nostr".into()]],
        content: "hello".into(),
        created_at: 1_700_000_000,
    };
    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        relays.clone(),
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );

    assert!(!outbound.is_empty(), "host-pinned publish must route");
    assert_eq!(kernel.last_error_toast_snapshot(), None);

    // The relay set is EXACTLY the explicit pin — and contains none of the
    // author's kind:10002 outbox. This distinguishes the Explicit route from
    // a silent fall-through to the NIP-65 outbox resolver.
    let mut got: Vec<String> = outbound.iter().map(|m| m.relay_url.clone()).collect();
    got.sort();
    let mut want = relays.clone();
    want.sort();
    assert_eq!(got, want, "must dispatch to exactly the pinned relays");
    for url in TEST_WRITE_RELAYS {
        assert!(
            !got.iter().any(|g| g == url),
            "host-pinned publish must NOT leak to the kind:10002 outbox relay {url}"
        );
    }

    // The event was signed by the active account: its pubkey is on the wire
    // frame even though the caller passed an empty `pubkey`.
    assert!(outbound[0]
        .text
        .contains(&format!("\"pubkey\":\"{active_pubkey}\"")));
    assert!(outbound[0].text.contains("\"kind\":9021"));
    assert_eq!(kernel.publish_queue_snapshot().last().unwrap().kind, 9021);
}

#[test]
fn publish_unsigned_event_to_relays_without_account_toasts() {
    // Unlike `publish_signed_event` (signature already exists, no account
    // needed), this path SIGNS — so a missing active account is surfaced as a
    // toast (D6), never a panic, and produces no outbound frames.
    let (id, mut kernel) = fresh();
    assert!(id.active_pubkey().is_none());

    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 9021,
        tags: vec![vec!["h".into(), "rust-nostr".into()]],
        content: String::new(),
        created_at: 1_700_000_000,
    };
    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        relays,
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );

    assert!(
        outbound.is_empty(),
        "no active account must produce no outbound frames"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("no active account")),
        "expected a no-account toast, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
}

#[test]
fn publish_unsigned_event_to_relays_empty_relays_fails_closed() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);

    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 9021,
        tags: vec![vec!["h".into(), "rust-nostr".into()]],
        content: String::new(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        Vec::new(),
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );

    assert!(
        outbound.is_empty(),
        "empty explicit relays must not publish"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("explicit publish target rejected")),
        "expected explicit-target rejection toast, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(kernel.publish_queue_snapshot().is_empty());
}

#[test]
fn publish_unsigned_event_to_relays_invalid_relay_fails_closed() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);

    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 9021,
        tags: vec![vec!["h".into(), "rust-nostr".into()]],
        content: String::new(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        vec!["https://not-a-nostr-relay.example".to_string()],
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );

    assert!(
        outbound.is_empty(),
        "invalid explicit relay must not publish"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("ws:// or wss://")),
        "expected malformed relay rejection toast, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
}

/// Flow B (explicit arm): the app-declared `outbound_public_tags` reach the
/// SIGNED event on the EXPLICIT-relay publish path too — proving the NIP-89
/// client tag is appended from BOTH publish arms via the single
/// `finalize_outbound_tags` decision site (D11 one-door), not just the Auto arm.
#[test]
fn explicit_arm_appends_client_tag_on_public_note() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    kernel.set_outbound_public_tags(vec![vec!["client".into(), "Chirp".into()]]);
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 1,
        tags: vec![],
        content: "a public note".into(),
        created_at: 1_700_000_000,
    };
    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        relays,
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(!outbound.is_empty(), "explicit-pin publish must route");
    assert!(
        outbound[0].text.contains("[\"client\",\"Chirp\"]"),
        "explicit-arm kind:1 signed event must carry the NIP-89 client tag, got: {}",
        outbound[0].text
    );
}
