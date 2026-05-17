# Design: Framework Magic Contract — Things That Just Work

> **Status:** Draft. Research citations folded in from `docs/research/applesauce/event-store-query-builders.md` and `docs/research/ndk/kind3-auto-tracking.md`. Doctrine wording aligned with `docs/product-spec/overview-and-dx.md` §1.5 (D0–D8 canonical set).
> **Date:** 2026-05-18.
> **Source directives:** `docs/plan/scope-adjustments-2026-05-18.md` "Framework magic contract" section; `docs/product-spec/overview-and-dx.md` §1.5 (cardinal doctrines D0–D8) + §3.3 (bug-class extinction); `docs/product-spec/subsystems.md` §7.1–§7.8.
> **Companion test file:** `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per contract bullet plus a coverage meta-test; layout in [test-scaffolding.md](framework-magic/test-scaffolding.md)).
> **Scope:** Enumerate every behavior the framework guarantees so the application does not have to author code for it. The user directive is explicit: *"apps shouldn't have to care or know about these operations happening in the background, things should just work."* This document is the contract; the test suite is the proof; the milestone implementations are the substrate.

This document is split into focused sub-files to stay under the 300 LOC ceiling (`AGENTS.md`) and to let milestone owners revise one chapter at a time without merge contention.

## Section map

- [Intro — purpose, doctrine alignment, per-bullet template, how this contract evolves](framework-magic/intro.md)
- [Kind:3 auto-tracking — follow-list change recompiles dependent subscriptions](framework-magic/kind3.md) (C5)
- [Replaceable & delete invariants — supersession, parameterized supersession, kind:5, NIP-40](framework-magic/replaceable.md) (C1–C4)
- [Outbox routing — read fan-out, write fan-out, private events fail closed](framework-magic/outbox.md) (C6, C7)
- [Subscriptions — dedup, coalesce, auto-close, buffered batches](framework-magic/subs.md) (C8)
- [Sync & provenance — watermarks, NIP-77 backfill, redelivery merge](framework-magic/sync.md) (C9, C10)
- [Signers & onboarding — bunker://, nsec creation, Keychain persistence](framework-magic/signers.md) (C11)
- [Sessions — account switch = state, view rebuild without imperative dance](framework-magic/sessions.md) (C12)
- [Capabilities & rendering — best-effort placeholders, in-place refinement](framework-magic/capabilities.md) (C13)
- [Test scaffolding — the `framework_magic_contract.rs` harness, naming convention, coverage meta-test](framework-magic/test-scaffolding.md)

## The 13 contract bullets

Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.

| # | Behavior | Sub-file | Test name | Milestone | Doctrine / spec |
|---|---|---|---|---|---|
| C1 | Replaceable-event supersession (kind 0 / 3 / 10000–19999) on insert | replaceable.md | `c1_replaceable_supersedes_on_insert` | **[DONE]** kernel | spec §7.1 row "Replaceable kinds"; §3.3 bug #1 |
| C2 | Parameterized replaceable supersession (30000–39999) by `(pubkey, kind, d-tag)` | replaceable.md | `c2_parameterized_replaceable_supersedes_by_dtag` | **[PENDING M3]** | spec §7.1 row "Parameterized replaceable"; §3.3 bug #1 |
| C3 | Kind:5 delete propagation: referenced events removed, tombstone persisted | replaceable.md | `c3_kind5_delete_removes_referenced_and_tombstones` | **[PENDING M3]** | spec §7.1 row "Kind 5 (delete)" |
| C4 | NIP-40 expiration auto-removes event at expiry; survives actor restart | replaceable.md | `c4_nip40_expiration_removes_and_persists_schedule` | **[PENDING M3]** | spec §7.1 row "NIP-40 expiration" |
| C5 | Kind:3 auto-tracking: active account's follow-list change recompiles dependent subscriptions transparently | kind3.md | `c5_kind3_change_recompiles_follow_dependent_subs` | **[PENDING M2]** | scope-adj §"Folded into M2"; D3; M2 design §4 (Trigger::Nip65Arrived analog for kind:3) |
| C6 | Outbox read routing: `authors`-filter subscriptions fan out to those authors' write relays (NIP-65), de-duplicated | outbox.md | `c6_authors_subscription_routes_to_per_author_write_relays` | **[PENDING M2]** | D3; spec §7.3 row "Subscription with `authors`"; M2 design §7 |
| C7 | Outbox write routing: publishes go to author write + `#p`-recipient inbox; private (gift-wrap) events fail closed when recipient inbox is unknown | outbox.md | `c7_publish_routes_outbox_and_private_fails_closed` | **[PENDING M2 seam → M6 publish]** | D3; spec §7.3 rows "Publish*"; §3.3 bugs #3, #4 |
| C8 | Subscription planner deduplicates overlapping interests into one wire REQ per relay, auto-closes on EOSE / last-consumer-drop, and buffers ingress to ≤60Hz per view | subs.md | `c8_subscriptions_coalesce_autoclose_and_buffer` | **[PENDING M2]** | spec §7.2; §3.3 bug #2, bug #8 |
| C9 | Provenance preserved: same event id arriving from N relays merges into one stored event with N-entry provenance set; original `id` and signature untouched | sync.md | `c9_provenance_merges_across_relay_redeliveries` | **[PENDING M3]** | aim §6 doctrine 10; spec §7.1 row "Provenance"; §3.3 bug #10 |
| C10 | Sync watermarks: planner consults `(filter, relay)` coverage before issuing historical REQ; full coverage makes cache-miss authoritative; NIP-77 negentropy is the default backfill where supported | sync.md | `c10_watermark_gates_backfill_and_authoritative_miss` | **[PENDING M4]** | D2; spec §7.1 watermarks, §7.8 sync engine |
| C11 | Signer onboarding: pasted `bunker://` URL parses + connects via NIP-46; "create new nsec" generates, NIP-49-encrypts, and persists via KeyringCapability — both as kernel actions, no app code | signers.md | `c11_bunker_url_and_nsec_creation_complete_via_actions` | **[PENDING M6]** | scope-adj §"Folded into M6"; spec §7.4 |
| C12 | Account switch is a state transition: dispatching the switch action re-resolves every `ActiveAccount`-scoped view without the app issuing CLOSE/REQ or rebuilding view handles | sessions.md | `c12_account_switch_rebinds_views_without_imperative_dance` | **[PENDING M8]** | D4; spec §7.4; §3.3 bug #5; M2 §4 trigger A4 |
| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |

