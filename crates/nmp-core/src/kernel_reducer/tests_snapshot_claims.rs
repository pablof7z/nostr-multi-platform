// ─── V-01 Stage 3c publish-from-signed-event surface ─────────────────────
//
// `publish_signed_event` is the new public seam the wasm runtime uses to
// feed `Nip07Signer::sign()` results through the publish engine. The
// tests here pin only the contract — total, no panic, returns an
// outbound vec — and defer deep publish-engine behaviour to the
// existing kernel-side tests in `publish/engine/tests.rs`.
//
// ─── F-CR-00 component-owned claim seam ──────────────────────────────────
//
// These tests cover the contracts the wasm dispatch arm depends on.
// Deep claim behaviour (batch routing, indexer-only lane, pending queue)
// is already covered by `kernel/profile_claim_tests.rs`,
// `kernel/event_claim_tests.rs`, and friends. What we pin here is that
// the `KernelReducer` delegations are wired correctly and the
// `any_relay_connected` gate accurately reflects kernel relay state.
//
// ─── Snapshot / update-frame surface ─────────────────────────────────────
//
// These tests pin the `make_update_frame` / `set_configured_relays` surface
// that the wasm32 runtime uses to push typed-projection snapshots to JS.

use super::*;
use crate::kernel::refs::{ProfileShape, RefLiveness, RefNamespace, RefShape};
use crate::substrate::{SignedEvent, UnsignedEvent};

const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const RELAY: &str = "wss://relay.example";

/// A genuinely Schnorr-signed event over `keys`. The id is the SHA-256 of the
/// canonical NIP-01 serialization and the signature verifies — so it passes the
/// single well-formedness chokepoint
/// (`Kernel::verify_externally_signed_event`) that #1748 added to the wasm
/// verbatim write path. `kind`/`content`/`tags` are caller-supplied so the same
/// helper builds a kind:1 note AND a kind:1059 gift-wrap envelope.
fn real_signed_event(
    keys: &::nostr::Keys,
    kind: u16,
    content: &str,
    tags: Vec<::nostr::Tag>,
) -> SignedEvent {
    let event = ::nostr::EventBuilder::new(::nostr::Kind::from(kind), content)
        .tags(tags)
        .custom_created_at(::nostr::Timestamp::from_secs(1_700_000_000))
        .sign_with_keys(keys)
        .expect("generated keys sign");
    SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: u32::from(event.kind.as_u16()),
            tags: event
                .tags
                .iter()
                .map(|t: &::nostr::Tag| t.as_slice().to_vec())
                .collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    }
}

/// A forged signed event: well-shaped fields but a placeholder id+sig that do
/// NOT verify. This is exactly the kind of bytes a malicious / buggy JS host
/// could hand the wasm verbatim path.
fn forged_signed_note() -> SignedEvent {
    SignedEvent {
        id: "a".repeat(64),
        sig: "b".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: PK.to_string(),
            kind: 1,
            tags: Vec::new(),
            content: "hello from wasm".to_string(),
            created_at: 1_700_000_000,
        },
    }
}

#[test]
fn publish_signed_event_rejects_forged_event_fail_closed() {
    // Regression for #1748 Fix 2: the wasm/verbatim write path
    // (`KernelReducer::publish_signed_event`) must validate signed-event
    // well-formedness through the SAME chokepoint as the native pre-signed
    // path. Before the fix this path fed the engine directly with NO
    // signature/id-hash verification, so a forged event from JS would have
    // reached the outbound EVENT frames and gone out on the wire.
    //
    // The forged event has a placeholder id+sig that fail the SHA-256
    // id-hash + Schnorr check, so it is rejected fail-closed (empty
    // outbound) and the categorized `malformed_event` toast is set — never
    // routed to a relay.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    let signed = forged_signed_note();
    let out = r.publish_signed_event(&signed, &[], None);
    assert!(
        out.is_empty(),
        "a forged signed event must be rejected fail-closed, never routed"
    );
    let toast = r.kernel.last_error_toast_snapshot();
    assert!(
        toast
            .as_deref()
            .map_or(false, |t| t.contains("signed event rejected")),
        "the malformed-event chokepoint must set its categorized toast; got: {toast:?}"
    );
}

