//! ADR-0063 (#1671 integration glue, codex "Artifact 1") — the campaign's
//! headline gate: an end-to-end `incremental == full` integration test driving
//! the REAL seam.
//!
//! This test wires every lane together with NO stub / mock:
//! - Lane B: the kernel-owned `RefResolver` (`resolve_ref` / `release_ref`) +
//!   real kind:0 / event ingest through the production chokepoints.
//! - The glue (this PR): `impl RefRowRevSource for Kernel` + the
//!   `refs.profile` / `refs.event` producer hooked into `make_update`.
//! - Lane A: the real NRRD decoder (`decode_ref_row_delta_batch`) applied to a
//!   real `RefRowCache` incrementally across ticks.
//!
//! It asserts the four ADR-0063 invariants the whole campaign exists to protect:
//! 1. incremental-applied cache == a full snapshot of the live resolver state;
//! 2. an epoch re-baseline fully repairs a corrupted host cache;
//! 3. a released key becomes `Cleared` (not stale);
//! 4. absence != `Cleared` (a never-resolvable key leaves the cache untouched).
//!
//! Pulled in via `#[path]` from `kernel::update` so the `kernel/mod.rs`
//! god-module stays at its size baseline (same pattern as `rung3_baseline_tests`).

use std::collections::BTreeMap;
use std::sync::Arc;

use super::super::nostr::NostrEvent;
use super::super::refs::{RefLiveness, RefNamespace, RefShape};
use super::super::refs::{EventShape, ProfileShape};
use super::super::snapshot_registry::new_snapshot_projection_slot;
use super::super::typed_projections::{
    decode_claimed_events, decode_profile, REFS_EVENT_KEY, REFS_PROFILE_KEY,
};
use super::super::Kernel;
use crate::refs::{decode_ref_row_delta_batch, RefRowCache, RefRowDeltaTracker, RefRowRevSource};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::update_envelope::{decode_snapshot_envelope, decode_snapshot_typed_projections};

fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

fn inject_kind0(kernel: &mut Kernel, pubkey: &str, display_name: &str) {
    let content = serde_json::json!({
        "display_name": display_name,
        "picture": "https://example.com/a.png",
    })
    .to_string();
    kernel.inject_profile(NostrEvent {
        id: "0".repeat(64),
        pubkey: pubkey.to_string(),
        created_at: 1_700_000_000,
        kind: 0,
        tags: Vec::new(),
        content,
        sig: String::new(),
    });
}

/// A real signed kind:1 note whose hex id is the event-ref key. Built with a
/// valid signature so it passes the production `verify_and_persist` chokepoint
/// (where the event-ingest per-key rev bump lives), and lands in `self.events`
/// so `lookup_for_primary_id` resolves it by id.
fn signed_note(keys: &::nostr::Keys, body: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Timestamp};
    let ev = EventBuilder::text_note(body)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign note");
    NostrEvent {
        id: ev.id.to_hex(),
        pubkey: ev.pubkey.to_hex(),
        created_at: ev.created_at.as_secs(),
        kind: ev.kind.as_u16() as u32,
        tags: ev.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: ev.content.clone(),
        sig: ev.sig.to_string(),
    }
}

/// The decode-before-commit preflight a real host uses: a `refs.profile` row's
/// inner bytes must decode as a `ProfileCard`; a `refs.event` row's bytes must
/// decode as the single-entry `claimed_events` batch. An empty payload (a
/// `Cleared` row) is always "decodable" (it carries no bytes to commit).
fn decode_ok_for(namespace: &str) -> impl Fn(&str, &[u8]) -> bool {
    let namespace = namespace.to_string();
    move |_key: &str, payload: &[u8]| {
        if payload.is_empty() {
            return true;
        }
        match namespace.as_str() {
            "profile" => decode_profile(payload).is_ok(),
            "event" => decode_claimed_events(payload).is_ok(),
            _ => false,
        }
    }
}

