//! Registry-level trip tests for the nip51 bookmark typed FlatBuffers payload
//! doorway (ADR-0064 / S9 #1747).
//!
//! These tests prove the fail-closed `schema_version` gate in
//! `ActionRegistry::start_bytes` rejects bad payloads BEFORE `start()` runs, for
//! BOTH the `nmp.nip51.add_bookmark` and `nmp.nip51.remove_bookmark` namespaces,
//! and that a well-formed payload (including each `BookmarkItem` variant)
//! round-trips through the registry boundary.
//!
//! Codec round-trip tests (positive + per-variant + per-field negative) live in
//! `src/wire/bookmark_update_fb_tests.rs`. These tests sit one level up — at the
//! registry boundary — so they exercise the same path the byte transport (S2
//! `DispatchEnvelope`) drives in production.

use std::sync::{Arc, Mutex};

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::substrate::{ActionContext, ActionPayload, ActionRejection, EventId, KernelEvent};
use nmp_core::KernelEventObserver;
use nmp_kinds::KIND_BOOKMARK_LIST;
use nmp_nip51::{
    register_bookmark_actions, BookmarkItem, BookmarkListProjection, BookmarkUpdateInput,
};

const ADD_NAMESPACE: &str = "nmp.nip51.add_bookmark";
const REMOVE_NAMESPACE: &str = "nmp.nip51.remove_bookmark";

fn account() -> String {
    "ab".repeat(32)
}

/// Register both bookmark action modules into a fresh registry, with the active
/// account preset to [`account()`] so `start()`'s account-match gate is reached
/// (it runs AFTER the typed decode + schema_version gate). Returns the shared
/// projection so tests can seed it with an existing kind:10003 bookmark.
fn registry_with_bookmark_actions() -> (ActionRegistry, Arc<BookmarkListProjection>) {
    let mut registry = ActionRegistry::new();
    let active = Arc::new(Mutex::new(Some(account())));
    let projection = Arc::new(BookmarkListProjection::new(active));
    register_bookmark_actions(&mut registry, Arc::clone(&projection));
    (registry, projection)
}

/// Feed the projection a kind:10003 bookmark list carrying a single `e`-tagged
/// event bookmark, so `remove_bookmark`'s presence check (run inside `start()`)
/// can succeed.
fn seed_event_bookmark(projection: &BookmarkListProjection, event_id: &str) {
    projection.on_kernel_event(&KernelEvent {
        id: EventId::from(
            "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
        ),
        author: account(),
        kind: KIND_BOOKMARK_LIST,
        created_at: 1,
        tags: vec![vec!["e".to_string(), event_id.to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    });
}

/// A finished `BookmarkUpdatePayload` (file identifier `N51B`) with
/// `schema_version = 999`. The fail-closed gate must reject it before `start`.
fn build_bad_version_bookmark_payload() -> Vec<u8> {
    // Inline the generated FlatBuffers API so this integration test does not
    // need to expose the private `wire` module from nmp-nip51. The wire layout
    // (identifier, vtable offsets) is fixed by the committed .fbs schema.
    use flatbuffers::FlatBufferBuilder;

    // BookmarkItem vtable slots: VT_KIND = 4, VT_VALUE = 6, VT_RELAY = 8.
    const VT_ITEM_KIND: flatbuffers::VOffsetT = 4;
    const VT_ITEM_VALUE: flatbuffers::VOffsetT = 6;
    // BookmarkUpdatePayload vtable slots:
    //   VT_SCHEMA_VERSION = 4, VT_ACCOUNT_PUBKEY = 6, VT_ITEM = 8.
    const N51B_IDENTIFIER: &str = "N51B";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_ACCOUNT_PUBKEY: flatbuffers::VOffsetT = 6;
    const VT_ITEM: flatbuffers::VOffsetT = 8;

    let mut fbb = FlatBufferBuilder::new();
    let value = fbb.create_string(&"cd".repeat(32));

    // Inner BookmarkItem table (kind = Event(0), value present, relay absent).
    let item_start = fbb.start_table();
    fbb.push_slot::<u8>(VT_ITEM_KIND, 0, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_ITEM_VALUE, value);
    let item = fbb.end_table(item_start);

    let account_pubkey = fbb.create_string(&account());

    // Outer BookmarkUpdatePayload table with the tripwire schema_version.
    let payload_start = fbb.start_table();
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_ACCOUNT_PUBKEY, account_pubkey);
    fbb.push_slot_always(VT_ITEM, item);
    let root = fbb.end_table(payload_start);
    fbb.finish(root, Some(N51B_IDENTIFIER));
    fbb.finished_data().to_vec()
}

/// ADR-0064 / S9 (#1747) — `nmp.nip51.add_bookmark` with a bad `schema_version`
/// MUST be rejected BEFORE `start()` runs.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_add_bookmark() {
    let (registry, _projection) = registry_with_bookmark_actions();
    let bad = build_bad_version_bookmark_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            ADD_NAMESPACE,
            &bad,
        )
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "rejection must name the version trip: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}

