//! ADR-0063 D7 (#1671 Lane H) — feed-author auto-resolve + guardrail tests.
//!
//! Proves the coverage-hole closure bites:
//! - a feed tick with N visible authors auto-resolves N profile refs through the
//!   SAME `resolve_ref` path (`profile_claims` grows by N under the feed
//!   consumer id);
//! - scrolling (the visible set shrinking) RELEASES the dropped authors —
//!   bounded, no leak (D5/D8);
//! - closing a feed RELEASES-ALL — the transient author/thread feed via the
//!   provider-removal sweep, AND the PERMANENT home feed (the #1 leak risk) when
//!   its provider is force-swept;
//! - dedup with an explicit `Live`/`Card` claim: one slot, the explicit claim
//!   survives the feed release and keeps the slot Live (Lane B "Live wins");
//! - the debug guardrail flags an emitted-but-unresolved author and stays silent
//!   for a normally auto-resolved (even content-empty) author.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use super::super::refs::{ProfileShape, RefLiveness, RefNamespace, RefShape};
use super::super::snapshot_registry::new_snapshot_projection_slot;
use super::super::*;
use super::feed_author_consumer_id;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use nmp_network::role::RelayRole;

const HOME_KEY: &str = "test.feed.home";

fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

/// A kernel with a snapshot slot bound, plus a connected relay so resolves
/// register a fetch interest (not just a cache-serve).
fn kernel_with_slot() -> (
    Kernel,
    super::super::snapshot_registry::SnapshotProjectionSlot,
) {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    let slot = new_snapshot_projection_slot();
    kernel.set_snapshot_projection_handle(Arc::clone(&slot));
    (kernel, slot)
}

/// Register a feed-author provider whose returned set is swappable at test time
/// via the shared `Arc<Mutex<Vec<String>>>` handle.
fn register_swappable_provider(
    slot: &super::super::snapshot_registry::SnapshotProjectionSlot,
    feed_key: &str,
) -> Arc<Mutex<Vec<String>>> {
    let authors = Arc::new(Mutex::new(Vec::<String>::new()));
    let authors_for_closure = Arc::clone(&authors);
    slot.lock()
        .expect("registry lock")
        .register_feed_author_provider(feed_key, move || {
            authors_for_closure.lock().expect("authors lock").clone()
        });
    authors
}

fn set_authors(handle: &Arc<Mutex<Vec<String>>>, keys: &[String]) {
    *handle.lock().expect("authors lock") = keys.to_vec();
}

// ─── N visible authors auto-resolve N refs through resolve_ref ────────────────

#[test]
fn feed_tick_auto_resolves_each_visible_author() {
    let (mut kernel, slot) = kernel_with_slot();
    let authors = register_swappable_provider(&slot, HOME_KEY);
    let consumer = feed_author_consumer_id(HOME_KEY);

    let a = hex64("aaaa1");
    let b = hex64("bbbb2");
    let c = hex64("cccc3");
    set_authors(&authors, &[a.clone(), b.clone(), c.clone()]);

    kernel.reconcile_feed_author_refs();

    // Each visible author is now claimed under the feed consumer id — the SAME
    // `profile_claims` refcount the explicit `resolve_ref` path
    // uses (origin-blind: one path).
    for pk in [&a, &b, &c] {
        let consumers = kernel.profile_claims.get(pk).expect("author claimed");
        assert!(
            consumers.contains(&consumer),
            "author {} must be claimed under {consumer}",
            super::super::short_hex(pk)
        );
        // The feed avatar wants the `Ref` shape (not full Card).
        assert_eq!(
            kernel.ref_demanded_profile_shape(pk),
            Some(ProfileShape::Ref)
        );
    }
    // The kernel tracks exactly these three under the consumer.
    let tracked: &BTreeSet<String> = kernel
        .auto_profile_refs_by_consumer
        .get(&consumer)
        .expect("consumer tracked");
    assert_eq!(tracked.len(), 3);
}

