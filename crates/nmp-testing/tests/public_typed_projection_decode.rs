//! External proof that `nmp-core` exposes a PUBLIC, reachable API for decoding
//! the typed-projection sidecar — the unblock for out-of-tree Rust consumers
//! (e.g. `tenex-off`) — the generic JSON `payload` no longer exists (PR-B).
//!
//! This test lives OUTSIDE the `nmp-core` crate on purpose: it exercises the
//! API exactly as an external dependent would, so it fails to compile if any
//! piece of the surface (`decode_snapshot_typed_projections`, the per-key
//! decoders, the DTO structs, or their fields) is not truly `pub`.
//!
//! The proof drives a real kernel actor through the public
//! `nmp_core::testing::spawn_actor` harness (a local-key sign-in + a kind:1
//! publish), drains the emitted FlatBuffers frames, and — using ONLY the public
//! API — finds the `publish_queue` and `action_results` typed sidecar entries
//! by key and decodes them into their typed Rust structs. No `nmp-core`
//! internals are reachable here; if this compiles and passes, an external crate
//! can do the same.

use std::time::{Duration, Instant};

use nmp_core::testing::{spawn_actor, ActorCommand};
use nmp_core::typed_projections::{
    decode_action_results, decode_publish_queue, ACTION_RESULTS_FILE_IDENTIFIER,
    ACTION_RESULTS_SCHEMA_ID, ACTION_RESULTS_SCHEMA_VERSION, PUBLISH_QUEUE_FILE_IDENTIFIER,
    PUBLISH_QUEUE_SCHEMA_ID, PUBLISH_QUEUE_SCHEMA_VERSION,
};
use nmp_core::{decode_snapshot_typed_projections, SignerSource, TypedProjectionData};

/// A fixed nsec used only in tests (same key the e2e pipeline test uses).
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// Find a typed sidecar entry by key, using only the public `TypedProjectionData`
/// surface that `decode_snapshot_typed_projections` returns.
fn find_entry<'a>(typed: &'a [TypedProjectionData], key: &str) -> Option<&'a TypedProjectionData> {
    typed.iter().find(|t| t.key == key)
}

