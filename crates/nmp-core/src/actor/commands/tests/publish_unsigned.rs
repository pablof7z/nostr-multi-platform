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
