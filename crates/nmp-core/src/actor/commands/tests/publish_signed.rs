//! Tests for `publish_signed_event` — verbatim routing, explicit relay
//! targeting, and tamper rejection.
//!
//! The D10 kind:1059 guard tests live in the sibling `publish_signed_d10`
//! module to keep this file under 500 LOC.
//!
//! The decisive difference from the unsigned sibling: the signer is NEVER
//! consulted. Tests produce genuine signed events via
//! `sign_active_nonblocking` (real Schnorr sig over TEST_NSEC's keys),
//! serialize them to flat NIP-01 JSON, and feed the signed path.

use super::*;

// ── basic publish_signed_event ────────────────────────────────────────────────

#[test]
fn flat_nip01_json_round_trips_into_raw_event() {
    // Lock in the RawEvent serde shape == the flat NIP-01 event object the
    // FFI contract advertises (field-name based, not order based).
    let literal = r#"{"id":"aa","pubkey":"bb","created_at":1700000000,
        "kind":30023,"tags":[["d","x"]],"content":"hi","sig":"cc"}"#;
    let raw: crate::store::RawEvent =
        serde_json::from_str(literal).expect("flat NIP-01 → RawEvent");
    assert_eq!(raw.id, "aa");
    assert_eq!(raw.pubkey, "bb");
    assert_eq!(raw.created_at, 1_700_000_000);
    assert_eq!(raw.kind, 30023);
    assert_eq!(raw.content, "hi");
    assert_eq!(raw.sig, "cc");
}

#[test]
fn publish_signed_event_routes_and_dispatches_verbatim() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let active_pubkey = id.active_pubkey().unwrap();
    let (json, ev_id, ev_sig) = signed_nip01_json(&id, "# signed body");

    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::explicit(
            TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect(),
            PublishRouteClass::ImportedOrPresigned,
        ),
        None,
    );

    assert!(!outbound.is_empty(), "valid signed event must route");
    assert_eq!(kernel.last_error_toast_snapshot(), None);
    // Verbatim: the exact id + sig bytes from the input appear on the wire
    // frame unchanged (no re-signing).
    assert!(
        outbound[0].text.contains(&format!("\"id\":\"{ev_id}\"")),
        "event id must be carried through verbatim"
    );
    assert!(
        outbound[0].text.contains(&format!("\"sig\":\"{ev_sig}\"")),
        "signature must be carried through verbatim — never re-signed"
    );
    assert!(outbound[0]
        .text
        .contains(&format!("\"pubkey\":\"{active_pubkey}\"")));
    assert!(outbound[0].text.contains("\"kind\":30023"));
    let q = kernel.publish_queue_snapshot();
    assert_eq!(q.last().unwrap().kind, 30023);
    assert_eq!(q.last().unwrap().status, "accepted_locally");
}

#[test]
fn publish_signed_event_publishes_without_active_account() {
    // Behavioral asymmetry vs. the unsigned sibling: the signature already
    // exists, routing keys off the event's own pubkey (its kind:10002), so
    // NO active account is required. Sign the event under a throwaway
    // identity, seed THAT pubkey's kind:10002, then publish on a kernel with
    // no active account.
    let (mut signer_id, mut signer_kernel) = fresh();
    sign_in_with_nip65(&mut signer_id, &mut signer_kernel);
    let _author = signer_id.active_pubkey().unwrap();
    let (json, ev_id, _sig) = signed_nip01_json(&signer_id, "no-account body");

    // Fresh kernel: NO account signed in. Externally signed publish is now
    // explicit imported/protocol routing only, so no kind:10002 seed is needed.
    let (no_acct_id, mut kernel) = fresh();
    assert!(no_acct_id.active_pubkey().is_none());

    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::explicit(
            TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect(),
            PublishRouteClass::ImportedOrPresigned,
        ),
        None,
    );

    assert!(
        !outbound.is_empty(),
        "signed event must publish even with no active account"
    );
    assert_eq!(kernel.last_error_toast_snapshot(), None);
    assert!(outbound[0].text.contains(&format!("\"id\":\"{ev_id}\"")));
}

#[test]
fn publish_signed_event_rejects_tampered_signature_with_toast() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, sig) = signed_nip01_json(&id, "tamper me");

    // Flip one hex char of the signature — id stays valid, sig is now forged.
    let flipped = if sig.starts_with('a') { 'b' } else { 'a' };
    let bad_json = json.replacen(&sig, &format!("{flipped}{}", &sig[1..]), 1);
    assert_ne!(bad_json, json, "signature must actually have changed");

    let raw: crate::store::RawEvent = serde_json::from_str(&bad_json).unwrap();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::explicit(
            TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect(),
            PublishRouteClass::ImportedOrPresigned,
        ),
        None,
    );

    assert!(
        outbound.is_empty(),
        "forged-signature event must produce no outbound frames"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("signed event rejected")),
        "expected rejection toast, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "forged event must never enter the publish queue"
    );
}