#[test]
fn publish_signed_event_does_not_reject_valid_event_for_nip_shape() {
    // The well-formedness chokepoint validates the OUTER envelope ONLY (id
    // hash + Schnorr sig). A genuinely-signed kind:1 note is NOT rejected
    // for any NIP-specific reason — on a fresh kernel with no kind:10002
    // outbox it surfaces `NoTargets` (the documented D6 path), but it is NOT
    // dropped at the malformed-event chokepoint. We prove that by asserting
    // the categorized malformed toast is absent.
    let keys = ::nostr::Keys::generate();
    let signed = real_signed_event(&keys, 1, "a real note", vec![]);

    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    let _ = r.publish_signed_event(&signed, &[], Some("dispatch-1".to_string()));
    let toast = r.kernel.last_error_toast_snapshot();
    assert!(
        !toast
            .as_deref()
            .map_or(false, |t| t.contains("signed event rejected")),
        "a well-formed event must NOT trip the malformed-event chokepoint; got: {toast:?}"
    );
}

#[test]
fn publish_signed_event_does_not_reject_valid_gift_wrap_for_nip_shape() {
    // Opacity (ADR-0025): a kind:1059 gift-wrap is opaque ciphertext under a
    // well-formed signed envelope. The chokepoint validates the OUTER
    // envelope's sig/id-hash ONLY and must NOT inspect or reject the inner
    // semantics. A genuinely-signed kind:1059 with an EXPLICIT relay pin (so
    // the D10 private-envelope routing gate is satisfied) must therefore pass
    // the malformed-event chokepoint — it is not rejected for "NIP shape".
    let keys = ::nostr::Keys::generate();
    // Opaque gift-wrap: random-looking ciphertext content + a recipient `p`
    // tag, signed by a (here, ephemeral) key — exactly the NIP-59 outer form.
    let signed = real_signed_event(
        &keys,
        1059,
        "AESGCM-ciphertext-opaque-payload",
        vec![::nostr::Tag::parse(["p", PK]).expect("valid p tag")],
    );

    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    // The verbatim path uses PublishTarget::Auto internally; the D10 gate
    // would refuse a kind:1059 with Auto, but that is a routing-policy
    // refusal, NOT the malformed-event chokepoint. We assert the
    // malformed-event chokepoint did not fire (the envelope is well-formed).
    let _ = r.publish_signed_event(&signed, &[], None);
    let toast = r.kernel.last_error_toast_snapshot();
    assert!(
        !toast
            .as_deref()
            .map_or(false, |t| t.contains("signed event rejected")),
        "a well-formed gift-wrap must NOT be rejected for NIP shape (ADR-0025 opacity); \
         got: {toast:?}"
    );
}