/// Emit one production frame, decode its envelope + typed sidecar, and apply the
/// `refs.profile` / `refs.event` NRRD batches (when present this tick) to the two
/// host caches under the frame's `(session_id, snapshot_epoch)`. Returns nothing;
/// the caches mutate in place — exactly the host incremental-apply contract.
fn emit_and_apply(
    kernel: &mut Kernel,
    profile_cache: &mut RefRowCache,
    event_cache: &mut RefRowCache,
) {
    let frame = kernel.make_update(true);
    let envelope = decode_snapshot_envelope(&frame).expect("decode envelope");
    let typed = decode_snapshot_typed_projections(&frame).unwrap_or_default();

    for entry in &typed {
        let (cache, namespace) = match entry.key.as_str() {
            REFS_PROFILE_KEY => (&mut *profile_cache, "profile"),
            REFS_EVENT_KEY => (&mut *event_cache, "event"),
            _ => continue,
        };
        // Rung-3 may have stripped the payload + flagged Cleared, but the refs.*
        // carrier is unconditional Tier-2 so it rides as Changed with a real NRRD
        // batch (possibly empty-rows). Decode whatever bytes are present.
        if entry.payload.is_empty() {
            // An omitted/cleared projection-level entry carries no batch; nothing
            // to apply (the per-row deltas are inside the NRRD payload only).
            continue;
        }
        let batch = decode_ref_row_delta_batch(&entry.payload).expect("decode NRRD batch");
        let outcome = cache.apply(
            &batch,
            envelope.session_id,
            envelope.snapshot_epoch,
            &decode_ok_for(namespace),
        );
        assert!(
            !outcome.decode_failed,
            "decode-before-commit must never fail on a real producer batch"
        );
    }
}

/// The producer's ground-truth FULL snapshot of the live resolver state for one
/// namespace: a fresh `RefRowDeltaTracker` baseline over the kernel's own
/// `RefRowRevSource`. This is what an incrementally-applied host cache must equal.
fn full_snapshot(kernel: &Kernel, namespace: &str) -> BTreeMap<String, Vec<u8>> {
    let mut tracker = RefRowDeltaTracker::new();
    let batch = tracker.build_baseline(namespace, kernel);
    batch
        .rows
        .into_iter()
        .map(|row| (row.key, row.payload))
        .collect()
}

/// Install a kernel with a snapshot slot + incremental-apply declared (so the
/// producer omits Unchanged rows and the first frame is a full baseline).
fn kernel_with_incremental() -> (Kernel, super::super::snapshot_registry::SnapshotProjectionSlot) {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    let slot = new_snapshot_projection_slot();
    kernel.set_snapshot_projection_handle(Arc::clone(&slot));
    {
        let mut registry = slot.lock().expect("registry lock");
        registry.declare_incremental_apply();
    }
    (kernel, slot)
}

