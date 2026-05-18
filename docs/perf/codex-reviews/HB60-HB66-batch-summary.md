# Codex Review — HB60-HB66 Batch Summary

**Date**: 2026-05-18
**Commits reviewed**: 8
**Verdicts**: 0 OK / 1 PARTIAL / 7 NEEDS-FIX
**Reviewer model**: gpt-5.5 (o3 unavailable with ChatGPT account)

---

## Commit Verdicts

| SHA | Subject | Verdict |
|-----|---------|---------|
| [5634896](5634896.md) | feat(planner): app_relays + drop indexer fallback for content (T134) | NEEDS-FIX |
| [0958ced](0958ced.md) | feat(chirp/onboarding): always-visible NIP-46 section with nostrconnect:// QR + paste | NEEDS-FIX |
| [e2fe770](e2fe770.md) | feat(planner/example): outbox_perf — wire production apply_selection + personal-relay filter | NEEDS-FIX |
| [f81f735](f81f735.md) | docs(outbox): app-relay lane + indexer-discovery-only + unroutable_authors | NEEDS-FIX |
| [99d979d](99d979d.md) | fix(planner/selection): add unroutable_authors to test fixture | OK |
| [8fdc2ff](8fdc2ff.md) | feat(subs): wire apply_selection + set_indexer_relays into recompile | NEEDS-FIX |
| [ec1e205](ec1e205.md) | feat(subs): dead-relay exclusion before apply_selection | NEEDS-FIX |
| [53e99db](53e99db.md) | fix(actor/relays): add_relay now dials a real socket via ensure_relay_worker (T158) | NEEDS-FIX |

---

## Cross-Cutting Findings (appear in 3+ reviews)

### F-CROSS-1: Wire-diff relay-blindness (HIGH — blocking)

`plan_diff` in `crates/nmp-core/src/subs/wire.rs:56` builds global
`BTreeSet<sub_id>` sets, not `(relay_url, sub_id)` sets. Because `sub_id_for` uses
only `canonical_filter_hash` (ignoring the relay URL), two relays that carry the
same filter share a `sub_id`. Consequences:

- **Dead-relay exclusion** (ec1e205): marking a relay dead removes it from `per_relay`,
  but the surviving relay still contributes the same `sub_id` → no CLOSE emitted for
  the dead relay.
- **App-relay add/remove** (5634896): adding an app relay for an author already routed
  via NIP-65 produces the same `sub_id` on the new relay → REQ skipped. Removing the
  app relay → CLOSE skipped.
- **Selection pruning** (8fdc2ff): `dropped_relay_emits_close_on_next_recompile`
  uses all-unique author/relay pairs so each has a unique `sub_id` — the common
  overlapping-filter case is not exercised.

**Fix**: key the diff by `(relay_url, sub_id)`. Appears in: 5634896, 8fdc2ff, ec1e205.

### F-CROSS-2: Tests assert plan shape, not emitted WireFrames (MEDIUM)

Multiple commits add tests that check `current_plan.per_relay.len()` or relay count
but do not assert the `WireFrame::Req` / `WireFrame::Close` frames emitted by
`plan_diff`. This misses the relay-blind diff bug above. Appears in: 8fdc2ff, ec1e205.

### F-CROSS-3: Stale tests and docs conflict with new doctrine (MEDIUM)

`crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:263` still asserts
old indexer-fallback behavior for unknown-mailbox authors after T134 removed that
fallback. `docs/design/subscription-compilation/compiler.md` and
`docs/builder-guide/10-outbox-routing.md` carry conflicting claims about publish-path
indexer use. Appears in: 5634896, f81f735.

---

## Commit-Specific Highlights

### 5634896 — T134 (NEEDS-FIX)
- Compile break: `planner/selection.rs:317` missing `unroutable_authors` field
  (fixed in 99d979d but should have shipped together).
- Relay-blind wire diff (F-CROSS-1).
- Stale audit test still asserts removed behavior.

### 0958ced — NIP-46 QR (NEEDS-FIX)
- `nostrconnect_uri()` generates ephemeral key + secret then discards both; no
  relay subscription, no secret validation, no `get_public_key` call. QR is
  effectively a non-functional UI prop until Phase 2 is implemented.
- `OnboardingView.swift` is 362 LOC (300-line soft limit).

### e2fe770 — outbox_perf example (NEEDS-FIX)
- `parse_kind10002` is last-received-wins, not newest-wins (no `created_at` check).
- 786 LOC violates 500-line hard ceiling.
- `is_personal_relay()` in example only; production ingest does not apply it.

### f81f735 — docs (NEEDS-FIX)
- Publish resolver docs claim indexer is discovery-only — false: `Nip65OutboxResolver`
  still falls back to indexer when author has no kind:10002.
- "Lane 7" terminology incorrect.

### 99d979d — fixture fix (OK)
- Minimal correct fix. No issues.

### 8fdc2ff — apply_selection wiring (NEEDS-FIX)
- Relay-blind diff (F-CROSS-1).
- `apply_selection` silently drops Case-D (no-author / hashtag) indexer relays.
- `set_selection_budget(0, 0)` foot-gun.

### ec1e205 — dead-relay exclusion (NEEDS-FIX)
- Relay-blind diff (F-CROSS-1).
- Tests assert plan shape, not frames (F-CROSS-2).

### 53e99db — T158 add_relay socket fix (NEEDS-FIX)
- `RemoveRelay` does not shut down the socket created by `AddRelay` — leak.
- No actor-level integration test through dispatch.
- URL canonicalization is `trim()` only.

---

## Follow-Up Tasks (REPORT class)

| ID | Finding | Priority |
|----|---------|----------|
| T-wire-diff-relay-scope | Fix `plan_diff` to key by `(relay_url, sub_id)` instead of `sub_id` alone | HIGH |
| T-remove-relay-shutdown | `RemoveRelay` must send `RelayCommand::Shutdown` and remove from `relay_controls` | HIGH |
| T-nostrconnect-phase2 | Implement stored client-initiated NIP-46 session state in `BunkerBroker` | HIGH |
| T-case-d-selection | Guard `apply_selection` against Case-D wildcard plans | HIGH |
| T-parse-kind10002-timestamp | Fix `parse_kind10002` to compare `created_at` and keep the newest event | MEDIUM |
| T-outbox-perf-split | Split `examples/outbox_perf.rs` (786 LOC) to comply with 500-line hard ceiling | MEDIUM |
| T-personal-relay-ingest | Move `is_personal_relay()` into production ingest or document as harness-only | MEDIUM |
| T-publish-resolver-indexer | Audit `Nip65OutboxResolver`; fix or document indexer fallback in publish path | MEDIUM |
| T-relay-url-normalize | Normalize relay URLs beyond `trim()` (strip trailing slash, normalize scheme/host) | MEDIUM |
| T-stale-m2-audit-test | Update `m2_subscription_compilation_audit.rs:263` indexer-fallback assertion | LOW |
| T-selection-budget-nonzero | Use `NonZeroUsize` for `set_selection_budget` params before FFI exposure | LOW |
| T-onboardingview-split | Split `OnboardingView.swift` (362 LOC) to stay under 300-line soft limit | LOW |
