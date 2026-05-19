# Framework Magic §1 — Intro, Doctrine Alignment, Per-Bullet Template

> Parent: `docs/design/framework-magic.md`.
> Read first: `docs/aim.md` §6; `docs/product-spec/overview-and-dx.md` §1.5; `docs/product-spec/subsystems.md` §7.1–§7.8.

## 1. Why this contract exists

The user's framing of the framework is one sentence: *make it nearly impossible to build a broken Nostr application* (`docs/aim.md` §1). Every other design doc in this repository is an answer to *how*. This document is the only one whose job is to **enumerate the WHAT** — the operations that happen invisibly to the application, named one by one, with the test that proves each one.

The framing is the user's: *"apps shouldn't have to care or know about these operations happening in the background, things should just work."* That word *"just"* is load-bearing. It is a UX claim, not an implementation claim. It says: an LLM-driven developer or a novice, given the framework's public API, *cannot* express the broken version of any of these operations because there is no surface on which to express it.

The contract enumerates 13 such operations. Each is bound to:

1. A doctrine clause in `docs/aim.md` §6 or `docs/product-spec/overview-and-dx.md` §1.5.
2. A subsystem section in `docs/product-spec/subsystems.md` §7 that names the mechanism.
3. A milestone in `docs/plan/scope-adjustments-2026-05-18.md` that owns the implementation.
4. A test in `crates/nmp-testing/tests/framework_magic_contract.rs` that verifies the guarantee.

## 2. Doctrine alignment

The 13 contract bullets map onto the cardinal doctrines (D0–D8 in `product-spec/overview-and-dx.md` §1.5). The mapping is intentionally many-to-many — a single behavior may discharge multiple doctrines, and a single doctrine may require several behaviors to be fully discharged.

| Cardinal doctrine | Contract bullets it requires |
|---|---|
| **D0** kernel + extension modules (no app nouns in `nmp-core`) | All 13 — the contract is the API the app sees in place of the missing nouns |
| **D1** best-effort rendering — render now, refine in place | C13 (placeholders), C1–C4 (refinement triggers), C5 (kind:3 → re-render of follow-derived views) |
| **D2** negentropy first, REQ second | C10 (watermarks + NIP-77 backfill) |
| **D3** outbox routing is automatic | C5 (kind:3 → recompile), C6 (read fan-out), C7 (write fan-out + private fail-closed) |
| **D4** single writer per fact; caches derive | C9 (provenance merge), C12 (account switch as state), C13 (refinement is a re-render, not a re-fetch) |
| **D5** snapshots bounded by what's open | C8 (view-scoped subscriptions auto-close when views close; payload set bounded to open views) |
| **D6** errors never cross FFI as exceptions | C7 (publish fail → `PublishPlanError` state, not exception); C11 (signer errors → ledger state); implicitly all 13 via the actor's toast-surface design |
| **D7** capabilities report; never decide policy | C11 (KeyringCapability reports bytes, framework decides retry/activation) |
| **D8** reactivity contract: composite reverse index · ≤60 Hz/view · working-set bounded | C8 sub-path 4 (≤60Hz emit cap); C13 (in-place refinement via projection cache) |

The bug-extinction list in `product-spec/overview-and-dx.md` §3.3 (10 bug classes) is the negative formulation of the same surface: each contract bullet rules out at least one bug class structurally.

## 3. Per-bullet template

Every contract chapter (kind3 / replaceable / outbox / subs / signers / sync / sessions / capabilities) renders each bullet with the same six-field template. This is the same shape `docs/design/subscription-compilation/tests.md` uses for its four assertions, scaled to thirteen.

```
### C_n. <one-sentence statement of the guarantee>

**Framework does:** <mechanism, with file:line refs to existing code where it lives today, or to the design doc that specifies it>

**App writes:** <"nothing" — or the one-line public surface the app calls, with the namespace of the type involved>

**Failure mode prevented:** <cross-ref to bug-extinction # in §3.3, or to a named anti-pattern in aim.md / subsystems.md>

**Test:** `c_n_<snake_case>` in `crates/nmp-testing/tests/framework_magic_contract.rs`. <one sentence on what the test asserts>

**Milestone owner:** M_n (or `[DONE]`). <one sentence on what implementation status looks like>
```

The template is load-bearing for two reasons:

1. **It forces honesty.** If a chapter cannot fill the "App writes" field with `"nothing"` or a single safe call, the framework has leaked the operation to the app, and the doctrine D0 boundary is violated. The author of that chapter is required to file an ADR rather than ship the bullet as-is.
2. **It is mechanically diffable.** A milestone delta is "this row's Milestone owner changed from `[PENDING M2]` to `[DONE]` and this `#[ignore]` came off." A contract regression is "this row's `App writes` grew from `nothing` to one line; ADR required."

## 4. How this contract evolves

The contract is **append-stable, not freeze-stable.** Adding a bullet (a new "thing that just works") is allowed; removing a bullet requires an ADR; renaming a bullet requires a deprecation marker so the test name does not silently drift.

Each milestone owner adds a **framework-magic delta** subsection to their exit-gate report (the milestone-design doc's "exit gate" section). The delta is the difference set of contract bullets and test status that the milestone delivered:

- bullets moved from `[PENDING M_n]` to `[DONE]`
- `#[ignore]` markers removed
- new bullets added (with ADR ref)
- any contract-text revisions

The heartbeat triage cron (`docs/perf/orchestration-log.md`) treats a milestone landing without a framework-magic delta as a structural defect — the milestone must either touch the contract or explicitly affirm it did not.

The post-merge codex review reads this contract and the delta together. Drift between contract claims and test outcomes (e.g., the doc says `[DONE]` but the test is still `#[ignore]`) is a flagged review issue.

## 5. What this document is not

- Not an implementation plan. The milestones in the right-hand column of the index table own that.
- Not a doctrine source. The doctrines live in `docs/aim.md` §6 and `docs/product-spec/overview-and-dx.md` §1.5; this contract derives from them.
- Not the API surface. `docs/product-spec/api-surface.md` is the API surface; this contract is what the API guarantees the app does not have to call.
- Not a spec for the test harness. `crates/nmp-testing` provides the harness; [test-scaffolding.md](test-scaffolding.md) describes how the contract tests use it.

## 6. The cross-reference burden

The contract is dense in cross-references because the alternative — restating the cited material — would (a) violate the LOC ceiling, (b) drift, and (c) duplicate the existing design docs the milestones already own. Every chapter therefore reads as a thin layer over already-specified mechanism, with the contract's value-add being the **App writes / Test name / Doctrine** triple per bullet.

Reading order recommended for a reviewer:

1. The index ([framework-magic.md](../framework-magic.md)) — the 13-row table.
2. This intro.
3. The chapter for the doctrine you care about.
4. The product-spec subsystem section the chapter cites.
5. The milestone design doc if you want the implementation path.

A reader who only wants to know "what does the app not have to do?" can stop at step 1.