#[test]
fn incremental_equals_full_across_resolve_ingest_release_and_rebaseline() {
    let (mut kernel, _slot) = kernel_with_incremental();
    let mut profile_cache = RefRowCache::new();
    let mut event_cache = RefRowCache::new();

    let alice = hex64("a11ce");
    let bob = hex64("b0b");
    let carol = hex64("ca401");

    let note_keys = ::nostr::Keys::generate();
    let note = signed_note(&note_keys, "hello world", 1_700_000_001);
    let note_id = note.id.clone();

    // ── tick 0: baseline frame, nothing resolved yet ─────────────────────────
    emit_and_apply(&mut kernel, &mut profile_cache, &mut event_cache);
    assert!(profile_cache.baselined(), "first frame baselines the cache");
    assert!(event_cache.baselined());
    assert!(
        profile_cache.snapshot("profile").is_empty(),
        "no live profile rows yet"
    );

    // ── tick 1: resolve two profiles (Card + Ref) and one event ──────────────
    kernel.resolve_ref(
        RefNamespace::Profile,
        alice.clone(),
        "view-a".into(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    kernel.resolve_ref(
        RefNamespace::Profile,
        bob.clone(),
        "view-b".into(),
        RefShape::Profile(ProfileShape::Ref),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    kernel.resolve_ref(
        RefNamespace::Event,
        note_id.clone(),
        "embed-1".into(),
        RefShape::Event(EventShape::Raw),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    // Ingest the matching kind:0s + the event so the rows become resolvable.
    inject_kind0(&mut kernel, &alice, "Alice");
    inject_kind0(&mut kernel, &bob, "Bob");
    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.example/", "sub", note.clone());
    emit_and_apply(&mut kernel, &mut profile_cache, &mut event_cache);

    assert_eq!(
        profile_cache.snapshot("profile"),
        full_snapshot(&kernel, "profile"),
        "incremental profile cache == full snapshot after resolve+ingest"
    );
    assert_eq!(
        event_cache.snapshot("event"),
        full_snapshot(&kernel, "event"),
        "incremental event cache == full snapshot after resolve+ingest"
    );
    assert!(
        profile_cache.get("profile", &alice).is_some(),
        "alice's profile row is present"
    );
    assert!(
        event_cache.get("event", &note_id).is_some(),
        "the claimed event row is present"
    );

    // ── invariant 4: absence != Cleared. Resolve carol but NEVER ingest her ──
    // kind:0. The row is not resolvable, so the producer emits NO row for her
    // (Unchanged), and the cache must NOT hold a (stale or cleared) carol entry.
    kernel.resolve_ref(
        RefNamespace::Profile,
        carol.clone(),
        "view-c".into(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    emit_and_apply(&mut kernel, &mut profile_cache, &mut event_cache);
    assert!(
        profile_cache.get("profile", &carol).is_none(),
        "absence (unresolvable key) must NOT create a cache row (absence != Cleared)"
    );
    // carol IS live (claimed) but unresolvable, so she is correctly absent from
    // BOTH the cache and the producer's full snapshot.
    assert!(
        !full_snapshot(&kernel, "profile").contains_key(&carol),
        "an unresolvable live key is not in the full snapshot either"
    );
    assert_eq!(
        profile_cache.snapshot("profile"),
        full_snapshot(&kernel, "profile"),
        "incremental == full still holds with an unresolvable live key present"
    );

    // ── invariant 3: release bob → his row becomes Cleared (not stale) ───────
    assert!(profile_cache.get("profile", &bob).is_some(), "bob present pre-release");
    kernel.release_ref(RefNamespace::Profile, &bob, "view-b");
    emit_and_apply(&mut kernel, &mut profile_cache, &mut event_cache);
    assert!(
        profile_cache.get("profile", &bob).is_none(),
        "a released key is Cleared from the cache, not left stale"
    );
    assert_eq!(
        profile_cache.snapshot("profile"),
        full_snapshot(&kernel, "profile"),
        "incremental == full after a release"
    );

    // ── interleave: a kind:0 replacement (re-resolve content) for alice ──────
    inject_kind0(&mut kernel, &alice, "Alice v2");
    emit_and_apply(&mut kernel, &mut profile_cache, &mut event_cache);
    assert_eq!(
        profile_cache.snapshot("profile"),
        full_snapshot(&kernel, "profile"),
        "incremental == full after a profile content replacement"
    );

    // ── invariant 2: corrupt the host cache, bump epoch → full repair ────────
    profile_cache.corrupt_for_test("profile", &alice, b"GARBAGE".to_vec());
    event_cache.corrupt_for_test("event", &note_id, b"GARBAGE".to_vec());
    assert_ne!(
        profile_cache.get("profile", &alice),
        full_snapshot(&kernel, "profile").get(&alice).cloned(),
        "cache is corrupted before the rebaseline"
    );
    kernel.projection_rev_tracker.bump_epoch();
    emit_and_apply(&mut kernel, &mut profile_cache, &mut event_cache);
    assert_eq!(
        profile_cache.snapshot("profile"),
        full_snapshot(&kernel, "profile"),
        "an epoch re-baseline fully repairs the corrupted profile cache"
    );
    assert_eq!(
        event_cache.snapshot("event"),
        full_snapshot(&kernel, "event"),
        "an epoch re-baseline fully repairs the corrupted event cache"
    );
}
