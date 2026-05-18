# Codex Review — 8fdc2ff

**Subject**: feat(subs): wire apply_selection + set_indexer_relays into recompile
**Batch**: HB60-HB66
**Date**: 2026-05-18
**Verdict**: NEEDS-FIX

## Summary

Wires `apply_selection()` into `SubscriptionLifecycle::recompile_and_diff()`, adds
`set_indexer_relays()` to `SubscriptionLifecycle`, introduces selection budget
controls (`set_selection_budget`, `select_max_connections`, `select_max_per_user`),
and adds new tests for `apply_selection` wiring including dropped-relay CLOSE behavior.

## Rubric

### Correctness

1. **Wire-diff relay-blindness** (HIGH): `plan_diff` is keyed by `sub_id` only, not
   `(relay_url, sub_id)`. Because `sub_id_for` uses only `canonical_filter_hash`
   (ignoring relay URL), removing one relay when the same filter remains on another
   relay will not emit a `CLOSE` for the dropped relay. The new `dropped_relay_emits_close_on_next_recompile`
   test uses different authors/filters for each relay, so each has a unique `sub_id`
   and the bug is not exercised. The common case that `apply_selection` is meant to
   prune — two relays with overlapping author coverage — is exactly where this fails.
   Fix: diff relay-local identities, e.g. `(relay_url, sub_id)`. See
   `wire.rs:56` and `wire.rs:130`.

2. **Case-D wildcard regression** (HIGH): wiring `apply_selection()` into every
   lifecycle recompile silently drops wildcard-only plans (Case D: no-author,
   hashtag/global subscriptions). `apply_selection` drops relays whose only
   contribution is wildcard coverage because it scores by per-author coverage.
   The new `set_indexer_relays` test avoids asserting the resulting plan for exactly
   this reason, masking the regression. See `case_d_no_author.rs` and
   `selection.rs:58`.

3. **Zero-budget foot-gun**: `set_selection_budget(0, 0)` is a public API that, when
   called, would drop all relays. No production callers yet, but if exposed to
   config/FFI a zero budget silently emits no subscriptions. Use `NonZeroUsize` or
   clamp at 1.

### Tests

Tests validate the greedy pruning path with all-unique filters. Missing:

- A test where two relays share the same filter hash — the dropped relay must still
  emit a CLOSE (directly exercises the `(relay_url, sub_id)` diff correctness).
- A Case D (no-author/hashtag) interest passing through `apply_selection` without
  losing its indexer relay.
- Assertions on emitted `WireFrame`s in dead-relay/dropped-relay tests (current tests
  assert `current_plan` shape, not the frames).

### Architecture Fit

Wiring `apply_selection` before the wire-emitter diff is the correct place per the
4-stage pipeline. The `coverage_hook` ordering is correct for the M4 NIP-77 hook.
The hook type is an arbitrary closure, so the non-expansion contract should be
documented if third-party hooks become real.

### Follow-Ups

- Fix `plan_diff` to scope by `(relay_url, sub_id)`.
- Guard `apply_selection` against Case-D wildcard plans (exempt relays with no
  per-author obligations from selection pruning, or make selection Case-D-aware).
- Use `NonZeroUsize` for selection budget params or clamp/reject zero.

## Findings

1. `plan_diff` is keyed by `sub_id` only, not `(relay_url, sub_id)` — dropping a
   relay that shares a filter hash with a surviving relay will not emit CLOSE.
   `wire.rs:56` and `wire.rs:130`.
2. `apply_selection` wired into every recompile silently drops wildcard-only (Case D)
   plans; `set_indexer_relays` test avoids asserting the plan to mask this regression.
3. Coverage-hook ordering is OK for the current M4 hook but the non-expansion
   contract is undocumented.
4. `set_selection_budget(0, 0)` is a foot-gun; use `NonZeroUsize` before FFI exposure.
5. `multi_relay_emits_identical_rewritten_since` using `usize::MAX` budget is
   acceptable for isolation but obscures budget-sensitive behavior.
6. Not recomputing `plan_id` is consistent with the post-compile mutator discipline.
