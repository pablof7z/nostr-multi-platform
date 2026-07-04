---
type: research-record
date: 2026-07-03
session: 91a86fdf-624c-446e-9b38-0fb02085121f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/91a86fdf-624c-446e-9b38-0fb02085121f.jsonl
source_lines: 985-1022
agent_attribution: wallet-spine
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Wallet-spine subagent completion report: codex-design-first build of journal/reducer/trail spine with 4 pre-registered invariants mapped to tests, 4 bugs caught by codex review, 23+2 integration tests passing, compat aliases hard-broken"
captured_at: 2026-07-03T10:28:13Z
---

Wallet-spine subagent completion report: codex-design-first build of journal/reducer/trail spine with 4 pre-registered invariants mapped to tests, 4 bugs caught by codex review, 23+2 integration tests passing, compat aliases hard-broken

---

<teammate-message teammate_id="wallet-spine" color="yellow" summary="Wallet journal spine PR #2876 open, #2871 closed">
Done. PR: https://github.com/pablof7z/nostr-multi-platform/pull/2876 (ready for review, not draft). #2871 closed in favor of it (comment posted explaining why).

## What shipped
Reworked `crates/nmp-wallet/src/journal.rs` (linear saga only) into `crates/nmp-wallet/src/journal/{mod,saga,fact,fact_privacy,ledger,ledger_state,trail}.rs` per the three-concern design (money-safety saga / event-sourced WalletFact reducer / causal trail), plus `crates/nmp-wallet/tests/journal_reconcile.rs` for the crash/restart acceptance tests.

## Codex design decisions (from `codex exec`, problem-first)
- File split: saga.rs / fact.rs / trail.rs / ledger.rs (+ledger_state.rs, split later purely for the 500-LOC hard cap).
- `TokenAdded` carries `proofs: Vec<ProofAtom>` not a flat amount, so the reducer owns proof-set membership, not just token-event membership.
- Saga emits an explicit `WalletSagaEvent` from `transition()` (not a callback/drain) — one-directional producer wiring into `WalletFact::from`.
- `rebuild_from` is a distinct entry point from `apply()`, folding seeds into one `StateRebuilt` genesis fact.
- Privacy test: exhaustive-match + sealed marker trait over field types, not a runtime-only check.

## Invariants → tests
- Confluence: `ledger::tests::confluence_delete_before_add_still_tombstones` (+ `rebuild_from_is_confluent_regardless_of_seed_order`, added after a codex-review-caught bug — see below).
- Ring-never-authority: `rebuild_from_records_one_state_rebuilt_genesis_fact`.
- Type-level privacy: `fact_privacy::tests::every_wallet_fact_field_is_privacy_safe_by_type` + JSON-substring smoke test.
- Mint-probe-found-spent: `mint_probe_found_spent_excludes_only_that_proof_from_balance` (+ `mint_probe_spent_verdict_is_absorbing`, added after codex found Spent wasn't monotonic).
- Crash-after-MintSettled / restart-reconcile: both in `tests/journal_reconcile.rs`.

## Codex review pass caught 4 real must-fix bugs before I opened the PR (all fixed + regression-tested)
1. `rebuild_from` wasn't actually tombstone-confluent — it bypassed the guard `fold()` used, so a tombstone-seed-then-live-seed for the same token would resurrect it. Fixed by routing both through shared `WalletDerivedState::apply_token_live/apply_token_tombstone`.
2. `MintProbed` was last-writer-wins — a stale `Spent -> Unspent` probe would resurrect balance. Made `Spent` absorbing.
3. The privacy module's doc comments overclaimed "type-level" enforcement (nothing stops `ProofRef::new(a_real_secret)` from compiling). Reworded to state precisely what is/isn't guaranteed.
4. `transition()` returned `Result<Option<WalletSagaEvent>,_>` — a caller could `.unwrap();` and silently drop the event. Collapsed to `Result<WalletSagaEvent,_>` since every real transition is non-no-op; verified empirically that `#[must_use]` on the bare struct actually warns at unwrap-and-discard sites (it does not propagate through `Option`, which is exactly why the Option had to go).

## Compat-alias hard-break
`ACTION_LEGACY_NWC_CONNECT`/`ACTION_LEGACY_NWC_DISCONNECT` deleted; there's now exactly one `ACTION_NWC_CONNECT`/`ACTION_NWC_DISCONNECT` pair using nmp-nip47's real current names (`nmp.wallet.connect`/`nmp.wallet.disconnect`). I did NOT rename these to the aspirational `nmp.wallet.nwc.connect`/`nwc.disconnect` from the design doc's Product Surface list, because nothing implements that name today — nmp-nip47 is the only real backend, under the current names. Renaming requires moving nmp-nip47's ActionModule + wire-schema registration, which is epic #2864 Phase 2 (NWC consolidation) and crosses into nmp-nip47's lane. `pay_invoice` was untouched — it's already the single canonical name per the design doc, not an alias pair. Added a test (`no_action_namespace_is_duplicated_as_a_compatibility_alias`) asserting no two wallet action constants share a string value.

## #2871 disposition
Closed. It only had the linear saga (not the #2869 design), baked in the compat-alias pair, and was CI-red on `rule_e_classifies_every_crates_manifest: unmapped: nmp-wallet` — a layer-classification gap (fixed in this PR by registering nmp-wallet as L4 in rule_e.rs + crate-boundaries.md), not anything wallet-logic-specific.

## Escalating, did not touch
1. **docs/architecture/nip60-nip61-wallet-design.md contradiction**: its Product Surface section (from #2869, still unmerged/open) lists `nmp.wallet.nwc.connect`/`nwc.disconnect` as canonical with today's names as aliases "while migration is in progress" — but no real nwc.* implementation exists to migrate to yet. Whoever owns #2869 should reconcile this before/when it merges (either soften the aliasing language to be phase-scoped, or explicitly defer to Phase 2). I didn't edit that file per your instructions.
2. **nmp-nip47/src/ownership.rs**: I checked for the ownership-claim collision I initially worried about (nip47 and nmp-wallet both scoping "nmp.wallet.pay_invoice") — it's NOT actually a collision; `workspace_ownership_audit_is_clean` passes because the two claims use different claim_types (mechanism vs. namespace) and the ownership audit doesn't flag that as a cross-crate collision. No nip47 changes needed or made.

All required gates green: `cargo test -p nmp-wallet` (23 unit + 2 integration), `cargo test -p nmp-nip60` (unaffected), `cargo test -p nmp-testing --test doctrine_lint_smoke` (203), `cargo test -p nmp-testing --test layer_inversion_doctrine_lint` (8, the one #2871 was failing), `cargo build --workspace`, clippy clean, fmt applied. Worktree cleaned up.
</teammate-message>
