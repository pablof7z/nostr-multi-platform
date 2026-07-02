//! Issue #2766 regression — the exact release→reclaim-before-emit interleaving
//! that a from-zero per-key rev counter can misreport as `Unchanged`.
//!
//! Split out of `refs_glue_integration_tests.rs` (AGENTS.md 500-LOC hard cap);
//! reuses that file's `#[cfg(test)]` helpers (`hex64`, `inject_kind0`,
//! `emit_and_apply`, `full_snapshot`, `kernel_with_incremental`) via
//! `pub(super)`, so there is no duplicated resolver-driving logic. Pulled in
//! via `#[path]` from `kernel::update` for the same reason as its sibling.

use super::super::refs::{ProfileShape, RefLiveness, RefNamespace, RefShape};
use super::super::typed_projections::decode_profile;
use super::refs_glue_integration_tests::{
    emit_and_apply, full_snapshot, hex64, inject_kind0, kernel_with_incremental,
};
use crate::refs::RefRowCache;

/// The documented `RefRowRevSource` contract is monotonic THROUGH release: a
/// released key's rev never restarts from zero, so a later reclaim can never
/// collide with a rev already emitted to a host. Sequence: claim alice → emit
/// (host caches "Alice") → release alice with NO emit → reclaim alice + a NEW
/// kind:0 ("Alice v2") with NO intervening emit → emit. On the pre-fix
/// production `SourceVersions` the reclaim's per-key counter restarted at 1,
/// which the host had already seen (or a rev <= the cached rev), so
/// `build_incremental` omitted the row as Unchanged and the host cache kept
/// the stale "Alice" payload — this assertion FAILS on that code. Post-fix,
/// the per-key rev is drawn from the never-rewinding per-namespace sequence,
/// so the reclaim's rev is strictly greater than the pre-release rev, the row
/// ships as `Changed`, and the cache reflects "Alice v2".
#[test]
fn release_then_reclaim_before_emit_is_not_reported_unchanged() {
    let (mut kernel, _slot) = kernel_with_incremental();
    let mut profile_cache = RefRowCache::new();
    let mut event_cache = RefRowCache::new();
    let alice = hex64("a11ce2766");

    // ── tick 0: baseline frame, nothing resolved yet ─────────────────────────
    emit_and_apply(&mut kernel, &mut profile_cache, &mut event_cache);

    // (a) claim alice + ingest her first kind:0.
    kernel.resolve_ref(
        RefNamespace::Profile,
        alice.clone(),
        "view-a".into(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    inject_kind0(&mut kernel, &alice, "Alice");

    // (b) emit — the host caches alice at "Alice".
    emit_and_apply(&mut kernel, &mut profile_cache, &mut event_cache);
    let cached = profile_cache
        .get("profile", &alice)
        .expect("alice cached after first emit");
    assert_eq!(
        decode_profile(&cached)
            .expect("decode ProfileCard")
            .display_name
            .as_deref(),
        Some("Alice"),
        "host cache holds the first resolution"
    );

    // (c) release alice — NO emit follows (the release→reclaim window).
    kernel.release_ref(RefNamespace::Profile, &alice, "view-a");

    // (d) reclaim alice + a NEW kind:0 — still NO emit yet.
    kernel.resolve_ref(
        RefNamespace::Profile,
        alice.clone(),
        "view-a2".into(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    inject_kind0(&mut kernel, &alice, "Alice v2");

    // (e) emit — the reclaimed+re-resolved row must ship as Changed, not be
    // omitted as Unchanged against the stale pre-release rev.
    emit_and_apply(&mut kernel, &mut profile_cache, &mut event_cache);
    assert_eq!(
        profile_cache.snapshot("profile"),
        full_snapshot(&kernel, "profile"),
        "incremental == full across a release-then-reclaim-before-emit interleaving"
    );
    let cached = profile_cache
        .get("profile", &alice)
        .expect("alice still cached after reclaim");
    assert_eq!(
        decode_profile(&cached)
            .expect("decode ProfileCard")
            .display_name
            .as_deref(),
        Some("Alice v2"),
        "the reclaimed resolution must NOT be reported Unchanged — a stale \
         pre-release value must not survive in the host cache (issue #2766)"
    );
}
