//! Shared fixtures for the W5 production-ingest claim-expansion tests:
//! signed kind:1 / kind:30023 event builders, the `refs.event` sidecar
//! decode-and-lookup helper, wire-frame text builders, and the
//! claim-registered-and-wired kernel setup every scenario starts from.

use std::time::Instant;

use crate::kernel::public_typed_projections::ClaimedEventRow;
use crate::kernel::Kernel;
use crate::refs::{RefEventStore, REFS_EVENT_KEY};
use crate::update_envelope::{decode_snapshot_envelope, decode_snapshot_typed_projections};

pub(super) fn signed_note(keys: &::nostr::Keys, content: &str, ts: u64) -> crate::kernel::NostrEvent {
    use nostr::{EventBuilder, Timestamp};
    let nostr_event = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    crate::kernel::NostrEvent {
        id: nostr_event.id.to_hex(),
        pubkey: nostr_event.pubkey.to_hex(),
        created_at: nostr_event.created_at.as_secs(),
        kind: nostr_event.kind.as_u16() as u32,
        tags: nostr_event
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: nostr_event.content.clone(),
        sig: nostr_event.sig.to_string(),
    }
}

/// Sign a kind:30023 addressable (long-form) article with a `d` tag, so a
/// real `naddr` coordinate can be built from `(kind, pubkey, d_tag)` and
/// the EVENT passes `verify_and_persist`'s signature check on the wire path.
pub(super) fn signed_article(
    keys: &::nostr::Keys,
    d_tag: &str,
    title: &str,
    content: &str,
    ts: u64,
) -> crate::kernel::NostrEvent {
    use nostr::{EventBuilder, Kind, Tag, Timestamp};
    let nostr_event = EventBuilder::new(Kind::from_u16(30023), content)
        .tags([
            Tag::parse(["d", d_tag]).expect("valid d tag"),
            Tag::parse(["title", title]).expect("valid title tag"),
        ])
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    crate::kernel::NostrEvent {
        id: nostr_event.id.to_hex(),
        pubkey: nostr_event.pubkey.to_hex(),
        created_at: nostr_event.created_at.as_secs(),
        kind: nostr_event.kind.as_u16() as u32,
        tags: nostr_event
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: nostr_event.content.clone(),
        sig: nostr_event.sig.to_string(),
    }
}

pub(super) fn event_ref_row(
    kernel: &mut Kernel,
    store: &mut RefEventStore,
    primary_id: &str,
) -> Option<ClaimedEventRow> {
    let frame = kernel.make_update(true);
    let envelope = decode_snapshot_envelope(&frame).expect("decode snapshot envelope");
    let typed = decode_snapshot_typed_projections(&frame).expect("decode typed projections");
    for entry in typed.iter().filter(|entry| entry.key == REFS_EVENT_KEY) {
        let outcome =
            store.apply_sidecar(&entry.payload, envelope.session_id, envelope.snapshot_epoch);
        assert!(
            !outcome.decode_failed,
            "refs.event rows emitted by the kernel must pass decode-before-commit"
        );
    }
    store.event(primary_id)
}

pub(super) fn event_frame(sub_id: &str, event: &crate::kernel::NostrEvent) -> String {
    serde_json::json!([
        "EVENT",
        sub_id,
        {
            "id": event.id,
            "pubkey": event.pubkey,
            "created_at": event.created_at,
            "kind": event.kind,
            "tags": event.tags,
            "content": event.content,
            "sig": event.sig,
        }
    ])
    .to_string()
}

pub(super) fn eose_frame(sub_id: &str) -> String {
    serde_json::json!(["EOSE", sub_id]).to_string()
}

/// Set up a kernel with a registered claim and wire frames applied.
///
/// Returns `(kernel, sub_id, event)` where `sub_id` is the planner-assigned
/// wire sub_id that `register_wire_frames_for_test` populated in
/// `claim_sub_index`.
pub(super) fn setup_kernel_with_wired_claim(
    relay_url: &str,
) -> (Kernel, String, crate::kernel::NostrEvent) {
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use crate::subs::WireFrame;

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "claim expansion test event", 1_700_000_000);
    let author_hex = event.pubkey.clone();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Register a claim expansion — mirrors what the event resolver does in production.
    // Use the authority (interest_id = 0 fallback) since we're not going
    // through a full event ref resolve here.
    kernel.register_claim_expansion(
        event.id.clone(),
        None,
        Some(author_hex.clone()),
        vec![relay_url.to_string()],
        Instant::now(),
    );

    // Derive the sub_id the planner would assign for a filter of this shape.
    // In production this is done by drain_lifecycle_tick → plan_diff. We
    // simulate it: the sub_id format is "sub-{canonical_filter_hash}".
    // For this test we use a synthetic sub_id and inject it directly via
    // register_wire_frames_for_test, mirroring the production bridge.
    let synthetic_sub_id = format!("sub-test-claim-{}", &event.id[..8]);

    // Manually populate pending_claims and inject a fake WireFrame::Req so
    // that register_wire_frames_for_test populates claim_sub_index.
    // The interest_id stored in the claim is InterestId(0) (fallback path).
    let frames = vec![WireFrame::Req {
        relay_url: relay_url.to_string(),
        sub_id: synthetic_sub_id.clone(),
        filter_json: r#"{"ids":["test"],"limit":1}"#.to_string(),
        interest_id: crate::planner::InterestId(0),
        lifecycle: crate::planner::InterestLifecycle::OneShot,
    }];
    kernel.register_wire_frames_for_test(&frames);

    (kernel, synthetic_sub_id, event)
}

// ── Helper: event_id for setup_kernel_with_wired_claim ──────────────────
// (The inner setup function uses a keys-generated event; this just provides
// a placeholder for the T-P2 phase assertion that doesn't need the actual id.)
pub(super) fn event_id_for_setup() -> String {
    // T-P2 doesn't need the real event id for its no-panic assertion.
    String::new()
}
