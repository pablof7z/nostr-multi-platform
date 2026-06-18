# Codex review — fix/1493-p2-classify-kind (#1493)

Diff: delete the per-NIP `classify_kind` / `explicit_set_for_kind` table from
the generic `GenericOutboxRouter`; route the explicit-targets publish path
through `RoutedRelaySet::from_explicit` (kind-agnostic `Other("explicit")`
attribution). Plus the nmp-router lib.rs doc-lie fix (all 7 lanes implemented).

## Verdict: 1 blocking finding (ADDRESSED), 1 non-blocking (ADDRESSED).

- BLOCKING (addressed): the test-only `TestOutboxRouter` in
  `nmp-core/src/kernel/test_router.rs` carried an identical copy of the
  per-NIP kind→class table — keeping protocol knowledge in nmp-core and
  risking test/prod divergence. Fixed in commit 2: deleted the duplicate,
  routed through `from_explicit`.
- Non-blocking (addressed): stale lane-5 module docs in router.rs and
  tests_lanes.rs still described per-kind Search/Draft/Wiki classification.
  Refreshed.

Checklist confirmed by codex:
1. GenericOutboxRouter: relay URL selection unchanged (ctx.explicit_targets
   minus blocked); only the RoutingSource class-attribution LABEL changes.
2. Removed imports correct; no dangling refs.
3. Test rewrite valid; `cargo test -p nmp-router
   explicit_publish_lane5_attributes_other_explicit_regardless_of_kind` passed.
4. lib.rs doc accurate.

## Verification
- `cargo test -p nmp-router` — 178/178 green.
- `cargo check -p nmp-core --tests` — compiles clean.
- nmp-core full routing-seam suite deferred: shared build host hit
  "No space left on device" mid-run; CI runs the full suite. The
  test_router.rs change is a mechanical mirror of `from_explicit` already
  proven by the nmp-router lane-5 suite.
