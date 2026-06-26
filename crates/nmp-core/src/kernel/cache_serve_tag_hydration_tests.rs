//! Generic single-letter tag cache-serve hydration proofs.
//!
//! These tests prove the payoff of the `StoreQuery::Etag`/`Ptag` → generic
//! `StoreQuery::Tags` generalization:
//!
//! - **#2088** — group events (`{"kinds":[9,11],"#h":["room"]}`) already
//!   cached on disk hydrate through the normal store cache-serve path with ZERO
//!   relay delivery once the group interest opens.
//! - the **latent `#t` hashtag back-fill gap** — a `{"kinds":[1],"#t":["nostr"]}`
//!   feed back-fills its already-stored notes from the local store.
//!
//! Both seed the store BEFORE the interest opens, then drain cache-serve and
//! assert the events reappear in the kernel's read cache without any relay.

use super::cache_serve_tests::{drain_cache_serves, hex_pk, simulate_cold_restart};
use super::*;

use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{EventStore, RawEvent, VerifiedEvent};
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use std::collections::{BTreeSet, BTreeMap};

/// Open a cache-serve interest for `shape` (mirrors the production
/// `open_interest_sub` path used by the universal-acceptance fixtures).
fn open_interest(kernel: &mut Kernel, seed: u64, shape: InterestShape) -> bool {
    let interest = LogicalInterest {
        id: InterestId(seed),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    let identity =
        SubIdentity::new(SubOwnerKey::new(seed), SubKey::new(seed), SubScope::Global);
    kernel.open_interest_sub(identity, interest)
}

/// Seed one event straight into the store (bypassing the ingest admission gate)
/// so the store holds it BEFORE any interest exists — the #2088 precondition.
fn store_seed(kernel: &Kernel, id: &str, pubkey: &str, kind: u32, ts: u64, tags: Vec<Vec<String>>) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at: ts,
        kind,
        tags,
        content: String::new(),
        sig: "a".repeat(128),
    };
    kernel
        .event_store_handle()
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &"wss://seed.relay/".to_string(),
            ts * 1000,
        )
        .expect("store seed insert");
}

fn tag_shape(kinds: &[u32], letter: &str, value: &str) -> InterestShape {
    let mut s = InterestShape {
        kinds: kinds.iter().copied().collect(),
        ..Default::default()
    };
    let mut tags = BTreeMap::new();
    tags.insert(letter.to_string(), BTreeSet::from([value.to_string()]));
    s.tags = tags;
    s
}

/// #2088 — group events seeded BEFORE the `#h` interest opens hydrate from the
/// store with zero relay delivery.
#[test]
fn group_h_tag_hydrates_from_store() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let base_ts: u64 = 1_700_000_000;

    // Seed the store: kind:9 + kind:11 group events tagged #h="room",
    // plus off-room / off-kind noise that must NOT hydrate.
    let id9 = hex_pk("9a");
    let id11 = hex_pk("11b");
    let author = hex_pk("c0ffee");
    store_seed(&kernel, &id9, &author, 9, base_ts,
        vec![vec!["h".into(), "room".into()]]);
    store_seed(&kernel, &id11, &author, 11, base_ts + 1,
        vec![vec!["h".into(), "room".into()]]);
    // noise: right kind, wrong room.
    store_seed(&kernel, &hex_pk("dd"), &author, 9, base_ts + 2,
        vec![vec!["h".into(), "elsewhere".into()]]);

    // Cold start: in-memory caches are empty (store is warm) — exactly the
    // restart precondition #2088 hits.
    simulate_cold_restart(&mut kernel);
    assert!(kernel.events.is_empty());

    // Open the group interest → enqueues cache-serve; drain with ZERO relay.
    open_interest(&mut kernel, 901, tag_shape(&[9, 11], "h", "room"));
    drain_cache_serves(&mut kernel, 10);

    assert!(
        kernel.events.contains_key(id9.as_str()),
        "#2088: kind:9 group event must hydrate via #h cache-serve"
    );
    assert!(
        kernel.events.contains_key(id11.as_str()),
        "#2088: kind:11 group event must hydrate via #h cache-serve"
    );
    assert!(
        !kernel.events.contains_key(hex_pk("dd").as_str()),
        "#2088: an off-room event must NOT hydrate"
    );
}

/// Latent `#t` hashtag back-fill: stored `kind:1 #t=nostr` notes back-fill from
/// the store when the hashtag feed opens.
#[test]
fn hashtag_t_tag_backfills_from_store() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let base_ts: u64 = 1_700_000_000;
    let author = hex_pk("a11ce");

    let id_a = hex_pk("7a");
    let id_b = hex_pk("7b");
    store_seed(&kernel, &id_a, &author, 1, base_ts,
        vec![vec!["t".into(), "nostr".into()]]);
    store_seed(&kernel, &id_b, &author, 1, base_ts + 1,
        vec![vec!["t".into(), "nostr".into()]]);
    // noise: different hashtag.
    store_seed(&kernel, &hex_pk("7c"), &author, 1, base_ts + 2,
        vec![vec!["t".into(), "bitcoin".into()]]);

    simulate_cold_restart(&mut kernel);
    assert!(kernel.events.is_empty());

    open_interest(&mut kernel, 902, tag_shape(&[1], "t", "nostr"));
    drain_cache_serves(&mut kernel, 10);

    assert!(
        kernel.events.contains_key(id_a.as_str()),
        "#t back-fill: stored #t=nostr note A must hydrate from the store"
    );
    assert!(
        kernel.events.contains_key(id_b.as_str()),
        "#t back-fill: stored #t=nostr note B must hydrate from the store"
    );
    assert!(
        !kernel.events.contains_key(hex_pk("7c").as_str()),
        "#t back-fill: a #t=bitcoin note must NOT hydrate for a #t=nostr feed"
    );
}
