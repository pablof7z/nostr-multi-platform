//! Tests for `publish_unsigned_event` — valid publish, kind-range validation,
//! and tag-parse validation.
//!
//! Findings 1 + 2 (codex batch review e895c09):
//! - Finding 1 (HIGH): `unsigned.kind as u16` silently truncates out-of-range
//!   kinds (e.g. 65559 → 23). Fix: validate range in `sign_with` and return
//!   `Err` so the caller surfaces a D6 toast. No publish must happen.
//! - Finding 2 (MEDIUM): `filter_map(|t| Tag::parse(t).ok())` silently drops
//!   malformed tags. Fix: count failures and hard-fail with a D6 toast listing
//!   the count. Valid tags must still pass through unchanged.

use super::*;

#[test]
fn publish_unsigned_event_without_account_toasts_and_no_outbound() {
    let (id, mut kernel) = fresh();
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(), // ignored by signer; irrelevant when no account
        kind: 30023,
        tags: vec![vec!["d".into(), "x".into()]],
        content: "body".into(),
        created_at: 0,
    };
    let outbound = publish_unsigned_event(
        &id,
        &mut kernel,
        unsigned,
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(outbound.is_empty());
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("no active account")));
}

#[test]
fn publish_unsigned_event_signs_and_publishes_arbitrary_kind() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let active_pubkey = id.active_pubkey().unwrap();
    // Construct a generic kind:30023 (NIP-23 article) UnsignedEvent inline —
    // no per-kind kernel logic; the kernel just signs + publishes.
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: "ignored-by-signer".into(),
        kind: 30023,
        tags: vec![
            vec!["d".into(), "test-article".into()],
            vec!["title".into(), "Hello".into()],
        ],
        content: "# body".into(),
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
    assert!(!outbound.is_empty());
    assert!(outbound[0].text.contains("\"kind\":30023"));
    assert!(outbound[0]
        .text
        .contains(&format!("\"pubkey\":\"{active_pubkey}\"")));
    assert!(!outbound[0].text.contains("ignored-by-signer"));
    assert!(outbound[0].text.contains("\"d\""));
    assert!(outbound[0].text.contains("test-article"));
    let q = kernel.publish_queue_snapshot();
    assert_eq!(q.last().unwrap().kind, 30023);
    assert_eq!(q.last().unwrap().status, "accepted_locally");
}

#[test]
fn publish_unsigned_event_rejects_oversized_kind_with_toast() {
    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    // kind 100_000 is above u16::MAX (65_535) — previously it would silently
    // truncate to kind:34_464 (100_000 mod 65_536); now it must be rejected.
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 100_000,
        tags: vec![],
        content: "should not publish".into(),
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
        "oversized kind must produce no outbound frames"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("invalid kind") && t.contains("100000")),
        "expected toast about invalid kind, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "oversized kind must not appear in the publish queue"
    );
}

#[test]
fn publish_unsigned_event_valid_kind_publishes_normally() {
    // Regression for Finding 1: a valid u32 kind within [0, 65535] must still
    // publish exactly as before.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 1,
        tags: vec![],
        content: "valid kind".into(),
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
        "valid kind:1 must produce outbound frames"
    );
    assert_eq!(kernel.last_error_toast_snapshot(), None);
    let q = kernel.publish_queue_snapshot();
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].kind, 1);
}

#[test]
fn publish_unsigned_event_rejects_malformed_tag_with_toast() {
    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    // An empty vec[] is rejected by Tag::parse (tag slice must be non-empty).
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 1,
        tags: vec![vec![]], // malformed: empty tag row
        content: "tag test".into(),
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
        "malformed tag must produce no outbound frames"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("malformed tag")),
        "expected toast about malformed tag, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "malformed tag must not appear in the publish queue"
    );
}

#[test]
fn publish_unsigned_event_valid_tags_pass_through() {
    // Regression for Finding 2: all-valid tags must still appear in the
    // signed event unchanged.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 30023,
        tags: vec![
            vec!["d".into(), "test-slug".into()],
            vec!["title".into(), "Hello".into()],
        ],
        content: "body".into(),
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
    assert!(!outbound.is_empty());
    assert_eq!(kernel.last_error_toast_snapshot(), None);
    assert!(outbound[0].text.contains("\"d\""));
    assert!(outbound[0].text.contains("test-slug"));
    assert!(outbound[0].text.contains("\"title\""));
}

/// Flow B (auto arm): when the kernel carries app-declared
/// `outbound_public_tags`, a PublicRoutable publish (kind:1 note) gets the tag
/// appended to the SIGNED event. Proves `finalize_outbound_tags` is wired into
/// the Auto (NIP-65 outbox) publish arm, not just the helper unit.
#[test]
fn auto_arm_appends_client_tag_on_public_note() {
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
    let outbound = publish_unsigned_event(
        &id,
        &mut kernel,
        unsigned,
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(!outbound.is_empty());
    // The client tag is in the signed payload (so it is covered by the sig).
    assert!(
        outbound[0].text.contains("[\"client\",\"Chirp\"]"),
        "kind:1 signed event must carry the NIP-89 client tag, got: {}",
        outbound[0].text
    );
}

#[test]
fn auto_arm_finalizes_before_parking_remote_sign() {
    let (mut id, mut kernel) = fresh();
    let signer = PendingCaptureRemoteSigner::new(&"a".repeat(64));
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
        content: "a parked public note".into(),
        created_at: 0,
    };
    let mut parked_ops = crate::actor::pending_sign::ParkedSignerOps::new();

    let outbound = publish_unsigned_event(
        &id,
        &mut kernel,
        unsigned,
        Some("auto-parked-cid".to_string()),
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
    assert!(matches!(target, PublishTarget::Auto));
    assert_eq!(correlation_id_override.as_deref(), Some("auto-parked-cid"));
}

/// Flow B negative (auto arm): with NO app-declared tags, a kind:1 note carries
/// no client tag — proves the tag is kernel-driven, never spuriously injected.
#[test]
fn auto_arm_no_client_tag_when_kernel_has_none() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 1,
        tags: vec![],
        content: "a public note".into(),
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
    assert!(!outbound.is_empty());
    assert!(
        !outbound[0].text.contains("client"),
        "no app-declared tags → no client tag, got: {}",
        outbound[0].text
    );
}