#[test]
fn re_resolving_same_set_is_idempotent() {
    let (mut kernel, slot) = kernel_with_slot();
    let authors = register_swappable_provider(&slot, HOME_KEY);
    let consumer = feed_author_consumer_id(HOME_KEY);
    let a = hex64("aaaa1");
    set_authors(&authors, &[a.clone()]);

    kernel.reconcile_feed_author_refs();
    kernel.reconcile_feed_author_refs();
    kernel.reconcile_feed_author_refs();

    // One consumer, one slot — no refcount inflation across identical ticks.
    let consumers = kernel.profile_claims.get(&a).expect("claimed");
    assert_eq!(consumers.iter().filter(|c| *c == &consumer).count(), 1);
}

// ─── scrolling releases dropped authors (bounded, D5/D8) ─────────────────────

#[test]
fn scrolling_off_an_author_releases_its_ref() {
    let (mut kernel, slot) = kernel_with_slot();
    let authors = register_swappable_provider(&slot, HOME_KEY);
    let consumer = feed_author_consumer_id(HOME_KEY);
    let a = hex64("aaaa1");
    let b = hex64("bbbb2");

    // Frame 1: a + b visible.
    set_authors(&authors, &[a.clone(), b.clone()]);
    kernel.reconcile_feed_author_refs();
    assert!(kernel.profile_claims.contains_key(&a));
    assert!(kernel.profile_claims.contains_key(&b));

    // Frame 2: scroll so only b remains visible. `a` must be released.
    set_authors(&authors, &[b.clone()]);
    kernel.reconcile_feed_author_refs();
    assert!(
        !kernel.profile_claims.contains_key(&a),
        "scrolled-off author must release its slot (no leak)"
    );
    assert!(
        kernel.profile_claims.contains_key(&b),
        "still-visible author retained"
    );
    let tracked = kernel.auto_profile_refs_by_consumer.get(&consumer).unwrap();
    assert_eq!(tracked.len(), 1);
}

// ─── closing a feed releases ALL its refs (the leak guard bites) ─────────────

/// The PERMANENT home feed is the #1 leak risk: prove a force-sweep
/// (release-all) tears down every ref it auto-resolved.
#[test]
fn closing_permanent_home_feed_releases_all_refs() {
    let (mut kernel, slot) = kernel_with_slot();
    let authors = register_swappable_provider(&slot, HOME_KEY);
    let consumer = feed_author_consumer_id(HOME_KEY);
    let a = hex64("aaaa1");
    let b = hex64("bbbb2");
    set_authors(&authors, &[a.clone(), b.clone()]);
    kernel.reconcile_feed_author_refs();
    assert_eq!(kernel.profile_claims.len(), 2);

    // Close the home feed: release-all by consumer id.
    kernel.release_all_feed_author_refs(&consumer);

    assert!(
        kernel.profile_claims.is_empty(),
        "release-all on the permanent home feed must drop EVERY auto-resolved ref"
    );
    assert!(
        kernel
            .auto_profile_refs_by_consumer
            .get(&consumer)
            .is_none(),
        "the consumer's tracking entry is gone after release-all"
    );
}

/// A transient author/thread feed closes via provider removal; the next
/// reconcile sweep (no live provider for that consumer) releases-all.
#[test]
fn unregistering_transient_feed_provider_releases_all_on_next_tick() {
    let (mut kernel, slot) = kernel_with_slot();
    let thread_key = "test.feed.thread.deadbeef";
    let authors = register_swappable_provider(&slot, thread_key);
    let consumer = feed_author_consumer_id(thread_key);
    let a = hex64("a11ce");
    let b = hex64("b0b22");
    set_authors(&authors, &[a.clone(), b.clone()]);
    kernel.reconcile_feed_author_refs();
    assert_eq!(kernel.profile_claims.len(), 2);

    // Transient feed closes: provider removed (the `unregister_feed` action).
    assert!(slot.lock().unwrap().remove_feed_author_provider(thread_key));

    // Next tick: the consumer has no live provider, so the sweep releases all.
    kernel.reconcile_feed_author_refs();
    assert!(
        kernel.profile_claims.is_empty(),
        "a provider that vanished must release every ref it held (transient-feed leak guard)"
    );
    assert!(kernel
        .auto_profile_refs_by_consumer
        .get(&consumer)
        .is_none());
}