#[test]
fn external_consumer_decodes_publish_queue_and_action_results_via_public_api() {
    let (tx, rx) = spawn_actor();

    // Start with a configured relay so the publish engine has a routing target
    // and records a `publish_queue` entry (mirrors the e2e pipeline harness).
    tx.send(ActorCommand::Start {
        visible_limit: 100,
        emit_hz: 30,
        initial_relays: vec![("wss://relay.test".to_string(), "both".to_string())],
    })
    .expect("send Start");

    // Sign in with a local key — establishes the active account that signs and
    // enqueues the publish below.
    tx.send(ActorCommand::AddSigner {
        source: SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        make_active: true,
    })
    .expect("send AddSigner");

    // Publish a kind:1 event with an explicit correlation id. The correlation id
    // surfaces as an `action_results` terminal once the action settles; the
    // publish itself populates `publish_queue`.
    // Derive the author pubkey from the test nsec so the publish engine has an
    // author to route for (mirrors the bunker-signing integration test).
    let author_pubkey = {
        use nostr::prelude::*;
        Keys::parse(TEST_NSEC)
            .expect("test nsec parses")
            .public_key()
            .to_hex()
    };
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: author_pubkey,
        kind: 1,
        tags: Vec::new(),
        content: "public-typed-decode proof".to_string(),
        created_at: 1_700_002_000,
    };
    tx.send(ActorCommand::PublishUnsignedEvent {
        event: unsigned,
        correlation_id: Some("proof-corr-1".to_string()),
        signer_pubkey: None,
    })
    .expect("send PublishUnsignedEvent");

    // Record a terminal action result for our correlation id. This is the
    // deterministic, relay-independent way to populate the drain-on-emit
    // `action_results` sidecar: the actor folds it into a terminal verdict the
    // SAME tick (no live relay echo required).
    tx.send(ActorCommand::RecordActionSuccess {
        correlation_id: "proof-corr-1".to_string(),
        result_json: None,
    })
    .expect("send RecordActionSuccess");

    tx.send(ActorCommand::MarkChangedSinceEmit)
        .expect("send MarkChangedSinceEmit");

    // Drain frames via the PUBLIC `decode_snapshot_typed_projections`,
    // accumulating the first typed `publish_queue` and `action_results`
    // payloads we observe.
    // `action_results` is drain-on-emit (present only on the tick it settles),
    // so we capture it the moment it appears rather than requiring it in the
    // same frame as `publish_queue`.
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut publish_queue_payload: Option<TypedProjectionData> = None;
    let mut action_results_payload: Option<TypedProjectionData> = None;

    while Instant::now() < deadline {
        let Ok(frame) = rx.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        // The PUBLIC entry point: the typed sidecar entries.
        let Ok(typed) = decode_snapshot_typed_projections(&frame) else {
            continue;
        };

        // `publish_queue` is emitted every tick (often empty before the publish
        // is signed + enqueued). Decode through the PUBLIC API and keep it only
        // once it carries at least one row — proving both reachability and that
        // the typed payload round-trips real data.
        if publish_queue_payload.is_none() {
            if let Some(entry) = find_entry(&typed, PUBLISH_QUEUE_SCHEMA_ID) {
                if let Ok(model) = decode_publish_queue(&entry.payload) {
                    if !model.entries.is_empty() {
                        publish_queue_payload = Some(entry.clone());
                    }
                }
            }
        }
        // `action_results` is drain-on-emit: present only on the tick its action
        // settles. Capture it once it carries our correlation id.
        if action_results_payload.is_none() {
            if let Some(entry) = find_entry(&typed, ACTION_RESULTS_SCHEMA_ID) {
                if let Ok(model) = decode_action_results(&entry.payload) {
                    if model.results.iter().any(|r| r.correlation_id == "proof-corr-1") {
                        action_results_payload = Some(entry.clone());
                    }
                }
            }
        }
        if publish_queue_payload.is_some() && action_results_payload.is_some() {
            break;
        }
    }

    // --- publish_queue: reachable, decodes, typed fields are readable --------
    let pq_entry = publish_queue_payload
        .expect("a `publish_queue` typed sidecar must appear after a local-key publish");
    assert_eq!(pq_entry.schema_id, PUBLISH_QUEUE_SCHEMA_ID);
    assert_eq!(pq_entry.schema_version, PUBLISH_QUEUE_SCHEMA_VERSION);
    assert_eq!(
        pq_entry.file_identifier.as_bytes(),
        PUBLISH_QUEUE_FILE_IDENTIFIER,
        "file identifier must match the public constant"
    );

    let pq = decode_publish_queue(&pq_entry.payload)
        .expect("public decode_publish_queue must decode the sidecar bytes");
    let first = pq
        .entries
        .first()
        .expect("the publish enqueued at least one publish_queue row");
    // Read typed fields through the PUBLIC struct — proves the fields are `pub`.
    assert_eq!(first.kind, 1, "the enqueued event is kind:1");
    assert!(
        !first.status.is_empty(),
        "publish queue row must carry a status string"
    );

    // --- action_results: reachable, decodes, typed fields are readable -------
    let ar_entry = action_results_payload
        .expect("an `action_results` typed sidecar must appear once the publish action settles");
    assert_eq!(ar_entry.schema_id, ACTION_RESULTS_SCHEMA_ID);
    assert_eq!(ar_entry.schema_version, ACTION_RESULTS_SCHEMA_VERSION);
    assert_eq!(
        ar_entry.file_identifier.as_bytes(),
        ACTION_RESULTS_FILE_IDENTIFIER,
        "file identifier must match the public constant"
    );

    let ar = decode_action_results(&ar_entry.payload)
        .expect("public decode_action_results must decode the sidecar bytes");
    let row = ar
        .results
        .iter()
        .find(|r| r.correlation_id == "proof-corr-1")
        .expect("the settled action_results must include our correlation id");
    assert!(
        !row.status.is_empty(),
        "the action result row must carry a status string"
    );

    // Clean shutdown of the actor thread.
    let _ = tx.send(ActorCommand::Shutdown);
}

/// A malformed payload must surface an `Err` (D6: no panic at the decode
/// boundary), reachable through the public API.
#[test]
fn public_decoders_reject_malformed_bytes() {
    assert!(
        decode_publish_queue(&[0u8; 3]).is_err(),
        "too-short input must be a decode error, not a panic"
    );
    assert!(
        decode_action_results(b"not a flatbuffer").is_err(),
        "garbage input must be a decode error, not a panic"
    );
}
