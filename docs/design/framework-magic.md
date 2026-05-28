# Design: Framework Magic Contract — Things That Just Work

> **Status:** Draft. Research citations folded in from `docs/research/applesauce/event-store-query-builders.md` and `docs/research/ndk/kind3-auto-tracking.md`. Doctrine wording aligned with `docs/product-spec/overview-and-dx.md` §1.5 (D0–D10 canonical set).
> **Date:** 2026-05-18.
> **Source directives:** user product directive that background framework behavior should "just work"; `docs/product-spec/overview-and-dx.md` §1.5 (cardinal doctrines D0–D10) + §3.3 (bug-class extinction); `docs/product-spec/subsystems.md` §7.1–§7.8.
> **Companion test file:** `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per contract bullet plus a coverage meta-test; layout in [test-scaffolding.md](framework-magic/test-scaffolding.md)).
> **Scope:** Enumerate every behavior the framework guarantees so the application does not have to author code for it. The user directive is explicit: *"apps shouldn't have to care or know about these operations happening in the background, things should just work."* This document is the contract; the test suite is the proof; the milestone implementations are the substrate.

This document is split into focused sub-files to stay under the 300 LOC ceiling (`AGENTS.md`) and to let milestone owners revise one chapter at a time without merge contention.

## Section map

- [Intro — purpose, doctrine alignment, per-bullet template, how this contract evolves](framework-magic/intro.md)
- [Kind:3 auto-tracking — follow-list change recompiles dependent subscriptions](framework-magic/kind3.md) (C5)
- [Replaceable & delete invariants — supersession, parameterized supersession, kind:5, NIP-40](framework-magic/replaceable.md) (C1–C4)
- [Outbox routing — read fan-out, write fan-out, private events fail closed](framework-magic/outbox.md) (C6, C7)
- [Subscriptions — dedup, coalesce, auto-close, buffered batches](framework-magic/subs.md) (C8)
- [Sync & provenance — watermarks, redelivery merge](framework-magic/sync.md) (C9)
- [Signers & onboarding — bunker://, nsec creation, Keychain persistence](framework-magic/signers.md) (C11)
- [Sessions — account switch = state, view rebuild without imperative dance](framework-magic/sessions.md) (C12)
- [Capabilities & rendering — best-effort placeholders, in-place refinement](framework-magic/capabilities.md) (C13)
- [Test scaffolding — the `framework_magic_contract.rs` harness, naming convention, coverage meta-test](framework-magic/test-scaffolding.md)

## The 13 contract bullets

Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.

> **Ground-truth note (reconciled 2026-05-18, PD-006).** Status cells below are
> derived from a real run of the proof target, not from milestone-doc state:
> `cargo test -p nmp-testing --test framework_magic_contract` →
> **14 tests; 14 pass; 0 fail; 0 ignored.**
> All gating milestones (M2/M3/M4/M6/M8) are DONE on master and every test is
> active (no `#[ignore]` remains — the un-ignore landed in commit `79e0257`).
> `[DONE]` ⇒ test active **and passing**; `[PARTIAL]` ⇒ test active but failing
> or only partially enforcing the bullet; `[PENDING M_n]` ⇒ test ignored/gated
> (none remain). Line numbers are `file:line` of the `fn` as it exists on
> master today; the cross-cutting sub-files live under
> `crates/nmp-testing/tests/framework_magic_contract/`.