/// Direct release-by-feed-key seam. NOTE: `unregister_feed` does NOT call this
/// today — it removes the provider and relies on the NEXT reconcile sweep to
/// release-all (see `unregister_feed` in `nmp-ffi/src/lib.rs` and
/// `unregistering_transient_feed_provider_releases_all_on_next_tick` above). This
/// seam is the IMMEDIATE-teardown alternative a future `ActorCommand` teardown
/// can call for same-call release; this test exercises it directly.
#[test]
fn release_by_feed_key_drops_the_consumers_refs() {
    let (mut kernel, slot) = kernel_with_slot();
    let author_key = "test.feed.author.cafef00d";
    let authors = register_swappable_provider(&slot, author_key);
    let a = hex64("a5511");
    set_authors(&authors, &[a.clone()]);
    kernel.reconcile_feed_author_refs();
    assert!(kernel.profile_claims.contains_key(&a));

    kernel.release_feed_author_refs_for_feed(author_key);
    assert!(kernel.profile_claims.is_empty());
}

// ─── dedup with an explicit Live/Card claim (one slot, Live wins) ────────────

#[test]
fn feed_ref_dedupes_with_explicit_live_card_claim() {
    let (mut kernel, slot) = kernel_with_slot();
    let authors = register_swappable_provider(&slot, HOME_KEY);
    let a = hex64("aaaa1");

    // A profile screen explicitly claims `a` at Card / Live FIRST.
    kernel.resolve_ref(
        RefNamespace::Profile,
        a.clone(),
        "open-profile-screen".to_string(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    // The feed ALSO surfaces `a` (Ref / CacheOk).
    set_authors(&authors, &[a.clone()]);
    kernel.reconcile_feed_author_refs();

    // ONE slot, TWO consumers (origin-blind dedup) — the per-pubkey
    // `profile_claims` set holds both owners.
    let consumers = kernel.profile_claims.get(&a).expect("claimed");
    assert_eq!(consumers.len(), 2, "feed + explicit screen share one slot");
    // Widest shape wins: Card (the screen) over Ref (the feed).
    assert_eq!(
        kernel.ref_demanded_profile_shape(&a),
        Some(ProfileShape::Card)
    );
    // Live wins: the slot stays Tailing while the screen's Live owner holds.
    assert!(
        kernel.live_profile_claims.contains_key(&a),
        "the explicit Live claim keeps the shared slot Live"
    );

    // The feed scrolls `a` off: the feed's CacheOk owner releases, but the
    // explicit Live/Card claim KEEPS the slot alive (no premature teardown).
    set_authors(&authors, &[]);
    kernel.reconcile_feed_author_refs();
    let consumers = kernel
        .profile_claims
        .get(&a)
        .expect("explicit claim still holds the slot");
    assert_eq!(consumers.len(), 1);
    assert_eq!(
        kernel.ref_demanded_profile_shape(&a),
        Some(ProfileShape::Card),
        "the surviving explicit claim still demands Card / Live"
    );
}

// ─── the debug guardrail bites ───────────────────────────────────────────────

/// With the auto-resolve helper wired, every reconciled author has live demand,
/// so the guardrail is SILENT — even for an author whose kind:0 has not yet
/// arrived (the normal empty-profile async gap is NOT a guardrail hit).
#[test]
fn guardrail_silent_for_normally_resolved_authors() {
    let (mut kernel, slot) = kernel_with_slot();
    let authors = register_swappable_provider(&slot, HOME_KEY);
    let consumer = feed_author_consumer_id(HOME_KEY);
    let a = hex64("aaaa1");
    set_authors(&authors, &[a.clone()]);
    kernel.reconcile_feed_author_refs();

    // `a` was just resolved → demand is Some even though no kind:0 has arrived
    // (content is empty). The guardrail must NOT flag this normal async gap.
    let live: BTreeSet<String> = [consumer].into_iter().collect();
    assert!(
        kernel.unresolved_feed_authors(&live).is_empty(),
        "a freshly auto-resolved (content-empty) author is NOT a guardrail hit"
    );
}

/// The guardrail FIRES when an author is in a feed's reconciled set but has NO
/// resolver demand — the future regression of a surface emitting a pubkey
/// WITHOUT routing it through `resolve_ref`. Simulated by force-releasing the
/// resolver slot out from under the feed's tracking entry.
#[test]
fn guardrail_fires_for_emitted_but_unresolved_author() {
    let (mut kernel, slot) = kernel_with_slot();
    let authors = register_swappable_provider(&slot, HOME_KEY);
    let consumer = feed_author_consumer_id(HOME_KEY);
    let a = hex64("aaaa1");
    set_authors(&authors, &[a.clone()]);
    kernel.reconcile_feed_author_refs();
    let live: BTreeSet<String> = [consumer.clone()].into_iter().collect();
    assert!(kernel.unresolved_feed_authors(&live).is_empty());

    // Simulate a surface that rendered `a` (so it stays in the consumer's
    // tracked set) while its resolver demand was torn down out of band — the
    // exact "rendered without a live resolve" hole the guardrail catches.
    kernel.release_ref(RefNamespace::Profile, &a, &consumer);
    kernel
        .auto_profile_refs_by_consumer
        .entry(consumer.clone())
        .or_default()
        .insert(a.clone());
    assert!(kernel.ref_demanded_profile_shape(&a).is_none());

    let hits = kernel.unresolved_feed_authors(&live);
    assert_eq!(
        hits.len(),
        1,
        "guardrail must flag the emitted-but-unresolved author"
    );
    assert_eq!(hits[0], (consumer, a));
}

// ─── the EMITTED-SET guardrail bites (BLOCKING 2) ────────────────────────────

/// The structural emitted-set guardrail FIRES when a feed's typed producer
/// EMITTED an author onto the wire that has NO live resolver demand — even
/// though that author is INVISIBLE to the provider-tracking check (it was never
/// in `auto_profile_refs_by_consumer`). This is the "missed provider / missed
/// FeedAuthorRefs field" hole BLOCKING 2 closes: the prior guardrail iterated
/// only the kernel's own provider tracking, so a feed with no provider (or a
/// provider whose set missed a rendered field) was invisible to it.
#[test]
fn emitted_set_guardrail_fires_for_unresolved_emitted_author() {
    let (kernel, slot) = kernel_with_slot();
    let tick_rev = slot.lock().unwrap().frame_tick_rev_handle();
    let rev = tick_rev.load(std::sync::atomic::Ordering::Acquire);

    // A feed's typed producer EMITTED `a` onto the wire this tick (recorded into
    // the sink) but NOTHING ever resolved it — no provider, no resolve_ref.
    let a = hex64("dead1");
    let consumer = feed_author_consumer_id("test.feed.author.deadbeef");
    slot.lock()
        .unwrap()
        .record_emitted_feed_authors(rev, consumer.clone(), [a.clone()]);

    // The emitted-set guardrail flags it (provider-tracking guardrail would NOT —
    // there is no `auto_profile_refs_by_consumer` entry for this consumer).
    assert!(
        kernel
            .unresolved_feed_authors(&[consumer.clone()].into_iter().collect())
            .is_empty(),
        "provider-tracking guardrail is blind to a feed with no provider"
    );
    let hits = kernel.emitted_unresolved_feed_authors(rev);
    assert_eq!(
        hits.len(),
        1,
        "emitted-set guardrail must flag the emitted-but-unresolved author"
    );
    assert_eq!(hits[0], (consumer, a));
}

/// The emitted-set guardrail is SILENT for an author that WAS resolved (demand
/// is `Some`) even though its `kind:0` content has not arrived — the normal
/// async gap is not a guardrail hit.
#[test]
fn emitted_set_guardrail_silent_for_resolved_emitted_author() {
    let (mut kernel, slot) = kernel_with_slot();
    let tick_rev = slot.lock().unwrap().frame_tick_rev_handle();
    let rev = tick_rev.load(std::sync::atomic::Ordering::Acquire);

    let a = hex64("beef2");
    let consumer = feed_author_consumer_id(HOME_KEY);
    // Resolve `a` (as the structural pairing would), then record it as emitted.
    kernel.resolve_ref(
        RefNamespace::Profile,
        a.clone(),
        consumer.clone(),
        RefShape::Profile(ProfileShape::Ref),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    slot.lock()
        .unwrap()
        .record_emitted_feed_authors(rev, consumer, [a.clone()]);

    assert!(
        kernel.emitted_unresolved_feed_authors(rev).is_empty(),
        "an emitted author with live demand (content-empty) is NOT a guardrail hit"
    );
}

// ─── load_older widening WITHIN a tick does not blank newly-visible authors ───

/// The HIGH fix, at the kernel seam: a feed's author provider and its typed
/// producer share ONE per-tick materialized window (the `FeedWindowSource`
/// memoization is unit-tested in `nmp-feed`; this asserts the kernel CONTRACT
/// that consumes it). When the provider resolves window W and the typed producer
/// emits the SAME window W at the same tick rev, the reconciled author set ==
/// the emitted author set and the emitted-set guardrail is SILENT — i.e. no
/// newly-visible row's author is left unresolved for a frame. The pre-fix bug
/// (two independent `snapshot_current_window` reads, one widened by `load_older`
/// between them) would make the emitted set a SUPERSET of the resolved set,
/// which the emitted-set guardrail would then flag.
#[test]
fn load_older_widening_within_tick_does_not_blank_authors() {
    use std::sync::atomic::Ordering as O;
    let (mut kernel, slot) = kernel_with_slot();
    let tick_rev = slot.lock().unwrap().frame_tick_rev_handle();

    let a = hex64("aaaa1");
    let b = hex64("bbbb2");

    // Model the SHARED per-tick window: ONE author vector that the provider and
    // the emit-record both read for the tick. (With `FeedWindowSource` this is the
    // memoized `Arc<S>`; here we model the kernel-visible consequence directly.)
    let window = register_swappable_provider(&slot, HOME_KEY);
    // Pre-widen window: only `a` is visible when the tick materializes.
    set_authors(&window, &[a.clone()]);

    // Tick: bump rev (as make_update), reconcile from the provider...
    slot.lock().unwrap().bump_frame_tick_rev();
    let rev = tick_rev.load(O::Acquire);
    kernel.reconcile_feed_author_refs();
    // ...and record the SAME window as emitted at the SAME rev (what the typed
    // producer does off the shared materialization). A pre-fix gap would instead
    // record the WIDENED set [a, b] here while the provider resolved only [a].
    let consumer = feed_author_consumer_id(HOME_KEY);
    let shared_window = window.lock().unwrap().clone();
    slot.lock()
        .unwrap()
        .record_emitted_feed_authors(rev, consumer.clone(), shared_window.clone());

    // The resolved set == the emitted set: `b` is neither emitted-yet-unresolved
    // nor resolved-yet-unemitted. No 1-frame blank.
    let resolved: BTreeSet<String> = kernel
        .auto_profile_refs_by_consumer
        .get(&consumer)
        .expect("consumer tracked")
        .clone();
    let emitted: BTreeSet<String> = shared_window.into_iter().collect();
    assert_eq!(
        resolved, emitted,
        "resolved set == emitted window (no load_older gap)"
    );
    assert!(resolved.contains(&a) && !resolved.contains(&b) && resolved.len() == 1);
    // The emitted-set guardrail is silent (every emitted author has demand).
    assert!(
        kernel.emitted_unresolved_feed_authors(rev).is_empty(),
        "no emitted author lacks demand when provider + emit share one window"
    );
}

// ─── the in-tick reconcile fires from make_update (no 1-frame gap) ───────────

/// Driving `make_update` (the real snapshot tick) reconciles the feed authors
/// IN-TICK — proving the resolve registers in the SAME frame the row appears,
/// not a tick later.
#[test]
fn make_update_reconciles_feed_authors_in_tick() {
    let (mut kernel, slot) = kernel_with_slot();
    let authors = register_swappable_provider(&slot, HOME_KEY);
    let a = hex64("aaaa1");
    set_authors(&authors, &[a.clone()]);

    // Before any tick, nothing is claimed.
    assert!(kernel.profile_claims.is_empty());
    let _frame = kernel.make_update(true);
    // After ONE make_update the author is already resolved (in-tick).
    assert!(
        kernel.profile_claims.contains_key(&a),
        "make_update must auto-resolve the visible author in the same tick"
    );
}