#[test]
fn publish_signed_event_rejects_id_mismatch_with_toast() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, _sig) = signed_nip01_json(&id, "id mismatch");

    // Mutate content without re-deriving the id → id-hash check must fail.
    let mut raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    raw.content = "tampered-after-signing".into();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::explicit(
            TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect(),
            PublishRouteClass::ImportedOrPresigned,
        ),
        None,
    );

    assert!(outbound.is_empty(), "id-mismatch event must not publish");
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("signed event rejected")));
    assert!(kernel.publish_queue_snapshot().is_empty());
}

// ── publish_signed_event_to — EXPLICIT relay targeting (Marmot D3 opt-out) ──
//
// kind:445 group messages must go to the pinned GROUP relay, kind:1059
// gift-wraps to recipient inbox relays — relays the author's kind:10002
// outbox does NOT cover. The explicit-target path routes the verbatim signed
// event to EXACTLY the named relays, bypassing the NIP-65 resolver, while
// still gating Schnorr+id and never invoking the signer.

#[test]
fn publish_signed_event_to_explicit_relays_routes_verbatim_to_exactly_those() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let active_pubkey = id.active_pubkey().unwrap();
    let (json, ev_id, ev_sig) = signed_nip01_json(&id, "group message body");

    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::explicit(relays.clone(), PublishRouteClass::ImportedOrPresigned),
        None,
    );

    assert!(!outbound.is_empty(), "explicit-target publish must route");
    assert_eq!(kernel.last_error_toast_snapshot(), None);

    // The relay set is EXACTLY the explicit targets — and contains none of
    // the author's kind:10002 outbox. This single assertion is what
    // distinguishes Explicit from a silent Auto fallback.
    let mut got: Vec<String> = outbound.iter().map(|m| m.relay_url.clone()).collect();
    got.sort();
    let mut want = relays.clone();
    want.sort();
    assert_eq!(got, want, "must dispatch to exactly the explicit relays");
    for url in TEST_WRITE_RELAYS {
        assert!(
            !got.iter().any(|g| g == url),
            "explicit target must NOT leak to the kind:10002 outbox relay {url}"
        );
    }

    // Verbatim id/sig/pubkey — the signer was never consulted.
    assert!(outbound[0].text.contains(&format!("\"id\":\"{ev_id}\"")));
    assert!(outbound[0].text.contains(&format!("\"sig\":\"{ev_sig}\"")));
    assert!(outbound[0]
        .text
        .contains(&format!("\"pubkey\":\"{active_pubkey}\"")));
}

#[test]
fn publish_signed_event_to_empty_explicit_relays_fails_closed() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, _sig) = signed_nip01_json(&id, "empty explicit body");

    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::explicit(Vec::new(), PublishRouteClass::ImportedOrPresigned),
        None,
    );

    assert!(
        outbound.is_empty(),
        "empty explicit relays must not publish"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("pre-signed publish target rejected")),
        "expected explicit-target rejection toast, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(kernel.publish_queue_snapshot().is_empty());
}

#[test]
fn publish_signed_event_to_explicit_relays_works_with_no_active_account() {
    // The realistic Marmot case: a kind:445 group message / kind:1059
    // gift-wrap was signed elsewhere (MDK group signer) and must go to a
    // pinned relay while the user is signed-out. The explicit path keys off
    // the verbatim relays — NOT the author's kind:10002 — so no active
    // account is required AND no kind:10002 seed is needed.
    let (mut signer_id, mut signer_kernel) = fresh();
    sign_in_with_nip65(&mut signer_id, &mut signer_kernel);
    let (json, ev_id, ev_sig) = signed_nip01_json(&signer_id, "signed-out group msg");

    // Fresh kernel: NO account signed in, NO kind:10002 seeded for anyone.
    let (no_acct_id, mut kernel) = fresh();
    assert!(no_acct_id.active_pubkey().is_none());

    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::explicit(relays.clone(), PublishRouteClass::ImportedOrPresigned),
        None,
    );

    assert!(
        !outbound.is_empty(),
        "explicit-target publish must work with no active account and no kind:10002"
    );
    assert_eq!(kernel.last_error_toast_snapshot(), None);
    let mut got: Vec<String> = outbound.iter().map(|m| m.relay_url.clone()).collect();
    got.sort();
    let mut want = relays.clone();
    want.sort();
    assert_eq!(got, want, "must dispatch to exactly the explicit relays");
    assert!(outbound[0].text.contains(&format!("\"id\":\"{ev_id}\"")));
    assert!(outbound[0].text.contains(&format!("\"sig\":\"{ev_sig}\"")));
}

#[test]
fn publish_signed_event_to_explicit_relays_still_rejects_tampered_sig() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, sig) = signed_nip01_json(&id, "explicit tamper");

    let flipped = if sig.starts_with('a') { 'b' } else { 'a' };
    let bad_json = json.replacen(&sig, &format!("{flipped}{}", &sig[1..]), 1);
    assert_ne!(bad_json, json);

    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let raw: crate::store::RawEvent = serde_json::from_str(&bad_json).unwrap();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::explicit(relays, PublishRouteClass::ImportedOrPresigned),
        None,
    );

    assert!(
        outbound.is_empty(),
        "forged-signature event must not publish even with explicit relays"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("signed event rejected")),
        "expected the same rejection toast contract as the Auto path"
    );
    assert!(kernel.publish_queue_snapshot().is_empty());
}