| # | Behavior | Sub-file | Test name | Status · Milestone · `file:line` | Doctrine / spec |
|---|---|---|---|---|---|
| C1 | Replaceable-event supersession (kind 0 / 3 / 10000–19999) on insert | replaceable.md | `c1_replaceable_supersedes_on_insert` | **[DONE]** · kernel/M3 · `c1_c4_c6_c9.rs:39` | spec §7.1 row "Replaceable kinds"; §3.3 bug #1 |
| C2 | Parameterized replaceable supersession (30000–39999) by `(pubkey, kind, d-tag)` | replaceable.md | `c2_parameterized_replaceable_supersedes_by_dtag` | **[DONE]** · M3 · `c1_c4_c6_c9.rs:87` | spec §7.1 row "Parameterized replaceable"; §3.3 bug #1 |
| C3 | Kind:5 delete propagation: referenced events removed, tombstone persisted | replaceable.md | `c3_kind5_delete_removes_referenced_and_tombstones` | **[DONE]** · M3 · `c1_c4_c6_c9.rs:134` | spec §7.1 row "Kind 5 (delete)" |
| C4 | NIP-40 expiration auto-removes event at expiry; survives actor restart | replaceable.md | `c4_nip40_expiration_removes_and_persists_schedule` | **[DONE]** · M3 · `c1_c4_c6_c9.rs:178` | spec §7.1 row "NIP-40 expiration" |
| C5 | Kind:3 auto-tracking: active account's follow-list change recompiles dependent subscriptions transparently | kind3.md | `c5_kind3_change_recompiles_follow_dependent_subs` | **[DONE]** · M2 · `c5_c8_c13.rs:64` | D3; M2 design §4 (`CompileTrigger::FollowListChanged` A11, landed `001ebf6`) |
| C6 | Outbox read routing: `authors`-filter subscriptions fan out to those authors' write relays (NIP-65), de-duplicated | outbox.md | `c6_authors_subscription_routes_to_per_author_write_relays` | **[DONE]** · M2 · `c1_c4_c6_c9.rs:229` | D3; spec §7.3 row "Subscription with `authors`"; M2 design §7 |
| C7 | Outbox write routing: publishes go to author write + `#p`-recipient inbox; private (gift-wrap) events fail closed when recipient inbox is unknown | outbox.md | `c7_publish_routes_outbox_and_private_fails_closed` | **[DONE]** · M6 · `c7_c11.rs:67` | D3; spec §7.3 rows "Publish*"; §3.3 bugs #3, #4 |
| C8 | Subscription planner deduplicates overlapping interests into one wire REQ per relay, auto-closes on EOSE / last-consumer-drop, and buffers ingress to ≤60Hz per view | subs.md | `c8_subscriptions_coalesce_autoclose_and_buffer` | **[DONE]** · M2 · `c5_c8_c13.rs:133` | spec §7.2; §3.3 bug #2, bug #8 |
| C9 | Provenance preserved: same event id arriving from N relays merges into one stored event with N-entry provenance set; original `id` and signature untouched | sync.md | `c9_provenance_merges_across_relay_redeliveries` | **[DONE]** · M3 · `c1_c4_c6_c9.rs:279` | aim §6 doctrine 10; spec §7.1 row "Provenance"; §3.3 bug #10 |
| C11 | Signer onboarding: pasted `bunker://` URL parses + connects via NIP-46; "create new nsec" generates, NIP-49-encrypts, and persists via KeyringCapability — both as kernel actions, no app code | signers.md | `c11_bunker_url_and_nsec_creation_complete_via_actions` | **[DONE]** · M6 · `c7_c11.rs:159` (¹) | spec §7.4 |
| C12 | Account switch is a state transition: dispatching the switch action re-resolves every `ActiveAccount`-scoped view without the app issuing CLOSE/REQ or rebuilding view handles | sessions.md | `c12_account_switch_rebinds_views_without_imperative_dance` | **[DONE]** · M8 · `c12.rs:53` | D4; spec §7.4; §3.3 bug #5; M2 §4 trigger A4 |
| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** · M2/M3 · `c5_c8_c13.rs:238` (²) | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |

**Footnotes.**

1. **C11 caveat (status unaffected).** The test passes and exercises the real
   primitives (`parse_bunker_uri`, `LocalKeySigner::generate`,
   `AccountManager::add`). The `KeyringCapability` / `IdentityModule` kernel
   action-module wrapper is still a substrate-layer gap tracked by
   `#57-c11-keyring` — an internal wiring caveat, **not** a bullet downgrade.
   Similarly C5's registry push that expands the author set is a synthetic
   stand-in for the M11 ViewModule rebuild; the trigger, ingest fan, and
   `drain_tick` routing it exercises are real.
2. **C13 is now `[DONE]`.** The D1 placeholder substrate
   (`Placeholder<T>` newtype, `picture_placeholder`, ADR-0017) and the actor
   projection path now satisfy the active proof target. The prior RED note is
   retained in git history, not in the current status table.

