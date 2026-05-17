Reading additional input from stdin...
2026-05-17T23:15:57.579361Z ERROR codex_core::session: failed to load skill /Users/pablofernandez/.agents/skills/voice-capture-sheet/SKILL.md: invalid YAML: mapping values are not allowed in this context at line 2 column 116
OpenAI Codex v0.129.0 (research preview)
--------
workdir: /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
model: gpt-5.5
provider: openai
approval: never
sandbox: workspace-write [workdir, /tmp, $TMPDIR, /Users/pablofernandez/.codex/memories]
reasoning effort: xhigh
reasoning summaries: none
session id: 019e3839-920d-70c3-b491-4fc2d11fbc62
--------
user
You are reviewing merge edc17b0 (design(framework-magic): contract + 10 sub-files + test-scaffolding (task #17)) on master in nostr-multi-platform. This adds the framework-magic design contract covering kind:3 auto-tracking, bunker:// signer, new-nsec flow, and 14 named behavioral tests. Doctrine D0-D5:
- D0: kernel never grows app nouns (nmp-core stays substrate-only; per ADR-0009)
- D1: best-effort rendering — render now, refine in place; no spinners gating renderable content
- D2: negentropy first, REQ second; every filter/relay pair is a tracked sync target
- D3: outbox routing is automatic; manual relay selection is the opt-out
- D4: single writer per fact; caches derive
- D5: snapshots bounded by what is open; AppState carries view payloads only for open views

File size: 300 LOC soft, 500 LOC hard.
Session goal: complete v1 with zero technical debt; M9 DMs + M12 Wallet deferred; M11 podcast + M11.5 Highlighter pending.

=== diff stat ===
 docs/design/framework-magic.md                  |  94 +++++++++++
 docs/design/framework-magic/capabilities.md     |  71 ++++++++
 docs/design/framework-magic/intro.md            |  93 +++++++++++
 docs/design/framework-magic/kind3.md            |  69 ++++++++
 docs/design/framework-magic/outbox.md           |  69 ++++++++
 docs/design/framework-magic/replaceable.md      |  92 +++++++++++
 docs/design/framework-magic/sessions.md         |  63 +++++++
 docs/design/framework-magic/signers.md          |  76 +++++++++
 docs/design/framework-magic/subs.md             |  48 ++++++
 docs/design/framework-magic/sync.md             |  66 ++++++++
 docs/design/framework-magic/test-scaffolding.md | 208 ++++++++++++++++++++++++
 11 files changed, 949 insertions(+)

=== commit log ===
edc17b0 design(framework-magic): contract + 10 sub-files + test-scaffolding (task #17)

=== diff (first 8000 chars) ===
diff --git a/docs/design/framework-magic.md b/docs/design/framework-magic.md
new file mode 100644
index 0000000..b853477
--- /dev/null
+++ b/docs/design/framework-magic.md
@@ -0,0 +1,94 @@
+# Design: Framework Magic Contract — Things That Just Work
+
+> **Status:** Draft (initial structure). Research-fold commit fills `TBD-from-research(...)` markers from `docs/research/applesauce/event-store-query-builders.md` and `docs/research/ndk/kind3-auto-tracking.md` when they land.
+> **Date:** 2026-05-18.
+> **Source directives:** `docs/plan/scope-adjustments-2026-05-18.md` "Framework magic contract" section; `docs/aim.md` §6 doctrines 1–12; `docs/product-spec/overview-and-dx.md` §1.5 (cardinal doctrines D0–D5) + §3.3 (bug-class extinction); `docs/product-spec/subsystems.md` §7.1–§7.8.
+> **Companion test file:** `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per contract bullet plus a coverage meta-test; layout in [test-scaffolding.md](framework-magic/test-scaffolding.md)).
+> **Scope:** Enumerate every behavior the framework guarantees so the application does not have to author code for it. The user directive is explicit: *"apps shouldn't have to care or know about these operations happening in the background, things should just work."* This document is the contract; the test suite is the proof; the milestone implementations are the substrate.
+
+This document is split into focused sub-files to stay under the 300 LOC ceiling (`AGENTS.md`) and to let milestone owners revise one chapter at a time without merge contention.
+
+## Section map
+
+- [Intro — purpose, doctrine alignment, per-bullet template, how this contract evolves](framework-magic/intro.md)
+- [Kind:3 auto-tracking — follow-list change recompiles dependent subscriptions](framework-magic/kind3.md) (C5)
+- [Replaceable & delete invariants — supersession, parameterized supersession, kind:5, NIP-40](framework-magic/replaceable.md) (C1–C4)
+- [Outbox routing — read fan-out, write fan-out, private events fail closed](framework-magic/outbox.md) (C6, C7)
+- [Subscriptions — dedup, coalesce, auto-close, buffered batches](framework-magic/subs.md) (C8)
+- [Sync & provenance — watermarks, NIP-77 backfill, redelivery merge](framework-magic/sync.md) (C9, C10)
+- [Signers & onboarding — bunker://, nsec creation, Keychain persistence](framework-magic/signers.md) (C11)
+- [Sessions — account switch = state, view rebuild without imperative dance](framework-magic/sessions.md) (C12)
+- [Capabilities & rendering — best-effort placeholders, in-place refinement](framework-magic/capabilities.md) (C13)
+- [Test scaffolding — the `framework_magic_contract.rs` harness, naming convention, coverage meta-test](framework-magic/test-scaffolding.md)
+
+## The 13 contract bullets
+
+Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
+
+| # | Behavior | Sub-file | Test name | Milestone | Doctrine / spec |
+|---|---|---|---|---|---|
+| C1 | Replaceable-event supersession (kind 0 / 3 / 10000–19999) on insert | replaceable.md | `c1_replaceable_supersedes_on_insert` | **[DONE]** kernel | spec §7.1 row "Replaceable kinds"; §3.3 bug #1 |
+| C2 | Parameterized replaceable supersession (30000–39999) by `(pubkey, kind, d-tag)` | replaceable.md | `c2_parameterized_replaceable_supersedes_by_dtag` | **[PENDING M3]** | spec §7.1 row "Parameterized replaceable"; §3.3 bug #1 |
+| C3 | Kind:5 delete propagation: referenced events removed, tombstone persisted | replaceable.md | `c3_kind5_delete_removes_referenced_and_tombstones` | **[PENDING M3]** | spec §7.1 row "Kind 5 (delete)" |
+| C4 | NIP-40 expiration auto-removes event at expiry; survives actor restart | replaceable.md | `c4_nip40_expiration_removes_and_persists_schedule` | **[PENDING M3]** | spec §7.1 row "NIP-40 expiration" |
+| C5 | Kind:3 auto-tracking: active account's follow-list change recompiles dependent subscriptions transparently | kind3.md | `c5_kind3_change_recompiles_follow_dependent_subs` | **[PENDING M2]** | scope-adj §"Folded into M2"; D3; M2 design §4 (Trigger::Nip65Arrived analog for kind:3) |
+| C6 | Outbox read routing: `authors`-filter subscriptions fan out to those authors' write relays (NIP-65), de-duplicated | outbox.md | `c6_authors_subscription_routes_to_per_author_write_relays` | **[PENDING M2]** | D3; spec §7.3 row "Subscription with `authors`"; M2 design §7 |
+| C7 | Outbox write routing: publishes go to author write + `#p`-recipient inbox; private (gift-wrap) events fail closed when recipient inbox is unknown | outbox.md | `c7_publish_routes_outbox_and_private_fails_closed` | **[PENDING M2 seam → M6 publish]** | D3; spec §7.3 rows "Publish*"; §3.3 bugs #3, #4 |
+| C8 | Subscription planner deduplicates overlapping interests into one wire REQ per relay, auto-closes on EOSE / last-consumer-drop, and buffers ingress to ≤60Hz per view | subs.md | `c8_subscriptions_coalesce_autoclose_and_buffer` | **[PENDING M2]** | spec §7.2; §3.3 bug #2, bug #8 |
+| C9 | Provenance preserved: same event id arriving from N relays merges into one stored event with N-entry provenance set; original `id` and signature untouched | sync.md | `c9_provenance_merges_across_relay_redeliveries` | **[PENDING M3]** | aim §6 doctrine 10; spec §7.1 row "Provenance"; §3.3 bug #10 |
+| C10 | Sync watermarks: planner consults `(filter, relay)` coverage before issuing historical REQ; full coverage makes cache-miss authoritative; NIP-77 negentropy is the default backfill where supported | sync.md | `c10_watermark_gates_backfill_and_authoritative_miss` | **[PENDING M4]** | D2; spec §7.1 watermarks, §7.8 sync engine |
+| C11 | Signer onboarding: pasted `bunker://` URL parses + connects via NIP-46; "create new nsec" generates, NIP-49-encrypts, and persists via KeyringCapability — both as kernel actions, no app code | signers.md | `c11_bunker_url_and_nsec_creation_complete_via_actions` | **[PENDING M6]** | scope-adj §"Folded into M6"; spec §7.4 |
+| C12 | Account switch is a state transition: dispatching the switch action re-resolves every `ActiveAccount`-scoped view without the app issuing CLOSE/REQ or rebuilding view handles | sessions.md | `c12_account_switch_rebinds_views_without_imperative_dance` | **[PENDING M8]** | D4; spec §7.4; §3.3 bug #5; M2 §4 trigger A4 |
+| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
+
+**Bullet count:** 13 (eleven sourced verbatim from `scope-adjustments-2026-05-18.md`; two — **C3** kind:5 delete propagation and **C4** NIP-40 expiration — derived from `product-spec/subsystems.md` §7.1 because they are guaranteed invariants of the same insert path and the contract is incomplete without them).
+
+**Test names** (13 behavior tests + 1 coverage meta-test = 14 total, all in `crates/nmp-testing/tests/framework_magic_contract.rs` — see [test-scaffolding.md](framework-magic/test-scaffolding.md) for the harness):
+
+1. `c1_replaceable_supersedes_on_insert`
+2. `c2_parameterized_replaceable_supersedes_by_dtag`
+3. `c3_kind5_delete_removes_referenced_and_tombstones`
+4. `c4_nip40_expiration_removes_and_persists_schedule`
+5. `c5_kind3_change_recompiles_follow_dependent_subs`
+6. `c6_authors_subscription_routes_to_per_author_write_relays`
+7. `c7_publish_routes_outbox_and_private_fails_closed`
+8. `c8_subscriptions_coalesce_autoclose_and_buffer`
+9. `c9_provenance_merges_across_relay_redeliveries`
+10. `c10_watermark_gates_backfill_and_authoritative_miss`
+11. `c11_bunker_url_and_nsec_creation_complete_via_actions`
+12. `c12_account_switch_rebinds_views_without_imperative_dance`
+13. `c13_view_payload_uses_placeholders_then_refines_in_place`
+14. `contract_surface_complete` — **meta-test**; asserts every behavior listed in this index has a corresponding `#[test] fn` in `framework_magic_contract.rs` (ignored or not). Drift between this doc and the test file fails the build.
+
+Tests for behaviors whose owning milestone has not landed are checked in with `#[ignore = "pending M_n"]`; the meta-test still counts them. This is the "doc says 13, code tests 11" regression the file-naming convention break (cross-cutting, not milestone-prefixed) is designed to support — see [test-scaffolding.md](framework-magic/test-scaffolding.md) §1.
+
+## How this contract evolves
+
+Every milestone owner adds a **"framework-magic delta"** subsection to their exit-gate report. The delta names:
+
+1. Which contract bullets transitioned from `[PENDING M_n]` to `[DONE]`.
+2. Which `#[ignore = "pending M_n"]` lines were removed from `framework_magic_contract.rs`.
+3. Whether the contract gained or lost a bullet during the milestone (rare; bullet count changes need an ADR).
+
+The orchestrator's heartbeat triage rule includes a "framework-magic regression" gate: any milestone landing that *removes* a bullet without an ADR fails review.
+
+## Open items resolved by the research-fold commit
+
+The following are `TBD-from-research(...)` markers in the sub-files; the research-fold commit replaces them with file:line refs and concrete API shapes. They are listed here so the orchestrator can sequence the work:
+
+- `kind3.md` §3 — `TBD-from-research(ndk/kind3-auto-tracking.md)`: NDK's exact mechanism for kind:3 → open-subscription recompile (event listener path, refcount handoff, race window).
+- `kind3.md` §4 — `TBD-from-research(applesauce/event-store-query-builders.md)`: Applesauce's query-builder pattern that makes `WhoFollows(active_user)` reactive without app code.
+- `outbox.md` §2 — `TBD-from-research(ndk/kind3-auto-tracking.md)`: how NDK rebinds in-flight REQs when an author's mailbox arrives mid-subscription.
+- `subs.md` §3 — `TBD-from-research(applesauce/event-store-query-builders.md)`: Applesauce's logical-vs-wire subscription split file:line refs (for cross-validation against `docs/design/subscription-compilation/intro.md` §2).
+- `sync.md` §4 — `TBD-from-research(applesauce/event-store-query-builders.md)`: Applesauce's coverage/watermark equivalent and how a query-builder reads it.
+
+None of the above blocks the *initial* structure of this contract; they refine evidence and citations in the research-fold commit. The framework-magic contract's bullets, tests, and milestone bindings are stable now.
+
+## Non-goals
+
+- This document does **not** specify HOW the framework implements each behavior — that lives in the milestone design doc named in the table.
+- This document does **not** duplicate `docs/product-spec/subsystems.md` §7.1 invariants — `replaceable.md` references the rows; it does not restate them.
+- This document does **not** introduce new types or traits — `PublishPlanner`, `ViewModule`, `LogicalInterest`, `SubscriptionCompiler`, `MailboxCache`, `KeyringCapability` are already defined in the cited design docs and product spec. The contract uses those names.
+- This document does **not** describe the proof app, the starter app, or the kernel-substrate trait families — those are the substrate the contract holds the framework to.
+
+The contract's job is exactly: *enumerate what the app does not have to do, name where the framework does it, name the test that proves it.* Nothing more.
diff --git a/docs/design/framework-magic/capabilities.md b/docs/design/framework-magic/capabilities.md
new file mode 100644
index 0000000..ddfbcfd
--- /dev/null
+++ b/docs/design/framework-magic/capabilities.md
@@ -0,0 +1,71 @@
+# Framework Magic §C13 — Best-Effort Rendering
+
+> Parent: `docs/design/framework-magic.md`.
+> Read first: `docs/product-spec/overview-and-dx.md` §1.5 doctrine D1; `docs/product-spec/subsystems.md` §7.6 (the per-field placeholder table + the `TimelineItem` concrete example); `docs/aim.md` §4.12; `docs/design/view-catalog/profile-timeline-thread-reactions.md`.
+
+The "capabilities" filename in the user's directive maps here, not to capability bridges. The rendering contract is the **rendering capability** the framework grants the app: render now, refine in place, never withhold cached data behind a spinner. (Capability bridges in the technical sense — `KeyringCapability` etc. — are covered as plumbing in C11 and `kernel-substrate.md` §5; they are not themselves a contract bullet because they are infrastructure, not an observable app guarantee.)
+
+## C13. Best-effort rendering: placeholders by construction; in-place refinement
+
+**Statement.** Every display-bearing field of every view payload is **non-`Option`** and carries either an authoritative value or a defined placeholder. When the authoritative value later arrives — a kind:0 for an author, kind:9735 zap receipts for a note, the decrypted body for a DM — the same payload re-emits with the field updated in place. The platform's reactive primitive (`@Observable` / `Flow` / signals) sees the change and only the affected cell re-renders. **No spinner ever gates an already-rendered cell, and no view module ever exposes a `loading: bool` to the platform.**
+
+**Framework does:**
+
+- The placeholder contract at `docs/product-spec/subsystems.md` §7.6 lines 181–192 (the seven-row table: display name → npub-shortened, picture → identicon URI, NIP-05 → empty string, timestamp → "just now", reaction count → 0, zap total → 0 sats, content body → empty string).
+- The view-payload typing at `subsystems.md` §7.6 lines 199–222 (the `TimelineItem` example with all fields non-`Option` except the optional `repost_of` / `quote_of` semantic-Option markers).
+- The freshness surface at `subsystems.md` §7.6 line 196 (`xxx_freshness: FreshnessHint` is an optional **sibling** field; UI may render a badge; the framework never withholds the value).
+- The in-place refinement mechanism: `ViewModule::on_projection_changed` (`docs/design/kernel-substrate.md` §3 lines 148–150). When a kind:0 lands for author X, the kernel's projection cache (a shared cross-view projection) updates X's display name; every view module that lists items by X re-runs `on_projection_changed`, produces a delta, and the wire-emitter sends a `ViewBatch` with the updated field.
+- The platform-shadow domain key (`kernel-substrate.md` §3 line 128 `fn key(spec: &Self::Spec) -> Self::Key`) ensures the cell-level re-render is targeted: the platform's reactive primitive updates only the row whose key matches, not the entire list.
+
+**App writes:** nothing. The app renders payload fields directly — `Text(item.author_display)`, `AsyncImage(url: item.author_picture)`. There is no `if has_profile { ... } else { Spinner() }` pattern because the API does not expose `has_profile`; the framework guarantees `author_display` and `author_picture` are always non-empty strings.
+
+**Failure mode prevented:** the entire class of "Nostr-client cold-start UI" bugs `subsystems.md` §1.5 D1 enumerates as ruled out by construction:
+
+- Hiding a post because the author's profile hasn't loaded yet.
+- Replacing cached profile metadata with a spinner because "we might have something newer."
+- Refusing to render threads because the root event isn't in cache.
+- Profile-picture flicker between cached and placeholder.
+
+The bug-extinction surface in `overview-and-dx.md` §3.3 does not have a single numbered bug for this because the failures are UX defects rather than data-corruption bugs, but the doctrine clause D1 is the explicit promise the contract holds.
+
+**Test:** `c13_view_payload_uses_placeholders_then_refines_in_place`. The test:
+
+1. **Placeholders at open:** open `TimelineView { authors: [alice], kinds: [1] }` against a fresh store with no kind:0 for Alice. Insert a kind:1 event by Alice. Assert the payload's `items[0]`:
+   - `author_display` matches the expected npub-shortened form for `alice_pk` (compare against `Pubkey::shortened()` output — deterministic).
+   - `author_picture` matches the expected identicon URI for `alice_pk` (deterministic from pubkey hash).
+   - `author_nip05_domain` is the empty string.
+   - `created_at_display` is "just now" (test uses `SimulatedClock` set to the event's `created_at`).
+   - `reaction_summary` has 0 reactions; `zap_sats_total` is 0; `reply_count` is 0.
+   - The payload contains **no** `loading`, `is_loaded`, `has_profile`, or `freshness_gate` field.
+2. **In-place refinement on kind:0:** insert a kind:0 for Alice with `name = "Alice"`, `picture = "https://example/alice.jpg"`, `nip05 = "alice@example.com"`. Assert the same view emits a `ViewBatch` (not a `FullState`); the `items[0]` payload now has `author_display = "Alice"`, `author_picture = "https://example/alice.jpg"`, `author_nip05_domain = "example.com"`. Assert the `id` field of `items[0]` is unchanged (same event row; the row updated, did not re-create).
+3. **In-place refinement on time:** advance the `SimulatedClock` by 5 minutes; trigger the per-tick re-format (per `kernel-substrate.md` §3 `fn on_tick` line 153). Assert `items[0].created_at_display` updates from "just now" to "5 min ago" without the row being torn down.
+4. **In-place refinement on reaction arrival:** insert a kind:7 reaction targeting the kind:1 event. Assert `items[0].reaction_summary` updates from 0 to 1 in the next `ViewBatch`; no row re-creation; `id` stable.
+5. **Freshness hint, not gate:** insert an older cached kind:0 for Alice (created two days ago), then a fresher one (created an hour ago). Assert the payload reflects the *fresher* one (per C1 supersession), and that the optional `author_display_freshness` field (if exposed by the view module) reads `Recent`, not `DaysOld`. Assert there is no API surface where the test can ask "is this stale?" and have the framework withhold the value pending re-fetch.
+
+**Milestone owner:** **[DONE]** for the placeholder shape (the M1 timeline slice already ships non-`Option` author fields with shortened-npub fallback — verified in `crates/nmp-core` timeline tests today). **[PENDING M2/M3]** for the full in-place refinement guarantees: sub-paths 1 and 5 are testable today; sub-paths 2 and 4 require the kernel's projection cache (`kernel-substrate.md` §3 line 148 `on_projection_changed`) which graduates in M2 alongside the view-module surface; sub-path 3 requires the per-tick re-format hook (`fn on_tick`, M2's `ViewModule` trait work).
+
+Test checked in **not** ignored for sub-paths 1 and 5; sub-paths 2/3/4 use a `#[cfg(feature = "m2_projection_cache")]` gate so they activate as M2 lands without a re-edit. The framework-magic delta at M2 exit removes the gate.
+
+## Why this is one bullet, not several
+
+The five sub-paths are five facets of one observable: *the payload field is always renderable, and updates appear without the row being destroyed.* Splitting them would suggest the platform might see a `ViewBatch` for kind:0 arrival but `FullState` for reaction arrival, or that some fields are non-`Option` and others are. The contract is uniform; the test enumerates the field categories that exercise it.
+
+## Doctrine alignment
+
+C13 is the canonical instance of cardinal doctrine **D1**. The doctrine clause's wording — *"There is no `if has_profile { render } else { spinner }` pattern available in the API"* — is testable through the payload shape itself, which is what sub-path 1's "no `loading` field" assertion checks. The framework cannot guarantee the app does not implement its own spinner over the payload, but it can guarantee the API does not give the app a way to ask the question that would justify one.
+
+C13 also intersects D4 (single writer per fact; caches derive). The "fact" is the projection (Alice's display name); the "caches" are every timeline cell, profile chip, thread author marker rendering that name. The in-place refinement is the derivation.
+
+## Cross-references
+
+- `docs/design/view-catalog/profile-timeline-thread-reactions.md` — the concrete view-module catalog with each view's payload shape.
+- `docs/design/reactivity/view-deltas-and-projections.md` — the projection cache that backs the cross-view refinement.
+- `docs/design/kernel-substrate.md` §3 — `ViewModule` trait including `on_projection_changed`, `on_tick`.
+- `docs/product-spec/subsystems.md` §7.6 — the placeholder table and the `TimelineItem` example.
+
+## What this chapter does not cover
+
+- **Per-view payload byte budgets.** `subsystems.md` §7.16 owns those. The contract guarantees the rendering shape; the budget is a perf concern.
+- **Cross-platform pixel-parity.** `subsystems.md` §3.5 owns the cross-platform consistency tests. C13 asserts the payload values are correct; the platforms agree to render the same payload identically.
+- **Long-form content parsing nodes.** `subsystems.md` §7.6 "Post-v1 content rendering contract" — explicitly post-v1; the v1 contract is summary-shaped payloads.
+- **DM body decryption inside the view payload.** The decrypted body fits the same C13 pattern (placeholder = empty string; in-place refinement when decrypt succeeds), but the decryption path itself is M9 territory and is not v1.
diff --git a/docs/design/framework-magic/intro.md b/docs/design/framework-magic/intro.md
new file mode 100644
index 0000000..a8415fe
--- /dev/null
+++ b/docs/design/framework-magic/intro.md
@@ -0,0 +1,93 @@
+# Framework Magic §1 — Intro, Doctrine Alignment, Per-Bullet Template
+
+> Parent: `docs/design/framework-magic.md`.
+> Read first: `docs/aim.md` §6; `docs/product-spec/overview-and-dx.md` §1.5; `docs/product-spec/subsystems.md` §7.1–§7.8.
+
+## 1. Why this contract exists
+
+The user's framing of the framework is one sentence: *make it nearly impossible to build a broken Nostr application* (`docs/aim.md` §1). Every other design doc in this repository is an answer to *how*. This document is the only one whose job is to **enumerate the WHAT** — the operations that happen invisibly to the application, named one by one, with the test that proves each one.
+
+The framing is the user's: *"apps shouldn't have to care or know about these operations happening in the background, things should just work."* That word *"just"* is load-bearing. It is a UX claim, not an implementation claim. It says: an LLM-driven developer or a novice, given the framework's public API, *cannot* express the broken version of any of these operations because there is no surface on which to express it.
+
+The contract enumerates 13 such operations. Each is bound to:
+
+1. A doctrine clause in `docs/aim.md` §6 or `docs/product-spec/overview-and-dx.md` §1.5.
+2. A subsystem section in `docs/product-spec/subsystems.md` §7 that names the mechanism.
+3. A milestone in `docs/plan/scope-adjustments-2026-05-18.md` that owns the implementation.
+4. A test in `crates/nmp-testing/tests/framework_magic_contract.rs` that verifies the guarantee.
+
+## 2. Doctrine alignment
+
+The 13 contract bullets map onto the cardinal doctrines (D0–D5 in `product-spec/overview-and-dx.md` §1.5) and onto the older `aim.md` §6 doctrines 1–12. The mapping is intentionally many-to-many — a single behavior may discharge multiple doctrines, and a single doctrine may require several behaviors to be fully discharged.
+
+| Cardinal doctrine | Contract bullets it requires |
+|---|---|
+| **D0** kernel + extension modules (no app nouns in `nmp-core`) | All 13 — the contract is the API the app sees in place of the missing nouns |
+| **D1** best-effort rendering — render now, refine in place | C13 (placeholders), C1–C4 (refinement triggers), C5 (kind:3 → re-render of follow-derived views) |
+| **D2** negentropy first, REQ second | C10 (watermarks + NIP-77 backfill) |
+| **D3** outbox routing is automatic | C5 (kind:3 → recompile), C6 (read fan-out), C7 (write fan-out + private fail-closed) |
+| **D4** single writer per fact; caches derive | C9 (provenance merge), C12 (account switch as state), C13 (refinement is a re-render, not a re-fetch) |
+| **D5** snapshots bounded by what's open | C8 (view-scoped subscriptions; buffered ≤60Hz per view) |
+
+The older `aim.md` §6 doctrines map similarly: doctrines 4 (replaceable invariants) → C1/C2/C3; doctrine 5 (outbox auto, manual is opt-out) → C5/C6/C7; doctrine 6 (subs auto-group/auto-close/auto-dedup/auto-buffer) → C8; doctrine 7 (sessions are state, switching is an action) → C11/C12; doctrine 10 (provenance preserved) → C9.
+
+The bug-extinction list in `product-spec/overview-and-dx.md` §3.3 (10 bug classes) is the negative formulation of the same surface: each contract bullet rules out at least one bug class structurally.
+
+## 3. Per-bullet template
+
+Every contract chapter (kind3 / replaceable / outbox / subs / signers / sync / sessions / capabilities) renders each bullet with the same six-field template. This is the same shape `docs/design/subscription-compilation/tests.md` uses for its four assertions, scaled to thirteen.
+
+```
+### C_n. <one-sentence statement of the guarantee>
+
+**Framework does:** <mechanism, with file:line refs to existing code where it lives today, or to the design doc that specifies it>
+
+**App writes:** <"nothing" — or the one-line public surface the app calls, with the namespace of the type involved>
+
+**Failure mode prevented:** <cross-ref to bug-extinction # in §3.3, or to a named anti-pattern in aim.md / subsystems.md>
+
+**Test:** `c_n_<snake_case>` in `crates/nmp-testing/tests/framework_magic_contract.rs`. <one sentence on what the test asserts>
+
+**Milestone owner:** M_n (or `[DONE]`). <one sentence on what implementation status looks like>
+```
+
+The template is load-bearing for two reasons:
+
+1. **It forces honesty.** If a chapter cannot fill the "App writes" field with `"nothing"` or a single safe call, the framework has leaked the operation to the app, and the doctrine D0 boundary is violated. The author of that chapter is required to file an ADR rather than ship the bullet as-is.
+2. **It is mechanically diffable.** A milestone delta is "this row's Milestone owner changed from `[PENDING M2]` to `[DONE]` and this `#[ignore]` came off." A contract regression is "this row's `App writes` grew from `nothing` to one line; ADR required."
+
+## 4. How this contract evolves
+
+The contract is **append-stable, not freeze-stable.** Adding a bullet (a new "thing that just works") is allowed; removing a bullet requires an ADR; renaming a bullet requires a deprecation marker so the test name does not silently drift.
+
+Each milestone owner adds a **framework-magic delta** subsection to their exit-gate report (the milestone-design doc's "exit gate" section). The delta is the difference set of contract bullets and test status that the milestone delivered:
+
+- bullets moved from `[PENDING M_n]` to `[DONE]`
+- `#[ignore]` markers removed
+- new bullets added (with ADR ref)
+- any contract-text revisions
+
+The heartbeat triage cron (`docs/perf/orchestration-log.md`) treats a milestone landing without a framework-magic delta as a structural defect — the milestone must either touch the contract or explicitly affirm it did not.
+
+The post-merge codex review reads this contract and the delta together. Drift between contract claims and test outcomes (e.g., the doc says `[DONE]` but the test is still `#[ignore]`) is a flagged review issue.
+
+## 5. What this document is not
+
+- Not an implementation plan. The milestones in the right-hand column of the index table own that.
+- Not a doctrine source. The doctrines live in `docs/aim.md` §6 and `docs/product-spec/overview-and-dx.md` §1.5; this contract derives from them.
+- Not the API surface. `docs/product-spec/api-surface.md` is the API surface; this contract is what the API guarantees the app does not have to call.
+- Not a spec for the test harness. `crates/nmp-testing` provides the harness; [test-scaffolding.md](test-scaffolding.md) describes how the contract tests use it.
+
+## 6. The cross-reference burden
+
+The contract is dense in cross-references because the alternative — restating the cited material — would (a) violate the LOC ceiling, (b) drift, and (c) duplicate the existing design docs the milestones already own. Every chapter therefore reads as a thin layer over already-specified mechanism, with the contract's value-add being the **App writes / Test name / Doctrine** triple per bullet.
+
+Reading order recommended for a reviewer:
+
+1. The index ([framework-magic.md](../framework-magic.md)) — the 13-row table.
+2. This intro.
+3. The chapter for the doctrine you care about.
+4. The product-spec subsystem section the chapter cites.
+5. The milestone design doc if you want the implementation path.
+
+A reader who only wants to know "what does the app not have to do?" can stop at step 1.
diff --git a/docs/design/framework-magic/kind3.md b/docs/design/framework-magic/kind3.md
new file mode 100644
index 0000000..5adc9c9
--- /dev/null
+++ b/docs/design/framework-magic/kind3.md
@@ -0,0 +1,69 @@
+# Framework Magic §C5 — Kind:3 Auto-Tracking
+
+> Parent: `docs/design/framework-magic.md`.
+> Read first: `docs/design/subscription-compilation/recompilation.md` (trigger model — kind:3 is the symmetric case to `Trigger::Nip65Arrived`); `docs/design/subscription-compilation/intro.md` §2.3 (account scope binding); `docs/plan/scope-adjustments-2026-05-18.md` §"Folded into M2".
+
+## 1. The bullet
+
+### C5. Kind:3 auto-tracking: the active account's follow-list change recompiles every dependent subscription transparently.
+
+**Framework does:**
+
+When a kind:3 event lands for the active account's pubkey *and* the replaceable-supersession rule (C1) decides it is fresher than the stored kind:3, the kernel:
+
+1. Replaces the stored kind:3 in the event store (per C1; mechanism at `crates/nmp-core/src/kernel/ingest.rs:187-207` — currently stored in `self.seed_contacts` map; M2 graduates this into the projection cache).
+2. Emits an internal planner trigger — proposed name `Trigger::FollowListChanged { account: AccountId, prev_follows: BTreeSet<Pubkey>, next_follows: BTreeSet<Pubkey> }` — symmetric to the existing `Trigger::Nip65Arrived` (`docs/design/subscription-compilation/recompilation.md` §4.1).
+3. The subscription compiler re-runs `interests()` on every `ViewModule` whose `dependencies()` declares `kind 3` *or* whose `interests()` consumes the active account's follow-set as an input to its filter shape (e.g. a "following timeline" view module).
+4. The wire-emitter diffs the new plan against the old; only the *delta* (authors added/removed from the union write-relay set) becomes CLOSE / new-REQ frames on the wire. Authors present in both old and new follow-sets see zero wire churn.
+5. The view payload's `items` recompute reactively per the standard `on_event_inserted` path (`docs/design/kernel-substrate.md` §3). No view handle is destroyed; the platform shadow's `useFollowingTimeline()` rune/observable continues to emit, just with a new payload.
+
+**App writes:** nothing. The "following timeline" view's spec does not name authors — the view module consumes the active account's follow-set internally. The app's only contact with this surface is opening `FollowingTimelineView { /* no fields */ }` and reading its `Payload.items`.
+
+**Failure mode prevented:** the canonical NDK-era bug: app code listens for kind:3 events, manually closes its open subscriptions, re-derives author lists, re-issues REQs, and either races itself (REQ ordering vs. local-state ordering) or leaks the old REQ. This contract structurally forbids that pattern: the view module never sees the kind:3 directly, and the app never issues a REQ. Specifically discharges aim.md §6 doctrine 6 ("subscriptions auto-group, auto-close, auto-dedup, auto-buffer; the developer never writes grouping/dedup/cleanup code") for the follow-list-change case.
+
+**Test:** `c5_kind3_change_recompiles_follow_dependent_subs` in `crates/nmp-testing/tests/framework_magic_contract.rs`. The test:
+
+1. Opens a `FollowingTimelineView` against an active account whose stored kind:3 follows pubkeys `{A, B, C}` with mailbox cache pre-seeded so A→relay1, B→relay2, C→relay3.
+2. Asserts the initial plan opens REQs on `{relay1, relay2, relay3}` and that the platform shadow has emitted exactly one payload.
+3. Ingests a fresher kind:3 for the active account with follows `{A, B, D}` (D's mailbox pre-seeded → relay4).
+4. Asserts the planner emitted exactly two wire frames: `CLOSE` on the relay3 slice for C, and `REQ` on relay4 for D. Crucially: no churn on relay1 (A is still there) or relay2 (B is still there).
+5. Asserts the same `FollowingTimelineView` handle is still open (refcount unchanged); the platform shadow has emitted one additional payload, not torn down and re-created.
+6. Asserts a stale kind:3 (older `created_at`) is rejected without firing the trigger — symmetric to C1 supersession; no payload re-emit.
+
+The test runs against the `PlannerHarness` introduced in `docs/design/subscription-compilation/tests.md` §9.3, extended with a `follow_set_for(account)` accessor.
+
+**Milestone owner:** **M2** (the subscription-compilation milestone owns the trigger and the recompile). M2's exit gate (`docs/design/subscription-compilation/tests.md` §9) currently lists four assertions covering the NIP-65 case; the M2 owner adds this fifth assertion as part of the framework-magic delta. Test starts as `#[ignore = "pending M2 trigger"]`; M2 lands the trigger and removes the ignore.
+
+## 2. Why kind:3 is its own bullet (not a sub-case of C1)
+
+Kind:3 is a replaceable event, so C1 already says "the stored kind:3 is the newest." The reason kind:3 deserves a separate bullet is that it is **referentially structural**: it changes which *other authors* the framework needs to subscribe to, not just which version of the kind:3 the app reads. That second-order effect — the change in the *open-subscription set* — is the one apps have historically failed at.
+
+C1 is a storage-layer invariant. C5 is a planner-layer reactive guarantee. The framework needs both.
+
+## 3. NDK reference path
+
+The user's directive in `scope-adjustments-2026-05-18.md` says: *"NDK reference: how NDK auto-follows kind:3 changes and re-routes its open subs. (Captured in M2 research wave; agents fan out.)"*
+
+The mechanism NDK uses is documented in the parallel research file `docs/research/ndk/kind3-auto-tracking.md` (pending agent landing). The contract here does not depend on NDK's specific code path; it depends on the *property* NDK demonstrates: that a kind:3 replacement re-shapes the open-REQ set without the application observing protocol churn.
+
+`TBD-from-research(ndk/kind3-auto-tracking.md)`: insert file:line ref to NDK's listener and the exact race-window it closes (specifically: what happens if a kind:3 arrives mid-EOSE on a follow-derived REQ). The contract is satisfied by *any* mechanism that produces the observable behavior in C5; NDK's path is one existence proof.
+
+## 4. Applesauce reference path
+
+`scope-adjustments-2026-05-18.md` also says: *"Applesauce reference: the 'event store query builder' magic that makes subscriptions auto-update without the app touching them. Highest-priority NDK/Applesauce lesson per user."*
+
+`TBD-from-research(applesauce/event-store-query-builders.md)`: cite the query-builder API shape that lets a consumer phrase `"things kind:1 by people I follow"` once and get a stream that re-evaluates on every kind:3 change. Applesauce's mechanism is a builder that registers itself as a dependent of the kind:3 projection; the contract's `ViewModule.dependencies()` is the NMP analog (`docs/design/kernel-substrate.md` §3 lines 131–132). The research-fold commit cross-validates that the analog covers Applesauce's pattern fully.
+
+## 5. Interaction with NIP-65 (kind:10002)
+
+A new follow (D in the test) needs a mailbox lookup. If D's kind:10002 is not in the mailbox cache, the planner's existing indexer-fallback logic (`docs/design/subscription-compilation/compiler.md` §3 Stage 2) routes D to the indexer set while concurrently fetching D's kind:10002. The fetch eventually triggers `Trigger::Nip65Arrived`, which recompiles again — moving D from the indexer slot to D's declared write relay.
+
+That second recompile is **not part of the C5 test** — it belongs to the M2 NIP-65 audit gate (test #3 in `docs/design/subscription-compilation/tests.md` §9.2). The C5 test asserts kind:3 alone caused exactly the right delta; the NIP-65 chained recompile is a separate observable that the M2 gate already covers.
+
+## 6. What this bullet does not cover
+
+- **The "following timeline" view module itself.** Its spec, payload, recompute logic live in `nmp-nip01` per `docs/design/view-catalog/profile-timeline-thread-reactions.md`. C5 cares only that *whatever view module* declares follow-set dependence gets the recompile.
+- **Mute-list changes (kind:10000).** The mute list is structurally analogous, but the user's scope-adjustments doc explicitly names kind:3. Mute-list auto-tracking would be a C5-shaped sibling bullet (potential C14 future addition); not in the v1 contract surface.
+- **Other people's follow lists.** A view module that opens kind:3 for `pubkey != active_account` is asking a one-shot question, not declaring a reactive dependency on the social graph. That path uses the normal C1 supersession; no C5 trigger fires.
+
+These exclusions keep the bullet sharp: C5 is exactly *"the active account's follow-list change re-shapes the open-subscription set."* Everything outside that sentence routes through other contract bullets.
diff --git a/docs/design/framework-magic/outbox.md b/docs/design/framework-magic/outbox.md
new file mode 100644
index 0000000..787d02f
--- /dev/null
+++ b/docs/design/framework-magic/outbox.md
@@ -0,0 +1,69 @@
+# Framework Magic §C6–§C7 — Outbox Routing
+
+> Parent: `docs/design/framework-magic.md`.
+> Read first: `docs/product-spec/subsystems.md` §7.3 (the resolution algorithm — read & write rows); `docs/design/subscription-compilation/outbox.md` (the `PublishPlanner` trait); `docs/design/subscription-compilation/compiler.md` §3 (the read-side compiler pipeline); `docs/design/ndk-applesauce-lessons.md` §9.5 (privacy-sensitive routes fail closed).
+
+Both bullets in this chapter discharge cardinal doctrine **D3** ("outbox routing is automatic; manual relay selection is the opt-out") and `aim.md` §6 doctrine 5. C7 additionally discharges `aim.md` §6 doctrine 10 ("private events cannot be accidentally republished to public relays") and `product-spec/overview-and-dx.md` §3.3 bug #4.
+
+## C6. Read fan-out: `authors`-filter subscriptions go to those authors' write relays, de-duplicated
+
+**Statement.** Any subscription whose canonical filter has a non-empty `authors` set is compiled into one wire REQ per relay in the **union of those authors' write relays** (kind:10002), with each per-relay REQ carrying only the authors that declared that relay. Authors with unknown mailboxes are routed to the configured indexer set as fallback; once their kind:10002 lands, the planner recompiles and the authors migrate to their declared relays.
+
+**Framework does:** the compilation pipeline at `docs/design/subscription-compilation/compiler.md` §3 — Stages 1 (resolve mailboxes), 2 (assign per-relay author subsets), 3 (merge sub-shapes), 4 (emit per-relay REQs). The indexer-fallback path is `RoutingSource::Indexer`; the post-NIP-65-arrival migration is `Trigger::Nip65Arrived` per `docs/design/subscription-compilation/recompilation.md` §4.2. The mailbox cache is read from the `MailboxCache` trait defined in `nmp-nip65` (`docs/design/subscription-compilation/nip65.md`).
+
+**App writes:** nothing. The view spec names authors (or, for follow-derived views, names nothing and the view module reads the active account's follow-set — see C5). The app never names a relay URL on a read path.
+
+**Failure mode prevented:** the bug `ndk-applesauce-lessons.md` §3 names: *"NDK's convenience can blur boundaries"* combined with the bug `product-spec/subsystems.md` §7.3 lines 89–90 names: *"Posts to relays the author hasn't declared as write relays."* On the read side, the symmetric failure is reading from the global content relay and missing an author's actual events because the author publishes only to their own write relay. The structural enforcement is that the view spec has no relay field; the only API surface that names a relay is the explicit override (named, audited, one-shot per `docs/design/subscription-compilation/outbox.md` §7.4).
+
+**Test:** `c6_authors_subscription_routes_to_per_author_write_relays`. This test is a **rename of and dependency on** the M2 audit gate test `timeline_compiles_to_per_relay_union` (`docs/design/subscription-compilation/tests.md` §9.2 assertion 2). The framework-magic version asserts the same observable but accesses the data through the **public view path**, not the planner harness:
+
+1. Pre-seed mailbox cache with 1000 authors using three overlapping relay sets (per the M2 test).
+2. Open `TimelineView { authors: <1000 pubkeys>, kinds: [1, 6] }` through the actor's public dispatch surface.
+3. Read the wire-emission audit log (exposed via `DebugDiagnostics`) and assert: relay count = union; per-relay author partition = subset semantics; sub-shape merge = one REQ per relay; plan-id stable on re-compile.
+4. Ingest a new kind:10002 for one author moving them off relay-1 onto relay-4; assert exactly one CLOSE-and-REQ pair fires for the affected slice; no churn for the unmoved authors.
+5. `TBD-from-research(ndk/kind3-auto-tracking.md)`: cross-check that the in-flight REQ for the moved author rebinds without losing the live tail across the CLOSE/REOPEN boundary.
+
+The "via the public view path" framing matters: M2's test exercises the compiler directly; the framework-magic test exercises the contract surface (open a view, watch the wire). Both must pass.
+
+**Milestone owner:** **[PENDING M2]**. Test checked in as `#[ignore = "pending M2 compiler + view bridge"]`. Removed in the M2 framework-magic delta.
+
+## C7. Write fan-out: outbox + recipient-inbox; private events fail closed
+
+**Statement.** Every publish action's signed event is routed by the `PublishPlanner` (`docs/design/subscription-compilation/outbox.md` §7.1) according to a `PublishPrivacy` mode the action declares. **Public** events go to author write relays. **PublicWithNotifications** events go to author writes ∪ recipient inboxes (`#p` tagged pubkeys). **PrivateToRecipients** events (gift-wrapped per NIP-59) go to **only** resolved recipient inbox relays — never the author's writes, never the active session's defaults, never the indexer set. If any recipient has no declared inbox, the publish fails closed with `PublishPlanError::PrivateRecipientUnroutable`.
+
+**Framework does:** the algorithm at `docs/design/subscription-compilation/outbox.md` §7.3 (write fan-out, all 6 numbered steps), specifically:
+
+- Step 2 forbids indexer fallback for any write path (`NoAuthorRelays` returned instead).
+- Step 3(b)'s `Indexer` check on recipient inbox lookups is the structural fail-closed for private events.
+- The `PublishWithOverride` action is the *only* `AppAction` variant carrying a `Vec<RelayUrl>` field, and it is forbidden from widening a `PrivateToRecipients` plan to public relays (`outbox.md` §7.4 rule 4).
+
+**App writes:** nothing — for the publish path. The app dispatches a publish action (`SendNote`, `React`, `SendDm`, etc.); the action's privacy mode is determined by the action type, not by an app-supplied parameter. There is no `relays` field on `SendNote`. The override exists for tests, migrations, and operator power-user flows; it is structurally outside the safe app path.
+
+**Failure mode prevented:** §3.3 bug #3 ("Publish of an event to relays the author has not declared as write relays") and bug #4 ("DM published to public relays"). Plus the doctrine-10 footgun: a "send everywhere" fallback that publishes a gift wrap to the global content relay because the recipient's inbox lookup returned empty.
+
+**Test:** `c7_publish_routes_outbox_and_private_fails_closed`. The test has three sub-paths:
+
+1. **Public:** seed Alice's mailbox with two write relays; dispatch a public `SendNote` action; assert the resulting publish plan has exactly those two relays and no others, and that `required_success_count = max(1, ceil(2/3)) = 1` per `outbox.md` §7.3 step 3(a).
+2. **PublicWithNotifications:** dispatch a note tagging Bob (Bob has one inbox relay seeded); assert the plan is Alice's writes ∪ Bob's inbox, with the correct `PublishRouteReason::AuthorWriteRelay` / `RecipientInbox` tagging per assignment.
+3. **PrivateToRecipients (fail-closed):** dispatch a (post-M9, but the planner shape is testable in isolation today) gift-wrap to Charlie, who has **no kind:10002**. Assert the publish plan errors with `PublishPlanError::PrivateRecipientUnroutable { recipient: charlie }` and that **no wire EVENT frame is emitted on any relay** — checked by reading the relay worker's outbound audit log.
+4. **Override rejection:** dispatch a `PublishWithOverride` carrying a `PrivateToRecipients` inner action and an override relay set that includes a non-inbox URL; assert it rejects with `PublishPlanError::OverrideRejected { reason: "private widen" }` (rule 4 of `outbox.md` §7.4).
+5. **Override audit:** dispatch a `PublishWithOverride` on a public action; assert the side-effect lane emits `Diagnostic::PublishOverrideUsed { ... }` and the debug log line per `outbox.md` §7.4 (3).
+
+**Milestone owner:** **[PENDING M2 seam → M6 publish]**. M2 lands the `PublishPlanner` trait + `Nip65PublishPlanner` + the `PublishWithOverride` action (`docs/design/subscription-compilation/outbox.md` §7.1, §7.2, §7.4). M6 lands `SendNoteAction` as the first concrete consumer. Test checked in as `#[ignore = "pending M2 planner + M6 first consumer"]`. Sub-paths 3 and 4 of the test exercise the planner in isolation (M2-completable); 1, 2, and 5 require M6's action consumer.
+
+## The two bullets together discharge D3
+
+C6 covers the read side; C7 covers the write side. Together they discharge cardinal doctrine D3 in full — every relay-touching operation routes through framework policy, and the only API surfaces that name a relay URL are:
+
+- the `PublishWithOverride` action (write path, audited);
+- the planner's diagnostic accessors (read-only);
+- the user-configured-relays settings surface (configuration, not per-operation).
+
+The app's domain code, view modules, and action modules **never** name a relay. That is the doctrine-D3 boundary the contract holds in place.
+
+## What this chapter does not cover
+
+- The publish-fail retry/back-off policy — that's M6 territory (`docs/design/subscription-compilation/outbox.md` §7.6 deferred items). The contract's fail-closed guarantee is structural (the wire frame is never emitted), not about how long the system retries before giving up.
+- The action ledger row schema — `docs/design/kernel-substrate.md` §4 owns it. C7 cares that the ledger correlates the per-relay attempts; the contract does not specify the row layout.
+- NIP-77 sync routing — that's C10 in `sync.md`. Sync and live REQ should share relay policy (`ndk-applesauce-lessons.md` §6 last paragraph), but the symmetric assertion lives with C10.
+- NIP-42 auth-paused publishes — M5. The override action does not unblock an auth-paused relay; auth pause is a wire-emitter gate, not a planner decision (`docs/design/subscription-compilation/recompilation.md` §4.2 trigger A9 open question).
diff --git a/docs/design/framework-magic/replaceable.md b/docs/design/framework-magic/replaceable.md
new file mode 100644
index 0000000..916c8af
--- /dev/null
+++ b/docs/design/framework-magic/replaceable.md
@@ -0,0 +1,92 @@
+# Framework Magic §C1–§C4 — Replaceable & Delete Invariants
+
+> Parent: `docs/design/framework-magic.md`.
+> Read first: `docs/product-spec/subsystems.md` §7.1 (the EventStore insert-time invariants table — this chapter references its rows, does not restate them); `docs/design/lmdb-schema.md` (storage backend for M3); `docs/design/lmdb/tests.md` §3 (kind:30023 d-tag corner cases).
+
+This chapter holds four bullets, all of which discharge `docs/product-spec/overview-and-dx.md` §3.3 **bug-extinction #1** ("Stale replaceable event retained in state after a newer one arrives") and `docs/aim.md` §6 **doctrine 4** ("replaceable-event invariants enforced on insert"). The four are split because they cover four distinct kind-class shapes and have four distinct test surfaces.
+
+## C1. Replaceable supersession on insert (kind 0 / 3 / 10000–19999)
+
+**Statement.** Any kind in `{0, 3, 10000..=19999}` arriving at the event store automatically supersedes the prior event with the same `(pubkey, kind)`; the prior event becomes unreachable through the public read path.
+
+**Framework does:** the insert-time supersession at `docs/product-spec/subsystems.md` §7.1 row "Replaceable kinds (0, 3, 10000-19999)". Mechanism: compare `(pubkey, kind)` against the existing entry, keep newest `created_at`, tie-break by lexicographically smallest `id`. The current in-memory store enforces this for kind:0 / kind:3 / kind:10002 today (kind:3 via `seed_contacts.insert` at `crates/nmp-core/src/kernel/ingest.rs:206`; kind:10002 via the `should_replace` branch at `crates/nmp-core/src/kernel/ingest.rs:218-222`). M3 graduates the rule into the LMDB-backed `EventStore` trait (`docs/design/lmdb/trait.md`).
+
+**App writes:** nothing. The app calls `ProfileView::open(pubkey)`; the view's payload reflects the latest kind:0 the store has, with no app-side comparison of `created_at`.
+
+**Failure mode prevented:** §3.3 bug #1. Plus the doctrine-4 footgun: an app caches kind:3 in its own state, fails to re-fetch on UI nav, renders a stale follow list, double-subscribes on the next session.
+
+**Test:** `c1_replaceable_supersedes_on_insert`. The test inserts kind:0 #1 at `created_at=T`, then kind:0 #2 at `T+1` with same pubkey; asserts `ProfileView` payload reflects #2 and that a subsequent insert at `T-1` is rejected (no payload re-emit, no event store change). Tie-break path: two inserts at the same `T` with different ids — the lexicographically-smaller-id event wins, deterministic across runs.
+
+**Milestone owner:** **[DONE]** for in-memory kernel (verified by `crates/nmp-core` kernel tests today, ref the existing `should_replace` branch). Test runs **not** ignored from day one; LMDB graduation in M3 must preserve the same observable, so the test stays green across M3.
+
+---
+
+## C2. Parameterized replaceable supersession (kind 30000–39999) by `(pubkey, kind, d-tag)`
+
+**Statement.** Any kind in `{30000..=39999}` is keyed by `(pubkey, kind, d-tag)`, not just `(pubkey, kind)`. Two events with the same kind and pubkey but different `d` tags coexist; two with the same `d` supersede.
+
+**Framework does:** the insert-time rule at `docs/product-spec/subsystems.md` §7.1 row "Parameterized replaceable (30000-39999)". M3 implements this in LMDB via the key encoding at `docs/design/lmdb/keys.md` and the `get_param_replaceable(pk, kind, d_tag)` accessor on the `EventStore` trait (`docs/design/lmdb/trait.md`).
+
+**App writes:** nothing. Long-form (kind:30023) reader views open by `(pubkey, d_tag)` coordinate; the framework resolves to the current event.
+
+**Failure mode prevented:** §3.3 bug #1 for the parameterized case — the most common subtlety being apps that key only on `(pubkey, kind)` and overwrite a kind:30023 with a different `d` tag, losing one of the author's articles.
+
+**Test:** `c2_parameterized_replaceable_supersedes_by_dtag`. Mirrors `docs/design/lmdb/tests.md` line 93: insert two kind:30023 with same `(pubkey, d=foo)`, second newer; assert only the second is read. Insert a third with same kind+pubkey but `d=bar`; assert both `foo` and `bar` are independently retrievable. Insert a kind:30024 with `d=foo`; assert it does not collide.
+
+**Milestone owner:** **[PENDING M3]**. Test checked in as `#[ignore = "pending M3 LMDB"]`; M3 owner removes the ignore as part of the framework-magic delta on M3's exit-gate report. (Note: the M3 LMDB-tests doc already contains the same scenario at the storage layer; C2 promotes it from a storage-layer test to a contract-surface test — the framework-magic test calls through the public view path, not through the EventStore trait directly.)
+
+---
+
+## C3. Kind:5 delete propagation: referenced events removed, tombstone persisted
+
+**Statement.** A signature-verified kind:5 event from author X referencing event ids `[e1, e2, ...]` and/or replaceable coordinates `[a1, a2, ...]` removes any matching events the store holds that are *authored by X*; the deletions persist as tombstones so the same events cannot be re-inserted later.
+
+**Framework does:** §7.1 row "Kind 5 (delete)". Mechanism: after signature verification, scan the referenced `e` and `a` tags, remove matching events *authored by the deleter* (other authors' events with the same id, if any, are untouched — a kind:5 by Alice cannot delete Bob's events), persist a tombstone keyed by event coordinate with a tombstone timestamp = maximum delete `created_at` observed for that target.
+
+**App writes:** nothing. The view payloads recompute (via `ViewModule::on_event_removed` per `docs/design/kernel-substrate.md` §3 lines 141–143) and the deleted note disappears from `TimelineView.items` in the next emit.
+
+**Failure mode prevented:** the cross-cutting "phantom note" bug: a kind:5 lands, the app's UI does nothing, the note still renders, and worse — re-inserts on app restart because the app's local cache predates the delete. The tombstone is the structural answer: even if the original event is re-delivered by another relay, the store refuses to re-insert it.
+
+**Test:** `c3_kind5_delete_removes_referenced_and_tombstones`. The test:
+
+1. Inserts a kind:1 event `e1` by author Alice; asserts it appears in `TimelineView`.
+2. Inserts a kind:5 by Alice referencing `e1`; asserts `TimelineView` no longer contains `e1`.
+3. Re-inserts `e1` (simulating a later relay redelivery); asserts the store rejects it and the timeline payload does not re-emit.
+4. Inserts a kind:5 by Bob referencing `e1`; asserts the tombstone is *not* upgraded (cross-author kind:5 has no effect).
+5. Restart the store (M3 path) and re-insert `e1`; assert tombstone is still in force.
+
+**Milestone owner:** **[PENDING M3]**. Test checked in as `#[ignore = "pending M3 tombstone persistence"]`. The in-memory kernel today does not enforce tombstone persistence across restart; M3's LMDB schema (`docs/design/lmdb/keys.md`) is where the tombstone subdatabase lands. Steps 1–4 of the test can pass against the in-memory kernel; step 5 requires M3.
+
+---
+
+## C4. NIP-40 expiration auto-removes event at expiry; survives actor restart
+
+**Statement.** An event carrying a NIP-40 `expiration` tag is automatically removed from the store at the expiration timestamp; the schedule survives actor restart.
+
+**Framework does:** §7.1 row "NIP-40 expiration": schedule a tokio timer to remove the event at the expiration timestamp; on actor restart, scan the persisted store and re-schedule. M3 implements the persistent rescan; the in-memory kernel can run the timer but loses schedules on restart.
+
+**App writes:** nothing. Same `on_event_removed` path as C3.
+
+**Failure mode prevented:** apps shipping their own "is this event expired?" filter, getting it wrong (off-by-one timezone, missing tag parser, not re-checking after restart), and rendering events that should be gone — especially relevant for ephemeral notifications and expiring offers.
+
+**Test:** `c4_nip40_expiration_removes_and_persists_schedule`. The test uses the `SimulatedClock` from `nmp-testing` (`docs/product-spec/subsystems.md` §7.13 line 343):
+
+1. Insert an event with `expiration` tag at clock-now + 60s.
+2. Advance clock to +30s; assert event still present.
+3. Advance clock to +61s; assert event removed; `TimelineView` payload re-emitted without it.
+4. Insert another event with expiration at +120s.
+5. Simulate actor restart (drop the actor, instantiate from persisted store); assert the +120s schedule is re-armed by the rescan; advance clock to +130s; assert removal fires.
+
+**Milestone owner:** **[PENDING M3]**. Test checked in as `#[ignore = "pending M3 expiration persistence"]`. Steps 1–3 are testable today (timer-only); step 5 needs M3.
+
+---
+
+## Why this chapter is four bullets, not one
+
+The four invariants ride the same insert path but have different observable surfaces, different test trigger shapes, and different milestones own them. Collapsing them would (a) hide which milestone owes which guarantee and (b) make the regression test ambiguous when one breaks while the others pass. The chapter is the granularity the milestone delta protocol needs.
+
+## What this chapter does not cover
+
+- The replaceable rule for kind:10002 (mailboxes) is C1 (it is in `10000..=19999`). C5 (kind:3 auto-tracking) and the M2 `Trigger::Nip65Arrived` are the *reactive* second-order effect; C1 is the *storage* invariant that triggers them.
+- Cross-replaceable-kind interactions (e.g., a kind:5 deleting a kind:0): legal but odd. The §7.1 row says kind:5 removes "matching events authored by the deleter" — the replaceable supersession just means the matched event might already be the latest version. No special-case in the contract; the existing rules compose.
+- Garbage collection of unreferenced non-pinned events: a separate concern. `docs/product-spec/subsystems.md` §7.1 "GC" + `docs/design/lmdb/gc.md`. Not a contract bullet because the app does not observe GC directly; it observes events appearing and disappearing per the four rules above, and GC just bounds memory.
diff --git a/docs/design/framework-magic/sessions.md b/docs/design/framework-magic/sessions.md
new file mode 100644
index 0000000..e4cd645
--- /dev/null
+++ b/docs/design/framework-magic/sessions.md
@@ -0,0 +1,63 @@
+# Framework Magic §C12 — Account Switch as State
+
+> Parent: `docs/design/framework-magic.md`.
+> Read first: `docs/product-spec/subsystems.md` §7.4 (`SessionState`); `docs/design/subscription-compilation/recompilation.md` §4.2 trigger A4 (`ActiveAccountChanged`); `docs/design/subscription-compilation/intro.md` §2.3 (account scope binding); `docs/aim.md` §6 doctrine 7.
+
+## C12. Account switch is a state transition; views rebind without imperative dance
+
+**Statement.** Switching the active account is a single dispatched action. After the dispatch, every `InterestScope::ActiveAccount`-scoped view (a "following timeline", "my profile", "my mentions", etc.) re-resolves against the new account's context — new follow-set, new mailbox set, new mute list, new signer — without the application issuing CLOSE/REQ frames, tearing down view handles, or rebuilding any UI. The view handles remain valid; their payloads update.
+
+**Framework does:**
+
+- `SessionState` (`subsystems.md` §7.4 lines 107–125) carries `accounts: Vec<Account>` and `active: Option<String>` as plain state fields. A `SwitchActiveAccount { pubkey }` action mutates `active`; the mutation is the only state change.
+- `Trigger::ActiveAccountChanged { from, to }` (`subscription-compilation/recompilation.md` §4.2) fires as a consequence of the state change. The planner re-runs `interests()` on every `ViewModule` whose registered interest carries `InterestScope::ActiveAccount` (`subscription-compilation/intro.md` §2.1 line 60 + §2.3); `InterestScope::Account(specific)` and `InterestScope::Global` interests are untouched.
+- The compiler diffs the new plan against the old; per-relay CLOSE/REQ frames fire only for the *delta* (e.g., previous account's follows that are not in new account's follows close their slices; new follows open new slices).
+- View payloads recompute via the same `on_event_replaced` / `on_event_inserted` cascade the kernel uses for any state change; the platform shadow's `useFollowingTimeline()` etc. emit a new payload.
+- The signer attached to operations dispatched after the switch is the new active account's signer (per `IdentityModule` routing in `kernel-substrate.md` §6).
+
+**App writes:** one dispatch: `dispatch(AppAction::SwitchActiveAccount { pubkey })`. The app's "switch account" UI is a button that fires that dispatch. No log-out / log-in dance, no view-tree rebuild, no manual REQ reissue, no clearing of caches — the framework handles all of it as a single tick of the actor's event loop.
+
+**Failure mode prevented:** `product-spec/overview-and-dx.md` §3.3 **bug #5** ("Two account contexts having overlapping mutable state"). Plus the operationally common bug where an app tears down its view tree on account switch — losing scroll position, in-flight composes, draft state — because it doesn't trust the framework to re-derive correctly. C12 makes the trust structural: the view handles remain valid; the app cannot accidentally observe the old account's data on the new account's views.
+
+**Test:** `c12_account_switch_rebinds_views_without_imperative_dance`. The test:
+
+1. **Setup:** seed two accounts in `SessionState.accounts` — Alice (follows `[X, Y]`) and Bob (follows `[Y, Z]`). Pre-seed mailboxes: X→r1, Y→r2, Z→r3. Set Alice active.
+2. **Initial open:** open `FollowingTimelineView` (no fields — derives from active account); assert the planner opens REQs on `{r1, r2}`; assert the payload emits with follow set `{X, Y}`.
+3. **Dispatch switch:** `dispatch(AppAction::SwitchActiveAccount { pubkey: bob_pk })`. The test makes no other calls; the harness drains the action ledger and the planner trigger queue.
+4. **Assert delta wire frames:** exactly two frames emitted by the planner — `CLOSE` for the r1 slice (X drops; X is not in Bob's follows), `REQ` for the r3 slice (Z appears; Z is in Bob's follows). The r2 slice is untouched (Y is in both follows).
+5. **Assert view handle stability:** the `FollowingTimelineView` handle from step 2 is **the same handle**; it has not been torn down. Its payload has been re-emitted once, now reflecting Bob's follow set `{Y, Z}`.
+6. **Assert signer rebinding:** dispatch a `SendNote { content: "hello" }`; assert the signed event's `pubkey = bob_pk` (the new active account's signer was used), without any explicit signer parameter on the `SendNote` action.
+7. **Assert specific-scoped views untouched:** before step 3, also open `ProfileView { pubkey: charlie_pk }` (an `InterestScope::Account(charlie)`-equivalent — actually Global since it names an explicit author). Assert this view's payload is not re-emitted after the switch; its underlying REQ stays alive on the same relay; no delta frames touch it. This is the symmetric assertion: the switch affects *only* `ActiveAccount`-scoped interests, per `subscription-compilation/recompilation.md` §4.2 line 113.
+8. **Assert no overlap:** read the audit log of any per-account domain-store namespace (e.g., Alice's drafts) and assert Bob cannot read it. The kernel's domain-store isolation per account is the structural enforcement (`kernel-substrate.md` §8 "Domain stores are isolated" and the per-account scoping in domain key prefixes).
+
+**Milestone owner:** **[PENDING M8]**. M8 is the multi-account session milestone (per `scope-adjustments-2026-05-18.md` ladder). M2 already lands the `Trigger::ActiveAccountChanged` shape (`subscription-compilation/recompilation.md` §4.2 line 109: "M2 establishes the trigger; M8 wires the multi-account state machine"). Test checked in as `#[ignore = "pending M8 multi-account state machine"]`. Sub-paths 4 and 7 are testable as soon as M2 lands (single-account boot fires the trigger once with `from: None, to: Some(active)` per the M2 design); the rest needs M8.
+
+## Why this is one bullet, not several
+
+The eight sub-paths assert different facets of one observable contract: *after the switch dispatch, every consequence is a derived re-emission, never an imperative reissue.* The kernel-substrate (`kernel-substrate.md` §8) ensures domain-store isolation; the planner (`subscription-compilation/recompilation.md` §4.2) ensures interest re-resolution; the identity machinery (`kernel-substrate.md` §6) ensures signer rebinding. The contract bullet covers all three as one because they are observed together: an app that does `dispatch(SwitchActiveAccount)` and then attempts any operation gets a correctly-rebound system; partial rebinding is a regression.
+
+## Doctrine alignment
+
+C12 is the most direct demonstration of cardinal doctrine **D4** ("single writer per fact; caches derive"). The "fact" is `SessionState.active`. The "caches" are every active-account-scoped view, every signer binding, every relay-routing decision. The framework's job is to make sure every cache derives mechanically; the app's job is to write the fact once.
+
+It also discharges `aim.md` §6 doctrine 7: "Sessions are state, switching is an action. No imperative 'log out, then log in, then reload' dance." That sentence is the contract C12 holds in place.
+
+## Cross-references
+
+- `docs/design/subscription-compilation/intro.md` §2.3 — `InterestScope::ActiveAccount` resolution at compile time, not registration time.
+- `docs/design/subscription-compilation/recompilation.md` §4.2 trigger A4 — the actor-message shape of the `ActiveAccountChanged` trigger.
+- `docs/design/kernel-substrate.md` §8 — module composition rules, specifically domain-store isolation.
+- `docs/product-spec/subsystems.md` §7.4 — `SessionState` field shapes.
+
+## Interaction with C11
+
+C11 covers *onboarding*: adding an account to `SessionState.accounts`. C12 covers *switching*: changing which account in that list is `active`. The two are independent: an app can onboard without switching, or switch among already-onboarded accounts without onboarding. The framework guarantees both.
+
+The full sequence (onboard → switch → use) is exercised by C11 sub-path 2(e): create a new identity, switch to it, sign an event. That test crosses both contract bullets and is the canonical end-to-end demonstration.
+
+## What this chapter does not cover
+
+- **The login UI itself.** The app provides the button; the contract specifies what the dispatch guarantees.
+- **The account-switcher view payload.** That is a view module (`AccountListView` or similar in `nmp-core`'s built-ins per `subsystems.md` §7.4); its spec/payload is owned by the view catalog, not the contract.
+- **Background account state** (per-account sync watermarks, per-account action ledger). Those are per-account scopes inside the storage backend; the contract does not specify the scoping mechanism, only that the switch does not leak state across.
+- **Logging out / removing an account.** A `RemoveAccount` action exists in the long-term catalog (`subsystems.md` §7.4 implied); its contract surface is a separate potential bullet, not in v1's 13. Removal cleanly through the same `IdentityModule::destroy` path (kernel-substrate.md §6 line 341).
diff --git a/docs/design/framework-magic/signers.md b/docs/design/framework-magic/signers.md
new file mode 100644
index 0000000..c489e66
--- /dev/null
+++ b/docs/design/framework-magic/signers.md
@@ -0,0 +1,76 @@
+# Framework Magic §C11 — Signer Onboarding
+
+> Parent: `docs/design/framework-magic.md`.
+> Read first: `docs/product-spec/subsystems.md` §7.4 (sessions + signer catalog); `docs/design/kernel-substrate.md` §5 (`CapabilityModule`), §6 (`IdentityModule`); `docs/plan/scope-adjustments-2026-05-18.md` §"Folded into M6".
+
+## C11. Signer onboarding: bunker:// + nsec creation as kernel actions
+
+**Statement.** Two signer-onboarding flows are first-class kernel actions, complete from a single dispatched intent without any app-side orchestration:
+
+1. **Bunker URL onboarding.** A pasted `bunker://...` URL parses into a `BunkerConnect` action; the action runs the NIP-46 rendezvous, establishes the remote-signer connection, persists the connection token via `KeyringCapability`, and emits an `Account` with `signer_kind = Nip46Bunker` into `SessionState.accounts`.
+2. **Create new nsec.** A `CreateLocalIdentity { passphrase, label }` action generates a new keypair, encrypts the nsec via NIP-49 with the given passphrase, persists the encrypted nsec via `KeyringCapability`, and emits an `Account` with `signer_kind = LocalKey` into `SessionState.accounts`.
+
+In both cases the new account becomes available to the active-session machinery (per C12) on a subsequent `SwitchActiveAccount` dispatch.
+
+**Framework does:**
+
+- The signer catalog at `subsystems.md` §7.4 lines 127–135 names both kinds as supported in `nmp-core` (no FFI signer extensibility — apps don't implement signers).
+- `IdentityModule` (`docs/design/kernel-substrate.md` §6) is the trait family that hosts the local-key and bunker signers. The kernel owns identity ID assignment, secure-store persistence, and session activation routing (kernel-substrate.md §6 last paragraph).
+- `KeyringCapability` (`kernel-substrate.md` §5 lines 305–308) is the kernel-provided capability that wraps macOS Keychain / Windows Credential Manager / Secret Service / Android Keystore. Capability calls report; they do not decide.
+- The NIP-46 rendezvous flow is the `nostr-connect` crate's behavior; the framework wraps it as an `ActionModule` with the standard ledger-correlated capability-await pattern (`kernel-substrate.md` §4 `AwaitCapability` transition).
+- The NIP-49 encryption is the `nostr` crate's `EncryptedSecretKey`; the framework wraps it as a step inside the `CreateLocalIdentity` action.
+
+**App writes:** for **bunker**, one dispatch with the pasted URL: `dispatch(AppAction::BunkerConnect { url: "bunker://..." })`. For **create new nsec**, one dispatch: `dispatch(AppAction::CreateLocalIdentity { passphrase, label })`. The action ledger row exposes progress (parsing, rendezvous, awaiting user approval on the bunker app, persisted, available); the app's UI renders the ledger row as a step indicator if it wants, but the orchestration is the framework's. The app does **not** call NIP-46 transport code, does **not** invoke NIP-49 encryption, does **not** touch the Keychain directly, and does **not** wire the new identity into the session state.
+
+**Failure mode prevented:** the constellation of "DIY signer onboarding" bugs that every Nostr-on-mobile app re-discovers — leaked plaintext nsec in app state during the encryption window, lost bunker connection on app suspend, race between persistence and session activation, partial-failure leaving an `Account` in `SessionState` with no usable signer. The action ledger's atomicity (`kernel-substrate.md` §4 "Atomicity" paragraph) makes the "partial success" path explicit and recoverable.
+
+**Test:** `c11_bunker_url_and_nsec_creation_complete_via_actions`. The test has two sub-paths against an in-memory `KeyringCapability` mock and a mock NIP-46 rendezvous endpoint:
+
+1. **Bunker onboarding:**
+   a. Dispatch `BunkerConnect { url: "bunker://abc?relay=wss%3A%2F%2Fmock&secret=xyz" }`.
+   b. Mock rendezvous endpoint responds with a successful `connect` response.
+   c. Assert the action ledger row transitions `Pending → Running(Parsing) → Running(Rendezvous) → Running(Persisting) → Completed { account_id }`.
+   d. Assert `SessionState.accounts` contains one new `Account` with `signer_kind = Nip46Bunker`; the `KeyringCapability` mock has one stored entry keyed by the new account id.
+   e. Assert no plaintext bunker secret crossed FFI (the test's reconciler audit log shows no `Account` snapshot field carrying the raw URL); only the typed `Account` + `signer_kind` enum.
+2. **Create new nsec:**
+   a. Dispatch `CreateLocalIdentity { passphrase: "test-passphrase", label: "alice" }`.
+   b. Assert the action ledger row transitions `Pending → Running(Generating) → Running(Encrypting) → Running(Persisting) → Completed { account_id }`.
+   c. Assert `SessionState.accounts` contains one new `Account` with `signer_kind = LocalKey`, `display.label = "alice"`; the `KeyringCapability` mock has one stored entry containing the NIP-49 ciphertext (the test inspects the mock's stored bytes — the prefix is `ncryptsec1`).
+   d. Assert the plaintext nsec is **not** present in `SessionState`, in any view payload, in any diagnostic surface, or in the test's reconciler audit log. The plaintext exists only inside the actor's transient action state during encryption.
+   e. Assert a follow-up `SwitchActiveAccount { account_id }` succeeds and that the actor can sign a test event using the newly-created identity (round-trip: dispatch a `SendNote` against the new account, observe a signed event in the action ledger before publish).
+
+**Milestone owner:** **[PENDING M6]**. M6 is the signers + write-path milestone (per `scope-adjustments-2026-05-18.md` ladder). M6 owner adds the framework-magic delta after the test goes green. Test checked in as `#[ignore = "pending M6 signers"]`.
+
+## Why only these two onboarding paths
+
+The full signer catalog at `subsystems.md` §7.4 lists five kinds:
+
+- Local key (raw nsec, encrypted at rest) — **covered by C11 sub-path 2**.
+- NIP-49 (password-encrypted) — **subsumed by C11 sub-path 2** (the NIP-49 encryption is the persistence step of the local-key creation, not a separate flow).
+- NIP-46 bunker — **covered by C11 sub-path 1**.
+- NIP-07 (web only) — wired via the web bindings shim; not a v1-ladder contract bullet because the web target is M15.
+- External Android Amber via NIP-55 — wired via the `ExternalSignerCapability` (`kernel-substrate.md` §5); not a v1-ladder contract bullet because Android is M15.
+
+C11 covers the two paths the user explicitly named in `scope-adjustments-2026-05-18.md` §"Folded into M6": *"NIP-46 bunker:// URL parsing + connection flow"* and *"Create new nsec flow. Generate, encrypt (NIP-49), and store via Keychain capability."* The other three signer kinds inherit the same atomicity guarantees by virtue of going through the same `IdentityModule` + `KeyringCapability` plumbing, but their onboarding flows have platform-specific surfaces that the v1 contract does not assert at this level.
+
+A potential C11.b sibling bullet covering NIP-07 + NIP-55 may be added in the M15 framework-magic delta.
+
+## The capability boundary
+
+This bullet is a load-bearing demonstration of the bible's capability pattern (aim.md §6 doctrine 11: "capabilities, not callbacks"). The KeyringCapability **reports** (here is the stored bytes; persistence succeeded/failed). It does **not decide** (whether to retry, whether to fall back to a different storage backend, whether to surface a UI prompt). The framework decides; the capability executes.
+
+The test's assertion that no plaintext nsec crosses FFI is the structural witness for `aim.md` §6 doctrine 5 (bounded native state) and for the implicit "secrets stay in Rust" rule — the platform layer never sees the unencrypted key material because every read/write of the key goes through the in-Rust `IdentityModule::sign` function.
+
+## Cross-references
+
+- `docs/design/kernel-substrate.md` §6 — `IdentityModule` trait definition.
+- `docs/design/kernel-substrate.md` §5 — `CapabilityModule` framing + the named `KeyringCapability` family.
+- `docs/product-spec/subsystems.md` §7.4 — `SessionState` + `Account` shapes.
+- The `nostr-connect` and `nostr-keyring` crates (aim.md §3) — the protocol/OS primitives the framework composes.
+
+## What this chapter does not cover
+
+- **Account switching mechanics** — that's C12 in `sessions.md`.
+- **Signing a publish** — the sign step inside `SendNoteAction` (C7). C11 covers onboarding; subsequent signing is the publish path.
+- **Multi-device account sync** — out of v1 scope per `aim.md` §9.
+- **Key-recovery and passphrase reset flows** — application-level UI on top of the framework primitives; not a contract bullet because these flows compose existing actions (delete identity, create new identity).
diff --git a/docs/design/framework-magic/subs.md b/docs/design/framework-magic/subs.md
new file mode 100644
index 0000000..8a9d53c
--- /dev/null
+++ b/docs/design/framework-magic/subs.md
@@ -0,0 +1,48 @@
+# Framework Magic §C8 — Subscription Planner Hygiene
+
+> Parent: `docs/design/framework-magic.md`.
+> Read first: `docs/product-spec/subsystems.md` §7.2 (subscription planner behaviors); `docs/design/subscription-compilation/compiler.md` (compilation pipeline); `docs/design/reactivity/scheduling-and-data-model.md` (buffer / batch policy); `docs/design/firehose-bench.md` (the modeled-perf companion benchmark for the ≤60Hz budget).
+
+## C8. Subscriptions auto-dedup, auto-coalesce, auto-close, and auto-buffer
+
+**Statement.** The framework guarantees four properties on every wire subscription it issues:
+
+1. **Dedup.** Two logical interests with the same canonical filter share one wire REQ per relay; each logical consumer still receives only events matching its own filter.
+2. **Coalesce / merge.** Logical interests with structurally compatible filters (per the merge lattice in `subsystems.md` §7.2) merge into one broader REQ per relay; each consumer is filtered locally from the broader stream.
+3. **Auto-close.** A wire REQ with no remaining logical consumers is CLOSE'd. One-shot interests (those without a live tail, only an `until` upper bound) are CLOSE'd on EOSE.
+4. **Buffered batching.** Inbound events for one view are batched into a single `ViewBatch` per actor tick at ≤60Hz; backpressure drops batches in favor of a single `FullState` catch-up. The platform's reactive primitive sees one re-render per tick, not per event.
+
+**Framework does:** the subscription-compilation pipeline (`docs/design/subscription-compilation/compiler.md`) for dedup and coalesce; the wire-emitter's diff (compiler §3 final stage) for auto-close on plan changes; the view registry's refcount drop for auto-close on consumer loss; `docs/design/reactivity/scheduling-and-data-model.md` for the per-tick batching; the FullState backpressure fallback at `subsystems.md` §7.2 line 69. The hard cap of 60Hz is the budget in `subsystems.md` §7.16 table row "ViewBatch frequency under hashtag firehose".
+
+**App writes:** nothing. The app opens views; it does not name a REQ. The reactivity scheduling is invisible — the platform's `useTimeline()` rune/observable emits at the framework's batched cadence regardless of relay throughput.
+
+**Failure mode prevented:** the entire class of subscription-management bugs in `product-spec/overview-and-dx.md` §3.3 numbers 2 ("Subscription leaked after its UI is destroyed") and 8 ("Two concurrent UI subscriptions for the same filter producing two relay REQs"). Plus the hand-rolled grouping-window + dedup-LRU pattern that `ndk-applesauce-lessons.md` §7 calls out as the work clients typically do manually.
+
+**Test:** `c8_subscriptions_coalesce_autoclose_and_buffer`. The test has four sub-paths in one `#[test] fn`:
+
+1. **Dedup:** open two `TimelineView`s with identical filters; assert the planner produces one wire REQ per relay (not two); destroy one; assert the wire REQ stays alive; destroy the second; assert the REQ is CLOSE'd after the warmth grace expires (`subsystems.md` §7.6 line 226: 30s default).
+2. **Coalesce:** open `TimelineView { authors: [A, B], kinds: [1] }` and `ProfileView { pubkey: C }`; assert the planner merges into one REQ per relay containing the union shape, with each view receiving only its filtered subset locally (no REQ for kind:0 alone if the relay already has the merged stream covering it). The merge lattice's exact rules live in `subsystems.md` §7.2 line 65 and `docs/design/subscription-compilation/intro.md` §1 open-question #2 (lattice formalization); the test asserts the *observable* (wire frame count = correct fewer-than-naive, payload coverage = correct) rather than the lattice mechanics.
+3. **Auto-close on EOSE for one-shot:** open `ProfileClaim { pubkey: D }` (which `docs/design/subscription-compilation/intro.md` §2.2 line 112 specifies as `lifecycle: OneShot, limit: 1`); the mock relay sends the kind:0 then EOSE; assert the planner CLOSEs the wire REQ within one tick of EOSE; assert no further REQs touch that relay for that filter.
+4. **Buffered batching under firehose:** the mock relay sends 600 events for one filter in 1 second (10× the budget); assert the platform reconciler observes ≤60 `ViewBatch` emissions in the window; assert no events are dropped from the underlying store (only the *render emission rate* is capped, not the ingestion); assert the actor queue depth stays below `subsystems.md` §7.16 budget (steady-state < 16).
+
+**Milestone owner:** **[PENDING M2]** for sub-paths 1–3 (the compiler + lifecycle); **partial overlap with reactivity-bench** for sub-path 4 (the buffer cadence is exercised by `docs/perf/reactivity-bench/` already; the contract test asserts the same property through the public view path). Test checked in as `#[ignore = "pending M2 compiler"]` initially.
+
+## Why this is one bullet, not four
+
+The four properties (dedup / coalesce / close / batch) are observable as one contract from the app's perspective: *the app opens N views, the framework opens ≤N REQs, the framework closes them at the right moment, the framework caps emit cadence.* Splitting into four bullets would suggest the app might experience them separately; it does not. The four sub-paths of the test are the four conditions the single contract bullet asserts.
+
+The reason this is C8 and not bundled with C6/C7 is that C6/C7 govern *which relay* a REQ targets; C8 governs *how many REQs and at what cadence* regardless of the relay. Different doctrines (D3 vs D5+aim §6 doctrine 6) and different milestone responsibility.
+
+## Cross-references to the existing test surface
+
+- `docs/design/subscription-compilation/tests.md` §9.2 assertion 2 already asserts the per-relay author partition + sub-shape merge (the coalesce property at the planner layer). The framework-magic version of sub-path 2 reuses that mailbox cache setup but reads the wire output through the platform shadow's audit log instead of through the planner harness.
+- `docs/design/firehose-bench.md` is the modeled-perf companion: it asserts ≤60Hz holds under sustained load. The framework-magic sub-path 4 asserts the *correctness* of the cap (no event loss); the bench asserts the *budget* under realistic load.
+- `docs/design/reactivity/validation-harness.md` covers reactive-primitive validation (Swift `@Observable`, Kotlin `Flow`, etc.). C8's sub-path 4 cross-validates that the platform-side emissions match the actor-side `ViewBatch` count.
+
+## What this chapter does not cover
+
+- **Reconnect-resumption.** When a relay disconnects and reconnects, the planner re-issues the same wire REQ set (`subsystems.md` §7.2 line 71). That is a planner *resumption* behavior, not a contract bullet — the app sees no surface change. It is covered implicitly by the dedup/close properties (the resumed REQs are the same REQs the planner already tracks).
+- **NIP-77 sync vs live REQ split.** C10 in `sync.md` covers the sync side; C8 covers the live tail only.
+- **Per-view payload size budgets.** `subsystems.md` §7.16 table rows. The contract guarantees the buffering happens; the budget is an instrumentation concern with its own test surface in `nmp-metrics`.
+
+`TBD-from-research(applesauce/event-store-query-builders.md)`: cite the Applesauce file:line for the logical-vs-wire subscription split that NMP's compiler mirrors. The cross-validation in the research-fold commit confirms NMP's `LogicalInterest` (`docs/design/subscription-compilation/intro.md` §2.1) covers Applesauce's surface and that no observable property is lost in translation.
diff --git a/docs/design/framework-magic/sync.md b/docs/design/framework-magic/sync.md
new file mode 100644
index 0000000..a4d7b02
--- /dev/null
+++ b/docs/design/framework-magic/sync.md
@@ -0,0 +1,66 @@
+# Framework Magic §C9–§C10 — Sync, Provenance, Watermarks
+
+> Parent: `docs/design/framework-magic.md`.
+> Read first: `docs/product-spec/subsystems.md` §7.1 (provenance + watermarks), §7.8 (sync engine); `docs/design/lmdb/watermarks.md` (storage); `docs/design/ndk-applesauce-lessons.md` §6 (NIP-77 lessons), §9.8 (coverage ≠ cache presence).
+
+## C9. Provenance preserved across redeliveries
+
+**Statement.** When the same event id arrives from N different relays, the event store keeps exactly one event record with an N-entry provenance set (relay URL + first-seen + last-seen + source, deterministic primary relay). The original `id` and `signature` are never re-derived; the event is byte-stable across redeliveries.
+
+**Framework does:** the dedup-with-provenance-merge rule at `docs/product-spec/subsystems.md` §7.1 row "Duplicate id". Storage of provenance sidecars at `docs/design/lmdb/watermarks.md` (the 32-distinct-relay-per-event bound is set there). The "primary relay" selection is a deterministic function of the first observer, used for cache locality and diagnostics.
+
+**App writes:** nothing. The view payload's `id` field is the event id from the first observation; any per-event diagnostic UI ("seen on N relays") reads `Provenance` through `DebugDiagnostics` per `subsystems.md` §7.16.
+
+**Failure mode prevented:** `product-spec/overview-and-dx.md` §3.3 **bug #10** ("Re-published event missing its original `id` due to re-signing"). Plus the related "duplicate event in timeline" bug where naive dedup-on-id is missing and the same note appears twice from two relays. Plus the diagnostic-visibility regression where the app loses the ability to say "this event came from relay X" because the cache layer collapsed provenance.
+
+**Test:** `c9_provenance_merges_across_relay_redeliveries`. The test uses two mock relays:
+
+1. Relay-1 delivers event `e1` (kind:1 by Alice) at clock-now; insert observed.
+2. Assert event store contains exactly one event with id = `e1.id`, provenance set = `[{ relay: "wss://r1", first_seen: T0, last_seen: T0, source: Live }]`, primary relay = `wss://r1`.
+3. Relay-2 delivers the same `e1` at T1 = T0 + 5s; insert observed.
+4. Assert event store **still contains exactly one event** with id = `e1.id`, provenance set has two entries (r1 unchanged at T0/T0, r2 added at T1/T1), primary relay = `wss://r1` (unchanged — primary is sticky to first observer).
+5. Assert the `signature` and `id` fields are byte-identical to the original Relay-1 delivery (no re-derivation; the second insert did not re-sign).
+6. Relay-1 delivers `e1` again at T2 = T0 + 60s; assert the existing provenance entry for r1 updates `last_seen` to T2, no duplicate r1 entry is created.
+7. Run 33 more relay deliveries of `e1` from distinct relay URLs; assert the provenance set caps at 32 entries per the `docs/design/lmdb/watermarks.md` bound, with the **primary** entry preserved as the anchor.
+
+**Milestone owner:** **[PENDING M3]**. Sub-paths 1–6 are testable today against the in-memory kernel (the current `relay_count` field at `crates/nmp-core/src/kernel/ingest.rs:238` is the primitive shape; M3 graduates it to a typed `Provenance` sidecar). Sub-path 7 requires M3's storage cap logic. Test checked in as `#[ignore = "pending M3 provenance schema"]`.
+
+## C10. Watermarks gate backfill; cache miss becomes authoritative; NIP-77 is the default
+
+**Statement.** Every `(filter, relay)` pair the framework reads from has a durable **sync watermark** recording how far back coverage has been reconciled. Before issuing any historical REQ, the planner consults the watermark: a fully-synced pair serves cache-misses as **authoritative** ("this event does not exist on that relay"); an unsynced or partially-synced pair triggers a backfill that prefers **NIP-77 negentropy** when the relay supports it, falling back to bounded REQ scan otherwise.
+
+**Framework does:** the watermark schema at `docs/product-spec/subsystems.md` §7.1 (the watermarks table); the consult-before-REQ behavior at `subsystems.md` §7.2 line 62 ("Coverage-aware backfill"); the three sync triggers (foreground, view open, reconnect) at `subsystems.md` §7.8 lines 261–263; per-relay NIP-77 capability negotiation at `subsystems.md` §7.8 line 277. The watermark is durable across restart (`subsystems.md` §7.1 line 44). The authoritative-miss rule lives at `subsystems.md` §7.1 line 46: *"A cache-miss query against a fully-synced (filter, relay) pair is authoritative."*
+
+**App writes:** nothing. The app opens a view; the framework decides whether to serve from cache (with confidence backed by coverage), backfill via NIP-77, or fall back to bounded REQ. The view payload streams in as the gap closes; no spinner gates the cached render (per C13 and D1).
+
+**Failure mode prevented:** the cache-miss-disguised-as-empty bug (`product-spec/overview-and-dx.md` §3.3 **bug #6**: "Cache miss returning empty without triggering a fallback fetch") and its inverse, the over-fetch bug — issuing the same historical REQ on every view open because the framework can't tell the cache is complete. Plus the bandwidth waste `ndk-applesauce-lessons.md` §6 highlights: re-fetching a 10k-event historical window via REQ scan when the relay supports NIP-77 reconciliation.
+
+**Test:** `c10_watermark_gates_backfill_and_authoritative_miss`. The test uses a mock relay with declared NIP-77 capability and a `SimulatedClock`:
+
+1. **Unsynced pair → fetch.** Open `TimelineView { authors: [A], kinds: [1], since: T-1d, until: T }` against a fresh store (no watermark for this `(filter, relay)` pair). Assert the planner schedules a backfill — NIP-77 reconciliation against the mock relay (because capability negotiation succeeded). Mock relay returns a 50-event set; assert all 50 land in the store; assert the watermark for `(filter_sig, "wss://mock")` updates to `synced_up_to = T`.
+2. **Fully-synced pair → authoritative miss.** Close the view, re-open with the same filter, query for an event known not to exist in the response set. Assert the planner **does not issue a wire frame** (no REQ, no NIP-77); the cache-miss returns empty as authoritative; the watermark is unchanged.
+3. **Capability fallback.** Switch to a second mock relay that **does not** support NIP-77 (capability negotiation reports unsupported). Open the same filter against the new relay; assert the planner falls back to bounded REQ scan for that relay only; assert the first relay's plan is untouched (the fallback is per-relay).
+4. **Reconnect gap-fill.** Simulate disconnect/reconnect on the mock relay after T+30s of being away; assert on reconnect, the planner re-establishes the live REQ tail (per C8 / `subsystems.md` §7.2 line 71) and schedules a NIP-77 gap fill for the disconnect window; assert the watermark updates after the gap closes.
+5. **`bytes_saved_vs_req` instrumentation.** Assert the cumulative counter for the synced relay is non-zero after step 1, per `subsystems.md` §7.1 watermarks-table column and §7.8 line 279.
+
+**Milestone owner:** **[PENDING M4]**. M4 is the NIP-77 milestone (per `docs/plan/scope-adjustments-2026-05-18.md` v1 ladder). The watermark *schema* lands earlier in M3 (`docs/design/lmdb/watermarks.md`); M4 lands the engine that reads/writes them and the capability negotiation. Test checked in as `#[ignore = "pending M4 sync engine"]`. Sub-path 2 (authoritative-miss given a populated watermark) is the structural assertion that does not require NIP-77 — it could be flipped to non-ignored as soon as M3 lands the schema and a stub engine.
+
+## Why these two are paired
+
+C9 is **what** the store remembers about each event's provenance. C10 is **what** the store remembers about each `(filter, relay)` pair's coverage. Together they answer the question `ndk-applesauce-lessons.md` §9.8 raises: *"Having an event in the local store does not prove that a view is complete."* C9 is "we have this event"; C10 is "we have everything matching this filter from this relay up to this timestamp." The framework needs both to render correctly without fetching needlessly.
+
+The two are paired in one chapter rather than split because their tests share the mock-relay-with-capability harness and because their failure modes intersect (a redelivered event from a new relay updates both the provenance set and the watermark, per `subsystems.md` §7.1 line 101 "Watermarks intersect with outbox").
+
+## Cross-references
+
+- `docs/design/lmdb/watermarks.md` for the storage schema and the 32-distinct-relay cap.
+- `docs/design/subscription-compilation/compiler.md` Stage X for the planner's watermark consultation in the compile pipeline. (TBD: confirm Stage number in research-fold; the compiler file specifies it.)
+- The "shared relay policy between sync and live REQ" lesson from `ndk-applesauce-lessons.md` §6 last paragraph is implicit in the per-relay watermark — both engines key by the same `(filter_sig, relay_url)` pair, so they cannot disagree on the relay universe.
+
+`TBD-from-research(applesauce/event-store-query-builders.md)`: cite Applesauce's coverage/watermark equivalent and the API by which a query-builder reads it. NMP's `WatermarksSummary` (`subsystems.md` §7.8 line 287) is the analogous app-visible surface; the research-fold commit verifies the surface covers the same diagnostic needs.
+
+## What this chapter does not cover
+
+- The action-ledger row schema for a manual `RunSync` action (`subsystems.md` §7.8 line 268 `SyncSpec`) — that's an actions-catalog concern owned by §7.5.
+- The proof-app sync overlay rendering — `subsystems.md` §4.5 owns the proof app.
+- Per-event verification re-running during sync — `subsystems.md` §7.1 row "Query matching" specifies that *every* stored event passes the canonical matcher; that is implicit, not a contract bullet.
diff --git a/docs/design/framework-magic/test-scaffolding.md b/docs/design/framework-magic/test-scaffolding.md
new file mode 100644
index 0000000..19a1472
--- /dev/null
+++ b/docs/design/framework-magic/test-scaffolding.md
@@ -0,0 +1,208 @@
+# Framework Magic — Test Scaffolding
+
+> Parent: `docs/design/framework-magic.md`.
+> Read first: `docs/design/subscription-compilation/tests.md` §9.3 (the `PlannerHarness` this scaffolding extends); `docs/design/lmdb/tests.md` (storage-layer test patterns); `docs/product-spec/subsystems.md` §7.13 (`nmp-testing` surface).
+
+## 1. File location and naming convention break
+
+```
+crates/nmp-testing/tests/framework_magic_contract.rs
+```
+
+The existing convention is **milestone-prefixed** (`m2_subscription_compilation_audit.rs`, `m3_lmdb_invariants.rs`, etc.). This file is intentionally **cross-cutting** — it is the *only* test file in `crates/nmp-testing/tests/` that is not milestone-prefixed. The convention break is deliberate:
+
+- The contract spans M2 + M3 + M4 + M6 + M8 + reactivity-bench; no single milestone owns it.
+- Renaming the file under a single milestone (e.g., `m_cross_framework_magic.rs`) would suggest one milestone is responsible for the whole contract; the opposite is true — every milestone owner adds to it.
+- The file is the *index test* — the meta-test (§4 below) reads `docs/design/framework-magic.md`'s row table and asserts every row has a `#[test] fn` with the expected name. A renaming under a milestone prefix would obscure this role.
+
+The next milestone owner must **not** rename the file to fit the milestone-prefix pattern. The framework-magic delta protocol in `intro.md` §4 covers this: a milestone landing that renames `framework_magic_contract.rs` requires an ADR.
+
+`Cargo.toml` for `nmp-testing` adds the standard `[[test]]` block:
+
+```toml
+[[test]]
+name = "framework_magic_contract"
+path = "tests/framework_magic_contract.rs"
+```
+
+Invocation: `cargo test -p nmp-testing --test framework_magic_contract`.
+
+## 2. Test names — the canonical 14
+
+Thirteen behavior tests (C1–C13; the table in `framework-magic.md` shows the exact names) plus the coverage meta-test, total 14 `#[test] fn` declarations:
+
+```
+c1_replaceable_supersedes_on_insert
+c2_parameterized_replaceable_supersedes_by_dtag
+c3_kind5_delete_removes_referenced_and_tombstones
+c4_nip40_expiration_removes_and_persists_schedule
+c5_kind3_change_recompiles_follow_dependent_subs
+c6_authors_subscription_routes_to_per_author_write_relays
+c7_publish_routes_outbox_and_private_fails_closed
+c8_subscriptions_coalesce_autoclose_and_buffer
+c9_provenance_merges_across_relay_redeliveries
+c10_watermark_gates_backfill_and_authoritative_miss
+c11_bunker_url_and_nsec_creation_complete_via_actions
+c12_account_switch_rebinds_views_without_imperative_dance
+c13_view_payload_uses_placeholders_then_refines_in_place
+contract_surface_complete                                  # meta-test
+```
+
+Test names are **stable identifiers**. Renaming any of them constitutes a contract revision per `intro.md` §4 and requires the deprecation marker (`#[test] fn old_name() { c_n_new_name() }` for at least one milestone cycle).
+
+## 3. The harness
+
+The harness is the union of three existing testing surfaces, exposed as one builder:
+
+```rust
+// crates/nmp-testing/src/framework_magic.rs (proposed)
+
+pub struct ContractHarness {
+    actor:            TestActor,                  // wraps the real actor with a recorded reconciler
+    planner:          PlannerHarness,             // from subscription-compilation/tests.md §9.3
+    clock:            SimulatedClock,             // from subsystems.md §7.13
+    network_chaos:    NetworkChaos,               // from subsystems.md §7.13
+    mock_relays:      Vec<MockRelay>,             // from nostr-relay-builder
+    keyring:          InMemoryKeyringCapability,  // for C11
+    audit:            WireFrameAuditLog,          // proposed; captures every CLOSE/REQ/EVENT frame
+    reconciler_log:   Vec<AppUpdate>,             // every AppUpdate emitted across the FFI seam
+}
+
+impl ContractHarness {
+    pub fn new() -> Self;
+    pub fn with_mock_relays(self, count: u8) -> Self;
+    pub fn with_nip77_capable_relays(self, capable: &[bool]) -> Self;
+    pub fn with_seeded_accounts(self, accounts: &[(Pubkey, SignerKind)]) -> Self;
+    pub fn with_active_account(self, pubkey: Pubkey) -> Self;
+    pub fn with_seeded_mailboxes(self, entries: &[(Pubkey, MailboxList)]) -> Self;
+    pub fn with_seeded_follows(self, account: Pubkey, follows: &[Pubkey]) -> Self;
+    pub fn build(self) -> Contract;
+}
+
+pub struct Contract {
+    // dispatch surface
+    pub fn dispatch(&mut self, action: AppAction);
+    pub fn open_view<V: ViewModule>(&mut self, spec: V::Spec) -> ViewHandle<V>;
+    pub fn close_view<V: ViewModule>(&mut self, handle: ViewHandle<V>);
+    pub fn ingest(&mut self, relay: usize, event: NostrEvent);
+    pub fn ingest_eose(&mut self, relay: usize, sub_id: &str);
+    pub fn disconnect_relay(&mut self, relay: usize);
+    pub fn reconnect_relay(&mut self, relay: usize);
+    pub fn advance_clock_ms(&mut self, ms: u64);
+    pub fn simulate_actor_restart(&mut self);
+
+    // assertion surface
+    pub fn wire_frames(&self, relay: usize) -> &[WireFrame];
+    pub fn reconciler_log(&self) -> &[AppUpdate];
+    pub fn event_store_get(&self, id: &EventId) -> Option<&StoredEvent>;
+    pub fn provenance_of(&self, id: &EventId) -> &Provenance;
+    pub fn watermark_of(&self, filter_sig: &FilterSig, relay: usize) -> Option<&Watermark>;
+    pub fn action_ledger(&self) -> &[ActionLedgerRow];
+    pub fn keyring_entries(&self) -> &[KeyringEntry];
+    pub fn session_state(&self) -> &SessionState;
+}
+```
+
+The harness extends `PlannerHarness` rather than wrapping it: every assertion the M2 audit gate makes against `PlannerHarness::compile_audit_log()` is accessible through the contract harness via `Contract::wire_frames(relay)`, but the contract harness also drives the full actor (so action ledger transitions, projection cache updates, and reconciler emissions are observable).
+
+`InMemoryKeyringCapability` is a new `nmp-testing` primitive for C11. It implements the `KeyringCapability` trait with a `HashMap<String, Vec<u8>>` backing store; the test inspects the stored bytes to verify NIP-49 encryption envelope shape.
+
+`WireFrameAuditLog` is a new `nmp-testing` primitive that captures every outbound frame the relay-worker emits. The M2 design has an audit log on the planner side; this harness has it on the wire side — both must agree, and a separate harness invariant could later assert that agreement.
+
+The harness does **not** include a real network — every relay is a `MockRelay`. Every contract test runs in deterministic time with no I/O. Total runtime budget for the full suite: <5 seconds.
+
+## 4. The coverage meta-test
+
+```rust
+#[test]
+fn contract_surface_complete() {
+    // 1. Read docs/design/framework-magic.md and parse the contract table.
+    let contract = parse_contract_table(include_str!("../../../docs/design/framework-magic.md"));
+
+    // 2. Enumerate the #[test] fns in this binary via inventory or a const list.
+    //    The const list is the canonical surface; inventory is the consistency check.
+    const EXPECTED_TESTS: &[&str] = &[
+        "c1_replaceable_supersedes_on_insert",
+        "c2_parameterized_replaceable_supersedes_by_dtag",
+        "c3_kind5_delete_removes_referenced_and_tombstones",
+        "c4_nip40_expiration_removes_and_persists_schedule",
+        "c5_kind3_change_recompiles_follow_dependent_subs",
+        "c6_authors_subscription_routes_to_per_author_write_relays",
+        "c7_publish_routes_outbox_and_private_fails_closed",
+        "c8_subscriptions_coalesce_autoclose_and_buffer",
+        "c9_provenance_merges_across_relay_redeliveries",
+        "c10_watermark_gates_backfill_and_authoritative_miss",
+        "c11_bunker_url_and_nsec_creation_complete_via_actions",
+        "c12_account_switch_rebinds_views_without_imperative_dance",
+        "c13_view_payload_uses_placeholders_then_refines_in_place",
+    ];
+
+    // 3. Assert every row in the contract table has a matching expected test name.
+    for row in &contract.rows {
+        assert!(
+            EXPECTED_TESTS.contains(&row.test_name.as_str()),
+            "contract row {} has test name '{}' which is not in EXPECTED_TESTS — \
+             update either the doc table or EXPECTED_TESTS so they agree",
+            row.id, row.test_name,
+        );
+    }
+
+    // 4. Assert no expected test name is missing from the contract table.
+    for expected in EXPECTED_TESTS {
+        let found = contract.rows.iter().any(|r| r.test_name == *expected);
+        assert!(found, "EXPECTED_TESTS lists '{}' which is not in the contract doc table", expected);
+    }
+
+    // 5. Assert every EXPECTED_TESTS entry is actually a #[test] fn in this binary.
+    //    Compile-time check via inventory crate or a build script that scans the file.
+    for expected in EXPECTED_TESTS {
+        assert!(
+            test_exists_in_binary(expected),
+            "EXPECTED_TESTS lists '{}' but no #[test] fn with that name exists in framework_magic_contract.rs",
+            expected,
+        );
+    }
+}
+```
+
+The meta-test is **not** `#[ignore]`. It runs on every CI run. It catches three classes of drift:
+
+1. The doc table grows a row but the test file doesn't grow a `#[test] fn` — caught by step 4.
+2. The test file grows a `#[test] fn` but the doc table doesn't list it — caught by step 3.
+3. A renamed test breaks the doc-test correspondence — caught by either step 3 or 4 depending on which side renamed first.
+
+The meta-test does **not** check `#[ignore]` status. A test for a pending milestone is correctly `#[ignore]`'d; the meta-test's job is structural correspondence, not implementation readiness. The milestone delta protocol (`intro.md` §4) handles the un-ignore cadence.
+
+## 5. `#[ignore]` discipline
+
+A test is `#[ignore = "pending M_n"]`'d when its owning milestone has not landed the implementation. The ignore reason **must** name the milestone (e.g., `"pending M3 tombstone persistence"`) so that a `grep -n "pending M" framework_magic_contract.rs` produces a checklist for each milestone's exit gate.
+
+The framework-magic delta in a milestone's exit-gate report enumerates which `pending M_n` ignore lines were removed during the milestone. Removing an ignore line without the delta entry fails the post-merge codex review.
+
+CI runs `cargo test --include-ignored` on a nightly schedule (not blocking) to catch the inverse drift: a `#[ignore]`'d test that has secretly started passing because the implementation landed without the milestone owner noticing.
+
+## 6. Why this harness, not the existing planner harness
+
+The M2 `PlannerHarness` (`subscription-compilation/tests.md` §9.3) is deliberately scoped to the planner subsystem: it tests the compile function in isolation, with no actor, no FFI seam, no storage backend. Several framework-magic bullets (C7 publish flow, C11 onboarding, C12 account switch, C13 in-place refinement) require the full actor and the full FFI emission path. The `ContractHarness` is therefore a strict superset.
+
+The two harnesses live alongside each other. M2-specific tests continue to use `PlannerHarness`; cross-cutting framework-magic tests use `ContractHarness`. No deduplication is forced; an assertion's logic may exist in both files (e.g., "kind:10002 arrival triggers recompile") — the planner version asserts the compile output, the contract version asserts the wire frame plus the view payload re-emit.
+
+## 7. Reverse-cross-reference: which milestone touches which test?
+
+| Milestone | Tests that flip from `#[ignore]` to active |
+|---|---|
+| M2 | C5, C6, C8 (all sub-paths); C7 sub-paths 3 + 4 (planner-only); C13 sub-paths 2 + 3 + 4 (projection cache) |
+| M3 | C2, C3, C4 (LMDB + tombstones + persistence); C9 (provenance schema + cap) |
+| M4 | C10 (full sync engine) |
+| M5 | (no contract bullets directly; auth-paused relays are an internal mechanism) |
+| M6 | C7 sub-paths 1 + 2 + 5 (SendNote consumer); C11 (signers + onboarding actions) |
+| M8 | C12 (multi-account state machine) |
+
+Total: 13 behavior tests + 1 meta-test = 14 `#[test] fn` declarations across six milestone exit-gate transitions. The framework-magic delta at each milestone removes a known subset of `#[ignore]` lines; the contract document's "Milestone owner" column is the canonical source for which.
+
+## 8. What this scaffolding does not specify
+
+- **The harness implementation.** The skeleton above is the API; the implementation is the next agent's deliverable (a `framework-magic-harness` task, or the M2 milestone implementation owner folding it in).
+- **The reverse mapping from `AppAction` variants to action-ledger rows.** That's `kernel-substrate.md` §4 territory; the harness exposes `action_ledger()` and the test reads rows by index/id.
+- **Per-platform binding tests.** Cross-platform consistency (`subsystems.md` §3.5) is a separate test suite that runs the same scripted actions on iOS / Android / Desktop / Web and diffs `AppState` JSON. The framework-magic contract is Rust-only; platform-binding regressions show up in the cross-platform suite.
+- **Negative tests for the API surface.** "The app cannot type `SendNote { content, relays: vec![...] }`" is a *compile-fail* test, owned by `docs/design/subscription-compilation/tests.md` §9.2 assertion 1. The framework-magic surface assertion is "no test passes the broken usage"; the structural inability is asserted there.

Review for: (1) doctrine compliance — does the framework-magic contract violate D0 by adding app nouns to the kernel, (2) TODO/FIXME/unimplemented sneaking in, (3) file-size budget per file, (4) internal consistency of the contract (are the 14 tests coherent with the contract). Be terse. Name file:line for any real concern.
codex
I’ll ground this in the committed files, not just the pasted diff. I’m checking the new docs for line counts, forbidden placeholders/TODOs, and contract/test naming drift.
exec
/bin/zsh -lc 'wc -l docs/design/framework-magic.md docs/design/framework-magic/*.md' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
exec
/bin/zsh -lc 'git show --stat --oneline --decorate --no-renames edc17b0' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
      94 docs/design/framework-magic.md
      71 docs/design/framework-magic/capabilities.md
      93 docs/design/framework-magic/intro.md
      69 docs/design/framework-magic/kind3.md
      69 docs/design/framework-magic/outbox.md
      92 docs/design/framework-magic/replaceable.md
      63 docs/design/framework-magic/sessions.md
      76 docs/design/framework-magic/signers.md
      48 docs/design/framework-magic/subs.md
      66 docs/design/framework-magic/sync.md
     208 docs/design/framework-magic/test-scaffolding.md
     949 total

 succeeded in 0ms:
edc17b0 design(framework-magic): contract + 10 sub-files + test-scaffolding (task #17)
 docs/design/framework-magic.md                  |  94 +++++++++++
 docs/design/framework-magic/capabilities.md     |  71 ++++++++
 docs/design/framework-magic/intro.md            |  93 +++++++++++
 docs/design/framework-magic/kind3.md            |  69 ++++++++
 docs/design/framework-magic/outbox.md           |  69 ++++++++
 docs/design/framework-magic/replaceable.md      |  92 +++++++++++
 docs/design/framework-magic/sessions.md         |  63 +++++++
 docs/design/framework-magic/signers.md          |  76 +++++++++
 docs/design/framework-magic/subs.md             |  48 ++++++
 docs/design/framework-magic/sync.md             |  66 ++++++++
 docs/design/framework-magic/test-scaffolding.md | 208 ++++++++++++++++++++++++
 11 files changed, 949 insertions(+)

exec
/bin/zsh -lc 'rg -n "TODO|FIXME|unimplemented|todo!|unimplemented!|panic!|TBD-from-research|TBD|pending|PENDING|DONE|M11|M11'"\\.5|M12|DM|Wallet|podcast|Highlighter|nmp-core|AppAction|TimelineView|ProfileView|SendNote|FollowingTimelineView\" docs/design/framework-magic.md docs/design/framework-magic" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
docs/design/framework-magic.md:3:> **Status:** Draft (initial structure). Research-fold commit fills `TBD-from-research(...)` markers from `docs/research/applesauce/event-store-query-builders.md` and `docs/research/ndk/kind3-auto-tracking.md` when they land.
docs/design/framework-magic.md:30:| C1 | Replaceable-event supersession (kind 0 / 3 / 10000–19999) on insert | replaceable.md | `c1_replaceable_supersedes_on_insert` | **[DONE]** kernel | spec §7.1 row "Replaceable kinds"; §3.3 bug #1 |
docs/design/framework-magic.md:31:| C2 | Parameterized replaceable supersession (30000–39999) by `(pubkey, kind, d-tag)` | replaceable.md | `c2_parameterized_replaceable_supersedes_by_dtag` | **[PENDING M3]** | spec §7.1 row "Parameterized replaceable"; §3.3 bug #1 |
docs/design/framework-magic.md:32:| C3 | Kind:5 delete propagation: referenced events removed, tombstone persisted | replaceable.md | `c3_kind5_delete_removes_referenced_and_tombstones` | **[PENDING M3]** | spec §7.1 row "Kind 5 (delete)" |
docs/design/framework-magic.md:33:| C4 | NIP-40 expiration auto-removes event at expiry; survives actor restart | replaceable.md | `c4_nip40_expiration_removes_and_persists_schedule` | **[PENDING M3]** | spec §7.1 row "NIP-40 expiration" |
docs/design/framework-magic.md:34:| C5 | Kind:3 auto-tracking: active account's follow-list change recompiles dependent subscriptions transparently | kind3.md | `c5_kind3_change_recompiles_follow_dependent_subs` | **[PENDING M2]** | scope-adj §"Folded into M2"; D3; M2 design §4 (Trigger::Nip65Arrived analog for kind:3) |
docs/design/framework-magic.md:35:| C6 | Outbox read routing: `authors`-filter subscriptions fan out to those authors' write relays (NIP-65), de-duplicated | outbox.md | `c6_authors_subscription_routes_to_per_author_write_relays` | **[PENDING M2]** | D3; spec §7.3 row "Subscription with `authors`"; M2 design §7 |
docs/design/framework-magic.md:36:| C7 | Outbox write routing: publishes go to author write + `#p`-recipient inbox; private (gift-wrap) events fail closed when recipient inbox is unknown | outbox.md | `c7_publish_routes_outbox_and_private_fails_closed` | **[PENDING M2 seam → M6 publish]** | D3; spec §7.3 rows "Publish*"; §3.3 bugs #3, #4 |
docs/design/framework-magic.md:37:| C8 | Subscription planner deduplicates overlapping interests into one wire REQ per relay, auto-closes on EOSE / last-consumer-drop, and buffers ingress to ≤60Hz per view | subs.md | `c8_subscriptions_coalesce_autoclose_and_buffer` | **[PENDING M2]** | spec §7.2; §3.3 bug #2, bug #8 |
docs/design/framework-magic.md:38:| C9 | Provenance preserved: same event id arriving from N relays merges into one stored event with N-entry provenance set; original `id` and signature untouched | sync.md | `c9_provenance_merges_across_relay_redeliveries` | **[PENDING M3]** | aim §6 doctrine 10; spec §7.1 row "Provenance"; §3.3 bug #10 |
docs/design/framework-magic.md:39:| C10 | Sync watermarks: planner consults `(filter, relay)` coverage before issuing historical REQ; full coverage makes cache-miss authoritative; NIP-77 negentropy is the default backfill where supported | sync.md | `c10_watermark_gates_backfill_and_authoritative_miss` | **[PENDING M4]** | D2; spec §7.1 watermarks, §7.8 sync engine |
docs/design/framework-magic.md:40:| C11 | Signer onboarding: pasted `bunker://` URL parses + connects via NIP-46; "create new nsec" generates, NIP-49-encrypts, and persists via KeyringCapability — both as kernel actions, no app code | signers.md | `c11_bunker_url_and_nsec_creation_complete_via_actions` | **[PENDING M6]** | scope-adj §"Folded into M6"; spec §7.4 |
docs/design/framework-magic.md:41:| C12 | Account switch is a state transition: dispatching the switch action re-resolves every `ActiveAccount`-scoped view without the app issuing CLOSE/REQ or rebuilding view handles | sessions.md | `c12_account_switch_rebinds_views_without_imperative_dance` | **[PENDING M8]** | D4; spec §7.4; §3.3 bug #5; M2 §4 trigger A4 |
docs/design/framework-magic.md:42:| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/design/framework-magic.md:63:Tests for behaviors whose owning milestone has not landed are checked in with `#[ignore = "pending M_n"]`; the meta-test still counts them. This is the "doc says 13, code tests 11" regression the file-naming convention break (cross-cutting, not milestone-prefixed) is designed to support — see [test-scaffolding.md](framework-magic/test-scaffolding.md) §1.
docs/design/framework-magic.md:69:1. Which contract bullets transitioned from `[PENDING M_n]` to `[DONE]`.
docs/design/framework-magic.md:70:2. Which `#[ignore = "pending M_n"]` lines were removed from `framework_magic_contract.rs`.
docs/design/framework-magic.md:77:The following are `TBD-from-research(...)` markers in the sub-files; the research-fold commit replaces them with file:line refs and concrete API shapes. They are listed here so the orchestrator can sequence the work:
docs/design/framework-magic.md:79:- `kind3.md` §3 — `TBD-from-research(ndk/kind3-auto-tracking.md)`: NDK's exact mechanism for kind:3 → open-subscription recompile (event listener path, refcount handoff, race window).
docs/design/framework-magic.md:80:- `kind3.md` §4 — `TBD-from-research(applesauce/event-store-query-builders.md)`: Applesauce's query-builder pattern that makes `WhoFollows(active_user)` reactive without app code.
docs/design/framework-magic.md:81:- `outbox.md` §2 — `TBD-from-research(ndk/kind3-auto-tracking.md)`: how NDK rebinds in-flight REQs when an author's mailbox arrives mid-subscription.
docs/design/framework-magic.md:82:- `subs.md` §3 — `TBD-from-research(applesauce/event-store-query-builders.md)`: Applesauce's logical-vs-wire subscription split file:line refs (for cross-validation against `docs/design/subscription-compilation/intro.md` §2).
docs/design/framework-magic.md:83:- `sync.md` §4 — `TBD-from-research(applesauce/event-store-query-builders.md)`: Applesauce's coverage/watermark equivalent and how a query-builder reads it.
docs/design/framework-magic/signers.md:17:- The signer catalog at `subsystems.md` §7.4 lines 127–135 names both kinds as supported in `nmp-core` (no FFI signer extensibility — apps don't implement signers).
docs/design/framework-magic/signers.md:23:**App writes:** for **bunker**, one dispatch with the pasted URL: `dispatch(AppAction::BunkerConnect { url: "bunker://..." })`. For **create new nsec**, one dispatch: `dispatch(AppAction::CreateLocalIdentity { passphrase, label })`. The action ledger row exposes progress (parsing, rendezvous, awaiting user approval on the bunker app, persisted, available); the app's UI renders the ledger row as a step indicator if it wants, but the orchestration is the framework's. The app does **not** call NIP-46 transport code, does **not** invoke NIP-49 encryption, does **not** touch the Keychain directly, and does **not** wire the new identity into the session state.
docs/design/framework-magic/signers.md:40:   e. Assert a follow-up `SwitchActiveAccount { account_id }` succeeds and that the actor can sign a test event using the newly-created identity (round-trip: dispatch a `SendNote` against the new account, observe a signed event in the action ledger before publish).
docs/design/framework-magic/signers.md:42:**Milestone owner:** **[PENDING M6]**. M6 is the signers + write-path milestone (per `scope-adjustments-2026-05-18.md` ladder). M6 owner adds the framework-magic delta after the test goes green. Test checked in as `#[ignore = "pending M6 signers"]`.
docs/design/framework-magic/signers.md:74:- **Signing a publish** — the sign step inside `SendNoteAction` (C7). C11 covers onboarding; subsequent signing is the publish path.
docs/design/framework-magic/subs.md:23:1. **Dedup:** open two `TimelineView`s with identical filters; assert the planner produces one wire REQ per relay (not two); destroy one; assert the wire REQ stays alive; destroy the second; assert the REQ is CLOSE'd after the warmth grace expires (`subsystems.md` §7.6 line 226: 30s default).
docs/design/framework-magic/subs.md:24:2. **Coalesce:** open `TimelineView { authors: [A, B], kinds: [1] }` and `ProfileView { pubkey: C }`; assert the planner merges into one REQ per relay containing the union shape, with each view receiving only its filtered subset locally (no REQ for kind:0 alone if the relay already has the merged stream covering it). The merge lattice's exact rules live in `subsystems.md` §7.2 line 65 and `docs/design/subscription-compilation/intro.md` §1 open-question #2 (lattice formalization); the test asserts the *observable* (wire frame count = correct fewer-than-naive, payload coverage = correct) rather than the lattice mechanics.
docs/design/framework-magic/subs.md:28:**Milestone owner:** **[PENDING M2]** for sub-paths 1–3 (the compiler + lifecycle); **partial overlap with reactivity-bench** for sub-path 4 (the buffer cadence is exercised by `docs/perf/reactivity-bench/` already; the contract test asserts the same property through the public view path). Test checked in as `#[ignore = "pending M2 compiler"]` initially.
docs/design/framework-magic/subs.md:48:`TBD-from-research(applesauce/event-store-query-builders.md)`: cite the Applesauce file:line for the logical-vs-wire subscription split that NMP's compiler mirrors. The cross-validation in the research-fold commit confirms NMP's `LogicalInterest` (`docs/design/subscription-compilation/intro.md` §2.1) covers Applesauce's surface and that no observable property is lost in translation.
docs/design/framework-magic/sessions.md:18:**App writes:** one dispatch: `dispatch(AppAction::SwitchActiveAccount { pubkey })`. The app's "switch account" UI is a button that fires that dispatch. No log-out / log-in dance, no view-tree rebuild, no manual REQ reissue, no clearing of caches — the framework handles all of it as a single tick of the actor's event loop.
docs/design/framework-magic/sessions.md:25:2. **Initial open:** open `FollowingTimelineView` (no fields — derives from active account); assert the planner opens REQs on `{r1, r2}`; assert the payload emits with follow set `{X, Y}`.
docs/design/framework-magic/sessions.md:26:3. **Dispatch switch:** `dispatch(AppAction::SwitchActiveAccount { pubkey: bob_pk })`. The test makes no other calls; the harness drains the action ledger and the planner trigger queue.
docs/design/framework-magic/sessions.md:28:5. **Assert view handle stability:** the `FollowingTimelineView` handle from step 2 is **the same handle**; it has not been torn down. Its payload has been re-emitted once, now reflecting Bob's follow set `{Y, Z}`.
docs/design/framework-magic/sessions.md:29:6. **Assert signer rebinding:** dispatch a `SendNote { content: "hello" }`; assert the signed event's `pubkey = bob_pk` (the new active account's signer was used), without any explicit signer parameter on the `SendNote` action.
docs/design/framework-magic/sessions.md:30:7. **Assert specific-scoped views untouched:** before step 3, also open `ProfileView { pubkey: charlie_pk }` (an `InterestScope::Account(charlie)`-equivalent — actually Global since it names an explicit author). Assert this view's payload is not re-emitted after the switch; its underlying REQ stays alive on the same relay; no delta frames touch it. This is the symmetric assertion: the switch affects *only* `ActiveAccount`-scoped interests, per `subscription-compilation/recompilation.md` §4.2 line 113.
docs/design/framework-magic/sessions.md:33:**Milestone owner:** **[PENDING M8]**. M8 is the multi-account session milestone (per `scope-adjustments-2026-05-18.md` ladder). M2 already lands the `Trigger::ActiveAccountChanged` shape (`subscription-compilation/recompilation.md` §4.2 line 109: "M2 establishes the trigger; M8 wires the multi-account state machine"). Test checked in as `#[ignore = "pending M8 multi-account state machine"]`. Sub-paths 4 and 7 are testable as soon as M2 lands (single-account boot fires the trigger once with `from: None, to: Some(active)` per the M2 design); the rest needs M8.
docs/design/framework-magic/sessions.md:61:- **The account-switcher view payload.** That is a view module (`AccountListView` or similar in `nmp-core`'s built-ins per `subsystems.md` §7.4); its spec/payload is owned by the view catalog, not the contract.
docs/design/framework-magic/sync.md:26:**Milestone owner:** **[PENDING M3]**. Sub-paths 1–6 are testable today against the in-memory kernel (the current `relay_count` field at `crates/nmp-core/src/kernel/ingest.rs:238` is the primitive shape; M3 graduates it to a typed `Provenance` sidecar). Sub-path 7 requires M3's storage cap logic. Test checked in as `#[ignore = "pending M3 provenance schema"]`.
docs/design/framework-magic/sync.md:40:1. **Unsynced pair → fetch.** Open `TimelineView { authors: [A], kinds: [1], since: T-1d, until: T }` against a fresh store (no watermark for this `(filter, relay)` pair). Assert the planner schedules a backfill — NIP-77 reconciliation against the mock relay (because capability negotiation succeeded). Mock relay returns a 50-event set; assert all 50 land in the store; assert the watermark for `(filter_sig, "wss://mock")` updates to `synced_up_to = T`.
docs/design/framework-magic/sync.md:46:**Milestone owner:** **[PENDING M4]**. M4 is the NIP-77 milestone (per `docs/plan/scope-adjustments-2026-05-18.md` v1 ladder). The watermark *schema* lands earlier in M3 (`docs/design/lmdb/watermarks.md`); M4 lands the engine that reads/writes them and the capability negotiation. Test checked in as `#[ignore = "pending M4 sync engine"]`. Sub-path 2 (authoritative-miss given a populated watermark) is the structural assertion that does not require NIP-77 — it could be flipped to non-ignored as soon as M3 lands the schema and a stub engine.
docs/design/framework-magic/sync.md:57:- `docs/design/subscription-compilation/compiler.md` Stage X for the planner's watermark consultation in the compile pipeline. (TBD: confirm Stage number in research-fold; the compiler file specifies it.)
docs/design/framework-magic/sync.md:60:`TBD-from-research(applesauce/event-store-query-builders.md)`: cite Applesauce's coverage/watermark equivalent and the API by which a query-builder reads it. NMP's `WatermarksSummary` (`subsystems.md` §7.8 line 287) is the analogous app-visible surface; the research-fold commit verifies the surface covers the same diagnostic needs.
docs/design/framework-magic/kind3.md:14:1. Replaces the stored kind:3 in the event store (per C1; mechanism at `crates/nmp-core/src/kernel/ingest.rs:187-207` — currently stored in `self.seed_contacts` map; M2 graduates this into the projection cache).
docs/design/framework-magic/kind3.md:20:**App writes:** nothing. The "following timeline" view's spec does not name authors — the view module consumes the active account's follow-set internally. The app's only contact with this surface is opening `FollowingTimelineView { /* no fields */ }` and reading its `Payload.items`.
docs/design/framework-magic/kind3.md:26:1. Opens a `FollowingTimelineView` against an active account whose stored kind:3 follows pubkeys `{A, B, C}` with mailbox cache pre-seeded so A→relay1, B→relay2, C→relay3.
docs/design/framework-magic/kind3.md:30:5. Asserts the same `FollowingTimelineView` handle is still open (refcount unchanged); the platform shadow has emitted one additional payload, not torn down and re-created.
docs/design/framework-magic/kind3.md:35:**Milestone owner:** **M2** (the subscription-compilation milestone owns the trigger and the recompile). M2's exit gate (`docs/design/subscription-compilation/tests.md` §9) currently lists four assertions covering the NIP-65 case; the M2 owner adds this fifth assertion as part of the framework-magic delta. Test starts as `#[ignore = "pending M2 trigger"]`; M2 lands the trigger and removes the ignore.
docs/design/framework-magic/kind3.md:47:The mechanism NDK uses is documented in the parallel research file `docs/research/ndk/kind3-auto-tracking.md` (pending agent landing). The contract here does not depend on NDK's specific code path; it depends on the *property* NDK demonstrates: that a kind:3 replacement re-shapes the open-REQ set without the application observing protocol churn.
docs/design/framework-magic/kind3.md:49:`TBD-from-research(ndk/kind3-auto-tracking.md)`: insert file:line ref to NDK's listener and the exact race-window it closes (specifically: what happens if a kind:3 arrives mid-EOSE on a follow-derived REQ). The contract is satisfied by *any* mechanism that produces the observable behavior in C5; NDK's path is one existence proof.
docs/design/framework-magic/kind3.md:55:`TBD-from-research(applesauce/event-store-query-builders.md)`: cite the query-builder API shape that lets a consumer phrase `"things kind:1 by people I follow"` once and get a stream that re-evaluates on every kind:3 change. Applesauce's mechanism is a builder that registers itself as a dependent of the kind:3 projection; the contract's `ViewModule.dependencies()` is the NMP analog (`docs/design/kernel-substrate.md` §3 lines 131–132). The research-fold commit cross-validates that the analog covers Applesauce's pattern fully.
docs/design/framework-magic/intro.md:25:| **D0** kernel + extension modules (no app nouns in `nmp-core`) | All 13 — the contract is the API the app sees in place of the missing nouns |
docs/design/framework-magic/intro.md:51:**Milestone owner:** M_n (or `[DONE]`). <one sentence on what implementation status looks like>
docs/design/framework-magic/intro.md:57:2. **It is mechanically diffable.** A milestone delta is "this row's Milestone owner changed from `[PENDING M2]` to `[DONE]` and this `#[ignore]` came off." A contract regression is "this row's `App writes` grew from `nothing` to one line; ADR required."
docs/design/framework-magic/intro.md:65:- bullets moved from `[PENDING M_n]` to `[DONE]`
docs/design/framework-magic/intro.md:72:The post-merge codex review reads this contract and the delta together. Drift between contract claims and test outcomes (e.g., the doc says `[DONE]` but the test is still `#[ignore]`) is a flagged review issue.
docs/design/framework-magic/outbox.md:21:2. Open `TimelineView { authors: <1000 pubkeys>, kinds: [1, 6] }` through the actor's public dispatch surface.
docs/design/framework-magic/outbox.md:24:5. `TBD-from-research(ndk/kind3-auto-tracking.md)`: cross-check that the in-flight REQ for the moved author rebinds without losing the live tail across the CLOSE/REOPEN boundary.
docs/design/framework-magic/outbox.md:28:**Milestone owner:** **[PENDING M2]**. Test checked in as `#[ignore = "pending M2 compiler + view bridge"]`. Removed in the M2 framework-magic delta.
docs/design/framework-magic/outbox.md:38:- The `PublishWithOverride` action is the *only* `AppAction` variant carrying a `Vec<RelayUrl>` field, and it is forbidden from widening a `PrivateToRecipients` plan to public relays (`outbox.md` §7.4 rule 4).
docs/design/framework-magic/outbox.md:40:**App writes:** nothing — for the publish path. The app dispatches a publish action (`SendNote`, `React`, `SendDm`, etc.); the action's privacy mode is determined by the action type, not by an app-supplied parameter. There is no `relays` field on `SendNote`. The override exists for tests, migrations, and operator power-user flows; it is structurally outside the safe app path.
docs/design/framework-magic/outbox.md:42:**Failure mode prevented:** §3.3 bug #3 ("Publish of an event to relays the author has not declared as write relays") and bug #4 ("DM published to public relays"). Plus the doctrine-10 footgun: a "send everywhere" fallback that publishes a gift wrap to the global content relay because the recipient's inbox lookup returned empty.
docs/design/framework-magic/outbox.md:46:1. **Public:** seed Alice's mailbox with two write relays; dispatch a public `SendNote` action; assert the resulting publish plan has exactly those two relays and no others, and that `required_success_count = max(1, ceil(2/3)) = 1` per `outbox.md` §7.3 step 3(a).
docs/design/framework-magic/outbox.md:52:**Milestone owner:** **[PENDING M2 seam → M6 publish]**. M2 lands the `PublishPlanner` trait + `Nip65PublishPlanner` + the `PublishWithOverride` action (`docs/design/subscription-compilation/outbox.md` §7.1, §7.2, §7.4). M6 lands `SendNoteAction` as the first concrete consumer. Test checked in as `#[ignore = "pending M2 planner + M6 first consumer"]`. Sub-paths 3 and 4 of the test exercise the planner in isolation (M2-completable); 1, 2, and 5 require M6's action consumer.
docs/design/framework-magic/replaceable.md:12:**Framework does:** the insert-time supersession at `docs/product-spec/subsystems.md` §7.1 row "Replaceable kinds (0, 3, 10000-19999)". Mechanism: compare `(pubkey, kind)` against the existing entry, keep newest `created_at`, tie-break by lexicographically smallest `id`. The current in-memory store enforces this for kind:0 / kind:3 / kind:10002 today (kind:3 via `seed_contacts.insert` at `crates/nmp-core/src/kernel/ingest.rs:206`; kind:10002 via the `should_replace` branch at `crates/nmp-core/src/kernel/ingest.rs:218-222`). M3 graduates the rule into the LMDB-backed `EventStore` trait (`docs/design/lmdb/trait.md`).
docs/design/framework-magic/replaceable.md:14:**App writes:** nothing. The app calls `ProfileView::open(pubkey)`; the view's payload reflects the latest kind:0 the store has, with no app-side comparison of `created_at`.
docs/design/framework-magic/replaceable.md:18:**Test:** `c1_replaceable_supersedes_on_insert`. The test inserts kind:0 #1 at `created_at=T`, then kind:0 #2 at `T+1` with same pubkey; asserts `ProfileView` payload reflects #2 and that a subsequent insert at `T-1` is rejected (no payload re-emit, no event store change). Tie-break path: two inserts at the same `T` with different ids — the lexicographically-smaller-id event wins, deterministic across runs.
docs/design/framework-magic/replaceable.md:20:**Milestone owner:** **[DONE]** for in-memory kernel (verified by `crates/nmp-core` kernel tests today, ref the existing `should_replace` branch). Test runs **not** ignored from day one; LMDB graduation in M3 must preserve the same observable, so the test stays green across M3.
docs/design/framework-magic/replaceable.md:36:**Milestone owner:** **[PENDING M3]**. Test checked in as `#[ignore = "pending M3 LMDB"]`; M3 owner removes the ignore as part of the framework-magic delta on M3's exit-gate report. (Note: the M3 LMDB-tests doc already contains the same scenario at the storage layer; C2 promotes it from a storage-layer test to a contract-surface test — the framework-magic test calls through the public view path, not through the EventStore trait directly.)
docs/design/framework-magic/replaceable.md:46:**App writes:** nothing. The view payloads recompute (via `ViewModule::on_event_removed` per `docs/design/kernel-substrate.md` §3 lines 141–143) and the deleted note disappears from `TimelineView.items` in the next emit.
docs/design/framework-magic/replaceable.md:52:1. Inserts a kind:1 event `e1` by author Alice; asserts it appears in `TimelineView`.
docs/design/framework-magic/replaceable.md:53:2. Inserts a kind:5 by Alice referencing `e1`; asserts `TimelineView` no longer contains `e1`.
docs/design/framework-magic/replaceable.md:58:**Milestone owner:** **[PENDING M3]**. Test checked in as `#[ignore = "pending M3 tombstone persistence"]`. The in-memory kernel today does not enforce tombstone persistence across restart; M3's LMDB schema (`docs/design/lmdb/keys.md`) is where the tombstone subdatabase lands. Steps 1–4 of the test can pass against the in-memory kernel; step 5 requires M3.
docs/design/framework-magic/replaceable.md:76:3. Advance clock to +61s; assert event removed; `TimelineView` payload re-emitted without it.
docs/design/framework-magic/replaceable.md:80:**Milestone owner:** **[PENDING M3]**. Test checked in as `#[ignore = "pending M3 expiration persistence"]`. Steps 1–3 are testable today (timer-only); step 5 needs M3.
docs/design/framework-magic/capabilities.md:10:**Statement.** Every display-bearing field of every view payload is **non-`Option`** and carries either an authoritative value or a defined placeholder. When the authoritative value later arrives — a kind:0 for an author, kind:9735 zap receipts for a note, the decrypted body for a DM — the same payload re-emits with the field updated in place. The platform's reactive primitive (`@Observable` / `Flow` / signals) sees the change and only the affected cell re-renders. **No spinner ever gates an already-rendered cell, and no view module ever exposes a `loading: bool` to the platform.**
docs/design/framework-magic/capabilities.md:33:1. **Placeholders at open:** open `TimelineView { authors: [alice], kinds: [1] }` against a fresh store with no kind:0 for Alice. Insert a kind:1 event by Alice. Assert the payload's `items[0]`:
docs/design/framework-magic/capabilities.md:43:5. **Freshness hint, not gate:** insert an older cached kind:0 for Alice (created two days ago), then a fresher one (created an hour ago). Assert the payload reflects the *fresher* one (per C1 supersession), and that the optional `author_display_freshness` field (if exposed by the view module) reads `Recent`, not `DaysOld`. Assert there is no API surface where the test can ask "is this stale?" and have the framework withhold the value pending re-fetch.
docs/design/framework-magic/capabilities.md:45:**Milestone owner:** **[DONE]** for the placeholder shape (the M1 timeline slice already ships non-`Option` author fields with shortened-npub fallback — verified in `crates/nmp-core` timeline tests today). **[PENDING M2/M3]** for the full in-place refinement guarantees: sub-paths 1 and 5 are testable today; sub-paths 2 and 4 require the kernel's projection cache (`kernel-substrate.md` §3 line 148 `on_projection_changed`) which graduates in M2 alongside the view-module surface; sub-path 3 requires the per-tick re-format hook (`fn on_tick`, M2's `ViewModule` trait work).
docs/design/framework-magic/capabilities.md:71:- **DM body decryption inside the view payload.** The decrypted body fits the same C13 pattern (placeholder = empty string; in-place refinement when decrypt succeeds), but the decryption path itself is M9 territory and is not v1.
docs/design/framework-magic/test-scaffolding.md:84:    pub fn dispatch(&mut self, action: AppAction);
docs/design/framework-magic/test-scaffolding.md:172:3. A renamed test breaks the doc-test correspondence — caught by either step 3 or 4 depending on which side renamed first.
docs/design/framework-magic/test-scaffolding.md:174:The meta-test does **not** check `#[ignore]` status. A test for a pending milestone is correctly `#[ignore]`'d; the meta-test's job is structural correspondence, not implementation readiness. The milestone delta protocol (`intro.md` §4) handles the un-ignore cadence.
docs/design/framework-magic/test-scaffolding.md:178:A test is `#[ignore = "pending M_n"]`'d when its owning milestone has not landed the implementation. The ignore reason **must** name the milestone (e.g., `"pending M3 tombstone persistence"`) so that a `grep -n "pending M" framework_magic_contract.rs` produces a checklist for each milestone's exit gate.
docs/design/framework-magic/test-scaffolding.md:180:The framework-magic delta in a milestone's exit-gate report enumerates which `pending M_n` ignore lines were removed during the milestone. Removing an ignore line without the delta entry fails the post-merge codex review.
docs/design/framework-magic/test-scaffolding.md:198:| M6 | C7 sub-paths 1 + 2 + 5 (SendNote consumer); C11 (signers + onboarding actions) |
docs/design/framework-magic/test-scaffolding.md:206:- **The reverse mapping from `AppAction` variants to action-ledger rows.** That's `kernel-substrate.md` §4 territory; the harness exposes `action_ledger()` and the test reads rows by index/id.
docs/design/framework-magic/test-scaffolding.md:208:- **Negative tests for the API surface.** "The app cannot type `SendNote { content, relays: vec![...] }`" is a *compile-fail* test, owned by `docs/design/subscription-compilation/tests.md` §9.2 assertion 1. The framework-magic surface assertion is "no test passes the broken usage"; the structural inability is asserted there.

exec
/bin/zsh -lc 'git status -sb' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
## worktree-agent-a8c0acab50f455ada
?? docs/perf/codex-reviews/151c80c.md
?? docs/perf/codex-reviews/55dd5f2.md
?? docs/perf/codex-reviews/59f8d1b.md
?? docs/perf/codex-reviews/8302cc1.md
?? docs/perf/codex-reviews/9eace68.md
?? docs/perf/codex-reviews/d660735.md
?? docs/perf/codex-reviews/e9cbafa.md
?? docs/perf/codex-reviews/edc17b0.md
?? docs/perf/codex-reviews/f1e374b.md
?? docs/perf/codex-reviews/f68a479.md
?? docs/perf/codex-reviews/fd8f173.md

codex
The line-count check is clean: every new markdown file is under 300 lines. The first real drift I’m chasing is stronger: the contract text says the 14 tests are checked in, so I’m verifying whether that file actually exists and whether the referenced test surface is present.
exec
/bin/zsh -lc 'test -f crates/nmp-testing/tests/framework_magic_contract.rs; echo $?' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
1

exec
/bin/zsh -lc 'rg -n "framework_magic_contract|framework_magic|ContractHarness|contract_surface_complete|c1_replaceable_supersedes_on_insert|c13_view_payload_uses_placeholders_then_refines_in_place" crates docs Cargo.toml' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
docs/perf/orchestration-log.md:12:| 2026-05-18 02:55 | 4 | Heartbeat #4 (autonomous). MAJOR: T17 framework-magic-designer landed edc17b0 (13-bullet contract + 14 named tests in framework_magic_contract.rs across 10 sub-files + index). T15 applesauce-explorer + T16 ndk-explorer both completed but couldn't write/commit themselves; orchestrator transcribed 13 files (5 applesauce + 8 ndk) and pushed as 8d633e8. KEY FINDING from NDK research that propagated into framework-magic contract + scope memo: "auto-update of open subs when follow list changes — no app code involved" is empirically FALSE in core NDK; Svelte gets it via runes, React requires explicit deps, raw core has NO follow-list watcher. NMP must build this as framework code (~200 LOC per platform) AND the framework-magic contract guarantees app dispatches zero code. T8 codex-fixer-1 also wrapped up its iterative cycle (fb139ab + 80217cc final). User landed mid-tick directive: maintain top-level README.md with TL;DR + decisions + architecture map (committed 810d0f8); heartbeat cron rewritten (cc1fba11) to fold README refresh into every tick. ~~4 lingering m11-design followup commits~~ (59f8d1b/8302cc1/fd8f173/2477372) from codex-fixer-2 cleaned by its own internal codex loop. T15/T16/T17 marked completed. Pending T13 (M2 codex fixes), T14 (M3 codex fixes) still held. Plan-splitter, m1-hardener, highlighter-explorer, codex-fixer-1, clippy-cleaner still in flight. |
docs/design/framework-magic/kind3.md:24:**Test:** `c5_kind3_change_recompiles_follow_dependent_subs` in `crates/nmp-testing/tests/framework_magic_contract.rs`. The test:
docs/perf/codex-reviews/9eace68.md:84:+The deliverable: `docs/design/framework-magic.md` (the contract) + `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per bullet). The contract evolves alongside milestones; each milestone owner adds a "framework-magic delta" section to their exit-gate report.
docs/perf/codex-reviews/9eace68.md:338:docs/plan/scope-adjustments-2026-05-18.md:45:The deliverable: `docs/design/framework-magic.md` (the contract) + `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per bullet). The contract evolves alongside milestones; each milestone owner adds a "framework-magic delta" section to their exit-gate report.
docs/perf/codex-reviews/9eace68.md:384:docs/design/framework-magic.md:6:> **Companion test file:** `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per contract bullet plus a coverage meta-test; layout in [test-scaffolding.md](framework-magic/test-scaffolding.md)).
docs/perf/codex-reviews/9eace68.md:394:docs/design/framework-magic.md:22:- [Test scaffolding — the `framework_magic_contract.rs` harness, naming convention, coverage meta-test](framework-magic/test-scaffolding.md)
docs/perf/codex-reviews/9eace68.md:401:docs/design/framework-magic.md:42:| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/9eace68.md:402:docs/design/framework-magic.md:46:**Test names** (13 behavior tests + 1 coverage meta-test = 14 total, all in `crates/nmp-testing/tests/framework_magic_contract.rs` — see [test-scaffolding.md](framework-magic/test-scaffolding.md) for the harness):
docs/perf/codex-reviews/9eace68.md:542:docs/design/framework-magic/test-scaffolding.md:18:The next milestone owner must **not** rename the file to fit the milestone-prefix pattern. The framework-magic delta protocol in `intro.md` §4 covers this: a milestone landing that renames `framework_magic_contract.rs` requires an ADR.
docs/perf/codex-reviews/9eace68.md:549:docs/design/framework-magic/test-scaffolding.md:186:The M2 `PlannerHarness` (`subscription-compilation/tests.md` §9.3) is deliberately scoped to the planner subsystem: it tests the compile function in isolation, with no actor, no FFI seam, no storage backend. Several framework-magic bullets (C7 publish flow, C11 onboarding, C12 account switch, C13 in-place refinement) require the full actor and the full FFI emission path. The `ContractHarness` is therefore a strict superset.
docs/perf/codex-reviews/9eace68.md:550:docs/design/framework-magic/test-scaffolding.md:188:The two harnesses live alongside each other. M2-specific tests continue to use `PlannerHarness`; cross-cutting framework-magic tests use `ContractHarness`. No deduplication is forced; an assertion's logic may exist in both files (e.g., "kind:10002 arrival triggers recompile") — the planner version asserts the compile output, the contract version asserts the wire frame plus the view payload re-emit.
docs/perf/codex-reviews/9eace68.md:618:    45	The deliverable: `docs/design/framework-magic.md` (the contract) + `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per bullet). The contract evolves alongside milestones; each milestone owner adds a "framework-magic delta" section to their exit-gate report.
docs/perf/codex-reviews/9eace68.md:2653:docs/design/framework-magic.md:26:Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
docs/perf/codex-reviews/9eace68.md:2661:docs/design/framework-magic.md:42:| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/9eace68.md:2919:docs/perf/codex-reviews/9eace68.md:401:docs/design/framework-magic.md:42:| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/9eace68.md:3628:docs/perf/codex-reviews/f68a479.md:324:docs/design/framework-magic.md:42:| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/9eace68.md:4050:docs/perf/codex-reviews/f68a479.md:2120:docs/perf/codex-reviews/51120cb.md:231:+| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/9eace68.md:4091:docs/perf/codex-reviews/f68a479.md:2164:docs/perf/codex-reviews/51120cb.md:3546:    42	| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/9eace68.md:4259:docs/perf/codex-reviews/51120cb.md:215:+Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
docs/perf/codex-reviews/9eace68.md:4267:docs/perf/codex-reviews/51120cb.md:231:+| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/9eace68.md:4340:docs/perf/codex-reviews/51120cb.md:3295:docs/design/framework-magic.md:26:Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
docs/perf/codex-reviews/9eace68.md:4363:docs/perf/codex-reviews/51120cb.md:3530:    26	Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
docs/perf/codex-reviews/9eace68.md:4371:docs/perf/codex-reviews/51120cb.md:3546:    42	| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/9eace68.md:4634:docs/design/framework-magic.md:26:Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
docs/perf/codex-reviews/9eace68.md:4642:docs/design/framework-magic.md:42:| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/9eace68.md:4887:docs/perf/codex-reviews/edc17b0.md:75:+Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
docs/perf/codex-reviews/9eace68.md:4895:docs/perf/codex-reviews/edc17b0.md:91:+| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/design/framework-magic/test-scaffolding.md:9:crates/nmp-testing/tests/framework_magic_contract.rs
docs/design/framework-magic/test-scaffolding.md:15:- Renaming the file under a single milestone (e.g., `m_cross_framework_magic.rs`) would suggest one milestone is responsible for the whole contract; the opposite is true — every milestone owner adds to it.
docs/design/framework-magic/test-scaffolding.md:18:The next milestone owner must **not** rename the file to fit the milestone-prefix pattern. The framework-magic delta protocol in `intro.md` §4 covers this: a milestone landing that renames `framework_magic_contract.rs` requires an ADR.
docs/design/framework-magic/test-scaffolding.md:24:name = "framework_magic_contract"
docs/design/framework-magic/test-scaffolding.md:25:path = "tests/framework_magic_contract.rs"
docs/design/framework-magic/test-scaffolding.md:28:Invocation: `cargo test -p nmp-testing --test framework_magic_contract`.
docs/design/framework-magic/test-scaffolding.md:35:c1_replaceable_supersedes_on_insert
docs/design/framework-magic/test-scaffolding.md:47:c13_view_payload_uses_placeholders_then_refines_in_place
docs/design/framework-magic/test-scaffolding.md:48:contract_surface_complete                                  # meta-test
docs/design/framework-magic/test-scaffolding.md:58:// crates/nmp-testing/src/framework_magic.rs (proposed)
docs/design/framework-magic/test-scaffolding.md:60:pub struct ContractHarness {
docs/design/framework-magic/test-scaffolding.md:71:impl ContractHarness {
docs/design/framework-magic/test-scaffolding.md:118:fn contract_surface_complete() {
docs/design/framework-magic/test-scaffolding.md:125:        "c1_replaceable_supersedes_on_insert",
docs/design/framework-magic/test-scaffolding.md:137:        "c13_view_payload_uses_placeholders_then_refines_in_place",
docs/design/framework-magic/test-scaffolding.md:161:            "EXPECTED_TESTS lists '{}' but no #[test] fn with that name exists in framework_magic_contract.rs",
docs/design/framework-magic/test-scaffolding.md:178:A test is `#[ignore = "pending M_n"]`'d when its owning milestone has not landed the implementation. The ignore reason **must** name the milestone (e.g., `"pending M3 tombstone persistence"`) so that a `grep -n "pending M" framework_magic_contract.rs` produces a checklist for each milestone's exit gate.
docs/design/framework-magic/test-scaffolding.md:186:The M2 `PlannerHarness` (`subscription-compilation/tests.md` §9.3) is deliberately scoped to the planner subsystem: it tests the compile function in isolation, with no actor, no FFI seam, no storage backend. Several framework-magic bullets (C7 publish flow, C11 onboarding, C12 account switch, C13 in-place refinement) require the full actor and the full FFI emission path. The `ContractHarness` is therefore a strict superset.
docs/design/framework-magic/test-scaffolding.md:188:The two harnesses live alongside each other. M2-specific tests continue to use `PlannerHarness`; cross-cutting framework-magic tests use `ContractHarness`. No deduplication is forced; an assertion's logic may exist in both files (e.g., "kind:10002 arrival triggers recompile") — the planner version asserts the compile output, the contract version asserts the wire frame plus the view payload re-emit.
docs/design/framework-magic/intro.md:17:4. A test in `crates/nmp-testing/tests/framework_magic_contract.rs` that verifies the guarantee.
docs/design/framework-magic/intro.md:49:**Test:** `c_n_<snake_case>` in `crates/nmp-testing/tests/framework_magic_contract.rs`. <one sentence on what the test asserts>
docs/design/framework-magic/replaceable.md:18:**Test:** `c1_replaceable_supersedes_on_insert`. The test inserts kind:0 #1 at `created_at=T`, then kind:0 #2 at `T+1` with same pubkey; asserts `ProfileView` payload reflects #2 and that a subsequent insert at `T-1` is rejected (no payload re-emit, no event store change). Tie-break path: two inserts at the same `T` with different ids — the lexicographically-smaller-id event wins, deterministic across runs.
docs/plan/scope-adjustments-2026-05-18.md:45:The deliverable: `docs/design/framework-magic.md` (the contract) + `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per bullet). The contract evolves alongside milestones; each milestone owner adds a "framework-magic delta" section to their exit-gate report.
docs/design/framework-magic.md:6:> **Companion test file:** `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per contract bullet plus a coverage meta-test; layout in [test-scaffolding.md](framework-magic/test-scaffolding.md)).
docs/design/framework-magic.md:22:- [Test scaffolding — the `framework_magic_contract.rs` harness, naming convention, coverage meta-test](framework-magic/test-scaffolding.md)
docs/design/framework-magic.md:26:Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
docs/design/framework-magic.md:30:| C1 | Replaceable-event supersession (kind 0 / 3 / 10000–19999) on insert | replaceable.md | `c1_replaceable_supersedes_on_insert` | **[DONE]** kernel | spec §7.1 row "Replaceable kinds"; §3.3 bug #1 |
docs/design/framework-magic.md:42:| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/design/framework-magic.md:46:**Test names** (13 behavior tests + 1 coverage meta-test = 14 total, all in `crates/nmp-testing/tests/framework_magic_contract.rs` — see [test-scaffolding.md](framework-magic/test-scaffolding.md) for the harness):
docs/design/framework-magic.md:48:1. `c1_replaceable_supersedes_on_insert`
docs/design/framework-magic.md:60:13. `c13_view_payload_uses_placeholders_then_refines_in_place`
docs/design/framework-magic.md:61:14. `contract_surface_complete` — **meta-test**; asserts every behavior listed in this index has a corresponding `#[test] fn` in `framework_magic_contract.rs` (ignored or not). Drift between this doc and the test file fails the build.
docs/design/framework-magic.md:70:2. Which `#[ignore = "pending M_n"]` lines were removed from `framework_magic_contract.rs`.
docs/perf/codex-reviews/51120cb.md:22:- Framework-magic contract codified (13 behaviors, 14 tests in framework_magic_contract.rs)
docs/perf/codex-reviews/51120cb.md:95:+- **Framework-magic contract** (kind:3 auto-tracking, `bunker://` URL onboarding, new-nsec creation, outbox-by-default-on-publish, etc.): designed ([docs/design/framework-magic.md](docs/design/framework-magic.md)) with 13 behaviors and 14 named tests in `crates/nmp-testing/tests/framework_magic_contract.rs`.
docs/perf/codex-reviews/51120cb.md:195:+> **Companion test file:** `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per contract bullet plus a coverage meta-test; layout in [test-scaffolding.md](framework-magic/test-scaffolding.md)).
docs/perf/codex-reviews/51120cb.md:211:+- [Test scaffolding — the `framework_magic_contract.rs` harness, naming convention, coverage meta-test](framework-magic/test-scaffolding.md)
docs/perf/codex-reviews/51120cb.md:215:+Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
docs/perf/codex-reviews/51120cb.md:219:+| C1 | Replaceable-event supersession (kind 0 / 3 / 10000–19999) on insert | replaceable.md | `c1_replaceable_supersedes_on_insert` | **[DONE]** kernel | spec §7.1 row "Replaceable kinds"; §3.3 bug #1 |
docs/perf/codex-reviews/51120cb.md:231:+| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/51120cb.md:235:+**Test names** (13 behavior tests + 1 coverage meta-test = 14 total, all in `crates/nmp-testing/tests/framework_magic_contract.rs` — see [test-scaffolding.md](framework-magic/test-scaffolding.md) for the harness):
docs/perf/codex-reviews/51120cb.md:237:+1. `c1_replaceable_supersedes_on_insert`
docs/perf/codex-reviews/51120cb.md:249:+13. `c13_view_payload_uses_placeholders_then_refines_in_place`
docs/perf/codex-reviews/51120cb.md:250:+14. `contract_surface_complete` — **meta-test**; asserts every behavior listed in this index has a corresponding `#[test] fn` in `framework_magic_contract.rs` (ignored or not). Drift between this doc and the test file fails the build.
docs/perf/codex-reviews/51120cb.md:259:+2. Which `#[ignore = "pending M_n"]` lines were removed from `framework_magic_contract.rs`.
docs/perf/codex-reviews/51120cb.md:320:+**Test:** `c13_view_payload_uses_placeholders_then_refines_in_place`. The test:
docs/perf/codex-reviews/51120cb.md:383:+4. A test in `crates/nmp-testing/tests/framework_magic_contract.rs` that verifies the guarantee.
docs/perf/codex-reviews/51120cb.md:415:+**Test:** `c_n_<snake_case>` in `crates/nmp-testing/tests/framework_magic_contract.rs`. <one sentence on what the test asserts>
docs/perf/codex-reviews/51120cb.md:489:+**Test:** `c5_kind3_change_recompiles_follow_dependent_subs` in `crates/nmp-testing/tests/framework_magic_contract.rs`. The test:
docs/perf/codex-reviews/51120cb.md:633:+**Test:** `c1_replaceable_supersedes_on_insert`. The test inserts kind:0 #1 at `created_at=T`, then kind:0 #2 at `T+1` with same pubkey; asserts `ProfileView` payload reflects #2 and that a subsequent insert at `T-1` is rejected (no payload re-emit, no event store change). Tie-break path: two inserts at the same `T` with different ids — the lexicographically-smaller-id event wins, deterministic across runs.
docs/perf/codex-reviews/51120cb.md:999:+crates/nmp-testing/tests/framework_magic_contract.rs
docs/perf/codex-reviews/51120cb.md:1005:+- Renaming the file under a single milestone (e.g., `m_cross_framework_magic.rs`) would suggest one milestone is responsible for the whole contract; the opposite is true — every milestone owner adds to it.
docs/perf/codex-reviews/51120cb.md:1008:+The next milestone owner must **not** rename the file to fit the milestone-prefix pattern. The framework-magic delta protocol in `intro.md` §4 covers this: a milestone landing that renames `framework_magic_contract.rs` requires an ADR.
docs/perf/codex-reviews/51120cb.md:1014:+name = "framework_magic_contract"
docs/perf/codex-reviews/51120cb.md:1015:+path = "tests/framework_magic_contract.rs"
docs/perf/codex-reviews/51120cb.md:1018:+Invocation: `cargo test -p nmp-testing --test framework_magic_contract`.
docs/perf/codex-reviews/51120cb.md:1025:+c1_replaceable_supersedes_on_insert
docs/perf/codex-reviews/51120cb.md:1037:+c13_view_payload_uses_placeholders_then_refines_in_place
docs/perf/codex-reviews/51120cb.md:1038:+contract_surface_complete                                  # meta-test
docs/perf/codex-reviews/51120cb.md:1048:+// crates/nmp-testing/src/framework_magic.rs (proposed)
docs/perf/codex-reviews/51120cb.md:1050:+pub struct ContractHarness {
docs/perf/codex-reviews/51120cb.md:1061:+impl ContractHarness {
docs/perf/codex-reviews/51120cb.md:1108:+fn contract_surface_complete() {
docs/perf/codex-reviews/51120cb.md:1115:+        "c1_replaceable_supersedes_on_insert",
docs/perf/codex-reviews/51120cb.md:1127:+        "c13_view_payload_uses_placeholders_then_refines_in_place",
docs/perf/codex-reviews/51120cb.md:1151:+            "EXPECTED_TESTS lists '{}' but no #[test] fn with that name exists in framework_magic_contract.rs",
docs/perf/codex-reviews/51120cb.md:1168:+A test is `#[ignore = "pending M_n"]`'d when its owning milestone has not landed the implementation. The ignore reason **must** name the milestone (e.g., `"pending M3 tombstone persistence"`) so that a `grep -n "pending M" framework_magic_contract.rs` produces a checklist for each milestone's exit gate.
docs/perf/codex-reviews/51120cb.md:1176:+The M2 `PlannerHarness` (`subscription-compilation/tests.md` §9.3) is deliberately scoped to the planner subsystem: it tests the compile function in isolation, with no actor, no FFI seam, no storage backend. Several framework-magic bullets (C7 publish flow, C11 onboarding, C12 account switch, C13 in-place refinement) require the full actor and the full FFI emission path. The `ContractHarness` is therefore a strict superset.
docs/perf/codex-reviews/51120cb.md:1178:+The two harnesses live alongside each other. M2-specific tests continue to use `PlannerHarness`; cross-cutting framework-magic tests use `ContractHarness`. No deduplication is forced; an assertion's logic may exist in both files (e.g., "kind:10002 arrival triggers recompile") — the planner version asserts the compile output, the contract version asserts the wire frame plus the view payload re-emit.
docs/perf/codex-reviews/51120cb.md:1261:+| 2026-05-18 02:55 | 4 | Heartbeat #4 (autonomous). MAJOR: T17 framework-magic-designer landed edc17b0 (13-bullet contract + 14 named tests in framework_magic_contract.rs across 10 sub-files + index). T15 applesauce-explorer + T16 ndk-explorer both completed but couldn't write/commit themselves; orchestrator transcribed 13 files (5 applesauce + 8 ndk) and pushed as 8d633e8. KEY FINDING from NDK research that propagated into framework-magic contract + scope memo: "auto-update of open subs when follow list changes — no app code involved" is empirically FALSE in core NDK; Svelte gets it via runes, React requires explicit deps, raw core has NO follow-list watcher. NMP must build this as framework code (~200 LOC per platform) AND the framework-magic contract guarantees app dispatches zero code. T8 codex-fixer-1 also wrapped up its iterative cycle (fb139ab + 80217cc final). User landed mid-tick directive: maintain top-level README.md with TL;DR + decisions + architecture map (committed 810d0f8); heartbeat cron rewritten (cc1fba11) to fold README refresh into every tick. ~~4 lingering m11-design followup commits~~ (59f8d1b/8302cc1/fd8f173/2477372) from codex-fixer-2 cleaned by its own internal codex loop. T15/T16/T17 marked completed. Pending T13 (M2 codex fixes), T14 (M3 codex fixes) still held. Plan-splitter, m1-hardener, highlighter-explorer, codex-fixer-1, clippy-cleaner still in flight. |
docs/perf/codex-reviews/51120cb.md:3238:docs/design/framework-magic.md:70:2. Which `#[ignore = "pending M_n"]` lines were removed from `framework_magic_contract.rs`.
docs/perf/codex-reviews/51120cb.md:3249:docs/design/framework-magic/test-scaffolding.md:58:// crates/nmp-testing/src/framework_magic.rs (proposed)
docs/perf/codex-reviews/51120cb.md:3251:docs/design/framework-magic/test-scaffolding.md:178:A test is `#[ignore = "pending M_n"]`'d when its owning milestone has not landed the implementation. The ignore reason **must** name the milestone (e.g., `"pending M3 tombstone persistence"`) so that a `grep -n "pending M" framework_magic_contract.rs` produces a checklist for each milestone's exit gate.
docs/perf/codex-reviews/51120cb.md:3272:/bin/zsh -lc 'test -f crates/nmp-testing/tests/framework_magic_contract.rs; echo $?' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/51120cb.md:3277:/bin/zsh -lc 'rg -n "framework_magic_contract|Companion test file|14 named tests|Test names|Cargo.toml for `nmp-testing`|'"\\[\\[test\\]\\]\" README.md docs/design/framework-magic.md docs/design/framework-magic/test-scaffolding.md crates/nmp-testing/Cargo.toml crates/nmp-testing/tests || true" in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/51120cb.md:3281:README.md:21:- **Framework-magic contract** (kind:3 auto-tracking, `bunker://` URL onboarding, new-nsec creation, outbox-by-default-on-publish, etc.): designed ([docs/design/framework-magic.md](docs/design/framework-magic.md)) with 13 behaviors and 14 named tests in `crates/nmp-testing/tests/framework_magic_contract.rs`.
docs/perf/codex-reviews/51120cb.md:3282:docs/design/framework-magic/test-scaffolding.md:9:crates/nmp-testing/tests/framework_magic_contract.rs
docs/perf/codex-reviews/51120cb.md:3283:docs/design/framework-magic/test-scaffolding.md:18:The next milestone owner must **not** rename the file to fit the milestone-prefix pattern. The framework-magic delta protocol in `intro.md` §4 covers this: a milestone landing that renames `framework_magic_contract.rs` requires an ADR.
docs/perf/codex-reviews/51120cb.md:3286:docs/design/framework-magic/test-scaffolding.md:24:name = "framework_magic_contract"
docs/perf/codex-reviews/51120cb.md:3287:docs/design/framework-magic/test-scaffolding.md:25:path = "tests/framework_magic_contract.rs"
docs/perf/codex-reviews/51120cb.md:3288:docs/design/framework-magic/test-scaffolding.md:28:Invocation: `cargo test -p nmp-testing --test framework_magic_contract`.
docs/perf/codex-reviews/51120cb.md:3291:docs/design/framework-magic/test-scaffolding.md:161:            "EXPECTED_TESTS lists '{}' but no #[test] fn with that name exists in framework_magic_contract.rs",
docs/perf/codex-reviews/51120cb.md:3292:docs/design/framework-magic/test-scaffolding.md:178:A test is `#[ignore = "pending M_n"]`'d when its owning milestone has not landed the implementation. The ignore reason **must** name the milestone (e.g., `"pending M3 tombstone persistence"`) so that a `grep -n "pending M" framework_magic_contract.rs` produces a checklist for each milestone's exit gate.
docs/perf/codex-reviews/51120cb.md:3293:docs/design/framework-magic.md:6:> **Companion test file:** `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per contract bullet plus a coverage meta-test; layout in [test-scaffolding.md](framework-magic/test-scaffolding.md)).
docs/perf/codex-reviews/51120cb.md:3294:docs/design/framework-magic.md:22:- [Test scaffolding — the `framework_magic_contract.rs` harness, naming convention, coverage meta-test](framework-magic/test-scaffolding.md)
docs/perf/codex-reviews/51120cb.md:3295:docs/design/framework-magic.md:26:Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
docs/perf/codex-reviews/51120cb.md:3296:docs/design/framework-magic.md:46:**Test names** (13 behavior tests + 1 coverage meta-test = 14 total, all in `crates/nmp-testing/tests/framework_magic_contract.rs` — see [test-scaffolding.md](framework-magic/test-scaffolding.md) for the harness):
docs/perf/codex-reviews/51120cb.md:3297:docs/design/framework-magic.md:61:14. `contract_surface_complete` — **meta-test**; asserts every behavior listed in this index has a corresponding `#[test] fn` in `framework_magic_contract.rs` (ignored or not). Drift between this doc and the test file fails the build.
docs/perf/codex-reviews/51120cb.md:3298:docs/design/framework-magic.md:70:2. Which `#[ignore = "pending M_n"]` lines were removed from `framework_magic_contract.rs`.
docs/perf/codex-reviews/51120cb.md:3395:/bin/zsh -lc "rg -n \"nmp-testing|framework_magic_contract|\\[\\[test\\]\\]|ContractHarness\" crates Cargo.toml docs/design/framework-magic/test-scaffolding.md" in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/51120cb.md:3398:docs/design/framework-magic/test-scaffolding.md:9:crates/nmp-testing/tests/framework_magic_contract.rs
docs/perf/codex-reviews/51120cb.md:3400:docs/design/framework-magic/test-scaffolding.md:18:The next milestone owner must **not** rename the file to fit the milestone-prefix pattern. The framework-magic delta protocol in `intro.md` §4 covers this: a milestone landing that renames `framework_magic_contract.rs` requires an ADR.
docs/perf/codex-reviews/51120cb.md:3403:docs/design/framework-magic/test-scaffolding.md:24:name = "framework_magic_contract"
docs/perf/codex-reviews/51120cb.md:3404:docs/design/framework-magic/test-scaffolding.md:25:path = "tests/framework_magic_contract.rs"
docs/perf/codex-reviews/51120cb.md:3405:docs/design/framework-magic/test-scaffolding.md:28:Invocation: `cargo test -p nmp-testing --test framework_magic_contract`.
docs/perf/codex-reviews/51120cb.md:3406:docs/design/framework-magic/test-scaffolding.md:58:// crates/nmp-testing/src/framework_magic.rs (proposed)
docs/perf/codex-reviews/51120cb.md:3407:docs/design/framework-magic/test-scaffolding.md:60:pub struct ContractHarness {
docs/perf/codex-reviews/51120cb.md:3408:docs/design/framework-magic/test-scaffolding.md:71:impl ContractHarness {
docs/perf/codex-reviews/51120cb.md:3411:docs/design/framework-magic/test-scaffolding.md:161:            "EXPECTED_TESTS lists '{}' but no #[test] fn with that name exists in framework_magic_contract.rs",
docs/perf/codex-reviews/51120cb.md:3412:docs/design/framework-magic/test-scaffolding.md:178:A test is `#[ignore = "pending M_n"]`'d when its owning milestone has not landed the implementation. The ignore reason **must** name the milestone (e.g., `"pending M3 tombstone persistence"`) so that a `grep -n "pending M" framework_magic_contract.rs` produces a checklist for each milestone's exit gate.
docs/perf/codex-reviews/51120cb.md:3413:docs/design/framework-magic/test-scaffolding.md:186:The M2 `PlannerHarness` (`subscription-compilation/tests.md` §9.3) is deliberately scoped to the planner subsystem: it tests the compile function in isolation, with no actor, no FFI seam, no storage backend. Several framework-magic bullets (C7 publish flow, C11 onboarding, C12 account switch, C13 in-place refinement) require the full actor and the full FFI emission path. The `ContractHarness` is therefore a strict superset.
docs/perf/codex-reviews/51120cb.md:3414:docs/design/framework-magic/test-scaffolding.md:188:The two harnesses live alongside each other. M2-specific tests continue to use `PlannerHarness`; cross-cutting framework-magic tests use `ContractHarness`. No deduplication is forced; an assertion's logic may exist in both files (e.g., "kind:10002 arrival triggers recompile") — the planner version asserts the compile output, the contract version asserts the wire frame plus the view payload re-emit.
docs/perf/codex-reviews/51120cb.md:3441:    21	- **Framework-magic contract** (kind:3 auto-tracking, `bunker://` URL onboarding, new-nsec creation, outbox-by-default-on-publish, etc.): designed ([docs/design/framework-magic.md](docs/design/framework-magic.md)) with 13 behaviors and 14 named tests in `crates/nmp-testing/tests/framework_magic_contract.rs`.
docs/perf/codex-reviews/51120cb.md:3510:     6	> **Companion test file:** `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per contract bullet plus a coverage meta-test; layout in [test-scaffolding.md](framework-magic/test-scaffolding.md)).
docs/perf/codex-reviews/51120cb.md:3526:    22	- [Test scaffolding — the `framework_magic_contract.rs` harness, naming convention, coverage meta-test](framework-magic/test-scaffolding.md)
docs/perf/codex-reviews/51120cb.md:3530:    26	Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
docs/perf/codex-reviews/51120cb.md:3534:    30	| C1 | Replaceable-event supersession (kind 0 / 3 / 10000–19999) on insert | replaceable.md | `c1_replaceable_supersedes_on_insert` | **[DONE]** kernel | spec §7.1 row "Replaceable kinds"; §3.3 bug #1 |
docs/perf/codex-reviews/51120cb.md:3546:    42	| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/51120cb.md:3550:    46	**Test names** (13 behavior tests + 1 coverage meta-test = 14 total, all in `crates/nmp-testing/tests/framework_magic_contract.rs` — see [test-scaffolding.md](framework-magic/test-scaffolding.md) for the harness):
docs/perf/codex-reviews/51120cb.md:3552:    48	1. `c1_replaceable_supersedes_on_insert`
docs/perf/codex-reviews/51120cb.md:3564:    60	13. `c13_view_payload_uses_placeholders_then_refines_in_place`
docs/perf/codex-reviews/51120cb.md:3565:    61	14. `contract_surface_complete` — **meta-test**; asserts every behavior listed in this index has a corresponding `#[test] fn` in `framework_magic_contract.rs` (ignored or not). Drift between this doc and the test file fails the build.
docs/perf/codex-reviews/51120cb.md:3574:    70	2. Which `#[ignore = "pending M_n"]` lines were removed from `framework_magic_contract.rs`.
docs/perf/codex-reviews/51120cb.md:3610:    17	4. A test in `crates/nmp-testing/tests/framework_magic_contract.rs` that verifies the guarantee.
docs/perf/codex-reviews/51120cb.md:3993:    45	The deliverable: `docs/design/framework-magic.md` (the contract) + `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per bullet). The contract evolves alongside milestones; each milestone owner adds a "framework-magic delta" section to their exit-gate report.
docs/perf/codex-reviews/51120cb.md:4536:    18	**Test:** `c1_replaceable_supersedes_on_insert`. The test inserts kind:0 #1 at `created_at=T`, then kind:0 #2 at `T+1` with same pubkey; asserts `ProfileView` payload reflects #2 and that a subsequent insert at `T-1` is rejected (no payload re-emit, no event store change). Tie-break path: two inserts at the same `T` with different ids — the lexicographically-smaller-id event wins, deterministic across runs.
docs/perf/codex-reviews/51120cb.md:4656:    31	**Test:** `c13_view_payload_uses_placeholders_then_refines_in_place`. The test:
docs/perf/codex-reviews/51120cb.md:5117:README.md:21:- **Framework-magic contract** (kind:3 auto-tracking, `bunker://` URL onboarding, new-nsec creation, outbox-by-default-on-publish, etc.): designed ([docs/design/framework-magic.md](docs/design/framework-magic.md)) with 13 behaviors and 14 named tests in `crates/nmp-testing/tests/framework_magic_contract.rs`.
docs/perf/codex-reviews/51120cb.md:5194:/bin/zsh -lc 'rg -n "lmdb-schema|framework_magic_contract|TBD-from-research|pending agent landing|Test checked in|verified in `crates|current in-memory store enforces|research-fold commit" docs/design/framework-magic docs/design/framework-magic.md README.md' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/51120cb.md:5611:- [README.md](/Users/pablofernandez/Work/nostr-multi-platform/README.md:21), [docs/design/framework-magic.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/framework-magic.md:6): Claims 14 contract tests exist in `crates/nmp-testing/tests/framework_magic_contract.rs`, but that file and even `crates/nmp-testing/tests/` do not exist, and `crates/nmp-testing/Cargo.toml` has no `[[test]]` entry. Fix: add the skeletal ignored test file + Cargo entry, or downgrade all “checked in/test suite is proof” wording to “planned”.
docs/perf/codex-reviews/51120cb.md:5634:- [README.md](/Users/pablofernandez/Work/nostr-multi-platform/README.md:21), [docs/design/framework-magic.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/framework-magic.md:6): Claims 14 contract tests exist in `crates/nmp-testing/tests/framework_magic_contract.rs`, but that file and even `crates/nmp-testing/tests/` do not exist, and `crates/nmp-testing/Cargo.toml` has no `[[test]]` entry. Fix: add the skeletal ignored test file + Cargo entry, or downgrade all “checked in/test suite is proof” wording to “planned”.
docs/perf/codex-reviews/edc17b0.md:55:+> **Companion test file:** `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per contract bullet plus a coverage meta-test; layout in [test-scaffolding.md](framework-magic/test-scaffolding.md)).
docs/perf/codex-reviews/edc17b0.md:71:+- [Test scaffolding — the `framework_magic_contract.rs` harness, naming convention, coverage meta-test](framework-magic/test-scaffolding.md)
docs/perf/codex-reviews/edc17b0.md:75:+Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
docs/perf/codex-reviews/edc17b0.md:79:+| C1 | Replaceable-event supersession (kind 0 / 3 / 10000–19999) on insert | replaceable.md | `c1_replaceable_supersedes_on_insert` | **[DONE]** kernel | spec §7.1 row "Replaceable kinds"; §3.3 bug #1 |
docs/perf/codex-reviews/edc17b0.md:91:+| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/edc17b0.md:95:+**Test names** (13 behavior tests + 1 coverage meta-test = 14 total, all in `crates/nmp-testing/tests/framework_magic_contract.rs` — see [test-scaffolding.md](framework-magic/test-scaffolding.md) for the harness):
docs/perf/codex-reviews/edc17b0.md:97:+1. `c1_replaceable_supersedes_on_insert`
docs/perf/codex-reviews/edc17b0.md:109:+13. `c13_view_payload_uses_placeholders_then_refines_in_place`
docs/perf/codex-reviews/edc17b0.md:110:+14. `contract_surface_complete` — **meta-test**; asserts every behavior listed in this index has a corresponding `#[test] fn` in `framework_magic_contract.rs` (ignored or not). Drift between this doc and the test file fails the build.
docs/perf/codex-reviews/edc17b0.md:119:+2. Which `#[ignore = "pending M_n"]` lines were removed from `framework_magic_contract.rs`.
docs/perf/codex-reviews/edc17b0.md:180:+**Test:** `c13_view_payload_uses_placeholders_then_refines_in_place`. The test:
docs/perf/codex-reviews/edc17b0.md:243:+4. A test in `crates/nmp-testing/tests/framework_magic_contract.rs` that verifies the guarantee.
docs/perf/codex-reviews/edc17b0.md:275:+**Test:** `c_n_<snake_case>` in `crates/nmp-testing/tests/framework_magic_contract.rs`. <one sentence on what the test asserts>
docs/perf/codex-reviews/edc17b0.md:349:+**Test:** `c5_kind3_change_recompiles_follow_dependent_subs` in `crates/nmp-testing/tests/framework_magic_contract.rs`. The test:
docs/perf/codex-reviews/edc17b0.md:493:+**Test:** `c1_replaceable_supersedes_on_insert`. The test inserts kind:0 #1 at `created_at=T`, then kind:0 #2 at `T+1` with same pubkey; asserts `ProfileView` payload reflects #2 and that a subsequent insert at `T-1` is rejected (no payload re-emit, no event store change). Tie-break path: two inserts at the same `T` with different ids — the lexicographically-smaller-id event wins, deterministic across runs.
docs/perf/codex-reviews/edc17b0.md:859:+crates/nmp-testing/tests/framework_magic_contract.rs
docs/perf/codex-reviews/edc17b0.md:865:+- Renaming the file under a single milestone (e.g., `m_cross_framework_magic.rs`) would suggest one milestone is responsible for the whole contract; the opposite is true — every milestone owner adds to it.
docs/perf/codex-reviews/edc17b0.md:868:+The next milestone owner must **not** rename the file to fit the milestone-prefix pattern. The framework-magic delta protocol in `intro.md` §4 covers this: a milestone landing that renames `framework_magic_contract.rs` requires an ADR.
docs/perf/codex-reviews/edc17b0.md:874:+name = "framework_magic_contract"
docs/perf/codex-reviews/edc17b0.md:875:+path = "tests/framework_magic_contract.rs"
docs/perf/codex-reviews/edc17b0.md:878:+Invocation: `cargo test -p nmp-testing --test framework_magic_contract`.
docs/perf/codex-reviews/edc17b0.md:885:+c1_replaceable_supersedes_on_insert
docs/perf/codex-reviews/edc17b0.md:897:+c13_view_payload_uses_placeholders_then_refines_in_place
docs/perf/codex-reviews/edc17b0.md:898:+contract_surface_complete                                  # meta-test
docs/perf/codex-reviews/edc17b0.md:908:+// crates/nmp-testing/src/framework_magic.rs (proposed)
docs/perf/codex-reviews/edc17b0.md:910:+pub struct ContractHarness {
docs/perf/codex-reviews/edc17b0.md:921:+impl ContractHarness {
docs/perf/codex-reviews/edc17b0.md:968:+fn contract_surface_complete() {
docs/perf/codex-reviews/edc17b0.md:975:+        "c1_replaceable_supersedes_on_insert",
docs/perf/codex-reviews/edc17b0.md:987:+        "c13_view_payload_uses_placeholders_then_refines_in_place",
docs/perf/codex-reviews/edc17b0.md:1011:+            "EXPECTED_TESTS lists '{}' but no #[test] fn with that name exists in framework_magic_contract.rs",
docs/perf/codex-reviews/edc17b0.md:1028:+A test is `#[ignore = "pending M_n"]`'d when its owning milestone has not landed the implementation. The ignore reason **must** name the milestone (e.g., `"pending M3 tombstone persistence"`) so that a `grep -n "pending M" framework_magic_contract.rs` produces a checklist for each milestone's exit gate.
docs/perf/codex-reviews/edc17b0.md:1036:+The M2 `PlannerHarness` (`subscription-compilation/tests.md` §9.3) is deliberately scoped to the planner subsystem: it tests the compile function in isolation, with no actor, no FFI seam, no storage backend. Several framework-magic bullets (C7 publish flow, C11 onboarding, C12 account switch, C13 in-place refinement) require the full actor and the full FFI emission path. The `ContractHarness` is therefore a strict superset.
docs/perf/codex-reviews/edc17b0.md:1038:+The two harnesses live alongside each other. M2-specific tests continue to use `PlannerHarness`; cross-cutting framework-magic tests use `ContractHarness`. No deduplication is forced; an assertion's logic may exist in both files (e.g., "kind:10002 arrival triggers recompile") — the planner version asserts the compile output, the contract version asserts the wire frame plus the view payload re-emit.
docs/perf/codex-reviews/edc17b0.md:1100:docs/design/framework-magic.md:30:| C1 | Replaceable-event supersession (kind 0 / 3 / 10000–19999) on insert | replaceable.md | `c1_replaceable_supersedes_on_insert` | **[DONE]** kernel | spec §7.1 row "Replaceable kinds"; §3.3 bug #1 |
docs/perf/codex-reviews/edc17b0.md:1112:docs/design/framework-magic.md:42:| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/edc17b0.md:1115:docs/design/framework-magic.md:70:2. Which `#[ignore = "pending M_n"]` lines were removed from `framework_magic_contract.rs`.
docs/perf/codex-reviews/edc17b0.md:1167:docs/design/framework-magic/replaceable.md:18:**Test:** `c1_replaceable_supersedes_on_insert`. The test inserts kind:0 #1 at `created_at=T`, then kind:0 #2 at `T+1` with same pubkey; asserts `ProfileView` payload reflects #2 and that a subsequent insert at `T-1` is rejected (no payload re-emit, no event store change). Tie-break path: two inserts at the same `T` with different ids — the lexicographically-smaller-id event wins, deterministic across runs.
docs/perf/codex-reviews/edc17b0.md:1184:docs/design/framework-magic/test-scaffolding.md:178:A test is `#[ignore = "pending M_n"]`'d when its owning milestone has not landed the implementation. The ignore reason **must** name the milestone (e.g., `"pending M3 tombstone persistence"`) so that a `grep -n "pending M" framework_magic_contract.rs` produces a checklist for each milestone's exit gate.
docs/perf/codex-reviews/f68a479.md:324:docs/design/framework-magic.md:42:| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/f68a479.md:1389:docs/perf/codex-reviews/9eace68.md:401:docs/design/framework-magic.md:42:| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/f68a479.md:2120:docs/perf/codex-reviews/51120cb.md:231:+| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/perf/codex-reviews/f68a479.md:2164:docs/perf/codex-reviews/51120cb.md:3546:    42	| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
docs/design/framework-magic/capabilities.md:31:**Test:** `c13_view_payload_uses_placeholders_then_refines_in_place`. The test:

exec
/bin/zsh -lc "nl -ba docs/design/framework-magic/test-scaffolding.md | sed -n '1,220p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
exec
/bin/zsh -lc "nl -ba docs/design/framework-magic.md | sed -n '1,95p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
     1	# Design: Framework Magic Contract — Things That Just Work
     2	
     3	> **Status:** Draft (initial structure). Research-fold commit fills `TBD-from-research(...)` markers from `docs/research/applesauce/event-store-query-builders.md` and `docs/research/ndk/kind3-auto-tracking.md` when they land.
     4	> **Date:** 2026-05-18.
     5	> **Source directives:** `docs/plan/scope-adjustments-2026-05-18.md` "Framework magic contract" section; `docs/aim.md` §6 doctrines 1–12; `docs/product-spec/overview-and-dx.md` §1.5 (cardinal doctrines D0–D5) + §3.3 (bug-class extinction); `docs/product-spec/subsystems.md` §7.1–§7.8.
     6	> **Companion test file:** `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per contract bullet plus a coverage meta-test; layout in [test-scaffolding.md](framework-magic/test-scaffolding.md)).
     7	> **Scope:** Enumerate every behavior the framework guarantees so the application does not have to author code for it. The user directive is explicit: *"apps shouldn't have to care or know about these operations happening in the background, things should just work."* This document is the contract; the test suite is the proof; the milestone implementations are the substrate.
     8	
     9	This document is split into focused sub-files to stay under the 300 LOC ceiling (`AGENTS.md`) and to let milestone owners revise one chapter at a time without merge contention.
    10	
    11	## Section map
    12	
    13	- [Intro — purpose, doctrine alignment, per-bullet template, how this contract evolves](framework-magic/intro.md)
    14	- [Kind:3 auto-tracking — follow-list change recompiles dependent subscriptions](framework-magic/kind3.md) (C5)
    15	- [Replaceable & delete invariants — supersession, parameterized supersession, kind:5, NIP-40](framework-magic/replaceable.md) (C1–C4)
    16	- [Outbox routing — read fan-out, write fan-out, private events fail closed](framework-magic/outbox.md) (C6, C7)
    17	- [Subscriptions — dedup, coalesce, auto-close, buffered batches](framework-magic/subs.md) (C8)
    18	- [Sync & provenance — watermarks, NIP-77 backfill, redelivery merge](framework-magic/sync.md) (C9, C10)
    19	- [Signers & onboarding — bunker://, nsec creation, Keychain persistence](framework-magic/signers.md) (C11)
    20	- [Sessions — account switch = state, view rebuild without imperative dance](framework-magic/sessions.md) (C12)
    21	- [Capabilities & rendering — best-effort placeholders, in-place refinement](framework-magic/capabilities.md) (C13)
    22	- [Test scaffolding — the `framework_magic_contract.rs` harness, naming convention, coverage meta-test](framework-magic/test-scaffolding.md)
    23	
    24	## The 13 contract bullets
    25	
    26	Each row binds a behavior to: the sub-file that specifies it, the test name in `crates/nmp-testing/tests/framework_magic_contract.rs`, the milestone that owns the implementation, and the doctrine clause it discharges.
    27	
    28	| # | Behavior | Sub-file | Test name | Milestone | Doctrine / spec |
    29	|---|---|---|---|---|---|
    30	| C1 | Replaceable-event supersession (kind 0 / 3 / 10000–19999) on insert | replaceable.md | `c1_replaceable_supersedes_on_insert` | **[DONE]** kernel | spec §7.1 row "Replaceable kinds"; §3.3 bug #1 |
    31	| C2 | Parameterized replaceable supersession (30000–39999) by `(pubkey, kind, d-tag)` | replaceable.md | `c2_parameterized_replaceable_supersedes_by_dtag` | **[PENDING M3]** | spec §7.1 row "Parameterized replaceable"; §3.3 bug #1 |
    32	| C3 | Kind:5 delete propagation: referenced events removed, tombstone persisted | replaceable.md | `c3_kind5_delete_removes_referenced_and_tombstones` | **[PENDING M3]** | spec §7.1 row "Kind 5 (delete)" |
    33	| C4 | NIP-40 expiration auto-removes event at expiry; survives actor restart | replaceable.md | `c4_nip40_expiration_removes_and_persists_schedule` | **[PENDING M3]** | spec §7.1 row "NIP-40 expiration" |
    34	| C5 | Kind:3 auto-tracking: active account's follow-list change recompiles dependent subscriptions transparently | kind3.md | `c5_kind3_change_recompiles_follow_dependent_subs` | **[PENDING M2]** | scope-adj §"Folded into M2"; D3; M2 design §4 (Trigger::Nip65Arrived analog for kind:3) |
    35	| C6 | Outbox read routing: `authors`-filter subscriptions fan out to those authors' write relays (NIP-65), de-duplicated | outbox.md | `c6_authors_subscription_routes_to_per_author_write_relays` | **[PENDING M2]** | D3; spec §7.3 row "Subscription with `authors`"; M2 design §7 |
    36	| C7 | Outbox write routing: publishes go to author write + `#p`-recipient inbox; private (gift-wrap) events fail closed when recipient inbox is unknown | outbox.md | `c7_publish_routes_outbox_and_private_fails_closed` | **[PENDING M2 seam → M6 publish]** | D3; spec §7.3 rows "Publish*"; §3.3 bugs #3, #4 |
    37	| C8 | Subscription planner deduplicates overlapping interests into one wire REQ per relay, auto-closes on EOSE / last-consumer-drop, and buffers ingress to ≤60Hz per view | subs.md | `c8_subscriptions_coalesce_autoclose_and_buffer` | **[PENDING M2]** | spec §7.2; §3.3 bug #2, bug #8 |
    38	| C9 | Provenance preserved: same event id arriving from N relays merges into one stored event with N-entry provenance set; original `id` and signature untouched | sync.md | `c9_provenance_merges_across_relay_redeliveries` | **[PENDING M3]** | aim §6 doctrine 10; spec §7.1 row "Provenance"; §3.3 bug #10 |
    39	| C10 | Sync watermarks: planner consults `(filter, relay)` coverage before issuing historical REQ; full coverage makes cache-miss authoritative; NIP-77 negentropy is the default backfill where supported | sync.md | `c10_watermark_gates_backfill_and_authoritative_miss` | **[PENDING M4]** | D2; spec §7.1 watermarks, §7.8 sync engine |
    40	| C11 | Signer onboarding: pasted `bunker://` URL parses + connects via NIP-46; "create new nsec" generates, NIP-49-encrypts, and persists via KeyringCapability — both as kernel actions, no app code | signers.md | `c11_bunker_url_and_nsec_creation_complete_via_actions` | **[PENDING M6]** | scope-adj §"Folded into M6"; spec §7.4 |
    41	| C12 | Account switch is a state transition: dispatching the switch action re-resolves every `ActiveAccount`-scoped view without the app issuing CLOSE/REQ or rebuilding view handles | sessions.md | `c12_account_switch_rebinds_views_without_imperative_dance` | **[PENDING M8]** | D4; spec §7.4; §3.3 bug #5; M2 §4 trigger A4 |
    42	| C13 | Best-effort rendering: every view payload field is non-`Option`; missing data uses defined placeholders (shortened npub, identicon, "just now"); the same payload updates in place when authoritative data arrives | capabilities.md | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[DONE]** in placeholder shape; **[PENDING M2/M3]** for in-place refinement on enrich | D1; spec §7.6 "Best-effort field contract"; aim §4.12 |
    43	
    44	**Bullet count:** 13 (eleven sourced verbatim from `scope-adjustments-2026-05-18.md`; two — **C3** kind:5 delete propagation and **C4** NIP-40 expiration — derived from `product-spec/subsystems.md` §7.1 because they are guaranteed invariants of the same insert path and the contract is incomplete without them).
    45	
    46	**Test names** (13 behavior tests + 1 coverage meta-test = 14 total, all in `crates/nmp-testing/tests/framework_magic_contract.rs` — see [test-scaffolding.md](framework-magic/test-scaffolding.md) for the harness):
    47	
    48	1. `c1_replaceable_supersedes_on_insert`
    49	2. `c2_parameterized_replaceable_supersedes_by_dtag`
    50	3. `c3_kind5_delete_removes_referenced_and_tombstones`
    51	4. `c4_nip40_expiration_removes_and_persists_schedule`
    52	5. `c5_kind3_change_recompiles_follow_dependent_subs`
    53	6. `c6_authors_subscription_routes_to_per_author_write_relays`
    54	7. `c7_publish_routes_outbox_and_private_fails_closed`
    55	8. `c8_subscriptions_coalesce_autoclose_and_buffer`
    56	9. `c9_provenance_merges_across_relay_redeliveries`
    57	10. `c10_watermark_gates_backfill_and_authoritative_miss`
    58	11. `c11_bunker_url_and_nsec_creation_complete_via_actions`
    59	12. `c12_account_switch_rebinds_views_without_imperative_dance`
    60	13. `c13_view_payload_uses_placeholders_then_refines_in_place`
    61	14. `contract_surface_complete` — **meta-test**; asserts every behavior listed in this index has a corresponding `#[test] fn` in `framework_magic_contract.rs` (ignored or not). Drift between this doc and the test file fails the build.
    62	
    63	Tests for behaviors whose owning milestone has not landed are checked in with `#[ignore = "pending M_n"]`; the meta-test still counts them. This is the "doc says 13, code tests 11" regression the file-naming convention break (cross-cutting, not milestone-prefixed) is designed to support — see [test-scaffolding.md](framework-magic/test-scaffolding.md) §1.
    64	
    65	## How this contract evolves
    66	
    67	Every milestone owner adds a **"framework-magic delta"** subsection to their exit-gate report. The delta names:
    68	
    69	1. Which contract bullets transitioned from `[PENDING M_n]` to `[DONE]`.
    70	2. Which `#[ignore = "pending M_n"]` lines were removed from `framework_magic_contract.rs`.
    71	3. Whether the contract gained or lost a bullet during the milestone (rare; bullet count changes need an ADR).
    72	
    73	The orchestrator's heartbeat triage rule includes a "framework-magic regression" gate: any milestone landing that *removes* a bullet without an ADR fails review.
    74	
    75	## Open items resolved by the research-fold commit
    76	
    77	The following are `TBD-from-research(...)` markers in the sub-files; the research-fold commit replaces them with file:line refs and concrete API shapes. They are listed here so the orchestrator can sequence the work:
    78	
    79	- `kind3.md` §3 — `TBD-from-research(ndk/kind3-auto-tracking.md)`: NDK's exact mechanism for kind:3 → open-subscription recompile (event listener path, refcount handoff, race window).
    80	- `kind3.md` §4 — `TBD-from-research(applesauce/event-store-query-builders.md)`: Applesauce's query-builder pattern that makes `WhoFollows(active_user)` reactive without app code.
    81	- `outbox.md` §2 — `TBD-from-research(ndk/kind3-auto-tracking.md)`: how NDK rebinds in-flight REQs when an author's mailbox arrives mid-subscription.
    82	- `subs.md` §3 — `TBD-from-research(applesauce/event-store-query-builders.md)`: Applesauce's logical-vs-wire subscription split file:line refs (for cross-validation against `docs/design/subscription-compilation/intro.md` §2).
    83	- `sync.md` §4 — `TBD-from-research(applesauce/event-store-query-builders.md)`: Applesauce's coverage/watermark equivalent and how a query-builder reads it.
    84	
    85	None of the above blocks the *initial* structure of this contract; they refine evidence and citations in the research-fold commit. The framework-magic contract's bullets, tests, and milestone bindings are stable now.
    86	
    87	## Non-goals
    88	
    89	- This document does **not** specify HOW the framework implements each behavior — that lives in the milestone design doc named in the table.
    90	- This document does **not** duplicate `docs/product-spec/subsystems.md` §7.1 invariants — `replaceable.md` references the rows; it does not restate them.
    91	- This document does **not** introduce new types or traits — `PublishPlanner`, `ViewModule`, `LogicalInterest`, `SubscriptionCompiler`, `MailboxCache`, `KeyringCapability` are already defined in the cited design docs and product spec. The contract uses those names.
    92	- This document does **not** describe the proof app, the starter app, or the kernel-substrate trait families — those are the substrate the contract holds the framework to.
    93	
    94	The contract's job is exactly: *enumerate what the app does not have to do, name where the framework does it, name the test that proves it.* Nothing more.

 succeeded in 0ms:
     1	# Framework Magic — Test Scaffolding
     2	
     3	> Parent: `docs/design/framework-magic.md`.
     4	> Read first: `docs/design/subscription-compilation/tests.md` §9.3 (the `PlannerHarness` this scaffolding extends); `docs/design/lmdb/tests.md` (storage-layer test patterns); `docs/product-spec/subsystems.md` §7.13 (`nmp-testing` surface).
     5	
     6	## 1. File location and naming convention break
     7	
     8	```
     9	crates/nmp-testing/tests/framework_magic_contract.rs
    10	```
    11	
    12	The existing convention is **milestone-prefixed** (`m2_subscription_compilation_audit.rs`, `m3_lmdb_invariants.rs`, etc.). This file is intentionally **cross-cutting** — it is the *only* test file in `crates/nmp-testing/tests/` that is not milestone-prefixed. The convention break is deliberate:
    13	
    14	- The contract spans M2 + M3 + M4 + M6 + M8 + reactivity-bench; no single milestone owns it.
    15	- Renaming the file under a single milestone (e.g., `m_cross_framework_magic.rs`) would suggest one milestone is responsible for the whole contract; the opposite is true — every milestone owner adds to it.
    16	- The file is the *index test* — the meta-test (§4 below) reads `docs/design/framework-magic.md`'s row table and asserts every row has a `#[test] fn` with the expected name. A renaming under a milestone prefix would obscure this role.
    17	
    18	The next milestone owner must **not** rename the file to fit the milestone-prefix pattern. The framework-magic delta protocol in `intro.md` §4 covers this: a milestone landing that renames `framework_magic_contract.rs` requires an ADR.
    19	
    20	`Cargo.toml` for `nmp-testing` adds the standard `[[test]]` block:
    21	
    22	```toml
    23	[[test]]
    24	name = "framework_magic_contract"
    25	path = "tests/framework_magic_contract.rs"
    26	```
    27	
    28	Invocation: `cargo test -p nmp-testing --test framework_magic_contract`.
    29	
    30	## 2. Test names — the canonical 14
    31	
    32	Thirteen behavior tests (C1–C13; the table in `framework-magic.md` shows the exact names) plus the coverage meta-test, total 14 `#[test] fn` declarations:
    33	
    34	```
    35	c1_replaceable_supersedes_on_insert
    36	c2_parameterized_replaceable_supersedes_by_dtag
    37	c3_kind5_delete_removes_referenced_and_tombstones
    38	c4_nip40_expiration_removes_and_persists_schedule
    39	c5_kind3_change_recompiles_follow_dependent_subs
    40	c6_authors_subscription_routes_to_per_author_write_relays
    41	c7_publish_routes_outbox_and_private_fails_closed
    42	c8_subscriptions_coalesce_autoclose_and_buffer
    43	c9_provenance_merges_across_relay_redeliveries
    44	c10_watermark_gates_backfill_and_authoritative_miss
    45	c11_bunker_url_and_nsec_creation_complete_via_actions
    46	c12_account_switch_rebinds_views_without_imperative_dance
    47	c13_view_payload_uses_placeholders_then_refines_in_place
    48	contract_surface_complete                                  # meta-test
    49	```
    50	
    51	Test names are **stable identifiers**. Renaming any of them constitutes a contract revision per `intro.md` §4 and requires the deprecation marker (`#[test] fn old_name() { c_n_new_name() }` for at least one milestone cycle).
    52	
    53	## 3. The harness
    54	
    55	The harness is the union of three existing testing surfaces, exposed as one builder:
    56	
    57	```rust
    58	// crates/nmp-testing/src/framework_magic.rs (proposed)
    59	
    60	pub struct ContractHarness {
    61	    actor:            TestActor,                  // wraps the real actor with a recorded reconciler
    62	    planner:          PlannerHarness,             // from subscription-compilation/tests.md §9.3
    63	    clock:            SimulatedClock,             // from subsystems.md §7.13
    64	    network_chaos:    NetworkChaos,               // from subsystems.md §7.13
    65	    mock_relays:      Vec<MockRelay>,             // from nostr-relay-builder
    66	    keyring:          InMemoryKeyringCapability,  // for C11
    67	    audit:            WireFrameAuditLog,          // proposed; captures every CLOSE/REQ/EVENT frame
    68	    reconciler_log:   Vec<AppUpdate>,             // every AppUpdate emitted across the FFI seam
    69	}
    70	
    71	impl ContractHarness {
    72	    pub fn new() -> Self;
    73	    pub fn with_mock_relays(self, count: u8) -> Self;
    74	    pub fn with_nip77_capable_relays(self, capable: &[bool]) -> Self;
    75	    pub fn with_seeded_accounts(self, accounts: &[(Pubkey, SignerKind)]) -> Self;
    76	    pub fn with_active_account(self, pubkey: Pubkey) -> Self;
    77	    pub fn with_seeded_mailboxes(self, entries: &[(Pubkey, MailboxList)]) -> Self;
    78	    pub fn with_seeded_follows(self, account: Pubkey, follows: &[Pubkey]) -> Self;
    79	    pub fn build(self) -> Contract;
    80	}
    81	
    82	pub struct Contract {
    83	    // dispatch surface
    84	    pub fn dispatch(&mut self, action: AppAction);
    85	    pub fn open_view<V: ViewModule>(&mut self, spec: V::Spec) -> ViewHandle<V>;
    86	    pub fn close_view<V: ViewModule>(&mut self, handle: ViewHandle<V>);
    87	    pub fn ingest(&mut self, relay: usize, event: NostrEvent);
    88	    pub fn ingest_eose(&mut self, relay: usize, sub_id: &str);
    89	    pub fn disconnect_relay(&mut self, relay: usize);
    90	    pub fn reconnect_relay(&mut self, relay: usize);
    91	    pub fn advance_clock_ms(&mut self, ms: u64);
    92	    pub fn simulate_actor_restart(&mut self);
    93	
    94	    // assertion surface
    95	    pub fn wire_frames(&self, relay: usize) -> &[WireFrame];
    96	    pub fn reconciler_log(&self) -> &[AppUpdate];
    97	    pub fn event_store_get(&self, id: &EventId) -> Option<&StoredEvent>;
    98	    pub fn provenance_of(&self, id: &EventId) -> &Provenance;
    99	    pub fn watermark_of(&self, filter_sig: &FilterSig, relay: usize) -> Option<&Watermark>;
   100	    pub fn action_ledger(&self) -> &[ActionLedgerRow];
   101	    pub fn keyring_entries(&self) -> &[KeyringEntry];
   102	    pub fn session_state(&self) -> &SessionState;
   103	}
   104	```
   105	
   106	The harness extends `PlannerHarness` rather than wrapping it: every assertion the M2 audit gate makes against `PlannerHarness::compile_audit_log()` is accessible through the contract harness via `Contract::wire_frames(relay)`, but the contract harness also drives the full actor (so action ledger transitions, projection cache updates, and reconciler emissions are observable).
   107	
   108	`InMemoryKeyringCapability` is a new `nmp-testing` primitive for C11. It implements the `KeyringCapability` trait with a `HashMap<String, Vec<u8>>` backing store; the test inspects the stored bytes to verify NIP-49 encryption envelope shape.
   109	
   110	`WireFrameAuditLog` is a new `nmp-testing` primitive that captures every outbound frame the relay-worker emits. The M2 design has an audit log on the planner side; this harness has it on the wire side — both must agree, and a separate harness invariant could later assert that agreement.
   111	
   112	The harness does **not** include a real network — every relay is a `MockRelay`. Every contract test runs in deterministic time with no I/O. Total runtime budget for the full suite: <5 seconds.
   113	
   114	## 4. The coverage meta-test
   115	
   116	```rust
   117	#[test]
   118	fn contract_surface_complete() {
   119	    // 1. Read docs/design/framework-magic.md and parse the contract table.
   120	    let contract = parse_contract_table(include_str!("../../../docs/design/framework-magic.md"));
   121	
   122	    // 2. Enumerate the #[test] fns in this binary via inventory or a const list.
   123	    //    The const list is the canonical surface; inventory is the consistency check.
   124	    const EXPECTED_TESTS: &[&str] = &[
   125	        "c1_replaceable_supersedes_on_insert",
   126	        "c2_parameterized_replaceable_supersedes_by_dtag",
   127	        "c3_kind5_delete_removes_referenced_and_tombstones",
   128	        "c4_nip40_expiration_removes_and_persists_schedule",
   129	        "c5_kind3_change_recompiles_follow_dependent_subs",
   130	        "c6_authors_subscription_routes_to_per_author_write_relays",
   131	        "c7_publish_routes_outbox_and_private_fails_closed",
   132	        "c8_subscriptions_coalesce_autoclose_and_buffer",
   133	        "c9_provenance_merges_across_relay_redeliveries",
   134	        "c10_watermark_gates_backfill_and_authoritative_miss",
   135	        "c11_bunker_url_and_nsec_creation_complete_via_actions",
   136	        "c12_account_switch_rebinds_views_without_imperative_dance",
   137	        "c13_view_payload_uses_placeholders_then_refines_in_place",
   138	    ];
   139	
   140	    // 3. Assert every row in the contract table has a matching expected test name.
   141	    for row in &contract.rows {
   142	        assert!(
   143	            EXPECTED_TESTS.contains(&row.test_name.as_str()),
   144	            "contract row {} has test name '{}' which is not in EXPECTED_TESTS — \
   145	             update either the doc table or EXPECTED_TESTS so they agree",
   146	            row.id, row.test_name,
   147	        );
   148	    }
   149	
   150	    // 4. Assert no expected test name is missing from the contract table.
   151	    for expected in EXPECTED_TESTS {
   152	        let found = contract.rows.iter().any(|r| r.test_name == *expected);
   153	        assert!(found, "EXPECTED_TESTS lists '{}' which is not in the contract doc table", expected);
   154	    }
   155	
   156	    // 5. Assert every EXPECTED_TESTS entry is actually a #[test] fn in this binary.
   157	    //    Compile-time check via inventory crate or a build script that scans the file.
   158	    for expected in EXPECTED_TESTS {
   159	        assert!(
   160	            test_exists_in_binary(expected),
   161	            "EXPECTED_TESTS lists '{}' but no #[test] fn with that name exists in framework_magic_contract.rs",
   162	            expected,
   163	        );
   164	    }
   165	}
   166	```
   167	
   168	The meta-test is **not** `#[ignore]`. It runs on every CI run. It catches three classes of drift:
   169	
   170	1. The doc table grows a row but the test file doesn't grow a `#[test] fn` — caught by step 4.
   171	2. The test file grows a `#[test] fn` but the doc table doesn't list it — caught by step 3.
   172	3. A renamed test breaks the doc-test correspondence — caught by either step 3 or 4 depending on which side renamed first.
   173	
   174	The meta-test does **not** check `#[ignore]` status. A test for a pending milestone is correctly `#[ignore]`'d; the meta-test's job is structural correspondence, not implementation readiness. The milestone delta protocol (`intro.md` §4) handles the un-ignore cadence.
   175	
   176	## 5. `#[ignore]` discipline
   177	
   178	A test is `#[ignore = "pending M_n"]`'d when its owning milestone has not landed the implementation. The ignore reason **must** name the milestone (e.g., `"pending M3 tombstone persistence"`) so that a `grep -n "pending M" framework_magic_contract.rs` produces a checklist for each milestone's exit gate.
   179	
   180	The framework-magic delta in a milestone's exit-gate report enumerates which `pending M_n` ignore lines were removed during the milestone. Removing an ignore line without the delta entry fails the post-merge codex review.
   181	
   182	CI runs `cargo test --include-ignored` on a nightly schedule (not blocking) to catch the inverse drift: a `#[ignore]`'d test that has secretly started passing because the implementation landed without the milestone owner noticing.
   183	
   184	## 6. Why this harness, not the existing planner harness
   185	
   186	The M2 `PlannerHarness` (`subscription-compilation/tests.md` §9.3) is deliberately scoped to the planner subsystem: it tests the compile function in isolation, with no actor, no FFI seam, no storage backend. Several framework-magic bullets (C7 publish flow, C11 onboarding, C12 account switch, C13 in-place refinement) require the full actor and the full FFI emission path. The `ContractHarness` is therefore a strict superset.
   187	
   188	The two harnesses live alongside each other. M2-specific tests continue to use `PlannerHarness`; cross-cutting framework-magic tests use `ContractHarness`. No deduplication is forced; an assertion's logic may exist in both files (e.g., "kind:10002 arrival triggers recompile") — the planner version asserts the compile output, the contract version asserts the wire frame plus the view payload re-emit.
   189	
   190	## 7. Reverse-cross-reference: which milestone touches which test?
   191	
   192	| Milestone | Tests that flip from `#[ignore]` to active |
   193	|---|---|
   194	| M2 | C5, C6, C8 (all sub-paths); C7 sub-paths 3 + 4 (planner-only); C13 sub-paths 2 + 3 + 4 (projection cache) |
   195	| M3 | C2, C3, C4 (LMDB + tombstones + persistence); C9 (provenance schema + cap) |
   196	| M4 | C10 (full sync engine) |
   197	| M5 | (no contract bullets directly; auth-paused relays are an internal mechanism) |
   198	| M6 | C7 sub-paths 1 + 2 + 5 (SendNote consumer); C11 (signers + onboarding actions) |
   199	| M8 | C12 (multi-account state machine) |
   200	
   201	Total: 13 behavior tests + 1 meta-test = 14 `#[test] fn` declarations across six milestone exit-gate transitions. The framework-magic delta at each milestone removes a known subset of `#[ignore]` lines; the contract document's "Milestone owner" column is the canonical source for which.
   202	
   203	## 8. What this scaffolding does not specify
   204	
   205	- **The harness implementation.** The skeleton above is the API; the implementation is the next agent's deliverable (a `framework-magic-harness` task, or the M2 milestone implementation owner folding it in).
   206	- **The reverse mapping from `AppAction` variants to action-ledger rows.** That's `kernel-substrate.md` §4 territory; the harness exposes `action_ledger()` and the test reads rows by index/id.
   207	- **Per-platform binding tests.** Cross-platform consistency (`subsystems.md` §3.5) is a separate test suite that runs the same scripted actions on iOS / Android / Desktop / Web and diffs `AppState` JSON. The framework-magic contract is Rust-only; platform-binding regressions show up in the cross-platform suite.
   208	- **Negative tests for the API surface.** "The app cannot type `SendNote { content, relays: vec![...] }`" is a *compile-fail* test, owned by `docs/design/subscription-compilation/tests.md` §9.2 assertion 1. The framework-magic surface assertion is "no test passes the broken usage"; the structural inability is asserted there.

exec
/bin/zsh -lc "rg --files crates | rg 'nmp-testing|framework_magic|framework-magic|tests'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
crates/nmp-testing/src/lib.rs
crates/nmp-testing/bin/reactivity-bench/scenario.rs
crates/nmp-testing/bin/reactivity-bench/main.rs
crates/nmp-testing/bin/reactivity-bench/allocator.rs
crates/nmp-testing/bin/reactivity-bench/report.rs
crates/nmp-testing/bin/reactivity-bench/world.rs
crates/nmp-testing/bin/reactivity-bench/domain.rs
crates/nmp-testing/bin/reactivity-bench/config.rs
crates/nmp-testing/bin/reactivity-bench/rng.rs
crates/nmp-testing/bin/firehose-bench/main.rs
crates/nmp-testing/bin/firehose-bench/report.rs
crates/nmp-testing/bin/firehose-bench/scenarios.rs
crates/nmp-testing/bin/firehose-bench/config.rs
crates/nmp-testing/Cargo.toml
crates/nmp-codegen/tests/determinism.rs
crates/nmp-core/src/kernel/tests.rs
crates/nmp-core/tests/substrate_registry.rs

codex
The missing test file is a concrete blocker for the “test suite is the proof” claim. I’m also checking the D0 boundary now, because several examples use domain-shaped view/action names and I want to distinguish illustrative extension-module examples from claims that `nmp-core` owns app nouns.
exec
/bin/zsh -lc "nl -ba docs/design/framework-magic/capabilities.md | sed -n '1,90p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
     1	# Framework Magic §C13 — Best-Effort Rendering
     2	
     3	> Parent: `docs/design/framework-magic.md`.
     4	> Read first: `docs/product-spec/overview-and-dx.md` §1.5 doctrine D1; `docs/product-spec/subsystems.md` §7.6 (the per-field placeholder table + the `TimelineItem` concrete example); `docs/aim.md` §4.12; `docs/design/view-catalog/profile-timeline-thread-reactions.md`.
     5	
     6	The "capabilities" filename in the user's directive maps here, not to capability bridges. The rendering contract is the **rendering capability** the framework grants the app: render now, refine in place, never withhold cached data behind a spinner. (Capability bridges in the technical sense — `KeyringCapability` etc. — are covered as plumbing in C11 and `kernel-substrate.md` §5; they are not themselves a contract bullet because they are infrastructure, not an observable app guarantee.)
     7	
     8	## C13. Best-effort rendering: placeholders by construction; in-place refinement
     9	
    10	**Statement.** Every display-bearing field of every view payload is **non-`Option`** and carries either an authoritative value or a defined placeholder. When the authoritative value later arrives — a kind:0 for an author, kind:9735 zap receipts for a note, the decrypted body for a DM — the same payload re-emits with the field updated in place. The platform's reactive primitive (`@Observable` / `Flow` / signals) sees the change and only the affected cell re-renders. **No spinner ever gates an already-rendered cell, and no view module ever exposes a `loading: bool` to the platform.**
    11	
    12	**Framework does:**
    13	
    14	- The placeholder contract at `docs/product-spec/subsystems.md` §7.6 lines 181–192 (the seven-row table: display name → npub-shortened, picture → identicon URI, NIP-05 → empty string, timestamp → "just now", reaction count → 0, zap total → 0 sats, content body → empty string).
    15	- The view-payload typing at `subsystems.md` §7.6 lines 199–222 (the `TimelineItem` example with all fields non-`Option` except the optional `repost_of` / `quote_of` semantic-Option markers).
    16	- The freshness surface at `subsystems.md` §7.6 line 196 (`xxx_freshness: FreshnessHint` is an optional **sibling** field; UI may render a badge; the framework never withholds the value).
    17	- The in-place refinement mechanism: `ViewModule::on_projection_changed` (`docs/design/kernel-substrate.md` §3 lines 148–150). When a kind:0 lands for author X, the kernel's projection cache (a shared cross-view projection) updates X's display name; every view module that lists items by X re-runs `on_projection_changed`, produces a delta, and the wire-emitter sends a `ViewBatch` with the updated field.
    18	- The platform-shadow domain key (`kernel-substrate.md` §3 line 128 `fn key(spec: &Self::Spec) -> Self::Key`) ensures the cell-level re-render is targeted: the platform's reactive primitive updates only the row whose key matches, not the entire list.
    19	
    20	**App writes:** nothing. The app renders payload fields directly — `Text(item.author_display)`, `AsyncImage(url: item.author_picture)`. There is no `if has_profile { ... } else { Spinner() }` pattern because the API does not expose `has_profile`; the framework guarantees `author_display` and `author_picture` are always non-empty strings.
    21	
    22	**Failure mode prevented:** the entire class of "Nostr-client cold-start UI" bugs `subsystems.md` §1.5 D1 enumerates as ruled out by construction:
    23	
    24	- Hiding a post because the author's profile hasn't loaded yet.
    25	- Replacing cached profile metadata with a spinner because "we might have something newer."
    26	- Refusing to render threads because the root event isn't in cache.
    27	- Profile-picture flicker between cached and placeholder.
    28	
    29	The bug-extinction surface in `overview-and-dx.md` §3.3 does not have a single numbered bug for this because the failures are UX defects rather than data-corruption bugs, but the doctrine clause D1 is the explicit promise the contract holds.
    30	
    31	**Test:** `c13_view_payload_uses_placeholders_then_refines_in_place`. The test:
    32	
    33	1. **Placeholders at open:** open `TimelineView { authors: [alice], kinds: [1] }` against a fresh store with no kind:0 for Alice. Insert a kind:1 event by Alice. Assert the payload's `items[0]`:
    34	   - `author_display` matches the expected npub-shortened form for `alice_pk` (compare against `Pubkey::shortened()` output — deterministic).
    35	   - `author_picture` matches the expected identicon URI for `alice_pk` (deterministic from pubkey hash).
    36	   - `author_nip05_domain` is the empty string.
    37	   - `created_at_display` is "just now" (test uses `SimulatedClock` set to the event's `created_at`).
    38	   - `reaction_summary` has 0 reactions; `zap_sats_total` is 0; `reply_count` is 0.
    39	   - The payload contains **no** `loading`, `is_loaded`, `has_profile`, or `freshness_gate` field.
    40	2. **In-place refinement on kind:0:** insert a kind:0 for Alice with `name = "Alice"`, `picture = "https://example/alice.jpg"`, `nip05 = "alice@example.com"`. Assert the same view emits a `ViewBatch` (not a `FullState`); the `items[0]` payload now has `author_display = "Alice"`, `author_picture = "https://example/alice.jpg"`, `author_nip05_domain = "example.com"`. Assert the `id` field of `items[0]` is unchanged (same event row; the row updated, did not re-create).
    41	3. **In-place refinement on time:** advance the `SimulatedClock` by 5 minutes; trigger the per-tick re-format (per `kernel-substrate.md` §3 `fn on_tick` line 153). Assert `items[0].created_at_display` updates from "just now" to "5 min ago" without the row being torn down.
    42	4. **In-place refinement on reaction arrival:** insert a kind:7 reaction targeting the kind:1 event. Assert `items[0].reaction_summary` updates from 0 to 1 in the next `ViewBatch`; no row re-creation; `id` stable.
    43	5. **Freshness hint, not gate:** insert an older cached kind:0 for Alice (created two days ago), then a fresher one (created an hour ago). Assert the payload reflects the *fresher* one (per C1 supersession), and that the optional `author_display_freshness` field (if exposed by the view module) reads `Recent`, not `DaysOld`. Assert there is no API surface where the test can ask "is this stale?" and have the framework withhold the value pending re-fetch.
    44	
    45	**Milestone owner:** **[DONE]** for the placeholder shape (the M1 timeline slice already ships non-`Option` author fields with shortened-npub fallback — verified in `crates/nmp-core` timeline tests today). **[PENDING M2/M3]** for the full in-place refinement guarantees: sub-paths 1 and 5 are testable today; sub-paths 2 and 4 require the kernel's projection cache (`kernel-substrate.md` §3 line 148 `on_projection_changed`) which graduates in M2 alongside the view-module surface; sub-path 3 requires the per-tick re-format hook (`fn on_tick`, M2's `ViewModule` trait work).
    46	
    47	Test checked in **not** ignored for sub-paths 1 and 5; sub-paths 2/3/4 use a `#[cfg(feature = "m2_projection_cache")]` gate so they activate as M2 lands without a re-edit. The framework-magic delta at M2 exit removes the gate.
    48	
    49	## Why this is one bullet, not several
    50	
    51	The five sub-paths are five facets of one observable: *the payload field is always renderable, and updates appear without the row being destroyed.* Splitting them would suggest the platform might see a `ViewBatch` for kind:0 arrival but `FullState` for reaction arrival, or that some fields are non-`Option` and others are. The contract is uniform; the test enumerates the field categories that exercise it.
    52	
    53	## Doctrine alignment
    54	
    55	C13 is the canonical instance of cardinal doctrine **D1**. The doctrine clause's wording — *"There is no `if has_profile { render } else { spinner }` pattern available in the API"* — is testable through the payload shape itself, which is what sub-path 1's "no `loading` field" assertion checks. The framework cannot guarantee the app does not implement its own spinner over the payload, but it can guarantee the API does not give the app a way to ask the question that would justify one.
    56	
    57	C13 also intersects D4 (single writer per fact; caches derive). The "fact" is the projection (Alice's display name); the "caches" are every timeline cell, profile chip, thread author marker rendering that name. The in-place refinement is the derivation.
    58	
    59	## Cross-references
    60	
    61	- `docs/design/view-catalog/profile-timeline-thread-reactions.md` — the concrete view-module catalog with each view's payload shape.
    62	- `docs/design/reactivity/view-deltas-and-projections.md` — the projection cache that backs the cross-view refinement.
    63	- `docs/design/kernel-substrate.md` §3 — `ViewModule` trait including `on_projection_changed`, `on_tick`.
    64	- `docs/product-spec/subsystems.md` §7.6 — the placeholder table and the `TimelineItem` example.
    65	
    66	## What this chapter does not cover
    67	
    68	- **Per-view payload byte budgets.** `subsystems.md` §7.16 owns those. The contract guarantees the rendering shape; the budget is a perf concern.
    69	- **Cross-platform pixel-parity.** `subsystems.md` §3.5 owns the cross-platform consistency tests. C13 asserts the payload values are correct; the platforms agree to render the same payload identically.
    70	- **Long-form content parsing nodes.** `subsystems.md` §7.6 "Post-v1 content rendering contract" — explicitly post-v1; the v1 contract is summary-shaped payloads.
    71	- **DM body decryption inside the view payload.** The decrypted body fits the same C13 pattern (placeholder = empty string; in-place refinement when decrypt succeeds), but the decryption path itself is M9 territory and is not v1.

exec
/bin/zsh -lc 'rg -n "D0|no app nouns|app nouns|nmp-core stays|TimelineView|ProfileView|FollowingTimelineView|AccountListView|SendNote|SendDm|React|BunkerConnect|CreateLocalIdentity|built-ins|nmp-core" docs/aim.md docs/product-spec docs/design docs/plan crates/nmp-core' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
docs/aim.md:101:### 4.1 Reactive single source of truth ("EventStore")
docs/aim.md:113:### 4.2 Reactive models / derived views
docs/aim.md:242:3. **Reactive cross-FFI subscription protocol.** UniFFI gives callback interfaces, not native reactive streams. Swift wants `@Observable`, Kotlin wants `Flow`, JS wants Observables/Promises. Define a single `Subscription` opaque handle + reconciler-style callback that adapts cleanly per platform.
docs/plan/m0-fixture.md:9:**Subsystem deliverables.** `nmp-core::substrate`, `nmp-codegen`, `fixture-todo-core`, `nmp-testing` harness skeletons.
docs/plan/m0-fixture.md:11:**Exit gate.** ✅ Fixture compiles and runs; codegen determinism test passes; substrate registry test passes (`crates/nmp-core/tests/substrate_registry.rs`).
docs/product-spec/cli-toolchain-phasing.md:84:| §7.2 Where views live | Materialized lazily in `nmp-core`; surfaced as snapshots in `AppState.views` and as `ViewBatch` deltas. Opt-in opaque handles deferred. | §6.6 |
docs/product-spec/cli-toolchain-phasing.md:86:| §7.4 NIP-46 bunker as capability | Internal to `nmp-core`; not exposed as a `CallbackInterface`. Pairing flow surfaces as `Effect::BunkerPairingReady` for native rendering of QR/URI. | §6.4, §7.4 |
docs/product-spec/cli-toolchain-phasing.md:114:| 0. Foundations | Workspace, `nmp-core` kernel skeleton, `nmp-codegen` skeleton, empty per-app generated crate, headless test harness | Actor starts/stops; `nmp gen modules --check` deterministic; `cargo test --workspace` green |
docs/design/view-catalog.md:6:- [View Catalog: Profile, Timeline, Thread, Reactions](view-catalog/profile-timeline-thread-reactions.md)
docs/design/view-catalog.md:13:- Sections 3-6: [Profile, Timeline, Thread, and Reactions](view-catalog/profile-timeline-thread-reactions.md)
docs/plan/m14-uniffi.md:7:**Scope.** Replace the current raw C FFI surface in `crates/nmp-core/src/ffi.rs` with the per-app generated `nmp-app-<name>` crate per ADR-0010. The iOS app stops importing `NmpCore.h` and instead imports the generated Swift module.
docs/product-spec/subsystems.md:90:- "Publish leaked to wrong relays" → ruled out by the safe API. The developer cannot supply a relay list to `SendNote`. Explicit overrides are named, one-shot, and debug-flagged in logs.
docs/product-spec/subsystems.md:127:Signers are managed entirely in `nmp-core`. The initial product signer catalog is:
docs/product-spec/subsystems.md:135:The signer abstraction inside `nmp-core` is a Rust trait with `sign(unsigned_event) -> Future<signed_event>`. Adding a signer kind is an internal task; external developers do not implement signers.
docs/product-spec/subsystems.md:171:| Reactions | `event_coord` | grouped count by emoji + per-pubkey list |
docs/product-spec/subsystems.md:189:| Reaction count | 0 |
docs/product-spec/subsystems.md:201:pub struct TimelineView {
docs/product-spec/subsystems.md:216:    pub reaction_summary: ReactionSummary,
docs/plan/m10.5-ffi-hardening.md:25:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Hard zero — no deferral escape, no "tracking issue" carve-out. Every pre-existing one is resolved in M10.5. If something genuinely cannot be done in M10.5 because it belongs to a later milestone (e.g. NIP-65 outbox work), then it is not a TODO/FIXME in the scoped files — it lives as a milestone task in `docs/plan.md`, not as a code marker.
docs/plan/m10.5-ffi-hardening.md:42:- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
docs/design/view-catalog/template-and-enumeration.md:9:> **Status:** Rev 2, reframed per ADR-0009. These view kinds are not in `nmp-core`; apps consume them by adding the owning module crate to `nmp.toml` and regenerating the per-app FFI crate.
docs/design/view-catalog/template-and-enumeration.md:17:Every reference Nostr view module lives in a `nmp-nip*` crate and implements `ViewModule` from `nmp-core::substrate`:
docs/design/view-catalog/template-and-enumeration.md:74:| Reactions | target event coord | `useReactions(target)` |
docs/design/view-catalog/template-and-enumeration.md:96:| 9 | Reactions | `nmp-nip25` | yes | 1a.6 |
docs/product-spec/appendices.md:31:| **Reactive shared SQLite.** Rust writes; both sides hold read handles; reactive query libraries (GRDB / SQLDelight / Drift) re-run queries on table writes. | 1Password (Op core), Linear, Notion mobile, most local-first apps | Surrenders doctrine. Platforms now write queries, which is display-shaping logic. Pre-formatting (timestamps, npubs, sats) either moves into native (D-violation) or materializes as columns at write time (extra schema). Web fragments — wasm SQLite doesn't share with JS the way native does. Cross-platform consistency tests get harder (per-platform query results vs byte-diffable JSON). |
docs/product-spec/appendices.md:69:| 25 | Reactions | §6.3, §7.6 |
docs/design/view-catalog/stubs-validation-next.md:39:Reactive balance + pending transactions for the active wallet. Payload: `WalletBalanceView { sats, pending: Vec<PendingTx>, last_synced_at_ms }`. Backed by the wallet subsystem rather than the event store directly. Phase 6.
docs/design/view-catalog/stubs-validation-next.md:67:4. **Reactions aggregation.** 1 reactions view; 10k kind:7 events arrive in 30 seconds. Measure: `EmojiAdjusted` delta count vs ideal coalesced count. Gate: deltas/sec ≤ 60.
docs/design/view-catalog/stubs-validation-next.md:82:3. Implement view kinds 1–9 marked "Phase 1" in §2 of this doc (Profile, Contacts, Mailboxes, Timeline, Thread, Replies, Reactions, Search; Conversation/list deferred to Phase 5).
docs/plan/test-pyramid.md:11:| Reactivity bench | `reactivity-bench --standard --fail-on-gate` | Composite reverse index, delta coalescing, working-set memory, allocation gates | `crates/nmp-testing/bin/reactivity-bench/` |
docs/product-spec/api-surface.md:9:The concrete FFI API is per-app generated. `nmp-core` defines kernel primitives and extension traits; `nmp gen modules` composes the selected kernel, protocol modules, and app modules into a generated `nmp-app-<name>` crate that exposes closed typed enums to Swift/Kotlin/TypeScript.
docs/product-spec/api-surface.md:78:**Platform shadow is reorganized by domain key, not `ViewId` (ADR-0005).** While the FFI delivers `AppState.views` as a `HashMap<ViewId, ViewPayload>`, the per-platform wrapper layer (generated by `nmp gen`) reorganizes the shadow into typed domain-keyed dictionaries — `profiles: [PubKey: ProfileView]`, `reactionSummaries: [EventId: ReactionSummary]`, `conversations: [PubKey: ConversationView]`, etc. — so components read by domain concept (pubkey, event id) rather than by framework handle. `ViewId` remains an internal token used by the FFI; component code never sees it. Refcounted wrappers (`useProfile`, `@Profile`, `rememberProfile`) manage subscription lifecycle behind the domain-keyed API. See ADR-0005 for the per-view-kind cache-key table.
docs/product-spec/api-surface.md:82:`AppAction` is a generated per-app `uniffi::Enum`, not a closed enum in `nmp-core`. The generated enum composes kernel variants, selected Nostr protocol module variants, and app-specific module variants:
docs/product-spec/api-surface.md:93:The long-term action catalog below is illustrative. Each item belongs in the relevant module crate, not in `nmp-core`.
docs/product-spec/api-surface.md:122:    SendNote { content: String, mentions: Vec<String>, reply_to: Option<EventCoord> },
docs/product-spec/api-surface.md:123:    React { target: EventCoord, emoji: String },
docs/product-spec/api-surface.md:131:    SendDm { recipient: String, body: String, attachments: Vec<BlobRef> },
docs/product-spec/api-surface.md:235:**Views are opened via `dispatch(OpenView)` with a platform-generated `ViewId`, and updates arrive as `ViewBatch` entries keyed by that id.** Materialization is lazy in `nmp-core` — view payloads live in the actor and are projected into `ViewSnapshots`/`ViewBatch` on every change.
docs/design/view-catalog/profile-timeline-thread-reactions.md:1:# View Catalog: Profile, Timeline, Thread, Reactions
docs/design/view-catalog/profile-timeline-thread-reactions.md:17:| 9 | Reactions | yes (§6) | 1 |
docs/design/view-catalog/profile-timeline-thread-reactions.md:46:pub struct ProfileView {
docs/design/view-catalog/profile-timeline-thread-reactions.md:68:    Replaced { payload: ProfileView },
docs/design/view-catalog/profile-timeline-thread-reactions.md:132:pub struct TimelineView {
docs/design/view-catalog/profile-timeline-thread-reactions.md:225:| `reaction_summary` | `ReactionSummary::default()` (all zeros) |
docs/design/view-catalog/profile-timeline-thread-reactions.md:322:On reaction (kind:7) targeting any node: don't add to tree; update `nodes[target].item.reaction_summary` and emit `NodeUpdated`. Reactions on the same target are batched via projection cache (`reaction_summary` projection in `Projections`).
docs/design/view-catalog/profile-timeline-thread-reactions.md:343:## 6. View: Reactions
docs/design/view-catalog/profile-timeline-thread-reactions.md:350:pub struct ReactionsSpec {
docs/design/view-catalog/profile-timeline-thread-reactions.md:360:pub struct ReactionsView {
docs/design/view-catalog/profile-timeline-thread-reactions.md:365:    pub reactors: Vec<ReactorEntry>,      // empty if !include_pubkey_list
docs/design/view-catalog/profile-timeline-thread-reactions.md:375:pub struct ReactorEntry {
docs/design/view-catalog/profile-timeline-thread-reactions.md:386:pub enum ReactionsDelta {
docs/design/view-catalog/profile-timeline-thread-reactions.md:388:    MyReactionsChanged { reactions: Vec<String> },
docs/design/view-catalog/profile-timeline-thread-reactions.md:389:    ReactorAdded { entry: ReactorEntry },
docs/design/view-catalog/profile-timeline-thread-reactions.md:390:    ReactorRemoved { pubkey: String, emoji: String },
docs/design/view-catalog/profile-timeline-thread-reactions.md:408:- On a new kind:7 event referring to the target: increment count for the emoji; if `include_pubkey_list` and the reactor isn't already present, emit `ReactorAdded`.
docs/design/view-catalog/profile-timeline-thread-reactions.md:409:- On a kind:5 delete of a kind:7 event: decrement count; emit `ReactorRemoved`.
docs/design/view-catalog/profile-timeline-thread-reactions.md:410:- On the active account publishing a kind:7: include in `my_reactions`; emit `MyReactionsChanged`.
docs/design/view-catalog/profile-timeline-thread-reactions.md:419:- Reactor with no kind:0 → `author_display` is shortened npub.
docs/design/view-catalog/profile-timeline-thread-reactions.md:424:- **Reactions by deleted accounts.** If the reactor publishes kind:5 deleting their own reaction, decrement and remove. If a third party publishes kind:5 attempting to delete someone else's reaction, the store ignores the delete (per kind:5 spec, only self-deletes are honored).
docs/design/view-catalog/profile-timeline-thread-reactions.md:425:- **Reaction spam.** Aggregate by `(pubkey, emoji)`: a single pubkey reacting with the same emoji 50 times counts as 1. The projection cache enforces this.
docs/plan/m11.5-highlighter.md:30:- **`nmp-core` gains zero highlighter or group nouns.** Verified by grep + review.
docs/plan/m11.5-highlighter.md:32:- **Reactivity, GC, diagnostics** behave identically to the Twitter slice and podcast app.
docs/design/view-catalog/conversation-and-cross-cutting.md:28:    pub peer_display: ProfileView,        // for direct; placeholder for group
docs/design/view-catalog/conversation-and-cross-cutting.md:46:    pub reactions: ReactionSummary,
docs/product-spec/overview-and-dx.md:17:A Cargo workspace shipping a Nostr-native **app kernel** (`nmp-core`), reusable **Nostr protocol modules** (`nmp-nip01`, `nmp-nip17`, `nmp-nip65`, etc.), app-owned extension modules, a codegen tool (`nmp gen modules`) that produces per-app concrete FFI enums/wrappers, FFI bindings for Swift/Kotlin/TypeScript, a wasm target, a scaffolding CLI, and reference platform shells.
docs/product-spec/overview-and-dx.md:21:The kernel does **not** own Profile, Timeline, Thread, Reactions, Conversation, Wallet, DM, Blossom, or app-specific domain concepts. Those live in reusable protocol modules or app crates. Platform code renders state and dispatches user intents — nothing else.
docs/product-spec/overview-and-dx.md:31:### D0. Kernel + extension modules — no app nouns in `nmp-core`
docs/product-spec/overview-and-dx.md:33:Per ADR-0009, NMP is a Nostr-native app kernel plus extension modules. The kernel provides substrate; protocol modules and app modules contribute typed variants via `ViewModule`, `ActionModule`, `DomainModule`, `CapabilityModule`, and `IdentityModule`. If implementing a real app requires adding domain nouns to `nmp-core`, the kernel boundary is wrong and must change.
docs/product-spec/overview-and-dx.md:37:- `nmp-core` becoming a junk drawer of every consumer's domain concepts.
docs/product-spec/overview-and-dx.md:180:| `nmp-core` | Kernel substrate: actor, store, planner, ledger, registries, extension traits, diagnostics | Pure Rust |
docs/product-spec/overview-and-dx.md:184:| `nmp-nip01` | Event, Filter, Profile/Timeline views, SendNote/Delete actions | Pure Rust |
docs/product-spec/overview-and-dx.md:187:| `nmp-nip17` | Conversation view and SendDm action | Pure Rust |
docs/product-spec/overview-and-dx.md:188:| `nmp-nip25` | Reactions view and React action | Pure Rust |
docs/plan/scope-adjustments-2026-05-18.md:43:- Reactive recompute on event arrival (already done; validated by reactivity-bench)
docs/plan/scope-adjustments-2026-05-18.md:74:M7    Reactions + Thread + Reply                                      — pending
docs/design/podcast/podcast-core.md:21:nmp-core = { path = "../../../crates/nmp-core" }
docs/design/podcast/podcast-core.md:35:No tokio. The actor's runtime is owned by `nmp-core`; action modules return state machines, not async tasks (see ADR-0009).
docs/design/podcast/podcast-core.md:234:UUIDv7 would also work; ulid chosen to match existing `nmp-core` convention.
docs/design/podcast/podcast-core.md:264:Reactivity follows ADR-0001 (composite keys, broad-axis guardrails), ADR-0002 (≤60 Hz/view, audio-tick views capped at 4 Hz), ADR-0003 (hot-set working budget; only currently-open views materialize payloads).
docs/design/podcast/podcast-core.md:392:Per ADR-0001, projection cache changes feed `ProjectionChange` events into `ViewModule::on_projection_changed`. Each is registered as a `ProjectionCache` in `nmp-core::substrate::projections` (kernel-owned trait; modules opt in).
docs/design/podcast/podcast-core.md:407:kernel = "nmp-core"
docs/plan/m11-podcast.md:71:- **`nmp-core` gains zero podcast nouns.** No `Podcast`, `Episode`, `Transcript`, `Chapter`, `Player`, `Feed`, `Insight`, `Guest` types added to the kernel. Verified by grep + manual review at the commit.
docs/plan/m11-podcast.md:73:- **Reactivity behavior is identical** to the Twitter slice — composite-key dependencies, delta coalescing, claim-based GC, ADR-0007 diagnostics all work for podcast view modules.
docs/plan/m11-podcast.md:74:- **No app-state leaks across the boundary in either direction:** no Nostr type appears in `podcast-core`'s public surface; no podcast type appears in `nmp-core`'s public surface.
docs/design/podcast/podcast-llm.md:30:nmp-core = { path = "../../../crates/nmp-core" }
docs/plan/principles.md:8:4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
docs/plan/principles.md:9:5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the [M11](m11-podcast.md) podcast-app proof.
docs/plan/status.md:9:- **Kernel substrate** in `crates/nmp-core` (~3,800 LOC): actor on a dedicated OS thread, mailbox-driven (ADR feedback adopted — relay reads happen in tokio reader tasks, the actor blocks on its own channel with deadline timeouts), substrate trait families (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`) in `nmp-core/src/substrate/`, ingest pipeline (`kernel/ingest.rs`), claim/release refcounting for profile interest (commit `23ae829`), composite reverse-index dependency tracking.
crates/nmp-core/Cargo.toml:2:name = "nmp-core"
docs/plan/m6-signers-write.md:16:- Action ledger in `nmp-core::kernel::ledger`: durable rows with ULID action IDs, status transitions, retry/cancel, restart recovery.
docs/plan/m6-signers-write.md:17:- Action atomicity contract: a `SendNote` action's publish to relays and local store insert happen as one actor message; partial failure rolls back.
docs/plan/m6-signers-write.md:18:- `nmp-nip01::SendNoteActionModule` as the first write-path action.
docs/plan/m9-messaging.md:11:- `nmp-nip17` protocol module: Conversation view module + ConversationList view module; SendDm action module; NIP-44 encryption / NIP-59 gift-wrapping; outbox routing for DMs (recipient inbox relays only — never public).
docs/plan/m9-messaging.md:14:- Action atomicity for `SendDm`: gift-wrap → publish to all recipient inboxes → insert locally — atomic.
docs/plan/m15-cross-platform.md:5:**Demo product:** Same Twitter slice and (where capabilities allow) podcast slice running on Android (Compose), Desktop (iced), and Web (wasm + React/Solid TBD). Cross-platform consistency test passes — same scripted scenario produces byte-identical `AppState` JSON on all four platforms.
docs/plan/m15-cross-platform.md:28:- Web shell stack TBD (React + signals / Solid / Svelte — pick at start of milestone).
docs/design/podcast/lessons.md:106:- "No business logic in native." (Doctrine D0 + AGENTS.md guardrails.)
docs/plan/subsystem-matrix.md:11:| **Reactivity as planned** | [M0](m0-fixture.md)–[M7](m7-interaction-loop.md) | Already validated by reactivity-bench run 002 against the model; M1 runs the same code path against real iOS; subsequent milestones add view modules that exercise the contract under varied loads. |
docs/plan/decision-log.md:15:- **ADR-0009**: App-extension kernel boundary. Five trait families, four layers, no app nouns in nmp-core.
docs/design/podcast/wiring.md:216:- [ ] No `nmp-core` patches in this PR (kernel must stay app-agnostic)
docs/plan/m7-interaction-loop.md:1:# M7 — Reactions + Thread + Reply (the interaction loop)
docs/plan/m7-interaction-loop.md:7:**Scope.** `nmp-nip25` (Reactions view module + React action), `nmp-nip10` (Thread view module with NIP-10 reply-marker handling), `SendNote` extended for `reply_to`.
docs/plan/m7-interaction-loop.md:11:- Reactions view module with NIP-25 emoji normalization (`+` and missing content → "like"; deduplicate by `(pubkey, emoji)`).
docs/plan/m7-interaction-loop.md:12:- React action module on the action ledger.
docs/plan/m7-interaction-loop.md:19:- Reactions aggregation: 10k reactions over 30 s coalesce to ≤ 60 deltas/sec/view per ADR-0002.
docs/design/reactivity.md:1:# Design: Reactivity
docs/design/reactivity.md:5:- [Reactivity: Loop And Reverse Index](reactivity/loop-and-reverse-index.md)
docs/design/reactivity.md:6:- [Reactivity: View Deltas And Projections](reactivity/view-deltas-and-projections.md)
docs/design/reactivity.md:7:- [Reactivity: Scheduling And Data Model](reactivity/scheduling-and-data-model.md)
docs/design/reactivity.md:8:- [Reactivity: Validation Harness](reactivity/validation-harness.md)
docs/design/subscription-compilation.md:6:> **Scope:** Replace the "hardcoded two-role relay set" planner in `crates/nmp-core/src/kernel/{requests,ingest,mod}.rs` with a **subscription compilation stage** that turns logical interests into per-relay plans driven by NIP-65 mailboxes, and graduates outbox routing to a first-class planner subsystem. v1 is in-memory; M3 plugs it into LMDB. This is a design doc; no implementation lands in this PR.
docs/design/podcast/inventory.md:11:- **Substrate kind**: which trait family in `nmp-core/src/substrate/` backs it.
docs/design/podcast/inventory.md:53:| `Services/ProcessingQueue.swift` | 360 | The action ledger — already in `nmp-core::kernel::ledger`. Per-domain action chain (Download → Transcribe → Summarize → ExtractChapters) becomes a state machine in `podcast-core` orchestrator. Per-job statuses are kernel ledger rows. | `podcast-core` orchestrator | all of the above |
docs/design/subscription-compilation/nip65.md:27:Soft target per file: ≤ 300 LOC (AGENTS.md). The crate stays small; everything heavier (filter compilation, indexer probes) lives in `nmp-core::kernel::planner`, not here.
docs/design/subscription-compilation/nip65.md:179:This is the function currently inlined as a free fn in `crates/nmp-core/src/kernel/nostr.rs` (referenced by `kernel/ingest.rs:210` and tested in `kernel/tests.rs:150`). M2 moves it here and re-exports from `nmp-core` for compatibility during the migration.
docs/design/subscription-compilation/nip65.md:186:- **No outbox routing policy.** The decision "publish goes to author write relays + recipient inbox relays" is the publish planner ([outbox.md](outbox.md) §7), not this crate. This crate provides the lookups; the policy lives in `nmp-core::kernel::planner::publish`.
docs/design/subscription-compilation/nip65.md:194:- `nmp-core::substrate::{ViewModule, ViewContext, InterestContext, LogicalInterest, ...}` — kernel trait surface.
docs/design/subscription-compilation/nip65.md:195:- `nmp-core::kernel::projections` — for reading kind:10002 events out of the event store (the compiler's input).
docs/design/subscription-compilation/nip65.md:199:- `nmp-core::kernel::planner` — for `MailboxCache`, `resolve_author_outbox/inbox`, `parse_relay_list`.
docs/design/subscription-compilation/nip65.md:213:nmp-core   = { path = "../nmp-core" }
docs/design/subscription-compilation/nip65.md:221:No `nostr-sdk` dependency: this crate operates on parsed `Event` structs from `nmp-core`'s already-vetted ingest path. Avoiding a duplicate parse dependency keeps the surface auditable.
docs/design/podcast-app-rebuild.md:15:**This is M11's load-bearing kernel-boundary check.** If the kernel needs even one podcast noun to make it work, the boundary is wrong and we go back to fix it. The exit gate is dual: (a) `nmp-core` gains zero podcast nouns, verified by grep + manual review; (b) screenshot diff vs `../podcast` is ≤ 1 px on every screen, font-rendering exceptions whitelisted.
docs/design/podcast-app-rebuild.md:85:        │ nmp-core (kernel)                                     │
docs/design/podcast-app-rebuild.md:108:Plus seven new capability families landed in `nmp-core/src/substrate/capabilities/` (each pure-trait; impls live in the platform shell). See [`podcast/capabilities.md`](podcast/capabilities.md).
docs/design/podcast-app-rebuild.md:110:`nmp-core` itself gains **zero** podcast types. The exit-gate grep is:
docs/design/podcast-app-rebuild.md:114:     crates/nmp-core/src/ \
docs/design/subscription-compilation/diagnostics.md:22:// crates/nmp-core/src/kernel/diagnostics/lanes.rs (proposed)
docs/design/subscription-compilation/diagnostics.md:56:Emitted whenever `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`) replaces a mailbox entry. One record per `(pubkey, relay_url)` pair; an author with 4 declared relays produces 4 records on each update.
docs/design/subscription-compilation/diagnostics.md:90:Emitted by `handle_event` (`crates/nmp-core/src/kernel/ingest.rs:134-164`) for every EVENT arrival. This is the highest-cardinality lane and the only one where coalescing matters at the ADR-0007 boundary: the platform diagnostic view consumes a summarised projection (`ProvenanceSummary` per author or per event), not the raw fact stream.
docs/design/framework-magic.md:5:> **Source directives:** `docs/plan/scope-adjustments-2026-05-18.md` "Framework magic contract" section; `docs/aim.md` §6 doctrines 1–12; `docs/product-spec/overview-and-dx.md` §1.5 (cardinal doctrines D0–D5) + §3.3 (bug-class extinction); `docs/product-spec/subsystems.md` §7.1–§7.8.
docs/design/ffi-hardening.md:10:C FFI between `crates/nmp-core` and `ios/NmpStress` is rock-solid before a
docs/design/ffi-hardening.md:19:   (`docs/product-spec/overview-and-dx.md` §1.5 D0–D5), and every ownership
docs/design/ffi-hardening.md:37:  functions in `crates/nmp-core/src/ffi.rs`, JSON-string update callback) is
docs/design/ffi-hardening.md:52:`crates/nmp-core/src/ffi.rs` (lines 44–268) plus **one callback type**
docs/design/ffi-hardening.md:132:   linking `nmp-core` as `staticlib`. Models the iOS main-thread loop
docs/design/ffi-hardening.md:176:├── doctrine-review.md       # D0–D5 sign-off (M10.5 exit-gate artifact)
docs/design/ffi-hardening.md:192:| F1 | `crates/nmp-core/src/ffi.rs:75` | Add `// safe: ...` doc on unsafe `Box::from_raw` | 5 min |
docs/design/ffi-hardening.md:193:| F2 | `crates/nmp-core/src/ffi.rs:275` | Same on unsafe `&*app` | 5 min |
docs/design/ffi-hardening.md:194:| F3 | `crates/nmp-core/src/ffi.rs:284` | Same on unsafe `CStr::from_ptr` | 5 min |
docs/design/ffi-hardening.md:195:| F4 | `crates/nmp-core/src/relay_worker.rs:242` | Comment `#[allow(unreachable_patterns)]` rationale | 2 min |
docs/design/ffi-hardening.md:196:| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
docs/design/ffi-hardening.md:205:  crates/nmp-core/src crates/nmp-testing/src \
docs/design/ffi-hardening.md:231:`KernelUpdate` serialization — see `crates/nmp-core/src/kernel/update.rs`)
docs/design/ffi-hardening.md:242:Full D0–D5 line-item-to-scenario mapping in
docs/design/ffi-hardening.md:247:| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
docs/design/subscription-compilation/recompilation.md:37:// crates/nmp-core/src/kernel/planner/trigger.rs (proposed)
docs/design/subscription-compilation/recompilation.md:93:Emitted from `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`) when **and only when** the parser decides to replace the prior mailbox entry (the `should_replace` branch at line 218–222). Stale arrivals do not trigger recompilation.
docs/design/subscription-compilation/recompilation.md:117:Emitted by the relay worker (`crates/nmp-core/src/relay_worker.rs`) after a successful re-handshake. Compiler effect: the wire-emitter re-issues the relay's `SubShape` set as REQs to restore tail subscriptions; the compiler does *not* re-merge or re-resolve. This is a pure "replay current plan to one relay" operation, not a real recompilation, but it routes through the same trigger queue so the diagnostic stream sees it.
docs/design/lmdb/watermarks.md:95:This produces a deterministic hash that is stable across `Filter` field-order variations and across Rust HashMap ordering randomness. The implementation lives at `crates/nmp-core/src/store/watermarks.rs::canonical_filter_hash(&Filter) -> [u8; 32]` and is the single source of truth for the planner + sync engine + dump format.
docs/design/lmdb/watermarks.md:105:The existing `ModuleRegistry` (`crates/nmp-core/src/substrate/mod.rs:36-79`) stores only `ModuleDescriptor { namespace, family, rust_type }` — the concrete `M: DomainModule` type is consumed by the generic `register_domain::<M>()` call and not retained, so the store has no runtime path from a namespace string back to `M::SCHEMA_VERSION` or `M::migrations()`. M3 extends `ModuleDescriptor` for the Domain family with two `fn`-pointer factories — matching the existing `DomainIndex::key_fn: fn(&[u8]) -> ...` pattern (`substrate/domain.rs:18`):
docs/design/firehose-bench.md:279:The two are complementary. Reactivity-bench answers "does the algorithm scale?"; firehose-bench answers "does the system work under realistic conditions?"
docs/design/subscription-compilation/tests.md:64:This is a "shape of the API" assertion, not a behaviour assertion. If a future PR adds a relay field to `SendNote`, the test fails.
docs/design/subscription-compilation/tests.md:262:- **Wire-emitter diff correctness across two plans.** That is a separate unit test inside `nmp-core::kernel::wire`, not the milestone-exit gate.
docs/design/podcast/podcast-rag.md:239:nmp-core = { path = "../../../crates/nmp-core" }
docs/design/lmdb/trait.md:7:`crates/nmp-core/src/store/events.rs` (filename note: `trait` is a Rust keyword, so the file is named `events.rs` and exposes `pub trait EventStore`). Re-exported from `nmp_core::store::EventStore`. The actor (`crates/nmp-core/src/actor.rs`) holds the store as `store: Box<dyn EventStore>`; backends are constructed by the factory in `store/mod.rs::open_event_store(&AppConfig) -> Result<Box<dyn EventStore>, StoreError>`.
docs/design/lmdb/keys.md:111:- `domain_foo.bar_idx_<index>` — one sub-db per `DomainIndex` (per `crates/nmp-core/src/substrate/domain.rs:16`). Key = `index_key_fn(data_value) ‖ primary_key`; value = empty. The index is rewritten on every put (delete-old, write-new).
docs/design/podcast/capabilities.md:313:All trait files live in `crates/nmp-core/src/substrate/capabilities/`:
docs/design/podcast/capabilities.md:316:crates/nmp-core/src/substrate/capabilities/
docs/design/subscription-compilation/compiler.md:48:The wire-emitter (`crates/nmp-core/src/kernel/wire.rs`, to be added) diffs the new plan against the current wire-sub registry: opens new REQs, closes orphaned ones, leaves stable assignments untouched.
docs/design/subscription-compilation/compiler.md:52:Inputs: every `LogicalInterest` with non-empty `shape.authors` or non-empty `shape.tags[#p]`; the mailbox cache populated by `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`).
docs/design/subscription-compilation/compiler.md:84:The indexer set is a kernel-configured `Vec<RelayUrl>` (default: a small curated list; user-configurable in `AppConfig`). Today's `crates/nmp-core/src/relay.rs:2` is the placeholder for one indexer relay (`purplepag.es`); the v1 indexer set lives in `AppConfig.indexer_relays`.
docs/design/subscription-compilation/compiler.md:153:- **Recompilation with no change ⇒ same plan-id.** If `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:218-221`) deduplicates and decides not to replace a stale mailbox, no plan-id churn.
docs/design/subscription-compilation/compiler.md:158:The `plan_id` is stored on `CompiledPlan` and rendered into `LogicalInterestStatus` (extending the record at `crates/nmp-core/src/kernel/mod.rs:147-154` with `plan_id: String, plan_generation: u64`). Tests in §9 assert plan-id stability across no-op recompilations.
docs/design/subscription-compilation/compiler.md:162:This is the binding contract: each function in `crates/nmp-core/src/kernel/requests.rs` and `crates/nmp-core/src/kernel/ingest.rs` either disappears, becomes thin glue over the compiler, or graduates into a typed module. The compiler does not coexist with the old planner; M2 replaces it.
docs/design/subscription-compilation/compiler.md:188:- It does not move the action ledger into M2 — `SendNote` lands in M6.
docs/design/subscription-compilation/compiler.md:193:The compiler is **in-memory v1** by design. The mailbox cache is the existing `HashMap<String, AuthorRelayList>` (`crates/nmp-core/src/kernel/mod.rs:313`); it just gets a new consumer.
docs/design/podcast/podcast-feeds.md:136:nmp-core = { path = "../../../crates/nmp-core" }
docs/design/subscription-compilation/outbox.md:6:This section defines the **publish-side seam** the M2 milestone lands so the M6 write path has a ready surface. There is no publish code in the repo today (`crates/nmp-core/src/kernel/requests.rs` contains no `EVENT` outbound; the relay worker has no publish channel). M2 lands the trait and the override action; M6 writes the first concrete consumer (`SendNoteAction`).
docs/design/subscription-compilation/outbox.md:13:// crates/nmp-core/src/kernel/planner/publish.rs (proposed)
docs/design/subscription-compilation/outbox.md:81:// crates/nmp-core/src/kernel/planner/publish_default.rs (proposed)
docs/design/subscription-compilation/outbox.md:140:1. **Named** — its own typed `AppAction` variant, not a hidden parameter on `SendNote`.
docs/design/subscription-compilation/outbox.md:148:// crates/nmp-core/src/kernel/actions/publish_override.rs (proposed)
docs/design/podcast/exit-gate.md:14:| `nmp-core` gains zero podcast nouns | `docs/perf/m11/kernel-boundary.md` with the grep output | `grep -RE 'Podcast\|Episode\|Transcript\|Chapter\|Player\|Feed\|Insight\|Guest\|RSS\|Audio\|MP3' crates/nmp-core/src/` (whitelist the `audio_playback.rs` trait file) → expected empty |
docs/design/podcast/exit-gate.md:16:| Reactivity behavior identical to Twitter slice | `docs/perf/m11/reactivity.md` | Re-run `reactivity-bench --standard --fail-on-gate` with `podcast-core` views registered alongside the existing nip01 views; assert all gates pass |
docs/design/podcast/exit-gate.md:17:| No app-state leaks across the boundary in either direction | same as row 1 (kernel) + a sibling grep across `crates/nmp-core/src/` for `nostr\|relay\|nip` produces no hit inside `apps/podcast/` crates | the grep is added to CI |
docs/design/podcast/exit-gate.md:95:3. **`capability-review`** — runs `cargo run -p nmp-codegen -- validate-capabilities --capabilities-dir crates/nmp-core/src/substrate/capabilities/` — a new codegen subcommand that asserts request/result type definitions don't mention any podcast-domain noun (uses a curated wordlist + AST traversal).
docs/design/podcast/exit-gate.md:106:- The doctrine review at `docs/perf/m11/doctrine-review.md` signs off D0–D5 against the M11 surface (template: `docs/perf/m10.5/doctrine-review.md`).
docs/design/ffi-hardening/gates.md:7:2. **§D** doctrine D0–D5 review checklist — each line item maps to
docs/design/ffi-hardening/gates.md:150:## §D. Doctrine D0–D5 review checklist
docs/design/ffi-hardening/gates.md:155:> **Note.** The task brief mentioned "D0–D5". The canonical list in
docs/design/ffi-hardening/gates.md:156:> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
docs/design/ffi-hardening/gates.md:159:> not redundantly re-prove — items beyond D0–D5 are covered by
docs/design/ffi-hardening/gates.md:162:### D0. Kernel never grows app nouns
docs/design/ffi-hardening/gates.md:164:- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
docs/design/ffi-hardening/gates.md:165:  no app-domain types in `nmp-core` substrate.
docs/design/ffi-hardening/gates.md:171:  `docs/perf/m10.5/doctrine-review.md` § D0.
docs/design/ffi-hardening/gates.md:198:### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
docs/design/ffi-hardening/gates.md:278:| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
docs/design/lmdb-schema.md:44:crates/nmp-core/src/store/
docs/design/lmdb-schema.md:115:The current `ModuleRegistry` (`crates/nmp-core/src/substrate/mod.rs:41`) discards the concrete `M: DomainModule` type after `register_domain::<M>()` returns — only the `ModuleDescriptor` is retained. The store cannot get from a namespace string back to `M::SCHEMA_VERSION` or `M::migrations()` at runtime. M3 adds a `DomainFactories { schema_version: fn() -> u32, migrations: fn() -> Vec<DomainMigration>, indexes: fn() -> Vec<DomainIndex> }` struct attached per descriptor, populated by capturing the `M::*` consts and fns in `fn`-pointer closures at register time. This matches the existing `key_fn: fn(&[u8]) -> Option<Vec<u8>>` pattern in `DomainIndex` (`crates/nmp-core/src/substrate/domain.rs:18`) — no `Box<dyn DomainModule>` and no new trait object-safety constraints on `DomainModule`. The change is additive to the substrate module surface. See [`lmdb/watermarks.md`](lmdb/watermarks.md) §4.1 for the registry-side code shape.
docs/design/lmdb-schema.md:219:2. **Watermark `filter_hash` canonicalisation.** Two `Filter`s that are semantically identical but field-ordered differently must hash the same. The canonicalisation rule (likely: sort all tag-value arrays, sort kinds, sort authors, lexicographic field order before BLAKE3) needs to be specified once and shared with the planner so cache-coverage lookups hit. Candidate: a single `fn canonical_filter_hash(&Filter) -> [u8; 32]` in `nmp-core::store::watermarks`.
docs/design/lmdb-schema.md:227:- In-memory event store: `crates/nmp-core/src/kernel/mod.rs:294` (`events: HashMap<String, StoredEvent>`), `kernel/mod.rs:46` (`StoredEvent` struct).
docs/design/lmdb-schema.md:228:- Insert path under wrap: `crates/nmp-core/src/kernel/ingest.rs:166` (`ingest_profile`), `ingest.rs:235` (`ingest_timeline_event`), `ingest.rs:209` (`ingest_relay_list`).
docs/design/lmdb-schema.md:231:- Substrate `DomainModule` trait the store backs: `crates/nmp-core/src/substrate/domain.rs:1` (current shape, lines 1–49).
docs/design/lmdb-schema.md:232:- Module registry the store consumes at startup: `crates/nmp-core/src/substrate/mod.rs:41` (`ModuleRegistry::register_domain`).
docs/design/ffi-hardening/ci.md:65:    cargo build --release -p nmp-core --target aarch64-apple-ios
docs/design/ffi-hardening/ci.md:91:        --require-doctrines D0,D1,D2,D3,D4,D5 \
docs/design/ffi-hardening/ci-tiers.md:27:      - 'crates/nmp-core/**'
docs/design/ffi-hardening/ci-tiers.md:159:4. Doctrine review (D0–D5) signed off in `doctrine-review.md`.
docs/design/ffi-hardening/harness.md:176:only (not in `nmp-core`). Used by S1, S2, S3, S6, S8 to detect heap
docs/design/ffi-hardening/harness.md:189:`nmp-core` already compiles as `cdylib + staticlib + rlib`. The
docs/design/ffi-hardening/harness.md:191:`nmp-core-ffi-decls` crate that re-exports the `extern "C"` symbols
docs/design/ffi-hardening/harness.md:195:// crates/nmp-core-ffi-decls/src/lib.rs
docs/design/ffi-hardening/harness.md:208:(Alternative: use `nmp-core` directly as a crate dep and avoid the
docs/design/subscription-compilation/intro.md:10:- **Two hardcoded relays.** `crates/nmp-core/src/relay.rs:1-2` declares `CONTENT_RELAY_URL = "wss://relay.primal.net"` and `INDEXER_RELAY_URL = "wss://purplepag.es"` as module-level constants. There is no per-author routing.
docs/design/subscription-compilation/intro.md:11:- **Relay choice is a 2-variant enum, not a URL set.** `crates/nmp-core/src/relay.rs:15-39` defines `RelayRole::{Content, Indexer}` with a `.url() -> &'static str` that returns one of the two literals. This shape cannot express "this REQ should go to the union of these N authors' write relays."
docs/design/subscription-compilation/intro.md:12:- **The seam that emits REQs is parameterized by `RelayRole`.** `crates/nmp-core/src/kernel/requests.rs:530-556` (`req()`) inserts a `WireSub { role, .. }` keyed by a string sub-id and emits `OutboundMessage { role, text }`. The role *is* the routing decision; there is no relay-URL field on `WireSub` or `OutboundMessage`. Any compiler that fans an interest out across N URLs has to replace this helper.
docs/design/subscription-compilation/intro.md:13:- **Startup REQs ignore mailboxes by construction.** `crates/nmp-core/src/kernel/requests.rs:50-106` (`startup_requests`) issues six fixed REQs, each pinned to `Content` or `Indexer`. The seed-bootstrap timeline (line 65–70) fans seven hundred-author future timelines through one relay. The exit-gate test for M2 ([`docs/plan/m2-subscription-compilation.md`](../../plan/m2-subscription-compilation.md)) requires that this fan exactly equal the union of those authors' write relays.
docs/design/subscription-compilation/intro.md:14:- **View-open REQs ignore mailboxes too.** `crates/nmp-core/src/kernel/requests.rs:404-439` (`author_requests`) hardcodes a three-REQ shape — `author-relays-N` on Indexer, `author-profile-N` on Indexer, `author-notes-N` on Content. The author's notes are fetched from the global content relay even though by the time the view opens we may already have that author's kind:10002 in cache (see next bullet).
docs/design/subscription-compilation/intro.md:15:- **Mailbox cache exists but no consumer.** `crates/nmp-core/src/kernel/ingest.rs:209-233` (`ingest_relay_list`) already parses kind:10002 into `self.author_relay_lists: HashMap<String, AuthorRelayList>` (declared at `crates/nmp-core/src/kernel/mod.rs:269-275` and reserved at `mod.rs:313`). The cache is written; **nothing reads it for routing**. This is the bug doctrine D5 ("capabilities report, never decide") inverted: we have the data, we ignore it.
docs/design/subscription-compilation/intro.md:16:- **Profile claim path is single-relay.** `crates/nmp-core/src/kernel/requests.rs:390-402` (`profile_claim_request`) sends a kind:0 fetch to `RelayRole::Indexer` unconditionally. It cannot consult mailboxes for the claimed author.
docs/design/subscription-compilation/intro.md:17:- **No publish path exists yet.** `crates/nmp-core/src/kernel/requests.rs:30` (no occurrences of `EVENT` outbound) and `crates/nmp-core/src/relay.rs:42-45` (`OutboundMessage` carries only role + text). The first publish action (M6 `SendNote`) will hit this same `req()`-style seam. M2 must establish the planner shape before M6 builds the first user of it; the doctrine "no developer-supplied relays for a publish" (`docs/aim.md` §6 doctrine 5; `docs/product-spec/subsystems.md` §7.3 row "Publish leaked to wrong relays") needs a structural enforcement point.
docs/design/subscription-compilation/intro.md:19:The summary diagnosis: **the planner is a string formatter, not a compiler.** Every REQ is a per-call-site decision; routing is one of two literals; recompilation is impossible because nothing is compiled. The diagnostics in `crates/nmp-core/src/kernel/mod.rs:117-154` already type `RelayStatus` / `WireSubscriptionStatus` / `LogicalInterestStatus` per ADR-0007 — but the planner currently emits at most one `LogicalInterestStatus` per view kind because there is no logical-interest object to scope it against.
docs/design/subscription-compilation/intro.md:28:// crates/nmp-core/src/kernel/planner/interest.rs (proposed)
docs/design/subscription-compilation/intro.md:110:- `TimelineView { authors: [pablof7z, fiatjaf, jb55, ...follows] }` returns one `LogicalInterest { shape: { authors, kinds: {1, 6}, limit: 200 }, lifecycle: Tailing }`.
docs/design/subscription-compilation/intro.md:112:- `ProfileClaim { pubkey }` (the refcounted UI path from `crates/nmp-core/src/kernel/requests.rs:202-237`) returns one interest: `{ authors: [pubkey], kinds: {0}, limit: 1, lifecycle: OneShot }`.
docs/design/subscription-compilation/intro.md:115:The seed-bootstrap path (`crates/nmp-core/src/kernel/requests.rs:50-106`) becomes one `LogicalInterest` per concern, registered at actor `Start` rather than emitted as raw REQs. The compiler produces the wire artifacts.
docs/design/subscription-compilation/intro.md:121:Account-scoped interests with empty `authors` and empty `#p` (e.g. a free-form hashtag firehose) resolve against the active account's *read relays* (NIP-65 read side) — the user's own subscription preferences, not a globally hardcoded relay. Today's `firehose_requests()` at `crates/nmp-core/src/kernel/requests.rs:357-372` hardcodes `RelayRole::Content`; under the compiler this becomes "active-account read relays, falling back to indexer set if the active account has no kind:10002."
docs/design/reactivity/scheduling-and-data-model.md:1:# Reactivity: Scheduling And Data Model
docs/design/reactivity/scheduling-and-data-model.md:3:[Back to Design: Reactivity](../reactivity.md)
docs/design/reactivity/scheduling-and-data-model.md:48:| Reactions | N `EmojiAdjusted { emoji, delta }` for same emoji → one with summed delta. Different emojis stay separate. |
docs/design/reactivity/scheduling-and-data-model.md:74:│   - profiles: [PubKey: ProfileView]                      │
docs/design/reactivity/scheduling-and-data-model.md:75:│   - reactionSummaries: [EventId: ReactionSummary]        │
docs/design/reactivity/scheduling-and-data-model.md:76:│   - timelines: [SpecHash: TimelineView]                  │
docs/design/reactivity/view-deltas-and-projections.md:1:# Reactivity: View Deltas And Projections
docs/design/reactivity/view-deltas-and-projections.md:3:[Back to Design: Reactivity](../reactivity.md)
docs/design/reactivity/view-deltas-and-projections.md:20:pub fn open(spec: TimelineSpec, store: &EventStore) -> (State, Dependencies, TimelineView) {
docs/design/reactivity/view-deltas-and-projections.md:78:    Reactions { id: ViewId, delta: ReactionsDelta },
docs/design/reactivity/view-deltas-and-projections.md:100:pub enum ReactionsDelta {
docs/design/reactivity/view-deltas-and-projections.md:114:A `TimelineView` payload contains 200 `TimelineItem`s. Each item has an `author_display: String`. When a new kind:0 arrives for one of those authors, **every item in every view by that author needs its display updated**. Naively, that's a fan-out problem: each view scans every item.
docs/design/reactivity/view-deltas-and-projections.md:125:    reaction_summary: HashMap<EventId, ReactionSummary>,
docs/design/reactivity/view-deltas-and-projections.md:154:        ProjectionChange::ReactionSummary { event_id, new } => { ... }
docs/design/reactivity/view-deltas-and-projections.md:164:You could model this as "TimelineView subscribes to ProfileView for each author." That creates a dependency graph that's hard to reason about, hard to GC, and hard to debug. Shared projections in the store let us keep view internals flat and the dispatch story simple.
docs/design/framework-magic/test-scaffolding.md:198:| M6 | C7 sub-paths 1 + 2 + 5 (SendNote consumer); C11 (signers + onboarding actions) |
docs/design/framework-magic/test-scaffolding.md:208:- **Negative tests for the API surface.** "The app cannot type `SendNote { content, relays: vec![...] }`" is a *compile-fail* test, owned by `docs/design/subscription-compilation/tests.md` §9.2 assertion 1. The framework-magic surface assertion is "no test passes the broken usage"; the structural inability is asserted there.
docs/design/framework-magic/capabilities.md:33:1. **Placeholders at open:** open `TimelineView { authors: [alice], kinds: [1] }` against a fresh store with no kind:0 for Alice. Insert a kind:1 event by Alice. Assert the payload's `items[0]`:
docs/design/framework-magic/capabilities.md:45:**Milestone owner:** **[DONE]** for the placeholder shape (the M1 timeline slice already ships non-`Option` author fields with shortened-npub fallback — verified in `crates/nmp-core` timeline tests today). **[PENDING M2/M3]** for the full in-place refinement guarantees: sub-paths 1 and 5 are testable today; sub-paths 2 and 4 require the kernel's projection cache (`kernel-substrate.md` §3 line 148 `on_projection_changed`) which graduates in M2 alongside the view-module surface; sub-path 3 requires the per-tick re-format hook (`fn on_tick`, M2's `ViewModule` trait work).
docs/design/framework-magic/sync.md:26:**Milestone owner:** **[PENDING M3]**. Sub-paths 1–6 are testable today against the in-memory kernel (the current `relay_count` field at `crates/nmp-core/src/kernel/ingest.rs:238` is the primitive shape; M3 graduates it to a typed `Provenance` sidecar). Sub-path 7 requires M3's storage cap logic. Test checked in as `#[ignore = "pending M3 provenance schema"]`.
docs/design/framework-magic/sync.md:40:1. **Unsynced pair → fetch.** Open `TimelineView { authors: [A], kinds: [1], since: T-1d, until: T }` against a fresh store (no watermark for this `(filter, relay)` pair). Assert the planner schedules a backfill — NIP-77 reconciliation against the mock relay (because capability negotiation succeeded). Mock relay returns a 50-event set; assert all 50 land in the store; assert the watermark for `(filter_sig, "wss://mock")` updates to `synced_up_to = T`.
docs/design/framework-magic/replaceable.md:12:**Framework does:** the insert-time supersession at `docs/product-spec/subsystems.md` §7.1 row "Replaceable kinds (0, 3, 10000-19999)". Mechanism: compare `(pubkey, kind)` against the existing entry, keep newest `created_at`, tie-break by lexicographically smallest `id`. The current in-memory store enforces this for kind:0 / kind:3 / kind:10002 today (kind:3 via `seed_contacts.insert` at `crates/nmp-core/src/kernel/ingest.rs:206`; kind:10002 via the `should_replace` branch at `crates/nmp-core/src/kernel/ingest.rs:218-222`). M3 graduates the rule into the LMDB-backed `EventStore` trait (`docs/design/lmdb/trait.md`).
docs/design/framework-magic/replaceable.md:14:**App writes:** nothing. The app calls `ProfileView::open(pubkey)`; the view's payload reflects the latest kind:0 the store has, with no app-side comparison of `created_at`.
docs/design/framework-magic/replaceable.md:18:**Test:** `c1_replaceable_supersedes_on_insert`. The test inserts kind:0 #1 at `created_at=T`, then kind:0 #2 at `T+1` with same pubkey; asserts `ProfileView` payload reflects #2 and that a subsequent insert at `T-1` is rejected (no payload re-emit, no event store change). Tie-break path: two inserts at the same `T` with different ids — the lexicographically-smaller-id event wins, deterministic across runs.
docs/design/framework-magic/replaceable.md:20:**Milestone owner:** **[DONE]** for in-memory kernel (verified by `crates/nmp-core` kernel tests today, ref the existing `should_replace` branch). Test runs **not** ignored from day one; LMDB graduation in M3 must preserve the same observable, so the test stays green across M3.
docs/design/framework-magic/replaceable.md:46:**App writes:** nothing. The view payloads recompute (via `ViewModule::on_event_removed` per `docs/design/kernel-substrate.md` §3 lines 141–143) and the deleted note disappears from `TimelineView.items` in the next emit.
docs/design/framework-magic/replaceable.md:52:1. Inserts a kind:1 event `e1` by author Alice; asserts it appears in `TimelineView`.
docs/design/framework-magic/replaceable.md:53:2. Inserts a kind:5 by Alice referencing `e1`; asserts `TimelineView` no longer contains `e1`.
docs/design/framework-magic/replaceable.md:76:3. Advance clock to +61s; assert event removed; `TimelineView` payload re-emitted without it.
docs/design/framework-magic/signers.md:10:1. **Bunker URL onboarding.** A pasted `bunker://...` URL parses into a `BunkerConnect` action; the action runs the NIP-46 rendezvous, establishes the remote-signer connection, persists the connection token via `KeyringCapability`, and emits an `Account` with `signer_kind = Nip46Bunker` into `SessionState.accounts`.
docs/design/framework-magic/signers.md:11:2. **Create new nsec.** A `CreateLocalIdentity { passphrase, label }` action generates a new keypair, encrypts the nsec via NIP-49 with the given passphrase, persists the encrypted nsec via `KeyringCapability`, and emits an `Account` with `signer_kind = LocalKey` into `SessionState.accounts`.
docs/design/framework-magic/signers.md:17:- The signer catalog at `subsystems.md` §7.4 lines 127–135 names both kinds as supported in `nmp-core` (no FFI signer extensibility — apps don't implement signers).
docs/design/framework-magic/signers.md:21:- The NIP-49 encryption is the `nostr` crate's `EncryptedSecretKey`; the framework wraps it as a step inside the `CreateLocalIdentity` action.
docs/design/framework-magic/signers.md:23:**App writes:** for **bunker**, one dispatch with the pasted URL: `dispatch(AppAction::BunkerConnect { url: "bunker://..." })`. For **create new nsec**, one dispatch: `dispatch(AppAction::CreateLocalIdentity { passphrase, label })`. The action ledger row exposes progress (parsing, rendezvous, awaiting user approval on the bunker app, persisted, available); the app's UI renders the ledger row as a step indicator if it wants, but the orchestration is the framework's. The app does **not** call NIP-46 transport code, does **not** invoke NIP-49 encryption, does **not** touch the Keychain directly, and does **not** wire the new identity into the session state.
docs/design/framework-magic/signers.md:30:   a. Dispatch `BunkerConnect { url: "bunker://abc?relay=wss%3A%2F%2Fmock&secret=xyz" }`.
docs/design/framework-magic/signers.md:36:   a. Dispatch `CreateLocalIdentity { passphrase: "test-passphrase", label: "alice" }`.
docs/design/framework-magic/signers.md:40:   e. Assert a follow-up `SwitchActiveAccount { account_id }` succeeds and that the actor can sign a test event using the newly-created identity (round-trip: dispatch a `SendNote` against the new account, observe a signed event in the action ledger before publish).
docs/design/framework-magic/signers.md:74:- **Signing a publish** — the sign step inside `SendNoteAction` (C7). C11 covers onboarding; subsequent signing is the publish path.
docs/design/framework-magic/intro.md:21:The 13 contract bullets map onto the cardinal doctrines (D0–D5 in `product-spec/overview-and-dx.md` §1.5) and onto the older `aim.md` §6 doctrines 1–12. The mapping is intentionally many-to-many — a single behavior may discharge multiple doctrines, and a single doctrine may require several behaviors to be fully discharged.
docs/design/framework-magic/intro.md:25:| **D0** kernel + extension modules (no app nouns in `nmp-core`) | All 13 — the contract is the API the app sees in place of the missing nouns |
docs/design/framework-magic/intro.md:56:1. **It forces honesty.** If a chapter cannot fill the "App writes" field with `"nothing"` or a single safe call, the framework has leaked the operation to the app, and the doctrine D0 boundary is violated. The author of that chapter is required to file an ADR rather than ship the bullet as-is.
docs/design/framework-magic/kind3.md:14:1. Replaces the stored kind:3 in the event store (per C1; mechanism at `crates/nmp-core/src/kernel/ingest.rs:187-207` — currently stored in `self.seed_contacts` map; M2 graduates this into the projection cache).
docs/design/framework-magic/kind3.md:20:**App writes:** nothing. The "following timeline" view's spec does not name authors — the view module consumes the active account's follow-set internally. The app's only contact with this surface is opening `FollowingTimelineView { /* no fields */ }` and reading its `Payload.items`.
docs/design/framework-magic/kind3.md:26:1. Opens a `FollowingTimelineView` against an active account whose stored kind:3 follows pubkeys `{A, B, C}` with mailbox cache pre-seeded so A→relay1, B→relay2, C→relay3.
docs/design/framework-magic/kind3.md:30:5. Asserts the same `FollowingTimelineView` handle is still open (refcount unchanged); the platform shadow has emitted one additional payload, not torn down and re-created.
docs/design/framework-magic/subs.md:23:1. **Dedup:** open two `TimelineView`s with identical filters; assert the planner produces one wire REQ per relay (not two); destroy one; assert the wire REQ stays alive; destroy the second; assert the REQ is CLOSE'd after the warmth grace expires (`subsystems.md` §7.6 line 226: 30s default).
docs/design/framework-magic/subs.md:24:2. **Coalesce:** open `TimelineView { authors: [A, B], kinds: [1] }` and `ProfileView { pubkey: C }`; assert the planner merges into one REQ per relay containing the union shape, with each view receiving only its filtered subset locally (no REQ for kind:0 alone if the relay already has the merged stream covering it). The merge lattice's exact rules live in `subsystems.md` §7.2 line 65 and `docs/design/subscription-compilation/intro.md` §1 open-question #2 (lattice formalization); the test asserts the *observable* (wire frame count = correct fewer-than-naive, payload coverage = correct) rather than the lattice mechanics.
docs/design/reactivity/validation-harness.md:1:# Reactivity: Validation Harness
docs/design/reactivity/validation-harness.md:3:[Back to Design: Reactivity](../reactivity.md)
docs/design/reactivity/loop-and-reverse-index.md:1:# Reactivity: Loop And Reverse Index
docs/design/reactivity/loop-and-reverse-index.md:3:[Back to Design: Reactivity](../reactivity.md)
docs/design/reactivity/loop-and-reverse-index.md:5:# Design: Reactivity (internal mechanism)
docs/design/reactivity/loop-and-reverse-index.md:165:Cost: O(K + P) composite lookups plus O(|broad indexes used|) plus O(|catch_all|). For an event with K e-tags and P p-tags in a well-shaped app, that's a handful of HashMap probes. Reactivity-bench run 001 measured p99 lookup at 84 ns to 1,083 ns — far below the 100 µs gate.
docs/design/kernel-substrate.md:13:`nmp-core` defines five trait families. Each extension crate implements one or more of them. The kernel runtime knows nothing about a module's specific types — only that the module conforms to these traits and contributes variants to the generated per-app enums (per ADR-0010).
docs/design/kernel-substrate.md:182:Replaces the closed `AppAction` enum. Every user intent — `SendNote`, `React`, `Repost`, `CreateHighlight`, `UploadBlob`, `RunSync` — is an `ActionModule`.
docs/design/kernel-substrate.md:231:- Action types (e.g. `SendNote { content, reply_to }`, `React { target, emoji }`).
docs/design/kernel-substrate.md:243:pub struct SendNoteAction;
docs/design/kernel-substrate.md:246:pub struct SendNote {
docs/design/kernel-substrate.md:252:pub enum SendNoteStep {
docs/design/kernel-substrate.md:258:impl ActionModule for SendNoteAction {
docs/design/kernel-substrate.md:260:    type Action = SendNote;
docs/design/kernel-substrate.md:261:    type Step = SendNoteStep;
docs/design/kernel-substrate.md:264:    fn start(cx: &mut ActionContext, action: SendNote)
docs/design/kernel-substrate.md:265:        -> Result<ActionPlan<SendNoteStep>, ActionRejection>
docs/design/kernel-substrate.md:271:            initial_step: SendNoteStep::Validating,
docs/design/kernel-substrate.md:277:    fn reduce(cx: &mut ActionContext, id: ActionId, input: ActionInput<SendNoteStep>)
docs/design/kernel-substrate.md:278:        -> ActionTransition<SendNoteStep, EventId>
docs/design/kernel-substrate.md:364:  kernel: nmp-core
docs/design/kernel-substrate.md:448:- `nmp-nip01`: Event types, Filter, Profile / Contacts / Timeline view modules, SendNote / DeleteEvent actions.
docs/design/kernel-substrate.md:451:- `nmp-nip25`: Reactions view module + React action.
docs/design/kernel-substrate.md:455:- `nmp-nip17`: Conversation view module + SendDm action + NSE crate.
docs/design/kernel-substrate.md:484:2. Phase 1a.2 onward (Twitter clone) implements the demo entirely as extension modules with no `nmp-core` patches needed.
docs/design/kernel-substrate.md:485:3. A future Highlighter-lite, TENEX-lite, or podcast-lite module can be added without changes to `nmp-core` traits. Demonstrated on paper for v1; demonstrated in code post-v1.
docs/design/framework-magic/outbox.md:21:2. Open `TimelineView { authors: <1000 pubkeys>, kinds: [1, 6] }` through the actor's public dispatch surface.
docs/design/framework-magic/outbox.md:40:**App writes:** nothing — for the publish path. The app dispatches a publish action (`SendNote`, `React`, `SendDm`, etc.); the action's privacy mode is determined by the action type, not by an app-supplied parameter. There is no `relays` field on `SendNote`. The override exists for tests, migrations, and operator power-user flows; it is structurally outside the safe app path.
docs/design/framework-magic/outbox.md:46:1. **Public:** seed Alice's mailbox with two write relays; dispatch a public `SendNote` action; assert the resulting publish plan has exactly those two relays and no others, and that `required_success_count = max(1, ceil(2/3)) = 1` per `outbox.md` §7.3 step 3(a).
docs/design/framework-magic/outbox.md:52:**Milestone owner:** **[PENDING M2 seam → M6 publish]**. M2 lands the `PublishPlanner` trait + `Nip65PublishPlanner` + the `PublishWithOverride` action (`docs/design/subscription-compilation/outbox.md` §7.1, §7.2, §7.4). M6 lands `SendNoteAction` as the first concrete consumer. Test checked in as `#[ignore = "pending M2 planner + M6 first consumer"]`. Sub-paths 3 and 4 of the test exercise the planner in isolation (M2-completable); 1, 2, and 5 require M6's action consumer.
docs/design/framework-magic/sessions.md:25:2. **Initial open:** open `FollowingTimelineView` (no fields — derives from active account); assert the planner opens REQs on `{r1, r2}`; assert the payload emits with follow set `{X, Y}`.
docs/design/framework-magic/sessions.md:28:5. **Assert view handle stability:** the `FollowingTimelineView` handle from step 2 is **the same handle**; it has not been torn down. Its payload has been re-emitted once, now reflecting Bob's follow set `{Y, Z}`.
docs/design/framework-magic/sessions.md:29:6. **Assert signer rebinding:** dispatch a `SendNote { content: "hello" }`; assert the signed event's `pubkey = bob_pk` (the new active account's signer was used), without any explicit signer parameter on the `SendNote` action.
docs/design/framework-magic/sessions.md:30:7. **Assert specific-scoped views untouched:** before step 3, also open `ProfileView { pubkey: charlie_pk }` (an `InterestScope::Account(charlie)`-equivalent — actually Global since it names an explicit author). Assert this view's payload is not re-emitted after the switch; its underlying REQ stays alive on the same relay; no delta frames touch it. This is the symmetric assertion: the switch affects *only* `ActiveAccount`-scoped interests, per `subscription-compilation/recompilation.md` §4.2 line 113.
docs/design/framework-magic/sessions.md:61:- **The account-switcher view payload.** That is a view module (`AccountListView` or similar in `nmp-core`'s built-ins per `subsystems.md` §7.4); its spec/payload is owned by the view catalog, not the contract.
docs/design/app-extension-kernel.md:21:The lesson is not that these nouns belong in `nmp-core`. The lesson is that NMP must provide a kernel where app crates can define those nouns safely, durably, observably, and with generated platform bindings while still obeying the "no native business logic" rule.
docs/design/app-extension-kernel.md:52:> If implementing Highlighter, TENEX, Win the Day, Cut Tracker, or Podcast requires adding app nouns to `nmp-core`, the extension boundary is wrong.
docs/design/app-extension-kernel.md:58:| Layer | Owns | May contain app nouns? |
docs/design/app-extension-kernel.md:60:| `nmp-core` kernel | actor, event store, planner, ledger, domain-store traits, view/action/capability registries, diagnostics | No |
docs/design/app-extension-kernel.md:69:- `nmp-core` should know neither.
docs/design/app-extension-kernel.md:213:This supports app-local agents, feedback identities, human accounts, external signers, and provider credential owners without adding "agent" or "feedback" semantics to `nmp-core`.
docs/design/app-extension-kernel.md:244:Generated code is allowed to know app nouns because it is generated for that app. `nmp-core` is not.
docs/design/app-extension-kernel.md:248:These concepts should not be added to `nmp-core`:
docs/design/app-extension-kernel.md:284:Pass condition: no `Room`, `Highlight`, `Artifact`, or `CaptureDraft` nouns are added to `nmp-core`. Only reusable protocol modules may contain NIP-specific nouns.
docs/design/app-extension-kernel.md:348:5. A small fixture app module that proves the platform shell has no business logic and `nmp-core` has no app nouns.

exec
/bin/zsh -lc "nl -ba docs/design/framework-magic/kind3.md | sed -n '1,90p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
     1	# Framework Magic §C5 — Kind:3 Auto-Tracking
     2	
     3	> Parent: `docs/design/framework-magic.md`.
     4	> Read first: `docs/design/subscription-compilation/recompilation.md` (trigger model — kind:3 is the symmetric case to `Trigger::Nip65Arrived`); `docs/design/subscription-compilation/intro.md` §2.3 (account scope binding); `docs/plan/scope-adjustments-2026-05-18.md` §"Folded into M2".
     5	
     6	## 1. The bullet
     7	
     8	### C5. Kind:3 auto-tracking: the active account's follow-list change recompiles every dependent subscription transparently.
     9	
    10	**Framework does:**
    11	
    12	When a kind:3 event lands for the active account's pubkey *and* the replaceable-supersession rule (C1) decides it is fresher than the stored kind:3, the kernel:
    13	
    14	1. Replaces the stored kind:3 in the event store (per C1; mechanism at `crates/nmp-core/src/kernel/ingest.rs:187-207` — currently stored in `self.seed_contacts` map; M2 graduates this into the projection cache).
    15	2. Emits an internal planner trigger — proposed name `Trigger::FollowListChanged { account: AccountId, prev_follows: BTreeSet<Pubkey>, next_follows: BTreeSet<Pubkey> }` — symmetric to the existing `Trigger::Nip65Arrived` (`docs/design/subscription-compilation/recompilation.md` §4.1).
    16	3. The subscription compiler re-runs `interests()` on every `ViewModule` whose `dependencies()` declares `kind 3` *or* whose `interests()` consumes the active account's follow-set as an input to its filter shape (e.g. a "following timeline" view module).
    17	4. The wire-emitter diffs the new plan against the old; only the *delta* (authors added/removed from the union write-relay set) becomes CLOSE / new-REQ frames on the wire. Authors present in both old and new follow-sets see zero wire churn.
    18	5. The view payload's `items` recompute reactively per the standard `on_event_inserted` path (`docs/design/kernel-substrate.md` §3). No view handle is destroyed; the platform shadow's `useFollowingTimeline()` rune/observable continues to emit, just with a new payload.
    19	
    20	**App writes:** nothing. The "following timeline" view's spec does not name authors — the view module consumes the active account's follow-set internally. The app's only contact with this surface is opening `FollowingTimelineView { /* no fields */ }` and reading its `Payload.items`.
    21	
    22	**Failure mode prevented:** the canonical NDK-era bug: app code listens for kind:3 events, manually closes its open subscriptions, re-derives author lists, re-issues REQs, and either races itself (REQ ordering vs. local-state ordering) or leaks the old REQ. This contract structurally forbids that pattern: the view module never sees the kind:3 directly, and the app never issues a REQ. Specifically discharges aim.md §6 doctrine 6 ("subscriptions auto-group, auto-close, auto-dedup, auto-buffer; the developer never writes grouping/dedup/cleanup code") for the follow-list-change case.
    23	
    24	**Test:** `c5_kind3_change_recompiles_follow_dependent_subs` in `crates/nmp-testing/tests/framework_magic_contract.rs`. The test:
    25	
    26	1. Opens a `FollowingTimelineView` against an active account whose stored kind:3 follows pubkeys `{A, B, C}` with mailbox cache pre-seeded so A→relay1, B→relay2, C→relay3.
    27	2. Asserts the initial plan opens REQs on `{relay1, relay2, relay3}` and that the platform shadow has emitted exactly one payload.
    28	3. Ingests a fresher kind:3 for the active account with follows `{A, B, D}` (D's mailbox pre-seeded → relay4).
    29	4. Asserts the planner emitted exactly two wire frames: `CLOSE` on the relay3 slice for C, and `REQ` on relay4 for D. Crucially: no churn on relay1 (A is still there) or relay2 (B is still there).
    30	5. Asserts the same `FollowingTimelineView` handle is still open (refcount unchanged); the platform shadow has emitted one additional payload, not torn down and re-created.
    31	6. Asserts a stale kind:3 (older `created_at`) is rejected without firing the trigger — symmetric to C1 supersession; no payload re-emit.
    32	
    33	The test runs against the `PlannerHarness` introduced in `docs/design/subscription-compilation/tests.md` §9.3, extended with a `follow_set_for(account)` accessor.
    34	
    35	**Milestone owner:** **M2** (the subscription-compilation milestone owns the trigger and the recompile). M2's exit gate (`docs/design/subscription-compilation/tests.md` §9) currently lists four assertions covering the NIP-65 case; the M2 owner adds this fifth assertion as part of the framework-magic delta. Test starts as `#[ignore = "pending M2 trigger"]`; M2 lands the trigger and removes the ignore.
    36	
    37	## 2. Why kind:3 is its own bullet (not a sub-case of C1)
    38	
    39	Kind:3 is a replaceable event, so C1 already says "the stored kind:3 is the newest." The reason kind:3 deserves a separate bullet is that it is **referentially structural**: it changes which *other authors* the framework needs to subscribe to, not just which version of the kind:3 the app reads. That second-order effect — the change in the *open-subscription set* — is the one apps have historically failed at.
    40	
    41	C1 is a storage-layer invariant. C5 is a planner-layer reactive guarantee. The framework needs both.
    42	
    43	## 3. NDK reference path
    44	
    45	The user's directive in `scope-adjustments-2026-05-18.md` says: *"NDK reference: how NDK auto-follows kind:3 changes and re-routes its open subs. (Captured in M2 research wave; agents fan out.)"*
    46	
    47	The mechanism NDK uses is documented in the parallel research file `docs/research/ndk/kind3-auto-tracking.md` (pending agent landing). The contract here does not depend on NDK's specific code path; it depends on the *property* NDK demonstrates: that a kind:3 replacement re-shapes the open-REQ set without the application observing protocol churn.
    48	
    49	`TBD-from-research(ndk/kind3-auto-tracking.md)`: insert file:line ref to NDK's listener and the exact race-window it closes (specifically: what happens if a kind:3 arrives mid-EOSE on a follow-derived REQ). The contract is satisfied by *any* mechanism that produces the observable behavior in C5; NDK's path is one existence proof.
    50	
    51	## 4. Applesauce reference path
    52	
    53	`scope-adjustments-2026-05-18.md` also says: *"Applesauce reference: the 'event store query builder' magic that makes subscriptions auto-update without the app touching them. Highest-priority NDK/Applesauce lesson per user."*
    54	
    55	`TBD-from-research(applesauce/event-store-query-builders.md)`: cite the query-builder API shape that lets a consumer phrase `"things kind:1 by people I follow"` once and get a stream that re-evaluates on every kind:3 change. Applesauce's mechanism is a builder that registers itself as a dependent of the kind:3 projection; the contract's `ViewModule.dependencies()` is the NMP analog (`docs/design/kernel-substrate.md` §3 lines 131–132). The research-fold commit cross-validates that the analog covers Applesauce's pattern fully.
    56	
    57	## 5. Interaction with NIP-65 (kind:10002)
    58	
    59	A new follow (D in the test) needs a mailbox lookup. If D's kind:10002 is not in the mailbox cache, the planner's existing indexer-fallback logic (`docs/design/subscription-compilation/compiler.md` §3 Stage 2) routes D to the indexer set while concurrently fetching D's kind:10002. The fetch eventually triggers `Trigger::Nip65Arrived`, which recompiles again — moving D from the indexer slot to D's declared write relay.
    60	
    61	That second recompile is **not part of the C5 test** — it belongs to the M2 NIP-65 audit gate (test #3 in `docs/design/subscription-compilation/tests.md` §9.2). The C5 test asserts kind:3 alone caused exactly the right delta; the NIP-65 chained recompile is a separate observable that the M2 gate already covers.
    62	
    63	## 6. What this bullet does not cover
    64	
    65	- **The "following timeline" view module itself.** Its spec, payload, recompute logic live in `nmp-nip01` per `docs/design/view-catalog/profile-timeline-thread-reactions.md`. C5 cares only that *whatever view module* declares follow-set dependence gets the recompile.
    66	- **Mute-list changes (kind:10000).** The mute list is structurally analogous, but the user's scope-adjustments doc explicitly names kind:3. Mute-list auto-tracking would be a C5-shaped sibling bullet (potential C14 future addition); not in the v1 contract surface.
    67	- **Other people's follow lists.** A view module that opens kind:3 for `pubkey != active_account` is asking a one-shot question, not declaring a reactive dependency on the social graph. That path uses the normal C1 supersession; no C5 trigger fires.
    68	
    69	These exclusions keep the bullet sharp: C5 is exactly *"the active account's follow-list change re-shapes the open-subscription set."* Everything outside that sentence routes through other contract bullets.

exec
/bin/zsh -lc "nl -ba docs/design/framework-magic/sessions.md | sed -n '1,90p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
     1	# Framework Magic §C12 — Account Switch as State
     2	
     3	> Parent: `docs/design/framework-magic.md`.
     4	> Read first: `docs/product-spec/subsystems.md` §7.4 (`SessionState`); `docs/design/subscription-compilation/recompilation.md` §4.2 trigger A4 (`ActiveAccountChanged`); `docs/design/subscription-compilation/intro.md` §2.3 (account scope binding); `docs/aim.md` §6 doctrine 7.
     5	
     6	## C12. Account switch is a state transition; views rebind without imperative dance
     7	
     8	**Statement.** Switching the active account is a single dispatched action. After the dispatch, every `InterestScope::ActiveAccount`-scoped view (a "following timeline", "my profile", "my mentions", etc.) re-resolves against the new account's context — new follow-set, new mailbox set, new mute list, new signer — without the application issuing CLOSE/REQ frames, tearing down view handles, or rebuilding any UI. The view handles remain valid; their payloads update.
     9	
    10	**Framework does:**
    11	
    12	- `SessionState` (`subsystems.md` §7.4 lines 107–125) carries `accounts: Vec<Account>` and `active: Option<String>` as plain state fields. A `SwitchActiveAccount { pubkey }` action mutates `active`; the mutation is the only state change.
    13	- `Trigger::ActiveAccountChanged { from, to }` (`subscription-compilation/recompilation.md` §4.2) fires as a consequence of the state change. The planner re-runs `interests()` on every `ViewModule` whose registered interest carries `InterestScope::ActiveAccount` (`subscription-compilation/intro.md` §2.1 line 60 + §2.3); `InterestScope::Account(specific)` and `InterestScope::Global` interests are untouched.
    14	- The compiler diffs the new plan against the old; per-relay CLOSE/REQ frames fire only for the *delta* (e.g., previous account's follows that are not in new account's follows close their slices; new follows open new slices).
    15	- View payloads recompute via the same `on_event_replaced` / `on_event_inserted` cascade the kernel uses for any state change; the platform shadow's `useFollowingTimeline()` etc. emit a new payload.
    16	- The signer attached to operations dispatched after the switch is the new active account's signer (per `IdentityModule` routing in `kernel-substrate.md` §6).
    17	
    18	**App writes:** one dispatch: `dispatch(AppAction::SwitchActiveAccount { pubkey })`. The app's "switch account" UI is a button that fires that dispatch. No log-out / log-in dance, no view-tree rebuild, no manual REQ reissue, no clearing of caches — the framework handles all of it as a single tick of the actor's event loop.
    19	
    20	**Failure mode prevented:** `product-spec/overview-and-dx.md` §3.3 **bug #5** ("Two account contexts having overlapping mutable state"). Plus the operationally common bug where an app tears down its view tree on account switch — losing scroll position, in-flight composes, draft state — because it doesn't trust the framework to re-derive correctly. C12 makes the trust structural: the view handles remain valid; the app cannot accidentally observe the old account's data on the new account's views.
    21	
    22	**Test:** `c12_account_switch_rebinds_views_without_imperative_dance`. The test:
    23	
    24	1. **Setup:** seed two accounts in `SessionState.accounts` — Alice (follows `[X, Y]`) and Bob (follows `[Y, Z]`). Pre-seed mailboxes: X→r1, Y→r2, Z→r3. Set Alice active.
    25	2. **Initial open:** open `FollowingTimelineView` (no fields — derives from active account); assert the planner opens REQs on `{r1, r2}`; assert the payload emits with follow set `{X, Y}`.
    26	3. **Dispatch switch:** `dispatch(AppAction::SwitchActiveAccount { pubkey: bob_pk })`. The test makes no other calls; the harness drains the action ledger and the planner trigger queue.
    27	4. **Assert delta wire frames:** exactly two frames emitted by the planner — `CLOSE` for the r1 slice (X drops; X is not in Bob's follows), `REQ` for the r3 slice (Z appears; Z is in Bob's follows). The r2 slice is untouched (Y is in both follows).
    28	5. **Assert view handle stability:** the `FollowingTimelineView` handle from step 2 is **the same handle**; it has not been torn down. Its payload has been re-emitted once, now reflecting Bob's follow set `{Y, Z}`.
    29	6. **Assert signer rebinding:** dispatch a `SendNote { content: "hello" }`; assert the signed event's `pubkey = bob_pk` (the new active account's signer was used), without any explicit signer parameter on the `SendNote` action.
    30	7. **Assert specific-scoped views untouched:** before step 3, also open `ProfileView { pubkey: charlie_pk }` (an `InterestScope::Account(charlie)`-equivalent — actually Global since it names an explicit author). Assert this view's payload is not re-emitted after the switch; its underlying REQ stays alive on the same relay; no delta frames touch it. This is the symmetric assertion: the switch affects *only* `ActiveAccount`-scoped interests, per `subscription-compilation/recompilation.md` §4.2 line 113.
    31	8. **Assert no overlap:** read the audit log of any per-account domain-store namespace (e.g., Alice's drafts) and assert Bob cannot read it. The kernel's domain-store isolation per account is the structural enforcement (`kernel-substrate.md` §8 "Domain stores are isolated" and the per-account scoping in domain key prefixes).
    32	
    33	**Milestone owner:** **[PENDING M8]**. M8 is the multi-account session milestone (per `scope-adjustments-2026-05-18.md` ladder). M2 already lands the `Trigger::ActiveAccountChanged` shape (`subscription-compilation/recompilation.md` §4.2 line 109: "M2 establishes the trigger; M8 wires the multi-account state machine"). Test checked in as `#[ignore = "pending M8 multi-account state machine"]`. Sub-paths 4 and 7 are testable as soon as M2 lands (single-account boot fires the trigger once with `from: None, to: Some(active)` per the M2 design); the rest needs M8.
    34	
    35	## Why this is one bullet, not several
    36	
    37	The eight sub-paths assert different facets of one observable contract: *after the switch dispatch, every consequence is a derived re-emission, never an imperative reissue.* The kernel-substrate (`kernel-substrate.md` §8) ensures domain-store isolation; the planner (`subscription-compilation/recompilation.md` §4.2) ensures interest re-resolution; the identity machinery (`kernel-substrate.md` §6) ensures signer rebinding. The contract bullet covers all three as one because they are observed together: an app that does `dispatch(SwitchActiveAccount)` and then attempts any operation gets a correctly-rebound system; partial rebinding is a regression.
    38	
    39	## Doctrine alignment
    40	
    41	C12 is the most direct demonstration of cardinal doctrine **D4** ("single writer per fact; caches derive"). The "fact" is `SessionState.active`. The "caches" are every active-account-scoped view, every signer binding, every relay-routing decision. The framework's job is to make sure every cache derives mechanically; the app's job is to write the fact once.
    42	
    43	It also discharges `aim.md` §6 doctrine 7: "Sessions are state, switching is an action. No imperative 'log out, then log in, then reload' dance." That sentence is the contract C12 holds in place.
    44	
    45	## Cross-references
    46	
    47	- `docs/design/subscription-compilation/intro.md` §2.3 — `InterestScope::ActiveAccount` resolution at compile time, not registration time.
    48	- `docs/design/subscription-compilation/recompilation.md` §4.2 trigger A4 — the actor-message shape of the `ActiveAccountChanged` trigger.
    49	- `docs/design/kernel-substrate.md` §8 — module composition rules, specifically domain-store isolation.
    50	- `docs/product-spec/subsystems.md` §7.4 — `SessionState` field shapes.
    51	
    52	## Interaction with C11
    53	
    54	C11 covers *onboarding*: adding an account to `SessionState.accounts`. C12 covers *switching*: changing which account in that list is `active`. The two are independent: an app can onboard without switching, or switch among already-onboarded accounts without onboarding. The framework guarantees both.
    55	
    56	The full sequence (onboard → switch → use) is exercised by C11 sub-path 2(e): create a new identity, switch to it, sign an event. That test crosses both contract bullets and is the canonical end-to-end demonstration.
    57	
    58	## What this chapter does not cover
    59	
    60	- **The login UI itself.** The app provides the button; the contract specifies what the dispatch guarantees.
    61	- **The account-switcher view payload.** That is a view module (`AccountListView` or similar in `nmp-core`'s built-ins per `subsystems.md` §7.4); its spec/payload is owned by the view catalog, not the contract.
    62	- **Background account state** (per-account sync watermarks, per-account action ledger). Those are per-account scopes inside the storage backend; the contract does not specify the scoping mechanism, only that the switch does not leak state across.
    63	- **Logging out / removing an account.** A `RemoveAccount` action exists in the long-term catalog (`subsystems.md` §7.4 implied); its contract surface is a separate potential bullet, not in v1's 13. Removal cleanly through the same `IdentityModule::destroy` path (kernel-substrate.md §6 line 341).

exec
/bin/zsh -lc "nl -ba docs/design/framework-magic/outbox.md | sed -n '1,90p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
     1	# Framework Magic §C6–§C7 — Outbox Routing
     2	
     3	> Parent: `docs/design/framework-magic.md`.
     4	> Read first: `docs/product-spec/subsystems.md` §7.3 (the resolution algorithm — read & write rows); `docs/design/subscription-compilation/outbox.md` (the `PublishPlanner` trait); `docs/design/subscription-compilation/compiler.md` §3 (the read-side compiler pipeline); `docs/design/ndk-applesauce-lessons.md` §9.5 (privacy-sensitive routes fail closed).
     5	
     6	Both bullets in this chapter discharge cardinal doctrine **D3** ("outbox routing is automatic; manual relay selection is the opt-out") and `aim.md` §6 doctrine 5. C7 additionally discharges `aim.md` §6 doctrine 10 ("private events cannot be accidentally republished to public relays") and `product-spec/overview-and-dx.md` §3.3 bug #4.
     7	
     8	## C6. Read fan-out: `authors`-filter subscriptions go to those authors' write relays, de-duplicated
     9	
    10	**Statement.** Any subscription whose canonical filter has a non-empty `authors` set is compiled into one wire REQ per relay in the **union of those authors' write relays** (kind:10002), with each per-relay REQ carrying only the authors that declared that relay. Authors with unknown mailboxes are routed to the configured indexer set as fallback; once their kind:10002 lands, the planner recompiles and the authors migrate to their declared relays.
    11	
    12	**Framework does:** the compilation pipeline at `docs/design/subscription-compilation/compiler.md` §3 — Stages 1 (resolve mailboxes), 2 (assign per-relay author subsets), 3 (merge sub-shapes), 4 (emit per-relay REQs). The indexer-fallback path is `RoutingSource::Indexer`; the post-NIP-65-arrival migration is `Trigger::Nip65Arrived` per `docs/design/subscription-compilation/recompilation.md` §4.2. The mailbox cache is read from the `MailboxCache` trait defined in `nmp-nip65` (`docs/design/subscription-compilation/nip65.md`).
    13	
    14	**App writes:** nothing. The view spec names authors (or, for follow-derived views, names nothing and the view module reads the active account's follow-set — see C5). The app never names a relay URL on a read path.
    15	
    16	**Failure mode prevented:** the bug `ndk-applesauce-lessons.md` §3 names: *"NDK's convenience can blur boundaries"* combined with the bug `product-spec/subsystems.md` §7.3 lines 89–90 names: *"Posts to relays the author hasn't declared as write relays."* On the read side, the symmetric failure is reading from the global content relay and missing an author's actual events because the author publishes only to their own write relay. The structural enforcement is that the view spec has no relay field; the only API surface that names a relay is the explicit override (named, audited, one-shot per `docs/design/subscription-compilation/outbox.md` §7.4).
    17	
    18	**Test:** `c6_authors_subscription_routes_to_per_author_write_relays`. This test is a **rename of and dependency on** the M2 audit gate test `timeline_compiles_to_per_relay_union` (`docs/design/subscription-compilation/tests.md` §9.2 assertion 2). The framework-magic version asserts the same observable but accesses the data through the **public view path**, not the planner harness:
    19	
    20	1. Pre-seed mailbox cache with 1000 authors using three overlapping relay sets (per the M2 test).
    21	2. Open `TimelineView { authors: <1000 pubkeys>, kinds: [1, 6] }` through the actor's public dispatch surface.
    22	3. Read the wire-emission audit log (exposed via `DebugDiagnostics`) and assert: relay count = union; per-relay author partition = subset semantics; sub-shape merge = one REQ per relay; plan-id stable on re-compile.
    23	4. Ingest a new kind:10002 for one author moving them off relay-1 onto relay-4; assert exactly one CLOSE-and-REQ pair fires for the affected slice; no churn for the unmoved authors.
    24	5. `TBD-from-research(ndk/kind3-auto-tracking.md)`: cross-check that the in-flight REQ for the moved author rebinds without losing the live tail across the CLOSE/REOPEN boundary.
    25	
    26	The "via the public view path" framing matters: M2's test exercises the compiler directly; the framework-magic test exercises the contract surface (open a view, watch the wire). Both must pass.
    27	
    28	**Milestone owner:** **[PENDING M2]**. Test checked in as `#[ignore = "pending M2 compiler + view bridge"]`. Removed in the M2 framework-magic delta.
    29	
    30	## C7. Write fan-out: outbox + recipient-inbox; private events fail closed
    31	
    32	**Statement.** Every publish action's signed event is routed by the `PublishPlanner` (`docs/design/subscription-compilation/outbox.md` §7.1) according to a `PublishPrivacy` mode the action declares. **Public** events go to author write relays. **PublicWithNotifications** events go to author writes ∪ recipient inboxes (`#p` tagged pubkeys). **PrivateToRecipients** events (gift-wrapped per NIP-59) go to **only** resolved recipient inbox relays — never the author's writes, never the active session's defaults, never the indexer set. If any recipient has no declared inbox, the publish fails closed with `PublishPlanError::PrivateRecipientUnroutable`.
    33	
    34	**Framework does:** the algorithm at `docs/design/subscription-compilation/outbox.md` §7.3 (write fan-out, all 6 numbered steps), specifically:
    35	
    36	- Step 2 forbids indexer fallback for any write path (`NoAuthorRelays` returned instead).
    37	- Step 3(b)'s `Indexer` check on recipient inbox lookups is the structural fail-closed for private events.
    38	- The `PublishWithOverride` action is the *only* `AppAction` variant carrying a `Vec<RelayUrl>` field, and it is forbidden from widening a `PrivateToRecipients` plan to public relays (`outbox.md` §7.4 rule 4).
    39	
    40	**App writes:** nothing — for the publish path. The app dispatches a publish action (`SendNote`, `React`, `SendDm`, etc.); the action's privacy mode is determined by the action type, not by an app-supplied parameter. There is no `relays` field on `SendNote`. The override exists for tests, migrations, and operator power-user flows; it is structurally outside the safe app path.
    41	
    42	**Failure mode prevented:** §3.3 bug #3 ("Publish of an event to relays the author has not declared as write relays") and bug #4 ("DM published to public relays"). Plus the doctrine-10 footgun: a "send everywhere" fallback that publishes a gift wrap to the global content relay because the recipient's inbox lookup returned empty.
    43	
    44	**Test:** `c7_publish_routes_outbox_and_private_fails_closed`. The test has three sub-paths:
    45	
    46	1. **Public:** seed Alice's mailbox with two write relays; dispatch a public `SendNote` action; assert the resulting publish plan has exactly those two relays and no others, and that `required_success_count = max(1, ceil(2/3)) = 1` per `outbox.md` §7.3 step 3(a).
    47	2. **PublicWithNotifications:** dispatch a note tagging Bob (Bob has one inbox relay seeded); assert the plan is Alice's writes ∪ Bob's inbox, with the correct `PublishRouteReason::AuthorWriteRelay` / `RecipientInbox` tagging per assignment.
    48	3. **PrivateToRecipients (fail-closed):** dispatch a (post-M9, but the planner shape is testable in isolation today) gift-wrap to Charlie, who has **no kind:10002**. Assert the publish plan errors with `PublishPlanError::PrivateRecipientUnroutable { recipient: charlie }` and that **no wire EVENT frame is emitted on any relay** — checked by reading the relay worker's outbound audit log.
    49	4. **Override rejection:** dispatch a `PublishWithOverride` carrying a `PrivateToRecipients` inner action and an override relay set that includes a non-inbox URL; assert it rejects with `PublishPlanError::OverrideRejected { reason: "private widen" }` (rule 4 of `outbox.md` §7.4).
    50	5. **Override audit:** dispatch a `PublishWithOverride` on a public action; assert the side-effect lane emits `Diagnostic::PublishOverrideUsed { ... }` and the debug log line per `outbox.md` §7.4 (3).
    51	
    52	**Milestone owner:** **[PENDING M2 seam → M6 publish]**. M2 lands the `PublishPlanner` trait + `Nip65PublishPlanner` + the `PublishWithOverride` action (`docs/design/subscription-compilation/outbox.md` §7.1, §7.2, §7.4). M6 lands `SendNoteAction` as the first concrete consumer. Test checked in as `#[ignore = "pending M2 planner + M6 first consumer"]`. Sub-paths 3 and 4 of the test exercise the planner in isolation (M2-completable); 1, 2, and 5 require M6's action consumer.
    53	
    54	## The two bullets together discharge D3
    55	
    56	C6 covers the read side; C7 covers the write side. Together they discharge cardinal doctrine D3 in full — every relay-touching operation routes through framework policy, and the only API surfaces that name a relay URL are:
    57	
    58	- the `PublishWithOverride` action (write path, audited);
    59	- the planner's diagnostic accessors (read-only);
    60	- the user-configured-relays settings surface (configuration, not per-operation).
    61	
    62	The app's domain code, view modules, and action modules **never** name a relay. That is the doctrine-D3 boundary the contract holds in place.
    63	
    64	## What this chapter does not cover
    65	
    66	- The publish-fail retry/back-off policy — that's M6 territory (`docs/design/subscription-compilation/outbox.md` §7.6 deferred items). The contract's fail-closed guarantee is structural (the wire frame is never emitted), not about how long the system retries before giving up.
    67	- The action ledger row schema — `docs/design/kernel-substrate.md` §4 owns it. C7 cares that the ledger correlates the per-relay attempts; the contract does not specify the row layout.
    68	- NIP-77 sync routing — that's C10 in `sync.md`. Sync and live REQ should share relay policy (`ndk-applesauce-lessons.md` §6 last paragraph), but the symmetric assertion lives with C10.
    69	- NIP-42 auth-paused publishes — M5. The override action does not unblock an auth-paused relay; auth pause is a wire-emitter gate, not a planner decision (`docs/design/subscription-compilation/recompilation.md` §4.2 trigger A9 open question).

exec
/bin/zsh -lc "nl -ba docs/product-spec/overview-and-dx.md | sed -n '15,45p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
    15	## 1. Product summary
    16	
    17	A Cargo workspace shipping a Nostr-native **app kernel** (`nmp-core`), reusable **Nostr protocol modules** (`nmp-nip01`, `nmp-nip17`, `nmp-nip65`, etc.), app-owned extension modules, a codegen tool (`nmp gen modules`) that produces per-app concrete FFI enums/wrappers, FFI bindings for Swift/Kotlin/TypeScript, a wasm target, a scaffolding CLI, and reference platform shells.
    18	
    19	The kernel composes the `rust-nostr` crate family plus OS capability crates into a substrate. It owns actor runtime, verified event store, subscription planner, relay routing pipeline, signer/session plumbing, durable action ledger, domain-store substrate, typed view registry, capability bridge, platform shadow/codegen machinery, diagnostics, and test harnesses.
    20	
    21	The kernel does **not** own Profile, Timeline, Thread, Reactions, Conversation, Wallet, DM, Blossom, or app-specific domain concepts. Those live in reusable protocol modules or app crates. Platform code renders state and dispatches user intents — nothing else.
    22	
    23	The framework treats common Nostr-correctness failures (stale replaceable events, lost subscriptions, mis-routed publishes, double-publication, multi-account desync, leaked secrets across FFI, naive cache invalidation, withheld cached data, blocking-on-fetch UI patterns) as **product defects in the framework** rather than as developer mistakes. The public API is designed so that the wrong thing is hard to type.
    24	
    25	---
    26	
    27	## 1.5 Cardinal doctrines
    28	
    29	Six named principles that subsume the rest of this spec. Every API decision answers to at least one of these; conflicts between them resolve in the order listed.
    30	
    31	### D0. Kernel + extension modules — no app nouns in `nmp-core`
    32	
    33	Per ADR-0009, NMP is a Nostr-native app kernel plus extension modules. The kernel provides substrate; protocol modules and app modules contribute typed variants via `ViewModule`, `ActionModule`, `DomainModule`, `CapabilityModule`, and `IdentityModule`. If implementing a real app requires adding domain nouns to `nmp-core`, the kernel boundary is wrong and must change.
    34	
    35	This rules out:
    36	
    37	- `nmp-core` becoming a junk drawer of every consumer's domain concepts.
    38	- App-specific business logic in Swift, Kotlin, or TypeScript shells.
    39	- Closed FFI enums that prevent modules from contributing typed views, actions, updates, capabilities, or identity scopes.
    40	
    41	### D1. Best-effort rendering — render now, refine in place
    42	
    43	Apps built with this framework **never withhold cached data and never block on fetches**. Every view payload field carries a value, not a "loading" status. Missing display names default to a shortened npub; missing pictures default to a deterministic identicon URI; missing timestamps default to "now". When a more authoritative value (e.g., the author's kind:0) arrives later, the view payload updates in place and the affected cell re-renders. The UI never sees a spinner gating already-renderable content.
    44	
    45	The doctrine is enforced by the view payload **types**: display fields are non-`Option`, placeholders are part of the type contract, and freshness is exposed (when relevant) as an optional badge hint, not a render gate. There is no `if has_profile { render } else { spinner }` pattern available in the API — the framework does not provide one.

exec
/bin/zsh -lc 'rg -n "Timeline|Profile|FollowingTimeline|Reaction|SendNote|SendDm|AccountList|Conversation|Wallet|Podcast|Highlighter|Room|Highlight|Episode" crates/nmp-core/src crates/nmp-core/tests crates/nmp-core/Cargo.toml' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
crates/nmp-core/src/ffi.rs:211:    let _ = app.tx.send(ActorCommand::ClaimProfile {
crates/nmp-core/src/ffi.rs:236:    let _ = app.tx.send(ActorCommand::ReleaseProfile {
crates/nmp-core/src/kernel/status.rs:43:            key: format!("Profile({})", short_hex(TEST_PUBKEY)),
crates/nmp-core/src/kernel/status.rs:57:            key: "SeedTimeline(fiatjaf,jb55,pablof7z)".to_string(),
crates/nmp-core/src/kernel/status.rs:113:                    "UIProfileClaims({claim_count} components / {} pubkeys)",
crates/nmp-core/src/kernel/status.rs:138:                key: format!("AuthorProfile({})", short_hex(pubkey)),
crates/nmp-core/src/kernel/mod.rs:58:struct Profile {
crates/nmp-core/src/kernel/mod.rs:70:struct TimelineItem {
crates/nmp-core/src/kernel/mod.rs:85:struct ProfileCard {
crates/nmp-core/src/kernel/mod.rs:101:    profile: ProfileCard,
crates/nmp-core/src/kernel/mod.rs:102:    items: Vec<TimelineItem>,
crates/nmp-core/src/kernel/mod.rs:111:    items: Vec<TimelineItem>,
crates/nmp-core/src/kernel/mod.rs:207:    profile: ProfileCard,
crates/nmp-core/src/kernel/mod.rs:208:    items: Vec<TimelineItem>,
crates/nmp-core/src/kernel/mod.rs:211:    inserted: Vec<TimelineItem>,
crates/nmp-core/src/kernel/mod.rs:212:    updated: Vec<TimelineItem>,
crates/nmp-core/src/kernel/mod.rs:293:    profiles: HashMap<String, Profile>,
crates/nmp-core/src/kernel/mod.rs:322:    last_emitted_items: Vec<TimelineItem>,
crates/nmp-core/src/kernel/update.rs:121:    pub(super) fn visible_items(&self) -> Vec<TimelineItem> {
crates/nmp-core/src/kernel/update.rs:130:    pub(super) fn timeline_item(&self, event: &StoredEvent) -> TimelineItem {
crates/nmp-core/src/kernel/update.rs:132:        TimelineItem {
crates/nmp-core/src/kernel/update.rs:162:    pub(super) fn profile_card(&self) -> ProfileCard {
crates/nmp-core/src/kernel/update.rs:175:    ) -> ProfileCard {
crates/nmp-core/src/kernel/update.rs:177:        ProfileCard {
crates/nmp-core/src/kernel/update.rs:225:    pub(super) fn author_items(&self, pubkey: &str) -> Vec<TimelineItem> {
crates/nmp-core/src/kernel/update.rs:273:    pub(super) fn thread_items(&self, focused_id: &str, root_id: &str) -> Vec<TimelineItem> {
crates/nmp-core/src/kernel/nostr.rs:14:pub(super) struct ProfileContent {
crates/nmp-core/src/kernel/nostr.rs:24:pub(super) fn parse_profile(event: &NostrEvent) -> Profile {
crates/nmp-core/src/kernel/nostr.rs:25:    let parsed = serde_json::from_str::<ProfileContent>(&event.content).unwrap_or_default();
crates/nmp-core/src/kernel/nostr.rs:32:    Profile {
crates/nmp-core/src/kernel/nostr.rs:45:    previous: &[TimelineItem],
crates/nmp-core/src/kernel/nostr.rs:46:    current: &[TimelineItem],
crates/nmp-core/src/kernel/nostr.rs:47:) -> (Vec<TimelineItem>, Vec<TimelineItem>, Vec<String>) {
crates/nmp-core/src/actor.rs:17:    ClaimProfile { pubkey: String, consumer_id: String },
crates/nmp-core/src/actor.rs:18:    ReleaseProfile { pubkey: String, consumer_id: String },
crates/nmp-core/src/actor.rs:110:                    ActorCommand::ClaimProfile {
crates/nmp-core/src/actor.rs:118:                    ActorCommand::ReleaseProfile {

exec
/bin/zsh -lc "nl -ba docs/design/view-catalog/template-and-enumeration.md | sed -n '1,110p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
exec
/bin/zsh -lc "nl -ba docs/product-spec/api-surface.md | sed -n '1,140p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
     1	# View Catalog: Template And Enumeration
     2	
     3	[Back to Design: View Catalog](../view-catalog.md)
     4	
     5	# Design: View Catalog
     6	
     7	> **Audience:** Framework contributors building reference Nostr protocol modules. Each view kind below is a `ViewModule` shipped in a reusable protocol crate such as `nmp-nip01`, `nmp-nip10`, `nmp-nip25`, `nmp-nip65`, or `nmp-nip17`.
     8	
     9	> **Status:** Rev 2, reframed per ADR-0009. These view kinds are not in `nmp-core`; apps consume them by adding the owning module crate to `nmp.toml` and regenerating the per-app FFI crate.
    10	
    11	> **Prerequisites:** `product-spec.md` §7.6, `reactivity.md`, `kernel-substrate.md` §3, ADR-0005, ADR-0010.
    12	
    13	---
    14	
    15	## 1. Per-view-kind template
    16	
    17	Every reference Nostr view module lives in a `nmp-nip*` crate and implements `ViewModule` from `nmp-core::substrate`:
    18	
    19	```
    20	crates/nmp-<protocol>/src/views/<kind>.rs
    21	```
    22	
    23	with this public shape:
    24	
    25	```rust
    26	pub struct <Kind>Module;
    27	
    28	impl ViewModule for <Kind>Module {
    29	    const NAMESPACE: &'static str = "nipXX.<kind>";
    30	
    31	    type Spec = <Kind>Spec;
    32	    type Payload = <Kind>View;
    33	    type Delta = <Kind>Delta;
    34	    type Key = <Kind>Key;
    35	    type State = <Kind>State;
    36	
    37	    fn key(spec: &Self::Spec) -> Self::Key;
    38	    fn dependencies(spec: &Self::Spec) -> ViewDependencies;
    39	    fn open(ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload);
    40	    fn on_event_inserted(ctx: &ViewContext, state: &mut Self::State, event: &KernelEvent)
    41	        -> Option<Self::Delta>;
    42	    fn on_event_removed(ctx: &ViewContext, state: &mut Self::State, id: &EventId)
    43	        -> Option<Self::Delta>;
    44	    fn on_event_replaced(
    45	        ctx: &ViewContext,
    46	        state: &mut Self::State,
    47	        old_id: &EventId,
    48	        new_event: &KernelEvent,
    49	    ) -> Option<Self::Delta>;
    50	    fn on_projection_changed(ctx: &ViewContext, state: &mut Self::State, change: &ProjectionChange)
    51	        -> Option<Self::Delta>;
    52	    fn snapshot(ctx: &ViewContext, state: &Self::State) -> Self::Payload;
    53	}
    54	```
    55	
    56	For each kind below, the catalog documents spec, payload, delta variants, dependencies, recompute strategy, pagination, best-effort placeholders, and subtleties learned from Applesauce/NDK-style clients.
    57	
    58	---
    59	
    60	## 1.1 Platform cache key
    61	
    62	The generated platform wrapper organizes the shadow as typed domain-keyed dictionaries, not as a flat `[ViewId: ViewPayload]` map.
    63	
    64	| View kind | Platform cache key | Wrapper API |
    65	|---|---|---|
    66	| Profile | pubkey | `useProfile(pubkey)` / `@Profile` |
    67	| Contacts | pubkey | `useContacts(pubkey)` |
    68	| Mailboxes | pubkey | `useMailboxes(pubkey)` |
    69	| Mutes | active account pubkey | `useMutes()` |
    70	| Blossom servers | pubkey | `useBlossomServers(pubkey)` |
    71	| Timeline | spec hash | `useTimeline(spec)` |
    72	| Thread | root event id | `useThread(rootEventId)` |
    73	| Replies | target event id | `useReplies(targetEventId)` |
    74	| Reactions | target event coord | `useReactions(target)` |
    75	| Conversation list | active account pubkey | `useConversationList()` |
    76	| Conversation | peer pubkey or group id | `useConversation(peer)` |
    77	| Zap history | active account pubkey | `useZapHistory()` |
    78	| Wallet balance | wallet id | `useWallet()` |
    79	| WoT rank | pubkey | `useWotRank(pubkey)` |
    80	| Search | spec hash | `useSearch(query)` |
    81	
    82	`ViewId` is an internal FFI token. Wrappers refcount per key, dispatch `OpenView`/`CloseView` to Rust, and enforce the same warmth/eviction policy as the kernel.
    83	
    84	## 2. View kinds
    85	
    86	| # | Kind | Protocol module | Detailed in this doc? | Phase |
    87	|---|---|---|---|---|
    88	| 1 | Profile | `nmp-nip01` | yes | 1a.2 |
    89	| 2 | Contacts | `nmp-nip02` | stub | 1a.4 |
    90	| 3 | Mailboxes | `nmp-nip65` | stub | 1a.4 |
    91	| 4 | Mutes | `nmp-nip01` | stub | post-1a |
    92	| 5 | Blossom servers | `nmp-blossom` | stub | post-1a |
    93	| 6 | Timeline | `nmp-nip01` | yes | 1a.4 |
    94	| 7 | Thread | `nmp-nip10` | yes | 1a.6 |
    95	| 8 | Replies | `nmp-nip10` | stub | 1a.6 |
    96	| 9 | Reactions | `nmp-nip25` | yes | 1a.6 |
    97	| 10 | Conversation list | `nmp-nip17` | stub | post-1a |
    98	| 11 | Conversation | `nmp-nip17` | yes | post-1a |
    99	| 12 | Zap history | `nmp-nip57` | stub | post-1a |
   100	| 13 | Wallet balance | `nmp-nwc` or `nmp-nip60` | stub | post-1a |
   101	| 14 | WoT rank | `nmp-wot` | stub | post-1a |
   102	| 15 | Search | TBD utility module | stub | post-1a |

 succeeded in 0ms:
     1	# Product Spec: API Surface
     2	
     3	[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)
     4	
     5	## 6. The framework API surface
     6	
     7	This section specifies what the developer sees. Implementation lives behind it.
     8	
     9	The concrete FFI API is per-app generated. `nmp-core` defines kernel primitives and extension traits; `nmp gen modules` composes the selected kernel, protocol modules, and app modules into a generated `nmp-app-<name>` crate that exposes closed typed enums to Swift/Kotlin/TypeScript.
    10	
    11	### 6.1 The App handle
    12	
    13	`FfiApp` (Swift/Kotlin) / `NmpApp` (TS) is the single object created at startup. Per RMP bible, it is a `uniffi::Object` constructed once per process.
    14	
    15	```rust
    16	#[derive(uniffi::Object)]
    17	pub struct FfiApp { /* opaque */ }
    18	
    19	#[uniffi::export]
    20	impl FfiApp {
    21	    /// Construct the app. Spawns the actor thread. Loads persisted sessions.
    22	    /// `config` carries data directory, default relays, storage backend choice,
    23	    /// feature flags. Infallible at the FFI boundary; catastrophic failure panics.
    24	    #[uniffi::constructor]
    25	    pub fn new(config: AppConfig) -> Arc<Self>;
    26	
    27	    /// Snapshot of current state. Cheap clone.
    28	    pub fn state(&self) -> AppState;
    29	
    30	    /// Fire-and-forget action dispatch. Never blocks, never returns a Result.
    31	    /// Results land as state changes.
    32	    pub fn dispatch(&self, action: AppAction);
    33	
    34	    /// Start the update listener. Must be called exactly once per process.
    35	    /// The reconciler is invoked from a background thread; native must hop.
    36	    pub fn listen_for_updates(&self, reconciler: Arc<dyn AppReconciler>);
    37	
    38	    /// Register platform capabilities. Each setter is idempotent and safe to
    39	    /// call multiple times. See §6.5.
    40	    pub fn set_keyring(&self, keyring: Arc<dyn KeyringCapability>);
    41	    pub fn set_push(&self, push: Arc<dyn PushCapability>);
    42	    pub fn set_external_signer(&self, signer: Arc<dyn ExternalSignerCapability>);
    43	    pub fn set_network_monitor(&self, mon: Arc<dyn NetworkMonitorCapability>);
    44	    pub fn set_blob_picker(&self, picker: Arc<dyn BlobPickerCapability>);
    45	}
    46	```
    47	
    48	`AppConfig` is a `uniffi::Record` containing only platform-resolved primitives (paths, lists of relay URLs, feature-flag booleans). No `Arc<dyn ...>` types in the config — capabilities are registered separately via setters so each can be bridged on its own schedule.
    49	
    50	### 6.2 AppState
    51	
    52	`AppState` is a `uniffi::Record`. It is the entire UI's source of truth. It is cloned across FFI on every `FullState` update.
    53	
    54	Top-level shape (long-term, illustrative; v1 contains the kernel fields needed by [`docs/plan.md`](../plan.md) and keeps product subsystems absent or empty):
    55	
    56	```rust
    57	#[derive(Clone, uniffi::Record)]
    58	pub struct AppState {
    59	    pub rev: u64,
    60	    pub router: Router,
    61	    pub session: SessionState,
    62	    pub store_summary: StoreSummary,        // counts, last sync, prune stats
    63	    pub views: ViewSnapshots,               // snapshot of all open view payloads
    64	    pub conversations: ConversationsState,
    65	    pub wallet: WalletState,
    66	    pub media: MediaState,
    67	    pub wot: WotState,
    68	    pub sync: SyncState,
    69	    pub outbox: OutboxState,
    70	    pub busy: BusyFlags,
    71	    pub toast: Option<Toast>,
    72	    pub debug: Option<DebugDiagnostics>,     // Some(_) only in debug builds
    73	}
    74	```
    75	
    76	`AppState` does **not** include the entire event store contents. Events are reached through `ViewSnapshots` which carry only the events relevant to currently-open views. This is the v1 resolution of `aim.md` §7.1: full state snapshots, but the "state" is a projection of open views — bounded by what the UI is showing.
    77	
    78	**Platform shadow is reorganized by domain key, not `ViewId` (ADR-0005).** While the FFI delivers `AppState.views` as a `HashMap<ViewId, ViewPayload>`, the per-platform wrapper layer (generated by `nmp gen`) reorganizes the shadow into typed domain-keyed dictionaries — `profiles: [PubKey: ProfileView]`, `reactionSummaries: [EventId: ReactionSummary]`, `conversations: [PubKey: ConversationView]`, etc. — so components read by domain concept (pubkey, event id) rather than by framework handle. `ViewId` remains an internal token used by the FFI; component code never sees it. Refcounted wrappers (`useProfile`, `@Profile`, `rememberProfile`) manage subscription lifecycle behind the domain-keyed API. See ADR-0005 for the per-view-kind cache-key table.
    79	
    80	### 6.3 AppAction
    81	
    82	`AppAction` is a generated per-app `uniffi::Enum`, not a closed enum in `nmp-core`. The generated enum composes kernel variants, selected Nostr protocol module variants, and app-specific module variants:
    83	
    84	```rust
    85	pub enum AppAction {
    86	    Kernel(nmp_core::KernelAction),
    87	    Nip01(nmp_nip01::Action),
    88	    Nip10(nmp_nip10::Action),
    89	    Twitter(twitter_core::Action),
    90	}
    91	```
    92	
    93	The long-term action catalog below is illustrative. Each item belongs in the relevant module crate, not in `nmp-core`.
    94	
    95	```rust
    96	#[derive(Clone, uniffi::Enum)]
    97	pub enum AppAction {
    98	    // Lifecycle
    99	    Bootstrap,
   100	    Foreground,
   101	    Background,
   102	    NetworkChanged { online: bool },
   103	
   104	    // Sessions
   105	    AddAccountPrivateKey { nsec_or_ncryptsec: String, passphrase: Option<String> },
   106	    AddAccountBunker { connect_uri: String },
   107	    AddAccountExternal { kind: ExternalSignerKind },
   108	    ActivateAccount { pubkey: String },
   109	    RemoveAccount { pubkey: String, wipe: bool },
   110	
   111	    // Routing
   112	    Navigate { screen: Screen },
   113	    Pop,
   114	    PopToRoot,
   115	
   116	    // Views
   117	    OpenView { id: ViewId, spec: ViewSpec },
   118	    CloseView { id: ViewId },
   119	    RefreshView { id: ViewId },
   120	
   121	    // Writes (delegated to nmp-actions)
   122	    SendNote { content: String, mentions: Vec<String>, reply_to: Option<EventCoord> },
   123	    React { target: EventCoord, emoji: String },
   124	    Repost { target: EventCoord },
   125	    Quote { target: EventCoord, comment: String },
   126	    FollowUser { pubkey: String },
   127	    UnfollowUser { pubkey: String },
   128	    MuteUser { pubkey: String },
   129	    UpdateProfile { patch: ProfilePatch },
   130	    PublishLongForm { article: ArticleDraft },
   131	    SendDm { recipient: String, body: String, attachments: Vec<BlobRef> },
   132	    OpenConversation { peer: String },
   133	    MarkConversationRead { peer: String, up_to: u64 },
   134	
   135	    // Wallet
   136	    AttachWallet { config: WalletConfig },
   137	    DetachWallet,
   138	    Zap { target: ZapTarget, sats: u64, comment: String },
   139	    AcceptNutzap { id: String },
   140	

exec
/bin/zsh -lc "nl -ba docs/design/framework-magic/sync.md | sed -n '50,65p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
    50	C9 is **what** the store remembers about each event's provenance. C10 is **what** the store remembers about each `(filter, relay)` pair's coverage. Together they answer the question `ndk-applesauce-lessons.md` §9.8 raises: *"Having an event in the local store does not prove that a view is complete."* C9 is "we have this event"; C10 is "we have everything matching this filter from this relay up to this timestamp." The framework needs both to render correctly without fetching needlessly.
    51	
    52	The two are paired in one chapter rather than split because their tests share the mock-relay-with-capability harness and because their failure modes intersect (a redelivered event from a new relay updates both the provenance set and the watermark, per `subsystems.md` §7.1 line 101 "Watermarks intersect with outbox").
    53	
    54	## Cross-references
    55	
    56	- `docs/design/lmdb/watermarks.md` for the storage schema and the 32-distinct-relay cap.
    57	- `docs/design/subscription-compilation/compiler.md` Stage X for the planner's watermark consultation in the compile pipeline. (TBD: confirm Stage number in research-fold; the compiler file specifies it.)
    58	- The "shared relay policy between sync and live REQ" lesson from `ndk-applesauce-lessons.md` §6 last paragraph is implicit in the per-relay watermark — both engines key by the same `(filter_sig, relay_url)` pair, so they cannot disagree on the relay universe.
    59	
    60	`TBD-from-research(applesauce/event-store-query-builders.md)`: cite Applesauce's coverage/watermark equivalent and the API by which a query-builder reads it. NMP's `WatermarksSummary` (`subsystems.md` §7.8 line 287) is the analogous app-visible surface; the research-fold commit verifies the surface covers the same diagnostic needs.
    61	
    62	## What this chapter does not cover
    63	
    64	- The action-ledger row schema for a manual `RunSync` action (`subsystems.md` §7.8 line 268 `SyncSpec`) — that's an actions-catalog concern owned by §7.5.
    65	- The proof-app sync overlay rendering — `subsystems.md` §4.5 owns the proof app.

exec
/bin/zsh -lc "nl -ba docs/design/framework-magic/replaceable.md | sed -n '1,100p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
exec
/bin/zsh -lc "nl -ba docs/design/framework-magic/subs.md | sed -n '1,55p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
     1	# Framework Magic §C8 — Subscription Planner Hygiene
     2	
     3	> Parent: `docs/design/framework-magic.md`.
     4	> Read first: `docs/product-spec/subsystems.md` §7.2 (subscription planner behaviors); `docs/design/subscription-compilation/compiler.md` (compilation pipeline); `docs/design/reactivity/scheduling-and-data-model.md` (buffer / batch policy); `docs/design/firehose-bench.md` (the modeled-perf companion benchmark for the ≤60Hz budget).
     5	
     6	## C8. Subscriptions auto-dedup, auto-coalesce, auto-close, and auto-buffer
     7	
     8	**Statement.** The framework guarantees four properties on every wire subscription it issues:
     9	
    10	1. **Dedup.** Two logical interests with the same canonical filter share one wire REQ per relay; each logical consumer still receives only events matching its own filter.
    11	2. **Coalesce / merge.** Logical interests with structurally compatible filters (per the merge lattice in `subsystems.md` §7.2) merge into one broader REQ per relay; each consumer is filtered locally from the broader stream.
    12	3. **Auto-close.** A wire REQ with no remaining logical consumers is CLOSE'd. One-shot interests (those without a live tail, only an `until` upper bound) are CLOSE'd on EOSE.
    13	4. **Buffered batching.** Inbound events for one view are batched into a single `ViewBatch` per actor tick at ≤60Hz; backpressure drops batches in favor of a single `FullState` catch-up. The platform's reactive primitive sees one re-render per tick, not per event.
    14	
    15	**Framework does:** the subscription-compilation pipeline (`docs/design/subscription-compilation/compiler.md`) for dedup and coalesce; the wire-emitter's diff (compiler §3 final stage) for auto-close on plan changes; the view registry's refcount drop for auto-close on consumer loss; `docs/design/reactivity/scheduling-and-data-model.md` for the per-tick batching; the FullState backpressure fallback at `subsystems.md` §7.2 line 69. The hard cap of 60Hz is the budget in `subsystems.md` §7.16 table row "ViewBatch frequency under hashtag firehose".
    16	
    17	**App writes:** nothing. The app opens views; it does not name a REQ. The reactivity scheduling is invisible — the platform's `useTimeline()` rune/observable emits at the framework's batched cadence regardless of relay throughput.
    18	
    19	**Failure mode prevented:** the entire class of subscription-management bugs in `product-spec/overview-and-dx.md` §3.3 numbers 2 ("Subscription leaked after its UI is destroyed") and 8 ("Two concurrent UI subscriptions for the same filter producing two relay REQs"). Plus the hand-rolled grouping-window + dedup-LRU pattern that `ndk-applesauce-lessons.md` §7 calls out as the work clients typically do manually.
    20	
    21	**Test:** `c8_subscriptions_coalesce_autoclose_and_buffer`. The test has four sub-paths in one `#[test] fn`:
    22	
    23	1. **Dedup:** open two `TimelineView`s with identical filters; assert the planner produces one wire REQ per relay (not two); destroy one; assert the wire REQ stays alive; destroy the second; assert the REQ is CLOSE'd after the warmth grace expires (`subsystems.md` §7.6 line 226: 30s default).
    24	2. **Coalesce:** open `TimelineView { authors: [A, B], kinds: [1] }` and `ProfileView { pubkey: C }`; assert the planner merges into one REQ per relay containing the union shape, with each view receiving only its filtered subset locally (no REQ for kind:0 alone if the relay already has the merged stream covering it). The merge lattice's exact rules live in `subsystems.md` §7.2 line 65 and `docs/design/subscription-compilation/intro.md` §1 open-question #2 (lattice formalization); the test asserts the *observable* (wire frame count = correct fewer-than-naive, payload coverage = correct) rather than the lattice mechanics.
    25	3. **Auto-close on EOSE for one-shot:** open `ProfileClaim { pubkey: D }` (which `docs/design/subscription-compilation/intro.md` §2.2 line 112 specifies as `lifecycle: OneShot, limit: 1`); the mock relay sends the kind:0 then EOSE; assert the planner CLOSEs the wire REQ within one tick of EOSE; assert no further REQs touch that relay for that filter.
    26	4. **Buffered batching under firehose:** the mock relay sends 600 events for one filter in 1 second (10× the budget); assert the platform reconciler observes ≤60 `ViewBatch` emissions in the window; assert no events are dropped from the underlying store (only the *render emission rate* is capped, not the ingestion); assert the actor queue depth stays below `subsystems.md` §7.16 budget (steady-state < 16).
    27	
    28	**Milestone owner:** **[PENDING M2]** for sub-paths 1–3 (the compiler + lifecycle); **partial overlap with reactivity-bench** for sub-path 4 (the buffer cadence is exercised by `docs/perf/reactivity-bench/` already; the contract test asserts the same property through the public view path). Test checked in as `#[ignore = "pending M2 compiler"]` initially.
    29	
    30	## Why this is one bullet, not four
    31	
    32	The four properties (dedup / coalesce / close / batch) are observable as one contract from the app's perspective: *the app opens N views, the framework opens ≤N REQs, the framework closes them at the right moment, the framework caps emit cadence.* Splitting into four bullets would suggest the app might experience them separately; it does not. The four sub-paths of the test are the four conditions the single contract bullet asserts.
    33	
    34	The reason this is C8 and not bundled with C6/C7 is that C6/C7 govern *which relay* a REQ targets; C8 governs *how many REQs and at what cadence* regardless of the relay. Different doctrines (D3 vs D5+aim §6 doctrine 6) and different milestone responsibility.
    35	
    36	## Cross-references to the existing test surface
    37	
    38	- `docs/design/subscription-compilation/tests.md` §9.2 assertion 2 already asserts the per-relay author partition + sub-shape merge (the coalesce property at the planner layer). The framework-magic version of sub-path 2 reuses that mailbox cache setup but reads the wire output through the platform shadow's audit log instead of through the planner harness.
    39	- `docs/design/firehose-bench.md` is the modeled-perf companion: it asserts ≤60Hz holds under sustained load. The framework-magic sub-path 4 asserts the *correctness* of the cap (no event loss); the bench asserts the *budget* under realistic load.
    40	- `docs/design/reactivity/validation-harness.md` covers reactive-primitive validation (Swift `@Observable`, Kotlin `Flow`, etc.). C8's sub-path 4 cross-validates that the platform-side emissions match the actor-side `ViewBatch` count.
    41	
    42	## What this chapter does not cover
    43	
    44	- **Reconnect-resumption.** When a relay disconnects and reconnects, the planner re-issues the same wire REQ set (`subsystems.md` §7.2 line 71). That is a planner *resumption* behavior, not a contract bullet — the app sees no surface change. It is covered implicitly by the dedup/close properties (the resumed REQs are the same REQs the planner already tracks).
    45	- **NIP-77 sync vs live REQ split.** C10 in `sync.md` covers the sync side; C8 covers the live tail only.
    46	- **Per-view payload size budgets.** `subsystems.md` §7.16 table rows. The contract guarantees the buffering happens; the budget is an instrumentation concern with its own test surface in `nmp-metrics`.
    47	
    48	`TBD-from-research(applesauce/event-store-query-builders.md)`: cite the Applesauce file:line for the logical-vs-wire subscription split that NMP's compiler mirrors. The cross-validation in the research-fold commit confirms NMP's `LogicalInterest` (`docs/design/subscription-compilation/intro.md` §2.1) covers Applesauce's surface and that no observable property is lost in translation.

 succeeded in 0ms:
     1	# Framework Magic §C1–§C4 — Replaceable & Delete Invariants
     2	
     3	> Parent: `docs/design/framework-magic.md`.
     4	> Read first: `docs/product-spec/subsystems.md` §7.1 (the EventStore insert-time invariants table — this chapter references its rows, does not restate them); `docs/design/lmdb-schema.md` (storage backend for M3); `docs/design/lmdb/tests.md` §3 (kind:30023 d-tag corner cases).
     5	
     6	This chapter holds four bullets, all of which discharge `docs/product-spec/overview-and-dx.md` §3.3 **bug-extinction #1** ("Stale replaceable event retained in state after a newer one arrives") and `docs/aim.md` §6 **doctrine 4** ("replaceable-event invariants enforced on insert"). The four are split because they cover four distinct kind-class shapes and have four distinct test surfaces.
     7	
     8	## C1. Replaceable supersession on insert (kind 0 / 3 / 10000–19999)
     9	
    10	**Statement.** Any kind in `{0, 3, 10000..=19999}` arriving at the event store automatically supersedes the prior event with the same `(pubkey, kind)`; the prior event becomes unreachable through the public read path.
    11	
    12	**Framework does:** the insert-time supersession at `docs/product-spec/subsystems.md` §7.1 row "Replaceable kinds (0, 3, 10000-19999)". Mechanism: compare `(pubkey, kind)` against the existing entry, keep newest `created_at`, tie-break by lexicographically smallest `id`. The current in-memory store enforces this for kind:0 / kind:3 / kind:10002 today (kind:3 via `seed_contacts.insert` at `crates/nmp-core/src/kernel/ingest.rs:206`; kind:10002 via the `should_replace` branch at `crates/nmp-core/src/kernel/ingest.rs:218-222`). M3 graduates the rule into the LMDB-backed `EventStore` trait (`docs/design/lmdb/trait.md`).
    13	
    14	**App writes:** nothing. The app calls `ProfileView::open(pubkey)`; the view's payload reflects the latest kind:0 the store has, with no app-side comparison of `created_at`.
    15	
    16	**Failure mode prevented:** §3.3 bug #1. Plus the doctrine-4 footgun: an app caches kind:3 in its own state, fails to re-fetch on UI nav, renders a stale follow list, double-subscribes on the next session.
    17	
    18	**Test:** `c1_replaceable_supersedes_on_insert`. The test inserts kind:0 #1 at `created_at=T`, then kind:0 #2 at `T+1` with same pubkey; asserts `ProfileView` payload reflects #2 and that a subsequent insert at `T-1` is rejected (no payload re-emit, no event store change). Tie-break path: two inserts at the same `T` with different ids — the lexicographically-smaller-id event wins, deterministic across runs.
    19	
    20	**Milestone owner:** **[DONE]** for in-memory kernel (verified by `crates/nmp-core` kernel tests today, ref the existing `should_replace` branch). Test runs **not** ignored from day one; LMDB graduation in M3 must preserve the same observable, so the test stays green across M3.
    21	
    22	---
    23	
    24	## C2. Parameterized replaceable supersession (kind 30000–39999) by `(pubkey, kind, d-tag)`
    25	
    26	**Statement.** Any kind in `{30000..=39999}` is keyed by `(pubkey, kind, d-tag)`, not just `(pubkey, kind)`. Two events with the same kind and pubkey but different `d` tags coexist; two with the same `d` supersede.
    27	
    28	**Framework does:** the insert-time rule at `docs/product-spec/subsystems.md` §7.1 row "Parameterized replaceable (30000-39999)". M3 implements this in LMDB via the key encoding at `docs/design/lmdb/keys.md` and the `get_param_replaceable(pk, kind, d_tag)` accessor on the `EventStore` trait (`docs/design/lmdb/trait.md`).
    29	
    30	**App writes:** nothing. Long-form (kind:30023) reader views open by `(pubkey, d_tag)` coordinate; the framework resolves to the current event.
    31	
    32	**Failure mode prevented:** §3.3 bug #1 for the parameterized case — the most common subtlety being apps that key only on `(pubkey, kind)` and overwrite a kind:30023 with a different `d` tag, losing one of the author's articles.
    33	
    34	**Test:** `c2_parameterized_replaceable_supersedes_by_dtag`. Mirrors `docs/design/lmdb/tests.md` line 93: insert two kind:30023 with same `(pubkey, d=foo)`, second newer; assert only the second is read. Insert a third with same kind+pubkey but `d=bar`; assert both `foo` and `bar` are independently retrievable. Insert a kind:30024 with `d=foo`; assert it does not collide.
    35	
    36	**Milestone owner:** **[PENDING M3]**. Test checked in as `#[ignore = "pending M3 LMDB"]`; M3 owner removes the ignore as part of the framework-magic delta on M3's exit-gate report. (Note: the M3 LMDB-tests doc already contains the same scenario at the storage layer; C2 promotes it from a storage-layer test to a contract-surface test — the framework-magic test calls through the public view path, not through the EventStore trait directly.)
    37	
    38	---
    39	
    40	## C3. Kind:5 delete propagation: referenced events removed, tombstone persisted
    41	
    42	**Statement.** A signature-verified kind:5 event from author X referencing event ids `[e1, e2, ...]` and/or replaceable coordinates `[a1, a2, ...]` removes any matching events the store holds that are *authored by X*; the deletions persist as tombstones so the same events cannot be re-inserted later.
    43	
    44	**Framework does:** §7.1 row "Kind 5 (delete)". Mechanism: after signature verification, scan the referenced `e` and `a` tags, remove matching events *authored by the deleter* (other authors' events with the same id, if any, are untouched — a kind:5 by Alice cannot delete Bob's events), persist a tombstone keyed by event coordinate with a tombstone timestamp = maximum delete `created_at` observed for that target.
    45	
    46	**App writes:** nothing. The view payloads recompute (via `ViewModule::on_event_removed` per `docs/design/kernel-substrate.md` §3 lines 141–143) and the deleted note disappears from `TimelineView.items` in the next emit.
    47	
    48	**Failure mode prevented:** the cross-cutting "phantom note" bug: a kind:5 lands, the app's UI does nothing, the note still renders, and worse — re-inserts on app restart because the app's local cache predates the delete. The tombstone is the structural answer: even if the original event is re-delivered by another relay, the store refuses to re-insert it.
    49	
    50	**Test:** `c3_kind5_delete_removes_referenced_and_tombstones`. The test:
    51	
    52	1. Inserts a kind:1 event `e1` by author Alice; asserts it appears in `TimelineView`.
    53	2. Inserts a kind:5 by Alice referencing `e1`; asserts `TimelineView` no longer contains `e1`.
    54	3. Re-inserts `e1` (simulating a later relay redelivery); asserts the store rejects it and the timeline payload does not re-emit.
    55	4. Inserts a kind:5 by Bob referencing `e1`; asserts the tombstone is *not* upgraded (cross-author kind:5 has no effect).
    56	5. Restart the store (M3 path) and re-insert `e1`; assert tombstone is still in force.
    57	
    58	**Milestone owner:** **[PENDING M3]**. Test checked in as `#[ignore = "pending M3 tombstone persistence"]`. The in-memory kernel today does not enforce tombstone persistence across restart; M3's LMDB schema (`docs/design/lmdb/keys.md`) is where the tombstone subdatabase lands. Steps 1–4 of the test can pass against the in-memory kernel; step 5 requires M3.
    59	
    60	---
    61	
    62	## C4. NIP-40 expiration auto-removes event at expiry; survives actor restart
    63	
    64	**Statement.** An event carrying a NIP-40 `expiration` tag is automatically removed from the store at the expiration timestamp; the schedule survives actor restart.
    65	
    66	**Framework does:** §7.1 row "NIP-40 expiration": schedule a tokio timer to remove the event at the expiration timestamp; on actor restart, scan the persisted store and re-schedule. M3 implements the persistent rescan; the in-memory kernel can run the timer but loses schedules on restart.
    67	
    68	**App writes:** nothing. Same `on_event_removed` path as C3.
    69	
    70	**Failure mode prevented:** apps shipping their own "is this event expired?" filter, getting it wrong (off-by-one timezone, missing tag parser, not re-checking after restart), and rendering events that should be gone — especially relevant for ephemeral notifications and expiring offers.
    71	
    72	**Test:** `c4_nip40_expiration_removes_and_persists_schedule`. The test uses the `SimulatedClock` from `nmp-testing` (`docs/product-spec/subsystems.md` §7.13 line 343):
    73	
    74	1. Insert an event with `expiration` tag at clock-now + 60s.
    75	2. Advance clock to +30s; assert event still present.
    76	3. Advance clock to +61s; assert event removed; `TimelineView` payload re-emitted without it.
    77	4. Insert another event with expiration at +120s.
    78	5. Simulate actor restart (drop the actor, instantiate from persisted store); assert the +120s schedule is re-armed by the rescan; advance clock to +130s; assert removal fires.
    79	
    80	**Milestone owner:** **[PENDING M3]**. Test checked in as `#[ignore = "pending M3 expiration persistence"]`. Steps 1–3 are testable today (timer-only); step 5 needs M3.
    81	
    82	---
    83	
    84	## Why this chapter is four bullets, not one
    85	
    86	The four invariants ride the same insert path but have different observable surfaces, different test trigger shapes, and different milestones own them. Collapsing them would (a) hide which milestone owes which guarantee and (b) make the regression test ambiguous when one breaks while the others pass. The chapter is the granularity the milestone delta protocol needs.
    87	
    88	## What this chapter does not cover
    89	
    90	- The replaceable rule for kind:10002 (mailboxes) is C1 (it is in `10000..=19999`). C5 (kind:3 auto-tracking) and the M2 `Trigger::Nip65Arrived` are the *reactive* second-order effect; C1 is the *storage* invariant that triggers them.
    91	- Cross-replaceable-kind interactions (e.g., a kind:5 deleting a kind:0): legal but odd. The §7.1 row says kind:5 removes "matching events authored by the deleter" — the replaceable supersession just means the matched event might already be the latest version. No special-case in the contract; the existing rules compose.
    92	- Garbage collection of unreferenced non-pinned events: a separate concern. `docs/product-spec/subsystems.md` §7.1 "GC" + `docs/design/lmdb/gc.md`. Not a contract bullet because the app does not observe GC directly; it observes events appearing and disappearing per the four rules above, and GC just bounds memory.

exec
/bin/zsh -lc "nl -ba docs/design/framework-magic/signers.md | sed -n '1,90p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
     1	# Framework Magic §C11 — Signer Onboarding
     2	
     3	> Parent: `docs/design/framework-magic.md`.
     4	> Read first: `docs/product-spec/subsystems.md` §7.4 (sessions + signer catalog); `docs/design/kernel-substrate.md` §5 (`CapabilityModule`), §6 (`IdentityModule`); `docs/plan/scope-adjustments-2026-05-18.md` §"Folded into M6".
     5	
     6	## C11. Signer onboarding: bunker:// + nsec creation as kernel actions
     7	
     8	**Statement.** Two signer-onboarding flows are first-class kernel actions, complete from a single dispatched intent without any app-side orchestration:
     9	
    10	1. **Bunker URL onboarding.** A pasted `bunker://...` URL parses into a `BunkerConnect` action; the action runs the NIP-46 rendezvous, establishes the remote-signer connection, persists the connection token via `KeyringCapability`, and emits an `Account` with `signer_kind = Nip46Bunker` into `SessionState.accounts`.
    11	2. **Create new nsec.** A `CreateLocalIdentity { passphrase, label }` action generates a new keypair, encrypts the nsec via NIP-49 with the given passphrase, persists the encrypted nsec via `KeyringCapability`, and emits an `Account` with `signer_kind = LocalKey` into `SessionState.accounts`.
    12	
    13	In both cases the new account becomes available to the active-session machinery (per C12) on a subsequent `SwitchActiveAccount` dispatch.
    14	
    15	**Framework does:**
    16	
    17	- The signer catalog at `subsystems.md` §7.4 lines 127–135 names both kinds as supported in `nmp-core` (no FFI signer extensibility — apps don't implement signers).
    18	- `IdentityModule` (`docs/design/kernel-substrate.md` §6) is the trait family that hosts the local-key and bunker signers. The kernel owns identity ID assignment, secure-store persistence, and session activation routing (kernel-substrate.md §6 last paragraph).
    19	- `KeyringCapability` (`kernel-substrate.md` §5 lines 305–308) is the kernel-provided capability that wraps macOS Keychain / Windows Credential Manager / Secret Service / Android Keystore. Capability calls report; they do not decide.
    20	- The NIP-46 rendezvous flow is the `nostr-connect` crate's behavior; the framework wraps it as an `ActionModule` with the standard ledger-correlated capability-await pattern (`kernel-substrate.md` §4 `AwaitCapability` transition).
    21	- The NIP-49 encryption is the `nostr` crate's `EncryptedSecretKey`; the framework wraps it as a step inside the `CreateLocalIdentity` action.
    22	
    23	**App writes:** for **bunker**, one dispatch with the pasted URL: `dispatch(AppAction::BunkerConnect { url: "bunker://..." })`. For **create new nsec**, one dispatch: `dispatch(AppAction::CreateLocalIdentity { passphrase, label })`. The action ledger row exposes progress (parsing, rendezvous, awaiting user approval on the bunker app, persisted, available); the app's UI renders the ledger row as a step indicator if it wants, but the orchestration is the framework's. The app does **not** call NIP-46 transport code, does **not** invoke NIP-49 encryption, does **not** touch the Keychain directly, and does **not** wire the new identity into the session state.
    24	
    25	**Failure mode prevented:** the constellation of "DIY signer onboarding" bugs that every Nostr-on-mobile app re-discovers — leaked plaintext nsec in app state during the encryption window, lost bunker connection on app suspend, race between persistence and session activation, partial-failure leaving an `Account` in `SessionState` with no usable signer. The action ledger's atomicity (`kernel-substrate.md` §4 "Atomicity" paragraph) makes the "partial success" path explicit and recoverable.
    26	
    27	**Test:** `c11_bunker_url_and_nsec_creation_complete_via_actions`. The test has two sub-paths against an in-memory `KeyringCapability` mock and a mock NIP-46 rendezvous endpoint:
    28	
    29	1. **Bunker onboarding:**
    30	   a. Dispatch `BunkerConnect { url: "bunker://abc?relay=wss%3A%2F%2Fmock&secret=xyz" }`.
    31	   b. Mock rendezvous endpoint responds with a successful `connect` response.
    32	   c. Assert the action ledger row transitions `Pending → Running(Parsing) → Running(Rendezvous) → Running(Persisting) → Completed { account_id }`.
    33	   d. Assert `SessionState.accounts` contains one new `Account` with `signer_kind = Nip46Bunker`; the `KeyringCapability` mock has one stored entry keyed by the new account id.
    34	   e. Assert no plaintext bunker secret crossed FFI (the test's reconciler audit log shows no `Account` snapshot field carrying the raw URL); only the typed `Account` + `signer_kind` enum.
    35	2. **Create new nsec:**
    36	   a. Dispatch `CreateLocalIdentity { passphrase: "test-passphrase", label: "alice" }`.
    37	   b. Assert the action ledger row transitions `Pending → Running(Generating) → Running(Encrypting) → Running(Persisting) → Completed { account_id }`.
    38	   c. Assert `SessionState.accounts` contains one new `Account` with `signer_kind = LocalKey`, `display.label = "alice"`; the `KeyringCapability` mock has one stored entry containing the NIP-49 ciphertext (the test inspects the mock's stored bytes — the prefix is `ncryptsec1`).
    39	   d. Assert the plaintext nsec is **not** present in `SessionState`, in any view payload, in any diagnostic surface, or in the test's reconciler audit log. The plaintext exists only inside the actor's transient action state during encryption.
    40	   e. Assert a follow-up `SwitchActiveAccount { account_id }` succeeds and that the actor can sign a test event using the newly-created identity (round-trip: dispatch a `SendNote` against the new account, observe a signed event in the action ledger before publish).
    41	
    42	**Milestone owner:** **[PENDING M6]**. M6 is the signers + write-path milestone (per `scope-adjustments-2026-05-18.md` ladder). M6 owner adds the framework-magic delta after the test goes green. Test checked in as `#[ignore = "pending M6 signers"]`.
    43	
    44	## Why only these two onboarding paths
    45	
    46	The full signer catalog at `subsystems.md` §7.4 lists five kinds:
    47	
    48	- Local key (raw nsec, encrypted at rest) — **covered by C11 sub-path 2**.
    49	- NIP-49 (password-encrypted) — **subsumed by C11 sub-path 2** (the NIP-49 encryption is the persistence step of the local-key creation, not a separate flow).
    50	- NIP-46 bunker — **covered by C11 sub-path 1**.
    51	- NIP-07 (web only) — wired via the web bindings shim; not a v1-ladder contract bullet because the web target is M15.
    52	- External Android Amber via NIP-55 — wired via the `ExternalSignerCapability` (`kernel-substrate.md` §5); not a v1-ladder contract bullet because Android is M15.
    53	
    54	C11 covers the two paths the user explicitly named in `scope-adjustments-2026-05-18.md` §"Folded into M6": *"NIP-46 bunker:// URL parsing + connection flow"* and *"Create new nsec flow. Generate, encrypt (NIP-49), and store via Keychain capability."* The other three signer kinds inherit the same atomicity guarantees by virtue of going through the same `IdentityModule` + `KeyringCapability` plumbing, but their onboarding flows have platform-specific surfaces that the v1 contract does not assert at this level.
    55	
    56	A potential C11.b sibling bullet covering NIP-07 + NIP-55 may be added in the M15 framework-magic delta.
    57	
    58	## The capability boundary
    59	
    60	This bullet is a load-bearing demonstration of the bible's capability pattern (aim.md §6 doctrine 11: "capabilities, not callbacks"). The KeyringCapability **reports** (here is the stored bytes; persistence succeeded/failed). It does **not decide** (whether to retry, whether to fall back to a different storage backend, whether to surface a UI prompt). The framework decides; the capability executes.
    61	
    62	The test's assertion that no plaintext nsec crosses FFI is the structural witness for `aim.md` §6 doctrine 5 (bounded native state) and for the implicit "secrets stay in Rust" rule — the platform layer never sees the unencrypted key material because every read/write of the key goes through the in-Rust `IdentityModule::sign` function.
    63	
    64	## Cross-references
    65	
    66	- `docs/design/kernel-substrate.md` §6 — `IdentityModule` trait definition.
    67	- `docs/design/kernel-substrate.md` §5 — `CapabilityModule` framing + the named `KeyringCapability` family.
    68	- `docs/product-spec/subsystems.md` §7.4 — `SessionState` + `Account` shapes.
    69	- The `nostr-connect` and `nostr-keyring` crates (aim.md §3) — the protocol/OS primitives the framework composes.
    70	
    71	## What this chapter does not cover
    72	
    73	- **Account switching mechanics** — that's C12 in `sessions.md`.
    74	- **Signing a publish** — the sign step inside `SendNoteAction` (C7). C11 covers onboarding; subsequent signing is the publish path.
    75	- **Multi-device account sync** — out of v1 scope per `aim.md` §9.
    76	- **Key-recovery and passphrase reset flows** — application-level UI on top of the framework primitives; not a contract bullet because these flows compose existing actions (delete identity, create new identity).

exec
/bin/zsh -lc "nl -ba crates/nmp-testing/Cargo.toml | sed -n '1,120p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
exec
/bin/zsh -lc 'find crates/nmp-testing -maxdepth 2 -type d -print' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
     1	[package]
     2	name = "nmp-testing"
     3	version.workspace = true
     4	edition.workspace = true
     5	license.workspace = true
     6	
     7	[dependencies]
     8	serde = { version = "1.0", features = ["derive"] }
     9	serde_json = "1.0"
    10	
    11	[[bin]]
    12	name = "reactivity-bench"
    13	path = "bin/reactivity-bench/main.rs"
    14	
    15	[[bin]]
    16	name = "firehose-bench"
    17	path = "bin/firehose-bench/main.rs"

 succeeded in 0ms:
crates/nmp-testing
crates/nmp-testing/bin
crates/nmp-testing/bin/firehose-bench
crates/nmp-testing/bin/reactivity-bench
crates/nmp-testing/src

exec
/bin/zsh -lc "rg -n \"\\[\\[test\\]\\]|framework_magic_contract|name = \\\"framework_magic_contract\\\"|path = \\\"tests/framework_magic_contract\\.rs\\\"\" crates/nmp-testing/Cargo.toml Cargo.toml crates" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 exited 1 in 0ms:
codex
**Findings**

- [docs/design/framework-magic.md](/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada/docs/design/framework-magic.md:6): Claims `crates/nmp-testing/tests/framework_magic_contract.rs` exists and proves the contract, but that file and even `crates/nmp-testing/tests/` are absent; [crates/nmp-testing/Cargo.toml](/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada/crates/nmp-testing/Cargo.toml:11) has only bins, no `[[test]]`. This makes the 14-test contract internally false.

- [docs/design/framework-magic/capabilities.md](/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada/docs/design/framework-magic/capabilities.md:45): D0 drift. C13 marks placeholder shape `[DONE]` by citing `crates/nmp-core` timeline tests, but D0 says `nmp-core` must not own Profile/Timeline nouns; those belong in protocol modules like `nmp-nip01`. The contract should not use legacy kernel-owned timeline behavior as proof.

- [docs/design/framework-magic.md](/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada/docs/design/framework-magic.md:77): Zero-debt drift. The merge leaves explicit `TBD-from-research(...)` placeholders and a “Stage X / TBD” reference in [sync.md](/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada/docs/design/framework-magic/sync.md:57). No literal `TODO/FIXME/unimplemented` found, but these are still TODO-equivalent markers.

- [docs/design/framework-magic/outbox.md](/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada/docs/design/framework-magic/outbox.md:44): Says the C7 test has “three sub-paths” but lists five at lines 46-50. Minor, but it undermines the “14 coherent tests” surface.

File-size budget is clean: every added file is under 300 LOC; largest is `test-scaffolding.md` at 208 lines.
tokens used
102,497
**Findings**

- [docs/design/framework-magic.md](/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada/docs/design/framework-magic.md:6): Claims `crates/nmp-testing/tests/framework_magic_contract.rs` exists and proves the contract, but that file and even `crates/nmp-testing/tests/` are absent; [crates/nmp-testing/Cargo.toml](/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada/crates/nmp-testing/Cargo.toml:11) has only bins, no `[[test]]`. This makes the 14-test contract internally false.

- [docs/design/framework-magic/capabilities.md](/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada/docs/design/framework-magic/capabilities.md:45): D0 drift. C13 marks placeholder shape `[DONE]` by citing `crates/nmp-core` timeline tests, but D0 says `nmp-core` must not own Profile/Timeline nouns; those belong in protocol modules like `nmp-nip01`. The contract should not use legacy kernel-owned timeline behavior as proof.

- [docs/design/framework-magic.md](/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada/docs/design/framework-magic.md:77): Zero-debt drift. The merge leaves explicit `TBD-from-research(...)` placeholders and a “Stage X / TBD” reference in [sync.md](/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada/docs/design/framework-magic/sync.md:57). No literal `TODO/FIXME/unimplemented` found, but these are still TODO-equivalent markers.

- [docs/design/framework-magic/outbox.md](/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada/docs/design/framework-magic/outbox.md:44): Says the C7 test has “three sub-paths” but lists five at lines 46-50. Minor, but it undermines the “14 coherent tests” surface.

File-size budget is clean: every added file is under 300 LOC; largest is `test-scaffolding.md` at 208 lines.
