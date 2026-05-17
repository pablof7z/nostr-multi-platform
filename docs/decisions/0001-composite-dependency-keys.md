# ADR 0001: Composite dependency keys as primary reverse-index entries

**Date:** 2026-05-17
**Status:** accepted
**Supersedes:** initial design in `reactivity.md` rev 0 §3.2

## Context

The initial reactivity design (`reactivity.md` rev 0 §3.2) proposed registering each view under independent axis buckets — `by_kind`, `by_author`, `by_e_tag`, etc. — and unioning matches on insert. The reactivity-bench harness (run 001, 2026-05-17) showed this produces severe false wakes:

- 98% false-wakeup rate in quiet_idle.
- 49% false-wakeup rate in following_timeline_scroll.

The naive union wakes every view sharing any single axis with the event, regardless of whether the conjunction of axes matches.

## Decision

Register each view under the **most specific composite key** its `Dependencies` declaration supports. Conjunctive dependencies are the default; single-axis registration is reserved for views with genuinely broad filters (search, hashtag scan) and triggers a `nmp-guardrails` warning in debug builds.

| View shape | Primary index |
|---|---|
| `kinds + authors` | `by_kind_author[(k, a)]` cartesian product |
| `kinds + e-tag refs` | `by_kind_e_tag[(k, e)]` |
| `kinds + p-tag refs` | `by_kind_p_tag[(k, p)]` |
| `kinds + d-tag refs` (parameterized replaceable) | `by_kind_author_d[(k, a, d)]` |
| `kinds` only | `by_kind[k]` — broad-cost flag |
| `authors` only | `by_author[a]` — broad-cost flag |
| no constraint | `catch_all` — explicit guardrail warning |

On insert, the event generates its tuple signature (every `(kind, axis-value)` pair it implies), and lookup is the union of small sets. False wakes go to near-zero in well-shaped views.

## Consequences

- Index registration size grows by the product of axis sizes for a view. A timeline with 1k authors × 3 kinds inserts 3k composite entries (vs ~1k under the v0 model). Acceptable; far smaller than the working-set memory budget.
- Empty composite buckets are free (never inserted).
- Single-axis registrations are guardrailed but legal.
- False-wakeup rate becomes a first-class quality gate (ADR-0005? — actually `reactivity.md` §10.3 update).

## Alternatives considered

- **Union of axis buckets (v0 design).** Rejected — 98% false-wake in quiet_idle.
- **Trie/B-tree by attribute prefix.** Premature; HashMap is sufficient.
- **Per-event run all view filters.** O(views × inserts); doesn't scale.

## Validation

Re-run reactivity-bench after implementation; require `false_wakeup_rate ≤ 0.10` and `candidates_per_delta ≤ 1.25` on all scenarios.