**Bullet count:** 13 (eleven came from the original user directive that framework behavior should "just work"; two — **C3** kind:5 delete propagation and **C4** NIP-40 expiration — derive from `product-spec/subsystems.md` §7.1 because they are guaranteed invariants of the same insert path and the contract is incomplete without them).

**Test names** (13 behavior tests + 1 coverage meta-test = 14 total, all in `crates/nmp-testing/tests/framework_magic_contract.rs` — see [test-scaffolding.md](framework-magic/test-scaffolding.md) for the harness):

1. `c1_replaceable_supersedes_on_insert`
2. `c2_parameterized_replaceable_supersedes_by_dtag`
3. `c3_kind5_delete_removes_referenced_and_tombstones`
4. `c4_nip40_expiration_removes_and_persists_schedule`
5. `c5_kind3_change_recompiles_follow_dependent_subs`
6. `c6_authors_subscription_routes_to_per_author_write_relays`
7. `c7_publish_routes_outbox_and_private_fails_closed`
8. `c8_subscriptions_coalesce_autoclose_and_buffer`
9. `c9_provenance_merges_across_relay_redeliveries`
10. `c11_bunker_url_and_nsec_creation_complete_via_actions`
11. `c12_account_switch_rebinds_views_without_imperative_dance`
12. `c13_view_payload_uses_placeholders_then_refines_in_place`
13. `contract_surface_complete` — **meta-test**; asserts every behavior listed in this index has a corresponding `#[test] fn` in `framework_magic_contract.rs` (ignored or not). Drift between this doc and the test file fails the build.