#[test]
fn claim_profile_on_fresh_reducer_parks_returns_empty() {
    // M2 migration: `claim_profile` always returns empty outbound — it
    // registers a kind:0 `LogicalInterest` and the planner emits the wire REQ
    // on the next drain (the reducer drains inline via `drain_lifecycle_outbound`).
    // A fresh reducer with no relay connected still returns empty from the call.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    // any_relay_connected is false on a fresh reducer — assert the gate.
    assert!(!r.any_relay_connected(), "fresh reducer: no relay connected");
    let out = r.resolve_ref(
        RefNamespace::Profile,
        PK.to_string(),
        "chirp-web-author-1".to_string(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk.into(),
    );
    assert!(out.is_empty(), "claim_profile must emit no outbound directly");
}

#[test]
fn claim_profile_refcount_dedup_does_not_double_fetch() {
    // Two different consumer_ids for the same pubkey dedup to ONE registry
    // interest (registry owner refcount); neither claim emits outbound directly.
    // (Detailed batch/routing/dedup assertions live in profile_claim_tests.rs.)
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);

    let _ = r.resolve_ref(
        RefNamespace::Profile,
        PK.to_string(),
        "chirp-web-author-card-a".to_string(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk.into(),
    );
    let out2 = r.resolve_ref(
        RefNamespace::Profile,
        PK.to_string(),
        "chirp-web-author-card-b".to_string(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk.into(),
    );
    assert!(
        out2.is_empty(),
        "second claim for same pubkey must not emit outbound directly: {out2:?}"
    );
}

#[test]
fn release_profile_is_total_no_panic() {
    // Releasing a pubkey that was never claimed is a no-op (D6).
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    let out = r.release_ref(RefNamespace::Profile, PK, "chirp-web-author-1");
    assert!(out.is_empty(), "release must emit no outbound");
}

#[test]
fn claim_event_malformed_uri_is_total_no_panic() {
    // D6: a garbled nostr URI must be silently dropped, not a panic.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    let out = r.claim_event(
        "not-a-nostr-uri".to_string(),
        "chirp-web-embed-1".to_string(),
        false,
        false,
    );
    assert!(out.is_empty(), "malformed URI must produce no outbound");
}

#[test]
fn release_event_malformed_uri_is_total_no_panic() {
    // D6 symmetry: release with a garbage URI must not panic.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    let out = r.release_event("not-a-nostr-uri", "chirp-web-embed-1");
    assert!(out.is_empty());
}

#[test]
fn any_relay_connected_false_before_connect_true_after() {
    // `any_relay_connected` must mirror the kernel's per-lane
    // `RelayHealth::connection` field — false before
    // `handle_relay_connected`, true after.
    let mut r = KernelReducer::new();
    assert!(!r.any_relay_connected(), "before connect: must be false");
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    assert!(r.any_relay_connected(), "after connect: must be true");
}

#[test]
fn any_relay_connected_returns_false_after_close() {
    // After `handle_relay_closed` the lane flips back to
    // `connection != "connected"`, so the gate must return false again.
    let mut r = KernelReducer::new();
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    assert!(r.any_relay_connected());
    r.handle_relay_closed(RelayRole::Content, RELAY);
    assert!(!r.any_relay_connected(), "after close: must be false");
}

#[test]
fn make_update_frame_bumps_rev_monotonically() {
    // D6 (total) + monotonic-rev contract: `make_update_frame` must never
    // panic on a fresh reducer, and each successive call must produce a
    // strictly larger revision number in the decoded envelope.
    let mut r = KernelReducer::new();

    let bytes0 = r.make_update_frame(false);
    assert!(
        !bytes0.is_empty(),
        "make_update_frame must return a non-empty frame"
    );

    let env0 = crate::decode_snapshot_envelope(&bytes0)
        .expect("first frame must decode without error");

    let bytes1 = r.make_update_frame(false);
    let env1 = crate::decode_snapshot_envelope(&bytes1)
        .expect("second frame must decode without error");

    assert!(
        env1.rev > env0.rev,
        "rev must increase monotonically: {} → {}",
        env0.rev,
        env1.rev
    );
}

#[test]
fn set_configured_relays_surfaces_in_update_frame() {
    // After `set_configured_relays`, the next `make_update_frame` must
    // carry the supplied URL in both the Tier-3 relay_statuses rows and
    // the `configured_relays` typed-projection sidecar.
    use crate::typed_projections::{decode_configured_relays, CONFIGURED_RELAYS_SCHEMA_ID};

    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(
        "wss://relay.test".to_string(),
        "both".to_string(),
    )]);

    let bytes = r.make_update_frame(true);

    // Tier-3 relay_statuses
    let env = crate::decode_snapshot_envelope(&bytes)
        .expect("frame must decode");
    assert!(
        env.relay_statuses
            .iter()
            .any(|row| row.relay_url == "wss://relay.test"),
        "relay_statuses must contain the configured URL; got: {:?}",
        env.relay_statuses
    );

    // Tier-2 configured_relays sidecar
    let projections = crate::decode_snapshot_typed_projections(&bytes)
        .expect("typed projections must decode");
    let cr_entry = projections
        .iter()
        .find(|p| p.schema_id == CONFIGURED_RELAYS_SCHEMA_ID)
        .expect("configured_relays sidecar must be present");
    let model = decode_configured_relays(&cr_entry.payload)
        .expect("configured_relays payload must decode");
    assert!(
        model.relays.iter().any(|row| row.url == "wss://relay.test"),
        "configured_relays sidecar must contain the configured URL; got: {:?}",
        model.relays
    );
}