**Bullet count:** 13 (eleven sourced verbatim from `scope-adjustments-2026-05-18.md`; two — **C3** kind:5 delete propagation and **C4** NIP-40 expiration — derived from `product-spec/subsystems.md` §7.1 because they are guaranteed invariants of the same insert path and the contract is incomplete without them).

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
10. `c10_watermark_gates_backfill_and_authoritative_miss`
11. `c11_bunker_url_and_nsec_creation_complete_via_actions`
12. `c12_account_switch_rebinds_views_without_imperative_dance`
13. `c13_view_payload_uses_placeholders_then_refines_in_place`
14. `contract_surface_complete` — **meta-test**; asserts every behavior listed in this index has a corresponding `#[test] fn` in `framework_magic_contract.rs` (ignored or not). Drift between this doc and the test file fails the build.

Tests for behaviors whose owning milestone has not landed are checked in with `#[ignore = "pending M_n"]`; the meta-test still counts them. This is the "doc says 13, code tests 11" regression the file-naming convention break (cross-cutting, not milestone-prefixed) is designed to support — see [test-scaffolding.md](framework-magic/test-scaffolding.md) §1.

## How this contract evolves

Every milestone owner adds a **"framework-magic delta"** subsection to their exit-gate report. The delta names:

1. Which contract bullets transitioned from `[PENDING M_n]` to `[DONE]`.
2. Which `#[ignore = "pending M_n"]` lines were removed from `framework_magic_contract.rs`.
3. Whether the contract gained or lost a bullet during the milestone (rare; bullet count changes need an ADR).

The orchestrator's heartbeat triage rule includes a "framework-magic regression" gate: any milestone landing that *removes* a bullet without an ADR fails review.

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
