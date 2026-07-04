//! Tests for `publish_unsigned_event_to_relays` — sign + EXPLICIT relay pin.
//!
//! The host-pinned twin of `publish_unsigned_event`: it SIGNS with the active
//! account (unlike `publish_signed_event` which carries an already-signed
//! event) and ROUTES to an explicit relay set (unlike `publish_unsigned_event`
//! which routes via the NIP-65 outbox). This is the path a protocol-owned
//! pinned publish action needs: the event must reach the requested relay, not
//! the author's kind:10002 outbox.

use super::*;

#[test]
fn publish_unsigned_event_to_relays_signs_and_routes_to_exactly_those() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let active_pubkey = id.active_pubkey().unwrap();

    // `pubkey` is a placeholder; the signer derives it from the active identity.
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 1,
        tags: vec![],
        content: "hello".into(),
        created_at: 1_700_000_000,
    };
    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        None,
        relays.clone(),
        PublishRouteClass::GroupHostPin,
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
    assert!(outbound[0].text.contains("\"kind\":1"));
    assert_eq!(kernel.publish_queue_snapshot().last().unwrap().kind, 1);
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
        kind: 1,
        tags: vec![],
        content: String::new(),
        created_at: 1_700_000_000,
    };
    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        None,
        relays,
        PublishRouteClass::GroupHostPin,
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
        kind: 1,
        tags: vec![],
        content: String::new(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        None,
        Vec::new(),
        PublishRouteClass::GroupHostPin,
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
        kind: 1,
        tags: vec![],
        content: String::new(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        None,
        vec!["https://not-a-nostr-relay.example".to_string()],
        PublishRouteClass::GroupHostPin,
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
        None,
        relays,
        PublishRouteClass::ManualOverride,
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

#[test]
fn explicit_arm_finalizes_before_parking_remote_sign() {
    let (mut id, mut kernel) = fresh();
    // Fixed clock (#2962): see the identical race + fix rationale on
    // `auto_arm_finalizes_before_parking_remote_sign` in publish_unsigned.rs —
    // this test has the same two-live-clock-reads assertion at :260.
    kernel.set_clock(std::sync::Arc::new(crate::kernel::clock::FixedClock(
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
    )));
    let signer = PendingCaptureRemoteSigner::new(&"b".repeat(64));
    let captured = signer.captured_handle();
    add_signer(
        &mut id,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(signer)),
        true,
        false,
    );
    kernel.set_outbound_public_tags(vec![vec!["client".into(), "Chirp".into()]]);
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 1,
        tags: vec![],
        content: "a parked explicit note".into(),
        created_at: 0,
    };
    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let mut parked_ops = crate::actor::pending_sign::ParkedSignerOps::new();

    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        None,
        relays.clone(),
        PublishRouteClass::ManualOverride,
        Some("explicit-parked-cid".to_string()),
        None,
        &mut parked_ops,
    );

    assert!(
        outbound.is_empty(),
        "pending remote sign must not publish yet"
    );
    assert_eq!(parked_ops.len(), 1, "remote sign must be parked");
    let captured = captured.lock().expect("capture mutex");
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].created_at, kernel.now_secs());
    assert!(
        captured[0]
            .tags
            .contains(&vec!["client".to_string(), "Chirp".to_string()]),
        "remote signer must receive the finalized unsigned event before parking"
    );

    let parked = parked_ops.into_vec();
    let crate::actor::pending_sign::ParkedOpSink::Publish {
        target,
        correlation_id_override,
        ..
    } = &parked[0].sink
    else {
        panic!("expected parked publish sink");
    };
    let PublishTarget::Explicit {
        relays: target_relays,
        route_class,
    } = target
    else {
        panic!("expected explicit parked target");
    };
    let mut got = target_relays.clone();
    got.sort();
    let mut want = relays;
    want.sort();
    assert_eq!(got, want);
    assert_eq!(*route_class, PublishRouteClass::ManualOverride);
    assert_eq!(
        correlation_id_override.as_deref(),
        Some("explicit-parked-cid")
    );
}