/// ADR-0064 / S9 (#1747) — `nmp.nip51.remove_bookmark` with a bad
/// `schema_version` MUST be rejected BEFORE `start()` runs.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_remove_bookmark() {
    let (registry, _projection) = registry_with_bookmark_actions();
    let bad = build_bad_version_bookmark_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            REMOVE_NAMESPACE,
            &bad,
        )
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "rejection must name the version trip: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}

/// ADR-0064 / S9 (#1747) — a well-formed, correct-version `add_bookmark`
/// payload passes the typed decode + schema_version gate at the registry
/// boundary, for every `BookmarkItem` variant (the tagged-table round-trip).
#[test]
fn start_bytes_accepts_well_formed_add_bookmark_for_every_item_variant() {
    let (registry, _projection) = registry_with_bookmark_actions();
    let items = [
        BookmarkItem::Event {
            event_id: "cd".repeat(32),
            relay: Some("wss://relay.example".to_string()),
        },
        BookmarkItem::Address {
            coordinate: format!("30023:{}:my-article", "ef".repeat(32)),
            relay: None,
        },
        BookmarkItem::Url {
            url: "https://example.com/article".to_string(),
        },
        BookmarkItem::Hashtag {
            hashtag: "nostr".to_string(),
        },
    ];
    for item in items {
        let action = BookmarkUpdateInput {
            account_pubkey: account(),
            item,
        };
        let bytes = action.encode();
        registry
            .start_bytes(
                &mut ActionContext::default(),
                1_700_000_000_000,
                ADD_NAMESPACE,
                &bytes,
            )
            .expect("a well-formed, correct-version add_bookmark payload must be accepted");
    }
}

/// ADR-0064 / S9 (#1747) — a well-formed, correct-version `remove_bookmark`
/// payload (an existing bookmark seeded into the projection) passes the typed
/// decode + schema_version gate AND `start()`'s presence check at the registry
/// boundary — proving the `remove_bookmark` namespace override is load-bearing,
/// not just covered by the bad-version negative.
#[test]
fn start_bytes_accepts_well_formed_remove_bookmark() {
    let (registry, projection) = registry_with_bookmark_actions();
    let event_id = "cd".repeat(32);
    seed_event_bookmark(&projection, &event_id);

    let action = BookmarkUpdateInput {
        account_pubkey: account(),
        item: BookmarkItem::Event {
            event_id,
            relay: None,
        },
    };
    let bytes = action.encode();
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            REMOVE_NAMESPACE,
            &bytes,
        )
        .expect("a well-formed remove of a present bookmark must be accepted");
}

/// ADR-0064 / S9 (#1747) — registration is namespace-scoped: a namespace this
/// crate never registers is rejected even after `register_bookmark_actions`.
#[test]
fn unregistered_namespace_is_rejected() {
    let (registry, _projection) = registry_with_bookmark_actions();
    let action = BookmarkUpdateInput {
        account_pubkey: account(),
        item: BookmarkItem::Hashtag {
            hashtag: "nostr".to_string(),
        },
    };
    let bytes = action.encode();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip51.not_a_bookmark_namespace",
            &bytes,
        )
        .expect_err("an unregistered namespace must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "expected Invalid rejection for unregistered namespace, got {err:?}"
    );
}
