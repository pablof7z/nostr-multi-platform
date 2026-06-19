//! Integration proof: kernel-resolved kind:30023 supersession reaches the
//! typed NIP-23 [`LongformProjection`], decoded back from the `NL23`
//! FlatBuffers sidecar payload (NOT a JSON map).
//!
//! The `LongformProjection` does an unconditional `state.insert(address, ...)`
//! with **no `created_at` comparison** — by design (the brief: "do NOT
//! reimplement is-newer"). That correctness rests entirely on ONE external
//! invariant: the kernel fans an event out to `KernelEventObserver`s **only on
//! store outcome `Inserted | Replaced`** (see
//! `crates/nmp-core/src/kernel/ingest/`), and the param-replaceable store
//! returns `Superseded` (NOT `Inserted | Replaced`) for an older
//! `(author, kind, d_tag)` arrival.
//!
//! The unit tests in `nmp-content` call `on_kernel_event` directly and so only
//! prove in-order last-write-wins — they bypass the store. This test closes the
//! gap: it drives kind:30023 events through the REAL `EventStore` in
//! **newer-first, older-second** order (the adversarial order), reproduces the
//! kernel's exact fan-out gate against the typed `InsertOutcome`, and asserts
//! the projection keeps the newest event — the older `id` never appears. If the
//! store ever stopped suppressing the older arrival, this test fails loudly
//! instead of the projection silently overwriting the winner with the loser.

use nmp_content::wire::longform_fb::decode_longform_articles;
use nmp_content::{LongformProjection, KIND_LONG_FORM_ARTICLE, LONGFORM_PROJECTION_KEY};
use nmp_store::InsertOutcome;
use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX, BOB_HEX};

/// Build the kind:30023 tag set the NIP-23 resolver reads.
fn article_tags(d_tag: &str, title: &str, summary: &str, image: &str) -> Vec<Vec<String>> {
    vec![
        vec!["d".to_string(), d_tag.to_string()],
        vec!["title".to_string(), title.to_string()],
        vec!["summary".to_string(), summary.to_string()],
        vec!["image".to_string(), image.to_string()],
    ]
}

/// Convert the store's winning `StoredEvent` into the substrate `KernelEvent`
/// the kernel hands its observers (mirrors `nmp_store_to_kernel_stored`).
fn kernel_event_from_stored(stored: &nmp_store::StoredEvent) -> KernelEvent {
    KernelEvent {
        id: stored.raw.id.clone(),
        author: stored.raw.pubkey.clone(),
        kind: stored.raw.kind,
        created_at: stored.raw.created_at,
        tags: stored.raw.tags.clone(),
        content: stored.raw.content.clone(),
        relay_provenance: Vec::new(),
    }
}

/// Read the current winning param-replaceable event for `(author, d_tag)` from
/// the store and feed it to the observer — EXACTLY what the kernel does after a
/// fan-out-eligible insert. The store returns the *winner*, so an observer fed
/// this way can never see a superseded loser.
fn fan_out_winner(
    store: &dyn nmp_store::EventStore,
    projection: &LongformProjection,
    author_hex: &str,
    d_tag: &str,
) {
    let author_bytes = hex32(author_hex);
    let winner = store
        .get_param_replaceable(&author_bytes, KIND_LONG_FORM_ARTICLE, d_tag.as_bytes())
        .expect("store read")
        .expect("winner present");
    projection.on_kernel_event(&kernel_event_from_stored(&winner));
}

fn hex32(hex: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex byte");
    }
    out
}

/// The headline proof. Newer arrives FIRST, older arrives SECOND (the order a
/// naive `created_at`-free insert would get wrong if the store didn't suppress
/// the older). The store resolves it; the typed projection shows only the
/// newest, decoded back from the `NL23` sidecar payload.
#[test]
fn kernel_supersession_reaches_longform_projection_out_of_order() {
    let h = StoreHarness::mem();
    let projection = LongformProjection::new();

    // ── 1. NEWER event first (created_at 2_000). Fresh insert → kernel fires. ──
    let newer = h.make_event_with_tags(
        ALICE_HEX,
        KIND_LONG_FORM_ARTICLE,
        2_000,
        article_tags(
            "rust-guide",
            "New Title",
            "New summary",
            "https://img/new.png",
        ),
    );
    let newer_id = newer.id.clone();
    let o_new = h.insert_raw(newer, "wss://r1/", 1_000_000);
    assert!(
        matches!(o_new, InsertOutcome::Inserted { .. }),
        "newer is a fresh insert: {o_new:?}"
    );
    fan_out_winner(&*h.store, &projection, ALICE_HEX, "rust-guide");

    // ── 2. OLDER event second (created_at 1_000) for the SAME coordinate. ──────
    // The store must report `Superseded` — NOT `Inserted | Replaced`. This is
    // the load-bearing invariant: the kernel's fan-out gate skips it, so the
    // projection is NEVER shown the older event.
    let older = h.make_event_with_tags(
        ALICE_HEX,
        KIND_LONG_FORM_ARTICLE,
        1_000,
        article_tags(
            "rust-guide",
            "Old Title",
            "Old summary",
            "https://img/old.png",
        ),
    );
    let older_id = older.id.clone();
    let o_old = h.insert_raw(older, "wss://r2/", 2_000_000);
    assert!(
        matches!(o_old, InsertOutcome::Superseded { .. }),
        "older arrival MUST be Superseded (not fan-out eligible): {o_old:?}"
    );
    // The kernel would NOT fan this out — so we deliberately do not call the
    // observer for it. (If a future kernel change started firing on Superseded,
    // the assertion above fails first, flagging the design-breaking regression.)

    // ── 3. Unrelated article (different author + d_tag) for scope. ────────────
    let other = h.make_event_with_tags(
        BOB_HEX,
        KIND_LONG_FORM_ARTICLE,
        1_500,
        article_tags(
            "nostr-intro",
            "Nostr Intro",
            "An intro",
            "https://img/nostr.png",
        ),
    );
    let o_other = h.insert_raw(other, "wss://r1/", 1_500_000);
    assert!(matches!(o_other, InsertOutcome::Inserted { .. }));
    fan_out_winner(&*h.store, &projection, BOB_HEX, "nostr-intro");

    // ── Decode the TYPED sidecar payload (NL23 FlatBuffer), not a JSON map. ────
    let entry = projection.typed_projection();
    assert_eq!(entry.key, LONGFORM_PROJECTION_KEY);
    assert_eq!(entry.key, "nmp.nip23.articles");
    assert_eq!(entry.file_identifier, "NL23");
    let snap = decode_longform_articles(&entry.payload).expect("NL23 payload decodes");

    // Scope: exactly the two surviving coordinates — older one is gone.
    assert_eq!(snap.articles.len(), 2, "one row per surviving coordinate");
    assert_eq!(snap.documents.len(), 2);

    let addr_a = format!("{KIND_LONG_FORM_ARTICLE}:{ALICE_HEX}:rust-guide");
    let doc_a = snap.documents.get(&addr_a).expect("ALICE document present");
    assert_eq!(doc_a.id, newer_id, "newest wins");
    assert_eq!(doc_a.title.as_deref(), Some("New Title"));
    assert_eq!(doc_a.created_at, 2_000);

    // The superseded older event's id must NEVER appear anywhere.
    assert_ne!(newer_id, older_id);
    let older_present = snap.documents.values().any(|d| d.id == older_id);
    assert!(
        !older_present,
        "superseded older event must be absent (kernel resolved it)"
    );
}