(C10 was removed when the `nmp-nip77` crate was deleted — zero shipping callers. The substrate seam it pinned — `PlanCoverageHook` / `set_coverage_hook` — remains, independently covered by `nmp-core`'s `subs::coverage_hook_tests`.)

All remaining gating milestones have landed, so **no `#[ignore]` remains** — all 13 tests are active (the un-ignore landed in `79e0257`; per-chapter split under `framework_magic_contract/`). The `#[ignore = "pending M_n"]` mechanism is retained for *future* bullets whose milestone has not yet landed (see the "How to add C14" recipe); the meta-test counts ignored tests too, so the historical doc-vs-code drift the cross-cutting (non-milestone-prefixed) file-naming convention guards against still cannot regress — see [test-scaffolding.md](framework-magic/test-scaffolding.md) §1.

## How this contract evolves

Every milestone owner adds a **"framework-magic delta"** subsection to their exit-gate report. The delta names:

1. Which contract bullets transitioned from `[PENDING M_n]` to `[DONE]`.
2. Which `#[ignore = "pending M_n"]` lines were removed from `framework_magic_contract.rs`.
3. Whether the contract gained or lost a bullet during the milestone (rare; bullet count changes need an ADR).

The orchestrator's heartbeat triage rule includes a "framework-magic regression" gate: any milestone landing that *removes* a bullet without an ADR fails review.

### How to add C14 (mini-recipe)

Adding a contract bullet is a four-step, drift-safe sequence. The
`contract_surface_complete` meta-test (`framework_magic_contract.rs:39`) is the
guardrail: it fails the build if the doc table and the test file disagree, so
do steps 1 and 2 in the **same commit**.

1. **Doc table row.** Add a `| C14 | … |` row to *The 13 contract bullets*
   table above (the count in the section title and the prose tally below it
   become 14). Column 4 **must** be the backticked test name exactly —
   `` `c14_<behavior>` `` — because the meta-test parses only that column
   (`framework_magic_contract.rs:59-74`); status/milestone/doctrine columns are
   free-form. Add the bullet to the numbered *Test names* list too, and to a
   sub-file (new or existing) that specifies the behavior.
2. **Test fn.** Add `#[test] fn c14_<behavior>()` to the right per-chapter
   file under `crates/nmp-testing/tests/framework_magic_contract/` (group by
   owning milestone, keeping each file ≤300 LOC; create a new `cN.rs` module
   and `pub mod` it in `framework_magic_contract.rs` if no chapter fits). Add
   the name to `EXPECTED_TESTS` in `framework_magic_contract.rs` so the
   meta-test's two-way check (doc⊆expected, expected⊆doc, equal length) stays
   green.
3. **Gate honestly.** If the owning milestone has not landed, check the test in
   with `#[ignore = "pending M_n"]` and set the row status to `[PENDING M_n]`.
   When the milestone lands, the owner removes the `#[ignore]`, runs
   `cargo test -p nmp-testing --test framework_magic_contract`, and flips the
   row to `[DONE]` **only if the test passes** (`[PARTIAL]` if active-but-red —
   see footnote 2; never inflate a red test to `[DONE]`).
4. **ADR for count change.** A bullet-count change (13→14) needs an ADR per the
   regression gate above; reference it from the row's doctrine cell.

## Research citations (resolved)

The following items were pending research; citations are now concrete per `docs/research/ndk/kind3-auto-tracking.md` and `docs/research/applesauce/event-store-query-builders.md`:

- `kind3.md` §3 — NDK's session layer opens a long-lived REQ for active-user events (including kind:3) at `sessions/src/store.ts:184-194`; kind:3 is processed in `handleContactListEvent` at `store.ts:492-512`, which updates `session.followSet`. Core NDK has **no** automatic open-subscription rewire on follow-list change; Svelte gets it via runes (`subscription.svelte.ts:164-177`), React requires explicit deps (`subscribe.ts:110`). NMP's kernel fills this gap with `Trigger::FollowListChanged` (C5).
- `kind3.md` §4 — Applesauce's query-builder magic is `EventModels.model(Constructor, ...args)` at `event-models.ts:50-86`, backed by `share({resetOnRefCountZero: timer(60_000)})`. The `OutboxModel` composition at `models/outbox.ts:14-24` uses `switchMap` into per-contact `ReplaceableModel(kind:10002)` instances — when a kind:3 arrives, `ContactsModel` re-emits, `OutboxModel` switchMaps the new contact list, and every downstream relay-set consumer updates automatically. NMP's `ViewModule.dependencies()` + `Trigger::FollowListChanged` is the analog.
- `outbox.md` §2 — NDK's relay auto-add on NIP-65 arrival is `refreshRelayConnections` at `core/src/ndk/index.ts:458-471` + `subscription/index.ts:787-812`. It **only adds** relays (never removes) and is triggered by NIP-65, not kind:3. NMP's wire-emitter diff (CLOSE + REQ delta) is strictly more correct.
- `subs.md` §3 — Applesauce's logical-vs-wire split: `EventModels.model()` at `event-models.ts:50-86` is the logical layer (one shared pipeline per `(constructor, args)` hash); the underlying `EventStore.insert$` / `remove$` streams are the wire layer. NMP's `LogicalInterest` (`subscription-compilation/intro.md` §2.1) covers the same split.
- `sync.md` §4 — Applesauce's watermark equivalent is the `claimLatest` / `claimEvents` refcount pair (`observable/claim-latest.ts`, `claim-events.ts`) plus the `EventMemory` LRU touch on claim (`event-memory.ts:188`). Coverage-awareness is implicit in the `eventLoader` fallback (`event-store.ts:102-104`): if the event is not in memory and the loader returns nothing, the miss is treated as authoritative for that pointer. NMP's explicit `(filter_sig, relay)` watermark is a more precise analog.

## Non-goals

- This document does **not** specify HOW the framework implements each behavior — that lives in the milestone design doc named in the table.
- This document does **not** duplicate `docs/product-spec/subsystems.md` §7.1 invariants — `replaceable.md` references the rows; it does not restate them.
- This document does **not** introduce new types or traits — `PublishPlanner`, `ViewModule`, `LogicalInterest`, `SubscriptionCompiler`, `MailboxCache`, `KeyringCapability` are already defined in the cited design docs and product spec. The contract uses those names.
- This document does **not** describe the proof app, the starter app, or the kernel-substrate trait families — those are the substrate the contract holds the framework to.

The contract's job is exactly: *enumerate what the app does not have to do, name where the framework does it, name the test that proves it.* Nothing more.
