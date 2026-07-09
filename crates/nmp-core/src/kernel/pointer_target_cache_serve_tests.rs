//! #2113 — cache-first serve of a pointer-source *target* through the
//! dependent-interest set.
//!
//! The pointer-source read model (`nmp_content::PointerSourceModel`, driven by
//! `explicit_composition::op_pointer_source`) materializes each demanded target as a
//! `DependentInterestChild`. This test proves the kernel half of that path: an
//! address target child opened via `apply_dependent_interest_delta` is
//! served from the warm store with ZERO relay connectivity — the same cache-first
//! guarantee every other interest gets, never an out-of-band fetch.
//!
//! Event-id targets are intentionally *not* structural cache-serve shapes
//! (`compile_store_query_plan` returns `EventIds`); they hydrate via the
//! pointer-loader / observed-projection replay path instead. The address path is
//! the one that flows through `run_cache_serve_step`, so it is what this test
//! falsifies.

use super::cache_serve_tests::{drain_cache_serves, simulate_cold_restart};
use super::{DependentInterestChild, DependentInterestDelta, DependentInterestDeltaCommand, Kernel};
use crate::planner::{InterestScope, InterestShape, NaddrCoord};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::SubOwnerKey;
use nmp_network::role::RelayRole;
use std::collections::BTreeSet;

fn signed_addressable(
    keys: &::nostr::Keys,
    kind: u32,
    d_tag: &str,
    created_at: u64,
) -> serde_json::Value {
    use ::nostr::{EventBuilder, Kind, Tag, Timestamp};
    let ev = EventBuilder::new(Kind::from(kind as u16), "pointer target article")
        .tags(vec![Tag::parse(["d", d_tag]).expect("d tag")])
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    let tag_vecs: Vec<Vec<String>> = ev
        .tags
        .iter()
        .map(|t: &::nostr::Tag| t.as_slice().to_vec())
        .collect();
    serde_json::json!({
        "id": ev.id.to_hex(),
        "pubkey": ev.pubkey.to_hex(),
        "created_at": ev.created_at.as_secs(),
        "kind": ev.kind.as_u16(),
        "tags": tag_vecs,
        "content": ev.content.clone(),
        "sig": ev.sig.to_string(),
    })
}

#[test]
fn address_target_child_serves_from_warm_store_zero_relay() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let d_tag = "pointer-target-slug";
    let base_ts = 1_700_000_000;

    // Seed an addressable target into the store via the live ingest chokepoint.
    let article = signed_addressable(&keys, 30_023, d_tag, base_ts);
    let target_id = article["id"].as_str().unwrap().to_string();
    kernel.handle_event(RelayRole::Content, "wss://seed.relay/", "seed", &article);

    // Cold restart: keep the store warm, drop in-memory caches.
    simulate_cold_restart(&mut kernel);
    assert!(
        !kernel.events.contains_key(target_id.as_str()),
        "pre-condition: target must not be cached before serve (non-vacuous)"
    );

    // Materialize the pointer target exactly as the read-model controller does:
    // one dependent child carrying the addressable coordinate (kind + coord).
    let owner = SubOwnerKey::new("pointer-source-2113");
    let shape = InterestShape {
        kinds: BTreeSet::from([30_023]),
        addresses: BTreeSet::from([NaddrCoord {
            pubkey: author.clone(),
            kind: 30_023,
            d_tag: d_tag.to_string(),
        }]),
        ..InterestShape::default()
    };
    let child = DependentInterestChild::tailing(shape, InterestScope::Global);
    kernel.apply_dependent_interest_delta(
        owner,
        DependentInterestDelta {
            commands: vec![DependentInterestDeltaCommand::Open(child)],
        },
        "pointer-source-2113-cache-first",
    );
    drain_cache_serves(&mut kernel, 10);

    assert!(
        kernel.events.contains_key(target_id.as_str()),
        "address target ({target_id}) must be served from the warm store with no relay"
    );
}
