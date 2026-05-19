Reading additional input from stdin...
2026-05-17T22:52:17.662879Z ERROR codex_core::session: failed to load skill /Users/pablofernandez/.agents/skills/voice-capture-sheet/SKILL.md: invalid YAML: mapping values are not allowed in this context at line 2 column 116
OpenAI Codex v0.129.0 (research preview)
--------
workdir: /Users/pablofernandez/Work/nostr-multi-platform
model: gpt-5.5
provider: openai
approval: never
sandbox: workspace-write [workdir, /tmp, $TMPDIR, /Users/pablofernandez/.codex/memories]
reasoning effort: xhigh
reasoning summaries: none
session id: 019e3823-e781-7e13-aef7-de4342c28982
--------
user
You are reviewing merge fb139ab on master in the nostr-multi-platform repo. Doctrine D0–D5 (kernel never grows app nouns; best-effort rendering; reactivity contract ≤60 Hz/view; errors never cross FFI; one writer per fact; capabilities report don't decide). File-size: 300 LOC soft, 500 hard.

This commit fixes 6 issues you flagged on fcf8b43. Verify each fix:
1. D5 doctrine mismatch in gates.md — should now cite CapabilityModule evidence, not snapshot evidence
2. Pre-merge CI gate inconsistency — should split into stress-gate-fast (pre-merge) and stress-close-gate (milestone-close)
3. Freed-pointer probe UB — should remove freed-pointer probe from scenarios.md S7; NULL app → silent no-op only
4. S10 conditional vs doctrine — S10 should no longer be used as D1 doctrine evidence; D1 should cite S3+S4+code proof
5. File-size violations — all ffi-hardening/*.md and debt-inventory.md should be ≤300 LOC; session-1.md should have archival header
6. 13 vs 14 symbol count — ffi-hardening.md should say 14 symbols

Are all 6 actually fixed? Any new issues introduced? Be terse.

=== diff stat ===
 docs/perf/codex-reviews/031fc07.md | 4424 +++++++++++++++++++++++++++++++++++
 docs/perf/codex-reviews/9fead0e.md | 4443 ++++++++++++++++++++++++++++++++++++
 docs/perf/orchestration-log.md     |    1 +
 3 files changed, 8868 insertions(+)

=== commit log ===
20b2e2d perf(codex): reviews of 031fc07 (M2) + 9fead0e (M3); heartbeat #2 log

---

=== diff ===
diff --git a/docs/perf/codex-reviews/031fc07.md b/docs/perf/codex-reviews/031fc07.md
new file mode 100644
index 0000000..9ac8254
--- /dev/null
+++ b/docs/perf/codex-reviews/031fc07.md
@@ -0,0 +1,4424 @@
+Reading additional input from stdin...
+2026-05-17T22:42:25.262680Z ERROR codex_core::session: failed to load skill /Users/pablofernandez/.agents/skills/voice-capture-sheet/SKILL.md: invalid YAML: mapping values are not allowed in this context at line 2 column 116
+OpenAI Codex v0.129.0 (research preview)
+--------
+workdir: /Users/pablofernandez/Work/nostr-multi-platform
+model: gpt-5.5
+provider: openai
+approval: never
+sandbox: workspace-write [workdir, /tmp, $TMPDIR, /Users/pablofernandez/.codex/memories]
+reasoning effort: xhigh
+reasoning summaries: none
+session id: 019e381a-dd71-7463-a543-31662fd3d61b
+--------
+user
+You are reviewing merge 031fc07 (M2 subscription compilation + outbox routing design) on master in nostr-multi-platform. Doctrine D0-D5 (D0 kernel never grows app nouns, D1 best-effort rendering, D2 reactivity contract, D3 no errors across FFI, D4 one writer per fact, D5 capabilities report don't decide). File size: 300 LOC soft, 500 hard.
+
+M2 goal (per docs/plan.md §M2): replace hardcoded relay set with a subscription compiler stage. Logical interests compile to per-relay plans. NIP-65 outbox routing default for reads + writes. Four diagnostic lanes (NIP-65 / hint / provenance / user-configured) never collapse.
+
+Merge:
+
+=== M2 design merge: 031fc07 ===
+=== diff stat ===
+ docs/design/subscription-compilation.md            |  43 ++++
+ docs/design/subscription-compilation/compiler.md   | 193 +++++++++++++++
+ .../design/subscription-compilation/diagnostics.md | 188 ++++++++++++++
+ docs/design/subscription-compilation/intro.md      | 132 ++++++++++
+ docs/design/subscription-compilation/nip65.md      | 221 +++++++++++++++++
+ docs/design/subscription-compilation/outbox.md     | 224 +++++++++++++++++
+ .../subscription-compilation/recompilation.md      | 159 ++++++++++++
+ docs/design/subscription-compilation/tests.md      | 273 +++++++++++++++++++++
+ 8 files changed, 1433 insertions(+)
+
+=== commit log ===
+031fc07 design(m2): subscription compilation + outbox routing spec
+Replaces the hardcoded two-role planner in crates/nmp-core/src/kernel/
+{requests,ingest,relay}.rs with a formal subscription compilation stage
+driven by NIP-65 mailboxes, plus an outbox-routing planner subsystem
+seam for the M6 write path.
+
+Split across:
+  - subscription-compilation.md           index + section map + open questions
+  - subscription-compilation/intro.md     problem statement (cited line refs)
+                                          + LogicalInterest model
+  - subscription-compilation/compiler.md  4-stage pipeline + plan-id contract
+                                          + per-function migration table for
+                                          every fn in requests.rs/ingest.rs
+  - subscription-compilation/recompilation.md
+                                          10 triggers with actor message shapes
+                                          (Nip65Arrived, ViewOpened/Closed,
+                                          ActiveAccountChanged, RelayReconnected,
+                                          InvalidateCompile, UserConfigured/
+                                          IndexerSetChanged, RelayAuthState,
+                                          SignerAvailable)
+  - subscription-compilation/diagnostics.md
+                                          four-lane records (NIP-65 / Hint /
+                                          Provenance / UserConfigured) per
+                                          ADR-0007; reverse-relay-coverage
+                                          diagnostic view
+  - subscription-compilation/nip65.md     nmp-nip65 crate layout:
+                                          MailboxesView (ViewModule),
+                                          MailboxCache trait, routing fns
+  - subscription-compilation/outbox.md    PublishPlanner trait + default
+                                          Nip65PublishPlanner; override action
+                                          PublishWithOverride with mandatory
+                                          audit string + debug warning
+  - subscription-compilation/tests.md     M2 exit-gate audit test at
+                                          crates/nmp-testing/tests/
+                                          m2_subscription_compilation_audit.rs
+                                          with assertions on (1) no publish-
+                                          API relay leak, (2) 1000-author
+                                          per-relay union fan-out, (3) late
+                                          NIP-65 recompile without churn,
+                                          (4) four-lane diagnostic distinctness
+
+All files ≤ 270 LOC (AGENTS.md 500-line ceiling honoured).
+
+Companion to docs/plan.md §M2; consumes docs/design/ndk-applesauce-lessons.md
+§7; extends ADR-0007 diagnostic records without collapsing the four lanes.
+
+Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
+
+---
+
+=== diff ===
+diff --git a/docs/design/subscription-compilation.md b/docs/design/subscription-compilation.md
+new file mode 100644
+index 0000000..d51e93d
+--- /dev/null
++++ b/docs/design/subscription-compilation.md
+@@ -0,0 +1,43 @@
++# Design: Subscription Compilation + Outbox Routing (M2)
++
++> **Status:** Draft (M2 design + impl-prep).
++> **Date:** 2026-05-18.
++> **Companion docs:** `docs/plan.md` §M2; `docs/aim.md` §4.4–§4.5; `docs/design/ndk-applesauce-lessons.md` §7; ADR-0007 (diagnostic lanes); `docs/design/kernel-substrate.md` §3 (`ViewModule`) and §4 (`ActionModule`); `docs/product-spec/subsystems.md` §7.2–§7.3.
++> **Scope:** Replace the "hardcoded two-role relay set" planner in `crates/nmp-core/src/kernel/{requests,ingest,mod}.rs` with a **subscription compilation stage** that turns logical interests into per-relay plans driven by NIP-65 mailboxes, and graduates outbox routing to a first-class planner subsystem. v1 is in-memory; M3 plugs it into LMDB. This is a design doc; no implementation lands in this PR.
++
++This document is split into focused sub-files to stay under the 500 LOC ceiling (`AGENTS.md`).
++
++- [Intro — problem statement + logical interest model](subscription-compilation/intro.md) (§1, §2)
++- [Compiler — pipeline + plan-id contract + function migration table](subscription-compilation/compiler.md) (§3)
++- [Recompilation — triggers and actor message shapes](subscription-compilation/recompilation.md) (§4)
++- [Diagnostics — four-lane records + reverse-coverage view](subscription-compilation/diagnostics.md) (§5, §8)
++- [nmp-nip65 — crate layout, traits, public surface](subscription-compilation/nip65.md) (§6)
++- [Outbox — publish-planner seam + override action](subscription-compilation/outbox.md) (§7)
++- [Tests — wire-frame audit gate](subscription-compilation/tests.md) (§9)
++
++## Section map
++
++| § | Topic | File |
++|---|---|---|
++| 1 | What is wrong with the current planner (cited line refs) | intro.md |
++| 2 | Logical interest — formal definition + how views express them | intro.md |
++| 3 | Compilation pipeline: authors → mailboxes → per-relay plans + plan-id | compiler.md |
++| 4 | Recompilation triggers (kind:10002, view open/close, reconnect, account switch, manual, user-configured change) | recompilation.md |
++| 5 | Four-lane diagnostic records (NIP-65 / hint / provenance / user-configured) | diagnostics.md |
++| 6 | `nmp-nip65` file layout, `MailboxesViewModule`, public surface | nip65.md |
++| 7 | `PublishPlanner` trait, write fan-out policy, override + debug warning | outbox.md |
++| 8 | Reverse-relay-coverage diagnostic view ("this relay serves N authors of our timeline") | diagnostics.md |
++| 9 | M2 exit-gate audit test path + assertions | tests.md |
++| 10 | Open questions for follow-up ADRs | this file (below) |
++
++## 10. Open questions
++
++These remain to be resolved by ADRs after design review, not in this design pass.
++
++1. **Plan-id stability under perturbation.** The intro/compiler picks "logical-interest set + author-mailbox snapshot ⇒ plan-id." That ties plan-id to mailbox membership, so a single new kind:10002 arrival reshuffles plan-ids for every interest including that author. An alternative scopes plan-id to the *logical-interest set only* and tracks per-relay assignment as a separate stable identifier. Pick one in an ADR; the test contracts in §9 assume the former.
++2. **Filter-merge lattice formal-isation.** §3 step 3 lists which filter fields are safely mergeable across logical interests (`authors`, `kinds`, `since`, `until`, `limit`, tag operators). It does not yet specify the merge algebra for the corner case where two interests differ only in `limit` but agree on authors and kinds. Worth an ADR-level note before the lattice is coded; `docs/product-spec/subsystems.md` §7.2 promises "a formal merge lattice for `limit`, `since`, `until`, multi-filter arrays, and tag operators."
++3. **Per-author indexer-fallback ledger row?** Today the compiler treats indexer fallback as an inline relay assignment. If the kind:10002 fetch is its own durable action (M6 ledger), the fallback becomes a tracked action with retry/cancel semantics. Cleaner for diagnostics; heavier for M2. Defer.
++4. **Read-relay vs write-relay use for subscriptions on the same author.** NIP-65 defines write relays (outbox) for the author's own events and read relays (inbox) for events directed *at* the author. For a `Timeline { authors: [...] }` we want write relays. For a `Notifications { p: [author] }` we want inbox relays. The compiler distinguishes them by filter shape (`authors` vs `#p`), matching the `docs/product-spec/subsystems.md` §7.3 routing table. Document a corner case: kind-1 filtered by both `authors` and `#p` is rare but real (replies to the author from the author). Pick a precedence in an ADR; current bias is `authors` wins (write relays).
++5. **User-configured relay precedence vs NIP-65.** A user adds `wss://my-private.example` to local config. Does it *augment* (union) or *override* (replace) NIP-65 routing for the active account? `subsystems.md` §7.3 default-resolves by NIP-65; user-configured is "fallback" in the indexer sense. ADR needs to spell out the augment/override question for the active account specifically.
++6. **Auth-paused relays in compiled plans.** If a relay is in `RelayAuthState::ChallengeReceived`, the compiler still produces a plan that assigns interests to it (so reconnect-after-auth resumes correctly), but emission must pause. Is the pause modeled inside the compiler (per-relay gate) or inside the wire-emitter (consumes plans, applies pause)? Bias: wire-emitter, but the compiler must surface the pause as a fact for `LogicalInterestStatus`. Resolve before M5.
++7. **NSE crate compilation surface.** `nmp-nip17-nse` (M9) runs in iOS Notification Service Extension with bounded memory; it needs to compile a single-author single-relay plan without the full planner. Confirm in an ADR that the compiler exposes a `compile_one(spec, mailbox_cache_snapshot) -> Plan` pure function suitable for NSE use, and that the function does not require a live actor.
+diff --git a/docs/design/subscription-compilation/compiler.md b/docs/design/subscription-compilation/compiler.md
+new file mode 100644
+index 0000000..21a4aa7
+--- /dev/null
++++ b/docs/design/subscription-compilation/compiler.md
+@@ -0,0 +1,193 @@
++# Subscription Compilation §3 — The Compilation Pipeline
++
++> Parent: `docs/design/subscription-compilation.md`.
++> Read first: [intro.md](intro.md) for the `LogicalInterest` shape this stage consumes.
++
++The compiler is a pure function plus a small amount of state (the mailbox cache and the active plan registry). It runs whenever a recompilation trigger fires (§4) and produces a `CompiledPlan` that the wire-emitter applies as a diff against the relay sockets.
++
++## 3.0 Pipeline overview
++
++```
++   logical_interests:                 mailbox_cache + relay_config
++   Vec<LogicalInterest>                       │
++            │                                 │
++            ▼                                 ▼
++   ┌───────────────────────────────────────────────────┐
++   │ Stage 1: Resolve authors → mailboxes              │  (§3.1)
++   │   each author → { write, read, both, missing }    │
++   └───────────────────────────────────────────────────┘
++            │
++            ▼
++   ┌───────────────────────────────────────────────────┐
++   │ Stage 2: Indexer fallback for missing mailboxes   │  (§3.2)
++   │   missing → enqueue kind:10002 probe              │
++   │   missing-author reads → indexer set (read only)  │
++   └───────────────────────────────────────────────────┘
++            │
++            ▼
++   ┌───────────────────────────────────────────────────┐
++   │ Stage 3: Per-relay shape merge                    │  (§3.3)
++   │   group interests by target relay URL             │
++   │   merge compatible shapes inside each relay       │
++   │   refuse merges that would change semantics       │
++   └───────────────────────────────────────────────────┘
++            │
++            ▼
++   ┌───────────────────────────────────────────────────┐
++   │ Stage 4: Plan-id binding                          │  (§3.4)
++   │   compute plan_id = hash(interest_set,            │
++   │                          mailbox_snapshot,        │
++   │                          merge_lattice_version)   │
++   │   stable across no-op recompilations              │
++   └───────────────────────────────────────────────────┘
++            │
++            ▼
++   CompiledPlan { plan_id, per_relay: Vec<RelayPlan> }
++```
++
++The wire-emitter (`crates/nmp-core/src/kernel/wire.rs`, to be added) diffs the new plan against the current wire-sub registry: opens new REQs, closes orphaned ones, leaves stable assignments untouched.
++
++## 3.1 Stage 1 — Resolve authors to mailboxes
++
++Inputs: every `LogicalInterest` with non-empty `shape.authors` or non-empty `shape.tags[#p]`; the mailbox cache populated by `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`).
++
++Output: an `AuthorRouting` per author per direction:
++
++```rust
++pub struct AuthorRouting {
++    pub author: Pubkey,
++    pub direction: RoutingDirection,        // Outbox or Inbox
++    pub relays: BTreeSet<RelayUrl>,         // resolved write/read/both
++    pub source: RoutingSource,              // Nip65 | UserConfigured | Indexer | Hint
++    pub freshness_ms: Option<u64>,          // age of the kind:10002 record
++}
++
++pub enum RoutingDirection {
++    Outbox,    // for `authors:` filters — the author's *write* relays
++    Inbox,     // for `#p:` filters    — the tagged author's *read* relays
++}
++```
++
++Direction is decided by the interest's filter shape per `docs/product-spec/subsystems.md` §7.3:
++
++| Interest shape | Direction | Source per author |
++|---|---|---|
++| Non-empty `authors`, no `#p` | Outbox | author's `write_relays ∪ both_relays` |
++| Empty `authors`, non-empty `#p` | Inbox | tagged author's `read_relays ∪ both_relays` |
++| Both populated | Outbox primarily; Inbox interests split (see §3.3) | both |
++| Neither populated | (handled by stage 3 as "use active-account read relays") | — |
++
++`docs/product-spec/subsystems.md` §7.3 specifies one explicit override: DMs (NIP-17 gift-wraps, M9) fail closed if recipient inbox relays are missing. The compiler enforces this by refusing to produce a plan for an interest tagged `privacy = FailClosed` if any tagged-pubkey inbox lookup returns `Indexer` source. §7 details the publish-side enforcement.
++
++## 3.2 Stage 2 — Indexer fallback for unknown mailboxes
++
++The indexer set is a kernel-configured `Vec<RelayUrl>` (default: a small curated list; user-configurable in `AppConfig`). Today's `crates/nmp-core/src/relay.rs:2` is the placeholder for one indexer relay (`purplepag.es`); the v1 indexer set lives in `AppConfig.indexer_relays`.
++
++Two distinct behaviours:
++
++1. **Mailbox probe.** For every author with `mailbox_cache.get(author) == None`, the compiler emits a `IndexerProbe { author }` side effect on the plan. The probe registers as its own short-lived `LogicalInterest { shape: { kinds: [10002], authors: [author], limit: 1 }, lifecycle: OneShot, scope: Global }`. Recompilation triggers (§4 trigger A1) re-route the original interest once the kind:10002 lands.
++2. **Read fallback.** For a `RoutingDirection::Outbox` interest whose author has no known mailboxes, the compiler routes the interest to the indexer set **as read-only fallback**. Per `docs/product-spec/subsystems.md` §7.3 line 99: "fall back to indexer set for reads only; do not publish to indexers." The `RoutingSource::Indexer` tag on the resulting `AuthorRouting` flows through to the diagnostic surface so the four-lane view (§5) can render "author X is being served by indexer Y because we have no mailbox for them."
++
++Bounded: a single author's indexer probe is enqueued at most once per `compiler_probe_window_secs` (default 60s) to prevent thundering-herd probes if a screen of N rows all claim the same unknown pubkey.
++
++## 3.3 Stage 3 — Per-relay shape merge
++
++After Stage 1, every interest has one or more `(relay_url, sub_shape)` assignments. Stage 3 groups by relay URL and merges shapes where merging preserves semantics.
++
++### Merge rules (the lattice)
++
++Two `InterestShape`s `A` and `B` are **mergeable on relay R** iff:
++
++1. `A.kinds == B.kinds` **or** one is empty (wildcard absorbs).
++2. `A.tags.keys() == B.tags.keys()` (same tag dimensions) **and** the union of values per dimension stays under the relay's per-filter limit (default 1000).
++3. `A.since` and `B.since`: merged `since = min(A.since, B.since)` *only if* both are present or both absent. Mixing a bounded interest with an unbounded one is **not** merged (would broaden the bounded one's window).
++4. `A.until` and `B.until`: same rule, mirror of (3) with `max`.
++5. `A.limit` and `B.limit`: mergeable iff both are absent. If either has a `limit`, **do not merge** — broadening would mask the limit's intent.
++6. `A.lifecycle == B.lifecycle`. Tailing and one-shot do not merge (one-shot would never close).
++7. `A.event_ids` and `B.event_ids`: merge by union, capped at the relay's per-filter `ids` limit.
++
++When mergeable, the merged shape is `{ authors: A.authors ∪ B.authors, ... }`. The merged interest tracks both originating `InterestId`s so per-event dispatch back to consumers stays correct.
++
++When not mergeable, the two interests get distinct sub-shapes on the same relay, producing two distinct REQs. That is fine and expected.
++
++Open question 2 in the parent index (`subscription-compilation.md`) covers the `limit`-only corner case formally.
++
++### Per-relay output
++
++```rust
++pub struct RelayPlan {
++    pub relay_url: RelayUrl,
++    pub role_tags: BTreeSet<RoutingSource>,   // why this relay is in the plan
++    pub sub_shapes: Vec<SubShape>,            // each emits one REQ
++}
++
++pub struct SubShape {
++    pub shape: InterestShape,                  // canonical, post-merge
++    pub originating_interests: Vec<InterestId>,
++    pub canonical_filter_hash: String,         // for ADR-0007 WireSubscriptionStatus
++}
++```
++
++The wire-emitter renders each `SubShape` as exactly one `REQ` on `relay_url` with a sub-id of `c{plan_id}-r{relay_idx}-s{shape_idx}`. The sub-id is meaningful only to the kernel; diagnostics use `canonical_filter_hash` for stable identity across re-emission.
++
++## 3.4 Stage 4 — Plan-id binding
++
++`plan_id` is the **stable identity** the platform observes for diagnostic continuity. It answers: "did this recompilation actually change anything observable?"
++
++Definition (this design picks the "interest-set + mailbox-snapshot" formulation; open question 1 in the parent index notes the alternative):
++
++```
++plan_id = blake3(
++    sorted(interest_set.iter().map(|i| (i.id, i.shape, i.scope, i.lifecycle))),
++    sorted(mailbox_snapshot.iter().map(|(pk, ml)| (pk, ml.created_at,
++                                                    sorted(ml.write),
++                                                    sorted(ml.read)))),
++    INDEXER_SET_VERSION,
++    USER_CONFIGURED_RELAYS_VERSION,
++    MERGE_LATTICE_VERSION,
++)
++```
++
++Properties:
++
++- **Recompilation with no change ⇒ same plan-id.** If `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:218-221`) deduplicates and decides not to replace a stale mailbox, no plan-id churn.
++- **Adding an interest changes plan-id even if no new wire REQ results.** Two interests can merge into the same SubShape; the plan-id changes because the *interest set* changed. The platform diagnostic correctly reports "logical-interest count went up; wire-sub count did not."
++- **A new kind:10002 for a covered author changes plan-id.** Even if the author's new write relays overlap entirely with the old set (e.g. relay added then removed), the snapshot's `created_at` advanced, so the hash changes. ADR-0007 diagnostics will reflect this; the wire-emitter's diff will be a no-op if the actual relay assignment is unchanged.
++- **Indexer set change changes plan-id.** Operator config edits surface immediately.
++
++The `plan_id` is stored on `CompiledPlan` and rendered into `LogicalInterestStatus` (extending the record at `crates/nmp-core/src/kernel/mod.rs:147-154` with `plan_id: String, plan_generation: u64`). Tests in §9 assert plan-id stability across no-op recompilations.
++
++## 3.5 Migration of existing functions
++
++This is the binding contract: each function in `crates/nmp-core/src/kernel/requests.rs` and `crates/nmp-core/src/kernel/ingest.rs` either disappears, becomes thin glue over the compiler, or graduates into a typed module. The compiler does not coexist with the old planner; M2 replaces it.
++
++| Current function (file:line) | M2 replacement |
++|---|---|
++| `startup_requests` (requests.rs:50-106) | Becomes `register_startup_interests()`: pushes 4 `LogicalInterest`s into the registry (seed timeline; account kind:0; account kind:10002; seed contacts kind:3). The compiler runs once and produces wire REQs. The TEST_PUBKEY-specific bootstrap (line 71-82) becomes an `InterestScope::Global` interest with hardcoded indexer routing flagged `RoutingSource::UserConfigured` (it is operator-debug seed data). |
++| `open_author` (requests.rs:118-140) | Registers three `LogicalInterest`s scoped `ActiveAccount` (kind:10002, kind:0, kinds:1+6 for author); calls `compiler.recompile(Trigger::ViewOpen)`. Refcount stays — but it lives on `InterestId` now, not on `ViewInterest { key, refcount }`. The `can_send` gate disappears: the compiler always produces a plan; the wire-emitter is the only thing that may queue deferrals. |
++| `open_thread` (requests.rs:142-168) | Registers a `Thread { event_id }` view-module spec; the view module returns interests with `event_ids` and `#e`-tag shapes. Hydration cascade in `prepare_thread_requests` (requests.rs:441-466) becomes part of the view module's `reduce` returning new interests when new event ids surface. |
++| `open_firehose_tag` (requests.rs:170-200) | Registers one `LogicalInterest { shape: { kinds: [1], tags: { #t: [tag] } }, scope: ActiveAccount, lifecycle: Tailing }`. Routes to active-account read relays per §3.1 table. |
++| `claim_profile` / `release_profile` (requests.rs:202-263) | Registers/unregisters one `LogicalInterest { shape: { authors: [pk], kinds: [0], limit: 1 }, lifecycle: OneShot }` per claim. Refcount of distinct consumers becomes the `InterestId` claim set inside the registry. **Dedup of (pk, kinds=[0]) across N timeline rows yields one merged SubShape and one wire REQ** — this is what bug-extinction "1000 avatars do not produce 1000 REQs" verifies (already true today via dedup; the compiler preserves it). |
++| `close_author` / `close_thread` (requests.rs:265-311) | Drop interests by `InterestId`; recompile with `Trigger::ViewClose`. Wire-emitter closes orphaned REQs. The "warm-close" grace from the view-warmth doctrine (`docs/design/kernel-substrate.md` §3 "lifecycle") is the compiler's, not the view's — interests stay registered for the warmth window after their last claim. |
++| `close_subscriptions_with_prefixes` (requests.rs:313-331) | **Deleted.** The wire-emitter closes by `WireSubId`, which is the compiler's diff output. String-prefix matching of sub-ids is a 2026-05-period scaffold that the compiler removes. |
++| `pending_view_requests` (requests.rs:333-355) | Becomes `compiler.flush_deferred_for_relay(role, url)`: called when a relay reconnects (§4 trigger A3). The compiler resubmits its current plan against that relay's slot. |
++| `firehose_requests` (requests.rs:357-372) | Replaced as described above for `open_firehose_tag`. The `diag-firehose-N` sub-id scheme goes away — `canonical_filter_hash` plus `plan_id` give stable identity. |
++| `pending_profile_claim_requests` (requests.rs:374-388) | Disappears. Claims are interests; the compiler is the only thing that decides "this interest needs a REQ." |
++| `profile_claim_request` (requests.rs:390-402) | Disappears. The compiler routes claimed-profile interests through Stage 1; indexer fallback (Stage 2) handles the no-mailbox case. |
++| `author_requests` (requests.rs:404-439) | Disappears (replaced by `open_author`'s interest registration). |
++| `prepare_thread_requests` / `enqueue_thread_*` / `maybe_open_thread_hydration` (requests.rs:441-528) | Move to a `ThreadViewModule` in `nmp-nip10`. The hydration cascade is `view_module.reduce(...)` returning additional interests as new event ids surface in store. |
++| `req` (requests.rs:530-556) | **Deleted.** Replaced by the wire-emitter's `emit_req(relay_url, sub_id, filter)`. No call site outside the wire-emitter is permitted to construct a REQ. |
++| `defer_outbound` (requests.rs:558-568) | Moves to the wire-emitter; deferral is per-relay, keyed by URL, not by role. |
++| `ingest_relay_list` (ingest.rs:209-233) | Stays, but emits a `Trigger::Nip65Arrived { pubkey }` event (§4 trigger A1) on a material update. Becomes the producer side of the recompilation cycle. |
++| `ingest_profile` / `ingest_contacts` / `ingest_timeline_event` (ingest.rs:166-279) | Unchanged in storage shape. Their relevance to compilation is that they feed the view modules' projections (per `docs/design/reactivity/view-deltas-and-projections.md`). |
++| `should_store_event` (ingest.rs:268-279) | Unchanged. Per-sub-id string filtering goes away when sub-ids become `c{plan}-r{relay}-s{shape}`, but the predicate switches to "is this event id covered by an active interest?" — a `compiler.is_covered(event)` call. |
++| `maybe_open_timeline` (ingest.rs:329-365) | The "seed-contacts arrive → open union timeline" logic becomes a small `TimelineSeedModule`: it watches `seed_contacts` projection and registers a `Timeline { authors: union }` interest once the seed cohort is complete. |
++
++What this migration does **not** do (deferred per parent index open questions 3, 6, 7):
++
++- It does not move the action ledger into M2 — `SendNote` lands in M6.
++- It does not implement LMDB persistence for the mailbox cache — M3.
++- It does not implement NIP-77 watermarks — M4.
++- It does not add a per-author indexer-fallback ledger row — open question 3.
++
++The compiler is **in-memory v1** by design. The mailbox cache is the existing `HashMap<String, AuthorRelayList>` (`crates/nmp-core/src/kernel/mod.rs:313`); it just gets a new consumer.
+diff --git a/docs/design/subscription-compilation/diagnostics.md b/docs/design/subscription-compilation/diagnostics.md
+new file mode 100644
+index 0000000..edb9a9c
+--- /dev/null
++++ b/docs/design/subscription-compilation/diagnostics.md
+@@ -0,0 +1,188 @@
++# Subscription Compilation §5 + §8 — Four-Lane Diagnostics
++
++> Parent: `docs/design/subscription-compilation.md`.
++> Read first: ADR-0007 (`docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md`) — every record here extends ADR-0007 types; it does not replace them.
++
++The compiler's routing decisions are the most subtle correctness surface in the M2 milestone. They are also the easiest to silently get wrong (`docs/design/ndk-applesauce-lessons.md` §3, "automatic behaviour also needs strong tests"). Diagnostics make the four sources of relay knowledge legible — separately, never collapsed.
++
++## 5.0 The four lanes
++
++Per `docs/design/ndk-applesauce-lessons.md` §4 (lines 39–46) and `docs/aim.md` §6 doctrine 10 ("provenance preserved"), the four relay-fact lanes are:
++
++1. **NIP-65** — a pubkey's declared relay preferences (kind:10002).
++2. **Hint** — relay URLs embedded in events or NIP-19 pointers (`e`/`a` tag third slot, `nevent`'s relay vector, etc.).
++3. **Provenance** — relays we have actually observed an event arriving from.
++4. **User-configured** — local-policy relays added by the user/operator, plus the kernel-configured indexer fallback set.
++
++Each lane is its own record stream. They never merge into a single "relays" field — that collapse is exactly the bug `docs/design/ndk-applesauce-lessons.md` §4 line 46 forbids. They may be displayed side-by-side in a diagnostic view; the actor stores them apart.
++
++This is structurally enforced: there is no `Vec<RelayUrl>` field on any compiler output type. Every relay-bearing field carries a `lane: RelayFactLane` discriminator.
++
++```rust
++// crates/nmp-core/src/kernel/diagnostics/lanes.rs (proposed)
++
++#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
++pub enum RelayFactLane {
++    Nip65,
++    Hint,
++    Provenance,
++    UserConfigured,
++}
++```
++
++## 5.1 Per-lane record schemas
++
++Each lane has one record type. All four are exposed to the platform via the existing ADR-0007 `ViewBatch` lane (low-cadence, coalesced to 1–4 Hz per ADR-0007 "How status crosses the bridge"). They feed into the diagnostics screen, not into normal product UI.
++
++### Lane 1 — `Nip65RelayFact`
++
++```rust
++pub struct Nip65RelayFact {
++    pub pubkey: Pubkey,
++    pub relay_url: RelayUrl,
++    pub roles: Nip65Roles,                    // read | write | both
++    pub kind10002_event_id: EventId,           // provenance of the kind:10002
++    pub kind10002_created_at: UnixSeconds,
++    pub kind10002_seen_from: Vec<RelayUrl>,    // which relays delivered it
++    pub freshness: FreshnessHint,              // recent / hours_old / days_old / never_verified
++}
++
++pub struct Nip65Roles {
++    pub read: bool,
++    pub write: bool,
++}
++```
++
++Emitted whenever `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`) replaces a mailbox entry. One record per `(pubkey, relay_url)` pair; an author with 4 declared relays produces 4 records on each update.
++
++### Lane 2 — `HintRelayFact`
++
++```rust
++pub struct HintRelayFact {
++    pub relay_url: RelayUrl,
++    pub source: HintSource,
++    pub freshness_ms: u64,                     // monotonic from observation
++    pub recently_succeeded: bool,              // last attempt produced ≥1 EVENT
++}
++
++pub enum HintSource {
++    EventTag    { event_id: EventId, tag: TagKey, position: u8 },
++    Nip19       { pointer: String /* nevent1.../naddr1... */ },
++    UserConfig  { config_path: String },        // for hints injected via config
++}
++```
++
++Emitted by the pointer loader (post-M2; for M2 the field exists but is rarely populated — only `e`/`a`-tag third-slot hints from thread-view hydration fill it). Per-event hints are de-duplicated; an event whose `e` tag contains a hint URL produces one `HintRelayFact` per (relay_url, source) pair.
++
++### Lane 3 — `ProvenanceRelayFact`
++
++```rust
++pub struct ProvenanceRelayFact {
++    pub relay_url: RelayUrl,
++    pub event_id: EventId,
++    pub seen_at_ms: u64,
++    pub wire_sub_id: String,                   // which REQ delivered it
++    pub kind: u32,
++    pub author: Pubkey,
++}
++```
++
++Emitted by `handle_event` (`crates/nmp-core/src/kernel/ingest.rs:134-164`) for every EVENT arrival. This is the highest-cardinality lane and the only one where coalescing matters at the ADR-0007 boundary: the platform diagnostic view consumes a summarised projection (`ProvenanceSummary` per author or per event), not the raw fact stream.
++
++### Lane 4 — `UserConfiguredRelayFact`
++
++```rust
++pub struct UserConfiguredRelayFact {
++    pub relay_url: RelayUrl,
++    pub category: UserConfiguredCategory,
++    pub generation: u64,                       // config version; matches Trigger::*Changed
++    pub added_at_ms: u64,
++}
++
++pub enum UserConfiguredCategory {
++    AccountRead,                                // user's own read relays (overrides NIP-65 read)
++    AccountWrite,                               // user's own write relays
++    Indexer,                                    // kernel indexer set member
++    Debug,                                      // operator-injected for testing
++}
++```
++
++Emitted on `Trigger::UserConfiguredRelaysChanged` / `Trigger::IndexerSetChanged`. Low-cardinality, low-cadence.
++
++## 5.2 What the compiler may *combine*; what stays distinct
++
++The compiler may *use* facts from multiple lanes to compute a routing decision; it may **not** present them as one. Concretely:
++
++- **Routing decision** (`AuthorRouting.source` from [compiler.md](compiler.md) §3.1): records *which lane* the relay set was derived from. Single-valued; one of `Nip65 | Hint | Indexer | UserConfigured`. The author may have facts in three lanes; the compiler picks one, says so, and the other lane records remain visible.
++- **`RelayPlan.role_tags`** is a `BTreeSet<RoutingSource>` because a single relay may be in the plan for multiple reasons (e.g. NIP-65 for author A + user-configured fallback for everyone). The set discriminates, it does not collapse.
++- **The platform diagnostic view** receives all four lanes as separate `ViewBatch` records. The UI may render them in one screen with four side-by-side columns, but the data path is four lanes.
++
++A test (§9) asserts that no compiler output type has a field of type `Vec<RelayUrl>` without an adjacent `RelayFactLane`. That is the structural enforcement.
++
++## 5.3 Lane interactions
++
++The lanes inform each other through these well-defined hooks:
++
++- `Provenance → NIP-65 hint refresh.` If we observe many `ProvenanceRelayFact { relay_url: R, author: A }` records but no `Nip65RelayFact { pubkey: A, relay_url: R }`, the operator diagnostic can suggest "we are receiving A's events from R but A has not declared R; their kind:10002 may be stale." This is a future operator-UI affordance, not a behaviour.
++- `Hint → planner suggestion.` `HintRelayFact` with `recently_succeeded: true` may surface in the diagnostic view as "you might want to add this to your indexer set." Again, not automatic.
++- `User-configured` is the **only** lane the compiler treats as authoritative-by-policy (the user said so). Open question 5 in the parent index resolves the augment-vs-override precedence between NIP-65 and user-configured for the active account.
++
++The lesson the four-lane discipline preserves: routing is **derivable but contested** evidence. Collapsing the lanes loses information; preserving them lets the diagnostic answer "why did we route this REQ to that relay?" months after the decision.
++
++---
++
++# §8 — Reverse-relay-coverage diagnostic view
++
++> The inverse question. For any relay we are talking to, *whose* timeline does it serve?
++
++This is one specific `ViewModule` that consumes the four-lane fact streams plus the compiler's `RelayPlan`s and produces a per-relay summary.
++
++## 8.1 Spec, payload, dependencies
++
++```rust
++pub struct RelayCoverageSpec {
++    pub relay_url: RelayUrl,
++}
++
++pub struct RelayCoveragePayload {
++    pub relay_url: RelayUrl,
++    pub serving_authors: u32,
++    pub author_examples: Vec<Pubkey>,      // first N (configurable, default 16)
++    pub by_lane: ByLaneCounts,
++    pub wire_sub_count: u32,
++    pub last_event_at_ms: Option<u64>,
++    pub provenance_count_last_minute: u32,
++}
++
++pub struct ByLaneCounts {
++    pub nip65: u32,             // authors for whom relay is in their NIP-65 set
++    pub hint: u32,              // authors for whom we routed here via hints
++    pub user_configured: u32,   // authors served via user-config
++    pub indexer_fallback: u32,  // authors with no mailbox, served via indexer
++}
++
++// `ViewModule::dependencies` returns:
++//   - Mailbox cache updates touching any author in our timeline
++//   - RelayPlan updates touching `relay_url`
++//   - Provenance facts on `relay_url` (rate-limited; only the count, not individual events)
++```
++
++## 8.2 Implementation outline
++
++The view's `reduce` consumes three input streams:
++
++1. `Nip65RelayFact` records — increments/decrements `by_lane.nip65` per (relay_url, pubkey) membership.
++2. `CompiledPlan` re-emissions — every plan recompile produces a `(plan_id, relay_url) → authors` projection that this view subscribes to. The compiler exposes this projection as `RelayAuthorCoverage` in the kernel's projection cache (per `docs/design/reactivity/view-deltas-and-projections.md`).
++3. `ProvenanceRelayFact` records — feeds the rolling 60-second counter for `provenance_count_last_minute`.
++
++This is the M2 exit-gate diagnostic listed in `docs/plan.md` line 130 ("Reverse-relay-coverage view for diagnostics: 'this relay is serving N authors of our timeline.'").
++
++## 8.3 Cardinality and emission cadence
++
++One `RelayCoverageSpec`/relay → ≤ N records, where N is the number of relays currently in the planner's union of `RelayPlan`s. For typical Nostr usage that is in the low tens; rendering all of them on one diagnostic screen is fine.
++
++Emission cadence follows ADR-0007's diagnostic-view rule: material-transition immediately, otherwise 1–4 Hz. The provenance counter ticks every second; the `by_lane` counts only emit on `CompiledPlan` recompiles or new mailbox arrivals.
++
++## 8.4 Why it lives in diagnostics, not in product UI
++
++Per `docs/aim.md` §4.4 ("the developer does not pick relays per operation; the framework does") and ADR-0007's domain-of-diagnostics separation, end-user product UIs do not show "relay X is serving 12 authors." That is operator/debug surface. Normal apps consume the `LogicalInterestStatus` summaries; `RelayCoveragePayload` is for the diagnostics screen (proof iOS app screenshot in `docs/perf/m2/outbox-routing.md` per `docs/plan.md` line 139).
+diff --git a/docs/design/subscription-compilation/intro.md b/docs/design/subscription-compilation/intro.md
+new file mode 100644
+index 0000000..b23699c
+--- /dev/null
++++ b/docs/design/subscription-compilation/intro.md
+@@ -0,0 +1,132 @@
++# Subscription Compilation §1–§2 — Problem and Logical Interest Model
++
++> Parent: `docs/design/subscription-compilation.md`.
++> Read first: `docs/design/ndk-applesauce-lessons.md` §7 (subscription compilation lessons), §2 (NDK outbox lessons), §9 (NMP principles).
++
++## 1. Problem — what is wrong with the current planner
++
++The kernel today encodes "where a REQ goes" as a two-valued enum and resolves it at the call site of every request. Concretely:
++
++- **Two hardcoded relays.** `crates/nmp-core/src/relay.rs:1-2` declares `CONTENT_RELAY_URL = "wss://relay.primal.net"` and `INDEXER_RELAY_URL = "wss://purplepag.es"` as module-level constants. There is no per-author routing.
++- **Relay choice is a 2-variant enum, not a URL set.** `crates/nmp-core/src/relay.rs:15-39` defines `RelayRole::{Content, Indexer}` with a `.url() -> &'static str` that returns one of the two literals. This shape cannot express "this REQ should go to the union of these N authors' write relays."
++- **The seam that emits REQs is parameterized by `RelayRole`.** `crates/nmp-core/src/kernel/requests.rs:530-556` (`req()`) inserts a `WireSub { role, .. }` keyed by a string sub-id and emits `OutboundMessage { role, text }`. The role *is* the routing decision; there is no relay-URL field on `WireSub` or `OutboundMessage`. Any compiler that fans an interest out across N URLs has to replace this helper.
++- **Startup REQs ignore mailboxes by construction.** `crates/nmp-core/src/kernel/requests.rs:50-106` (`startup_requests`) issues six fixed REQs, each pinned to `Content` or `Indexer`. The seed-bootstrap timeline (line 65–70) fans seven hundred-author future timelines through one relay. The exit-gate test for M2 (`docs/plan.md` lines 134–137) requires that this fan exactly equal the union of those authors' write relays.
++- **View-open REQs ignore mailboxes too.** `crates/nmp-core/src/kernel/requests.rs:404-439` (`author_requests`) hardcodes a three-REQ shape — `author-relays-N` on Indexer, `author-profile-N` on Indexer, `author-notes-N` on Content. The author's notes are fetched from the global content relay even though by the time the view opens we may already have that author's kind:10002 in cache (see next bullet).
++- **Mailbox cache exists but no consumer.** `crates/nmp-core/src/kernel/ingest.rs:209-233` (`ingest_relay_list`) already parses kind:10002 into `self.author_relay_lists: HashMap<String, AuthorRelayList>` (declared at `crates/nmp-core/src/kernel/mod.rs:269-275` and reserved at `mod.rs:313`). The cache is written; **nothing reads it for routing**. This is the bug doctrine D5 ("capabilities report, never decide") inverted: we have the data, we ignore it.
++- **Profile claim path is single-relay.** `crates/nmp-core/src/kernel/requests.rs:390-402` (`profile_claim_request`) sends a kind:0 fetch to `RelayRole::Indexer` unconditionally. It cannot consult mailboxes for the claimed author.
++- **No publish path exists yet.** `crates/nmp-core/src/kernel/requests.rs:30` (no occurrences of `EVENT` outbound) and `crates/nmp-core/src/relay.rs:42-45` (`OutboundMessage` carries only role + text). The first publish action (M6 `SendNote`) will hit this same `req()`-style seam. M2 must establish the planner shape before M6 builds the first user of it; the doctrine "no developer-supplied relays for a publish" (`docs/aim.md` §6 doctrine 5; `docs/product-spec/subsystems.md` §7.3 row "Publish leaked to wrong relays") needs a structural enforcement point.
++
++The summary diagnosis: **the planner is a string formatter, not a compiler.** Every REQ is a per-call-site decision; routing is one of two literals; recompilation is impossible because nothing is compiled. The diagnostics in `crates/nmp-core/src/kernel/mod.rs:117-154` already type `RelayStatus` / `WireSubscriptionStatus` / `LogicalInterestStatus` per ADR-0007 — but the planner currently emits at most one `LogicalInterestStatus` per view kind because there is no logical-interest object to scope it against.
++
++## 2. The logical interest model
++
++A **logical interest** is the actor-internal, semantics-preserving description of what a view, action, or monitor wants the kernel to keep alive on the wire. It is the input to compilation. It is *not* a Nostr filter (a filter is one possible wire artifact a plan can produce — `docs/design/ndk-applesauce-lessons.md` §7 lines 89–90).
++
++### 2.1 Formal shape
++
++```rust
++// crates/nmp-core/src/kernel/planner/interest.rs (proposed)
++
++/// A logical interest is what a kernel-side consumer (view, action, monitor,
++/// sync job, pointer loader) wants alive on the wire. The compiler turns N
++/// logical interests into M ≤ N per-relay plans.
++#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
++pub struct LogicalInterest {
++    /// Stable identity assigned by the registry on first insertion. Survives
++    /// recompilation. Two interests with identical content collide on hash but
++    /// keep distinct ids if they were registered by distinct claims.
++    pub id: InterestId,
++
++    /// Scope decides how mailbox resolution and indexer fallback behave.
++    /// Account-scoped interests resolve via the active account's mailbox view
++    /// for ambiguous filters (e.g. interests with no `authors` and no `#p`).
++    pub scope: InterestScope,
++
++    /// What the consumer wants. This is a normalised filter set, not a Nostr
++    /// wire filter. Tags use sorted vec representation so equality is stable.
++    pub shape: InterestShape,
++
++    /// Routing hints the consumer wants honoured. The compiler may ignore
++    /// hints if they conflict with policy (e.g. private-publish privacy).
++    pub hints: Vec<RelayHint>,
++
++    /// Lifecycle: tailing means "stay open after EOSE"; one_shot closes on
++    /// EOSE. Window is the planner's intent, not necessarily the relay
++    /// `since`/`until` it ends up emitting.
++    pub lifecycle: InterestLifecycle,
++}
++
++pub enum InterestScope {
++    /// Bound to the active account in SessionState. Re-routes on account switch.
++    ActiveAccount,
++    /// Bound to a specific account (multi-account UIs, M8). Re-routes on that
++    /// account's mailbox refresh; ignored on account switch.
++    Account(AccountId),
++    /// No account context. Used for global pointer loaders, NIP-19-driven
++    /// fetches, and indexer-direct probes.
++    Global,
++}
++
++pub struct InterestShape {
++    pub authors:    BTreeSet<Pubkey>,        // empty = wildcard
++    pub kinds:      BTreeSet<u32>,           // empty = wildcard (rare)
++    pub tags:       BTreeMap<TagKey, BTreeSet<String>>,  // sorted for hash stability
++    pub since:      Option<UnixSeconds>,
++    pub until:      Option<UnixSeconds>,
++    pub limit:      Option<u32>,
++    pub event_ids:  BTreeSet<EventId>,       // for pointer/thread hydration
++}
++
++pub enum InterestLifecycle {
++    Tailing,                                   // stays open after EOSE
++    OneShot,                                   // CLOSE on EOSE
++    BoundedTime { until_ms: u64 },             // CLOSE on EOSE or deadline
++}
++```
++
++`InterestShape` mirrors the Nostr filter shape closely on purpose: most logical interests correspond directly to a single filter, and the kernel ships canonical normalisation (sort, dedup, fold ranges) so equality and hashing are deterministic. The compiler is then free to merge two shapes (or refuse to) on the basis of structural compatibility (§3 step 3).
++
++### 2.2 How view modules express interests
++
++Every `ViewModule` (per `docs/design/kernel-substrate.md` §3) declares its dependencies via the existing `ViewDependencies` mechanism. M2 adds one new method:
++
++```rust
++pub trait ViewModule {
++    // ... existing methods ...
++
++    /// Translate a view spec into the logical interests required to keep it
++    /// live. Called by the planner when the view opens; re-called on
++    /// recompilation triggers that the view module opts into (account-switch,
++    /// mailbox refresh).
++    fn interests(spec: &Self::Spec, ctx: &InterestContext)
++        -> Vec<LogicalInterest>;
++}
++```
++
++`InterestContext` exposes read access to the mailbox cache and to the active account but **not** to the relay set; the view module never names relays. That keeps `nmp-aim.md` doctrine 5 ("outbox routing automatic; manual relay selection is the opt-out, not the default") structurally enforced — there is no place for a view module to write a URL.
++
++Concrete examples for the existing seed-timeline path:
++
++- `TimelineView { authors: [pablof7z, fiatjaf, jb55, ...follows] }` returns one `LogicalInterest { shape: { authors, kinds: {1, 6}, limit: 200 }, lifecycle: Tailing }`.
++- `AuthorView { pubkey }` returns three interests: kind:10002 (Indexer fallback policy, see §3), kind:0 (one-shot), and `{ authors: [pubkey], kinds: {1, 6}, limit: 100 }` (Tailing).
++- `ProfileClaim { pubkey }` (the refcounted UI path from `crates/nmp-core/src/kernel/requests.rs:202-237`) returns one interest: `{ authors: [pubkey], kinds: {0}, limit: 1, lifecycle: OneShot }`.
++- `ThreadView { event_id }` returns up to two interests: `{ ids: [...] }` for context, `{ kinds: {1, 6}, tags: { #e: [...] } }` for replies.
++
++The seed-bootstrap path (`crates/nmp-core/src/kernel/requests.rs:50-106`) becomes one `LogicalInterest` per concern, registered at actor `Start` rather than emitted as raw REQs. The compiler produces the wire artifacts.
++
++### 2.3 Account scope binding
++
++The kernel `SessionState` (`docs/product-spec/subsystems.md` §7.4; M8 graduates this to multi-account) carries an active account id. `InterestScope::ActiveAccount` resolves at compile time, not at registration time. On account switch (§4 trigger A4), the compiler re-evaluates every `ActiveAccount`-scoped interest against the new active account's mailbox view. This is the structural enforcement of bug-extinction #5 (account-context overlap) referenced in `docs/plan.md` line 284 — the kernel cannot "forget" to re-route because every plan re-derives from the active scope.
++
++Account-scoped interests with empty `authors` and empty `#p` (e.g. a free-form hashtag firehose) resolve against the active account's *read relays* (NIP-65 read side) — the user's own subscription preferences, not a globally hardcoded relay. Today's `firehose_requests()` at `crates/nmp-core/src/kernel/requests.rs:357-372` hardcodes `RelayRole::Content`; under the compiler this becomes "active-account read relays, falling back to indexer set if the active account has no kind:10002."
++
++### 2.4 What is *not* a logical interest
++
++To keep the surface small, the following are explicitly **not** logical interests:
++
++- A **wire REQ**. Wire REQs are produced by the compiler; they live in `WireSubscriptionStatus` per ADR-0007.
++- A **publish**. Publishes are durable actions on the action ledger (`docs/design/kernel-substrate.md` §4); they consult the `PublishPlanner` (§7) but they are not interests because they do not stay alive.
++- A **diagnostic record**. ADR-0007 lanes are facts derived from the planner's state, not inputs.
++- An **HTTP fetch** (Blossom upload, indexer JSON probe). Those are `CapabilityModule` requests.
++
++The boundary is intentional: an interest is anything that asks the planner to *keep a REQ open*. Everything else routes through a different seam.
+diff --git a/docs/design/subscription-compilation/nip65.md b/docs/design/subscription-compilation/nip65.md
+new file mode 100644
+index 0000000..8a24f05
+--- /dev/null
++++ b/docs/design/subscription-compilation/nip65.md
+@@ -0,0 +1,221 @@
++# Subscription Compilation §6 — `nmp-nip65` Crate Layout
++
++> Parent: `docs/design/subscription-compilation.md`.
++> Read first: `docs/design/kernel-substrate.md` §3 (`ViewModule`) for the trait this crate implements; `docs/design/app-extension-kernel.md` §3 layering table — `nmp-nip65` is a **protocol module**, not an app module.
++
++`nmp-nip65` is the first NMP protocol module (per the v1 reference-modules list in `docs/design/kernel-substrate.md` §11) whose job is *not* to expose product views. It exists primarily as **the canonical source of the mailbox cache** the compiler consults. It contributes one `ViewModule` (for app-side rendering of "this user's relay list") and a small public API the compiler imports directly without going through the FFI surface.
++
++## 6.1 File structure
++
++```
++crates/nmp-nip65/
++├── Cargo.toml
++├── README.md
++├── src/
++│   ├── lib.rs              # re-exports + crate-level documentation
++│   ├── module.rs           # impl ViewModule for MailboxesView
++│   ├── parse.rs            # kind:10002 tag parsing (extracted from kernel)
++│   ├── cache.rs            # MailboxCache trait + InMemory impl
++│   ├── routing.rs          # AuthorRouting, RoutingSource, mailbox lookup API
++│   └── tests/
++│       ├── parse.rs        # round-trip + edge-case tag parsing
++│       ├── routing.rs      # mailbox → relay-set resolution scenarios
++│       └── snapshot.rs     # cache snapshot/restore for compiler-input tests
++└── tests/                  # integration tests against the in-memory cache
++```
++
++Soft target per file: ≤ 300 LOC (AGENTS.md). The crate stays small; everything heavier (filter compilation, indexer probes) lives in `nmp-core::kernel::planner`, not here.
++
++## 6.2 Traits implemented
++
++`nmp-nip65` implements exactly one extension trait family: `ViewModule`. It does *not* implement `ActionModule` (kind:10002 publish is the user's own "update my relay list" action, deferred to M6's action ledger; in this milestone it has no write surface). It does *not* implement `DomainModule` (mailbox records live in the kernel-owned mailbox cache; they are queryable Nostr events, not durable app-defined records).
++
++### `MailboxesView` (`impl ViewModule`)
++
++```rust
++// crates/nmp-nip65/src/module.rs
++
++pub struct MailboxesView;
++
++#[derive(Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
++pub struct MailboxesSpec {
++    pub pubkey: Pubkey,
++}
++
++#[derive(Clone, Serialize)]
++pub struct MailboxesPayload {
++    pub pubkey: Pubkey,
++    pub read:  Vec<RelayUrl>,
++    pub write: Vec<RelayUrl>,
++    pub both:  Vec<RelayUrl>,
++    pub created_at: UnixSeconds,           // 0 if unknown
++    pub source: MailboxSource,
++}
++
++pub enum MailboxSource {
++    Cached    { freshness: FreshnessHint },
++    Fetching,
++    Unknown,
++}
++
++impl ViewModule for MailboxesView {
++    const NAMESPACE: &'static str = "nip65.mailboxes";
++    type Spec    = MailboxesSpec;
++    type Payload = MailboxesPayload;
++    type Delta   = MailboxesPayload;       // payloads are small; emit whole snapshots
++    type Key     = Pubkey;
++    type State   = MailboxesPayload;
++
++    fn key(spec: &MailboxesSpec) -> Pubkey {
++        spec.pubkey.clone()
++    }
++
++    fn dependencies(spec: &MailboxesSpec) -> ViewDependencies {
++        ViewDependencies::author_kind(&spec.pubkey, 10002)
++    }
++
++    fn interests(spec: &MailboxesSpec, ctx: &InterestContext) -> Vec<LogicalInterest> {
++        vec![LogicalInterest {
++            id: ctx.fresh_id(),
++            scope: InterestScope::Global,
++            shape: InterestShape {
++                authors: btreeset![spec.pubkey.clone()],
++                kinds:   btreeset![10002],
++                limit:   Some(1),
++                ..Default::default()
++            },
++            hints: vec![],
++            lifecycle: InterestLifecycle::OneShot,
++        }]
++    }
++
++    fn open(ctx: &ViewContext, spec: MailboxesSpec) -> (Self::State, Self::Payload) {
++        let snapshot = ctx.mailbox_cache().get(&spec.pubkey);
++        let payload = MailboxesPayload::from_snapshot(spec.pubkey, snapshot);
++        (payload.clone(), payload)
++    }
++
++    fn on_event_inserted(ctx: &ViewContext, st: &mut Self::State, ev: &Event)
++        -> Option<Self::Delta>
++    {
++        if ev.kind != 10002 || ev.pubkey != st.pubkey { return None; }
++        let parsed = parse::parse_relay_list(ev.created_at, &ev.tags);
++        if parsed.created_at < st.created_at { return None; }
++        *st = MailboxesPayload::from_parsed(st.pubkey.clone(), parsed);
++        Some(st.clone())
++    }
++
++    // on_event_removed / replaced / projection_changed / on_tick: defaults
++    fn snapshot(_ctx: &ViewContext, st: &Self::State) -> Self::Payload {
++        st.clone()
++    }
++}
++```
++
++The view exists so platform code can render "alice@example uses these relays" using the same path as any other view (`useMailboxes(pubkey)`); it is *not* the compiler's source of truth. The compiler reads `MailboxCache` directly.
++
++## 6.3 Public surface (compiler-facing, not FFI-facing)
++
++```rust
++// crates/nmp-nip65/src/cache.rs
++
++pub trait MailboxCache: Send + Sync {
++    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot>;
++    fn put(&mut self, pubkey: Pubkey, snapshot: MailboxSnapshot)
++        -> CachePutResult;
++    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)>;
++    fn generation(&self) -> u64;           // monotonic; advances on every accepted put
++}
++
++pub enum CachePutResult {
++    Inserted,
++    ReplacedNewer { prior_created_at: UnixSeconds },
++    RejectedStale { current_created_at: UnixSeconds },
++}
++
++#[derive(Clone, Debug)]
++pub struct MailboxSnapshot {
++    pub kind10002_event_id: EventId,
++    pub created_at: UnixSeconds,
++    pub read:  Vec<RelayUrl>,
++    pub write: Vec<RelayUrl>,
++    pub both:  Vec<RelayUrl>,
++    pub seen_from: Vec<RelayUrl>,          // ProvenanceRelayFact seed
++}
++
++pub struct InMemoryMailboxCache { /* HashMap<Pubkey, MailboxSnapshot> */ }
++impl MailboxCache for InMemoryMailboxCache { /* ... */ }
++```
++
++The `MailboxCache` trait is the seam M3 (LMDB persistence) replaces with a backing-store-aware implementation. The compiler does not know which backend it is reading.
++
++```rust
++// crates/nmp-nip65/src/routing.rs
++
++pub fn resolve_author_outbox(
++    cache: &dyn MailboxCache,
++    user_configured: &UserConfiguredRelays,
++    indexer_set: &[RelayUrl],
++    author: &Pubkey,
++) -> AuthorRouting { /* ... */ }
++
++pub fn resolve_author_inbox(
++    cache: &dyn MailboxCache,
++    user_configured: &UserConfiguredRelays,
++    indexer_set: &[RelayUrl],
++    author: &Pubkey,
++) -> AuthorRouting { /* ... */ }
++```
++
++These are the two pure functions [compiler.md](compiler.md) Stage 1 calls per author. They return `AuthorRouting` with the `RoutingSource` tag set per the four-lane discipline ([diagnostics.md](diagnostics.md) §5.2). Test fixtures live in `crates/nmp-nip65/src/tests/routing.rs`; the same fixtures plug into the audit gate (§9).
++
++```rust
++// crates/nmp-nip65/src/parse.rs
++
++pub fn parse_relay_list(created_at: UnixSeconds, tags: &[Vec<String>])
++    -> ParsedRelayList;
++```
++
++This is the function currently inlined as a free fn in `crates/nmp-core/src/kernel/nostr.rs` (referenced by `kernel/ingest.rs:210` and tested in `kernel/tests.rs:150`). M2 moves it here and re-exports from `nmp-core` for compatibility during the migration.
++
++## 6.4 What `nmp-nip65` does *not* contain
++
++By design, to keep the kernel boundary clean (per `docs/design/app-extension-kernel.md` §3):
++
++- **No publish action.** Updating a user's own kind:10002 is `nmp-nip01::UpdateRelayList` (M6); that action depends on `nmp-nip65::parse` to validate the local copy before publishing.
++- **No outbox routing policy.** The decision "publish goes to author write relays + recipient inbox relays" is the publish planner ([outbox.md](outbox.md) §7), not this crate. This crate provides the lookups; the policy lives in `nmp-core::kernel::planner::publish`.
++- **No indexer-probe scheduling.** Probes are kernel-side; this crate is unaware of probe lifecycle.
++- **No FFI types.** `MailboxesPayload` is exposed at FFI via the per-app generated enum (per ADR-0010 codegen); the crate itself ships pure Rust.
++
++## 6.5 Module composition (per `docs/design/kernel-substrate.md` §8)
++
++`nmp-nip65` consumes:
++
++- `nmp-core::substrate::{ViewModule, ViewContext, InterestContext, LogicalInterest, ...}` — kernel trait surface.
++- `nmp-core::kernel::projections` — for reading kind:10002 events out of the event store (the compiler's input).
++
++`nmp-nip65` is consumed by:
++
++- `nmp-core::kernel::planner` — for `MailboxCache`, `resolve_author_outbox/inbox`, `parse_relay_list`.
++- Future `nmp-nip01::UpdateRelayList` (M6).
++- Future `nmp-nip17` (M9) — DM publish path imports `resolve_author_inbox` for recipient lookups.
++- The per-app generated enum — `MailboxesView` becomes one variant of `ViewSpec` in `nmp-app-<name>` per ADR-0010.
++
++## 6.6 Cargo manifest sketch
++
++```toml
++[package]
++name = "nmp-nip65"
++version = "0.0.1"
++edition = "2021"
++
++[dependencies]
++nmp-core   = { path = "../nmp-core" }
++serde      = { workspace = true, features = ["derive"] }
++serde_json = { workspace = true }
++
++[dev-dependencies]
++nmp-testing = { path = "../nmp-testing" }
++```
++
++No `nostr-sdk` dependency: this crate operates on parsed `Event` structs from `nmp-core`'s already-vetted ingest path. Avoiding a duplicate parse dependency keeps the surface auditable.
+diff --git a/docs/design/subscription-compilation/outbox.md b/docs/design/subscription-compilation/outbox.md
+new file mode 100644
+index 0000000..a3d121a
+--- /dev/null
++++ b/docs/design/subscription-compilation/outbox.md
+@@ -0,0 +1,224 @@
++# Subscription Compilation §7 — Outbox Routing as a Planner Subsystem
++
++> Parent: `docs/design/subscription-compilation.md`.
++> Read first: `docs/product-spec/subsystems.md` §7.3 (outbox routing algorithm); `docs/aim.md` §4.4 ("outbox / smart relay routing") and §6 doctrines 5 and 10.
++
++This section defines the **publish-side seam** the M2 milestone lands so the M6 write path has a ready surface. There is no publish code in the repo today (`crates/nmp-core/src/kernel/requests.rs` contains no `EVENT` outbound; the relay worker has no publish channel). M2 lands the trait and the override action; M6 writes the first concrete consumer (`SendNoteAction`).
++
++This is the framing the parent index calls out as "design seam now, first concrete consumer in M6." Without this seam, M6 risks reinventing outbox routing inline.
++
++## 7.1 The `PublishPlanner` trait
++
++```rust
++// crates/nmp-core/src/kernel/planner/publish.rs (proposed)
++
++#[async_trait]
++pub trait PublishPlanner: Send + Sync {
++    /// Compute the relay set for publishing a signed event. Pure (no side
++    /// effects); the caller (action ledger) feeds the result into the
++    /// per-relay publish state machine.
++    fn plan_publish(
++        &self,
++        event: &Event,
++        privacy: PublishPrivacy,
++        override_: Option<PublishOverride>,
++    ) -> Result<PublishPlan, PublishPlanError>;
++}
++
++pub struct PublishPlan {
++    pub plan_id: PublishPlanId,                // hashes event coords + chosen relays
++    pub assignments: Vec<PublishAssignment>,
++    pub required_success_count: u8,            // ledger acceptance threshold
++    pub deadline_ms: u64,
++}
++
++pub struct PublishAssignment {
++    pub relay_url: RelayUrl,
++    pub reason: PublishRouteReason,            // which lane motivated this relay
++    pub privacy_role: PrivacyRole,             // for the audit log; not policy
++}
++
++pub enum PublishRouteReason {
++    AuthorWriteRelay  { lane: RelayFactLane },     // Nip65 or UserConfigured
++    RecipientInbox    { recipient: Pubkey, lane: RelayFactLane },
++    Hint              { source: HintSource },
++    Override          { audit: String },           // see §7.4
++}
++
++pub enum PrivacyRole {
++    Author,        // this relay is in the plan as the author's outbox
++    Recipient,     // this relay is in the plan as a recipient's inbox
++    Both,          // single relay is both
++}
++
++#[derive(Clone, Debug)]
++pub enum PublishPrivacy {
++    /// Public events (kind:1, kind:0, kind:3, kind:10002, kind:7, ...).
++    /// Falls back to indexer set if author has no write relays.
++    Public,
++    /// Private/gift-wrapped events (kind:1059 wrapping NIP-44). Fails closed
++    /// if any recipient has no inbox relays.
++    PrivateToRecipients { recipients: Vec<Pubkey> },
++    /// Notifications (kind:1 with `#p` tags, reactions, zaps, replies that
++    /// the author wants to surface to the tagged pubkey). Combines author
++    /// write + each `#p` inbox.
++    PublicWithNotifications { notify: Vec<Pubkey> },
++}
++
++#[derive(Clone, Debug)]
++pub enum PublishPlanError {
++    NoAuthorRelays { author: Pubkey, lane_facts: ByLaneCounts },
++    PrivateRecipientUnroutable { recipient: Pubkey },
++    OverrideRejected { reason: String },
++}
++```
++
++The trait is consumed by the action ledger (per `docs/design/kernel-substrate.md` §4 — the kernel owns "per-relay publish attempts" provenance). When an `ActionModule::reduce` reaches its publish step, it calls the planner, gets a `PublishPlan`, and the kernel fans out to relays with the standard ledger-correlated retry/cancel semantics.
++
++## 7.2 Default implementation: `Nip65PublishPlanner`
++
++```rust
++// crates/nmp-core/src/kernel/planner/publish_default.rs (proposed)
++
++pub struct Nip65PublishPlanner<'a> {
++    pub mailbox_cache:    &'a dyn MailboxCache,
++    pub user_configured:  &'a UserConfiguredRelays,
++    pub indexer_set:      &'a [RelayUrl],
++    pub active_account:   Option<AccountId>,
++}
++
++impl PublishPlanner for Nip65PublishPlanner<'_> {
++    fn plan_publish(&self, event: &Event, privacy: PublishPrivacy,
++                    override_: Option<PublishOverride>) -> Result<PublishPlan, PublishPlanError>
++    { /* algorithm in §7.3 */ }
++}
++```
++
++This is the only `PublishPlanner` impl shipped in v1. The trait exists so a future `Wot​PublishPlanner` (M13 WoT subsystem) or a sandbox planner used in tests can replace it without touching action-ledger code.
++
++## 7.3 Write fan-out algorithm (per `docs/product-spec/subsystems.md` §7.3)
++
++Inputs: a signed `event`, a `PublishPrivacy` mode, an optional `PublishOverride`.
++
++```
++1. If `override_` is Some:
++     return plan from override (see §7.4); set every PublishRouteReason::Override.
++2. Resolve author write relays:
++     author_outbox = resolve_author_outbox(cache, user_configured, indexer, event.pubkey)
++     If author_outbox.relays is empty:
++         return Err(NoAuthorRelays { ... })  // never fall back to indexer for writes
++3. Match on privacy:
++   a. Public:
++        assignments = [each author_outbox.relays → AuthorWriteRelay { lane }]
++        required_success_count = max(1, ceil(N/3))   // configurable
++   b. PrivateToRecipients { recipients }:
++        For each recipient r:
++            inbox = resolve_author_inbox(cache, user_configured, indexer, r)
++            If inbox.source == Indexer or inbox.relays is empty:
++                return Err(PrivateRecipientUnroutable { recipient: r })
++        assignments = union(each recipient's inbox.relays → RecipientInbox { recipient, lane })
++        // intentionally NO author-write inclusion: private events do not go to public outbox
++        required_success_count = recipients.len() as u8  // at least one per recipient
++   c. PublicWithNotifications { notify }:
++        assignments = author_outbox ∪ union(each notify pubkey's inbox)
++        required_success_count = max(1, ceil(author_outbox.len() / 3))
++4. plan_id = blake3(event.id, sorted assignments)
++5. deadline_ms = now + AppConfig.publish_deadline_ms (default 30_000)
++6. Return PublishPlan { plan_id, assignments, required_success_count, deadline_ms }
++```
++
++Notes on the algorithm:
++
++- **Step 2's "no indexer fallback for writes"** is the structural enforcement of the doctrine `docs/product-spec/subsystems.md` §7.3 line 99: "fall back to indexer set for reads only; do not publish to indexers." A failed Step 2 surfaces in the action ledger as `Failed { reason: NoAuthorRelays }`, which the UI renders as a toast per ADR-0007's `SideEffect` lane.
++- **Step 3(b)'s `Indexer` check** is the structural enforcement of bug-extinction #4 (`docs/plan.md` line 306 — "DM to public: no API path can send a DM to a non-inbox relay"). Indexer-sourced inbox means we have no NIP-65-declared inbox; for private events that is fail-closed. The recipient gets nothing rather than getting a public broadcast.
++- **`required_success_count`** is the threshold below which the ledger marks the publish `PartiallyFailed`. The default ⅓-of-fan-out is tunable per `AppConfig.publish_quorum_ratio`.
++
++## 7.4 The `PublishOverride` escape hatch
++
++The override exists for tests, migration tools, and operator power-user flows. Per `docs/aim.md` §6 doctrine 5 ("manual relay selection is the opt-out, not the default") and `docs/product-spec/subsystems.md` §7.3 line 90 ("explicit overrides are named, one-shot, and debug-flagged in logs"), the override must be:
++
++1. **Named** — its own typed `AppAction` variant, not a hidden parameter on `SendNote`.
++2. **One-shot** — does not persist as a default for future publishes.
++3. **Audited** — emits a `Diagnostic::PublishOverrideUsed { reason, action_id }` on the `SideEffect` lane and writes a debug-level log line on every dispatch.
++4. **Refused for privacy-sensitive modes** — `PublishPrivacy::PrivateToRecipients` rejects an override that adds non-inbox relays. The override may *narrow* a private fan-out to a subset of declared inboxes; it may not *widen* to public relays.
++
++### The override action
++
++```rust
++// crates/nmp-core/src/kernel/actions/publish_override.rs (proposed)
++
++#[derive(Clone, Serialize, Deserialize)]
++pub struct PublishWithOverride {
++    pub inner: AppAction,                    // the underlying publish action
++    pub override_relays: Vec<RelayUrl>,
++    pub override_audit: String,              // human-readable justification
++}
++
++pub struct PublishOverride {
++    pub relays: Vec<RelayUrl>,
++    pub audit:  String,
++}
++
++// In the action ledger:
++impl ActionModule for PublishWithOverride {
++    const NAMESPACE: &'static str = "kernel.publish_override";
++    type Action = PublishWithOverride;
++    type Step   = PublishOverrideStep;
++    type Output = PublishResult;
++
++    fn start(cx: &mut ActionContext, a: Self::Action)
++        -> Result<ActionPlan<Self::Step>, ActionRejection>
++    {
++        // Emit the debug warning immediately. This is the audit trail.
++        cx.emit_side_effect(SideEffect::Diagnostic(
++            Diagnostic::PublishOverrideUsed {
++                action_id: cx.id(),
++                reason: a.override_audit.clone(),
++                relays: a.override_relays.clone(),
++            },
++        ));
++        cx.log_warn(format!(
++            "OUTBOX OVERRIDE used by action {} → {} relays: {}",
++            cx.id(), a.override_relays.len(), a.override_audit
++        ));
++        // ... validate that inner action's privacy mode permits override ...
++    }
++
++    fn reduce(...) { /* delegate to inner action, but pass `override_` to PublishPlanner */ }
++}
++```
++
++The override action's existence is what test #2 in the bug-extinction list (`docs/plan.md` line 134) asserts: "no public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning." The `PublishWithOverride` variant is the *only* `AppAction` that carries a relay set; the audit string is required (compile-time non-optional); the warning fires unconditionally on dispatch.
++
++### Diagnostic shape
++
++```rust
++pub enum Diagnostic {
++    PublishOverrideUsed {
++        action_id: ActionId,
++        reason: String,
++        relays: Vec<RelayUrl>,
++    },
++    // ... other variants ...
++}
++```
++
++This is the SideEffect-lane payload per ADR-0007. The platform diagnostic UI renders an entry on every override; the count over a session is a soft metric (Cargo dependents using overrides routinely should re-examine their flow).
++
++## 7.5 Atomicity contract
++
++Per `docs/design/kernel-substrate.md` §4 ("Atomicity"): the action ledger ensures the action's local store insert (for the signed event) happens in the same actor message as the ledger transition. The publish plan's per-relay attempts are *not* atomic with the local insert — relays may NACK over a long window — but the ledger correlates them.
++
++The bug-extinction #7 test (`docs/plan.md` line 234) — "publish OK / store fail and store OK / publish fail both roll back atomically" — runs against the M6 implementation. The seam M2 lands here must make that test possible. Specifically:
++
++- The publish-fanout step in `PublishWithOverride::reduce` is `AwaitCapability { request: CapabilityRequest::Publish { ... }, next_step }` per the `ActionTransition` enum in `docs/design/kernel-substrate.md` §4. The kernel owns the publish attempts and reports per-relay outcomes back into the next `reduce`.
++- The local store insert happens *before* the publish step (optimistic insert), with rollback on `PartiallyFailed` if `required_success_count` is not met. This matches the "atomic with reversibility" reading of doctrine D4 (single writer per fact).
++
++## 7.6 What M2 does not cover (deferred)
++
++- **Action ledger schema** — `docs/design/kernel-substrate.md` §4 is the design; M6 implements.
++- **Retry policy** — exponential backoff parameters land in M6.
++- **Concurrent publish coalescing** — if two actions publish the same event (a republish), the planner can dedupe to one wire EVENT per relay. Defer to M6 / M7 stress test.
++- **NIP-42 auth challenge during publish** — relays may demand AUTH before accepting an EVENT. Wires up in M5.
++
++The publish-planner trait is intentionally finished enough that the M6 implementation does not need to extend it. That is the seam the milestone gates against.
+diff --git a/docs/design/subscription-compilation/recompilation.md b/docs/design/subscription-compilation/recompilation.md
+new file mode 100644
+index 0000000..d40da24
+--- /dev/null
++++ b/docs/design/subscription-compilation/recompilation.md
+@@ -0,0 +1,159 @@
++# Subscription Compilation §4 — Recompilation Triggers
++
++> Parent: `docs/design/subscription-compilation.md`.
++> Read first: [compiler.md](compiler.md) §3.4 for plan-id semantics.
++
++The compiler is idempotent and pure given `(interest_set, mailbox_snapshot, indexer_set, user_configured_relays)`. Recompilation is therefore safe to run on every trigger; the wire-emitter's diff turns no-op recompilations into zero wire effect.
++
++This section enumerates **every trigger that may cause recompilation** and the actor message shape each one carries. Triggers fan in from three sources: relay ingest, view registry mutations, and operator/user state changes. All of them route through the same actor mailbox (`docs/design/reactivity/loop-and-reverse-index.md`).
++
++## 4.0 Internal vs external triggers
++
++Two trigger classes exist:
++
++- **Internal** triggers are emitted by the actor itself in response to an `InternalEvent`. They are `Trigger::*` enum variants; the planner consumes them off its own internal queue. They have no public dispatch surface.
++- **External** triggers are `AppAction` variants the platform may dispatch directly. There is exactly one — `AppAction::InvalidateCompile { reason }` — to keep the public surface minimal per `docs/aim.md` §6 doctrine 5.
++
++The full list:
++
++| ID | Source | Trigger | Carries |
++|---|---|---|---|
++| A1 | ingest | `Trigger::Nip65Arrived { pubkey, created_at }` | kind:10002 just landed |
++| A2 | view registry | `Trigger::ViewOpened { interest_ids }` | one or more interests just registered |
++| A3 | view registry | `Trigger::ViewClosed { interest_ids }` | warmth grace expired; interests dropped |
++| A4 | session | `Trigger::ActiveAccountChanged { from, to }` | account switch (M8) |
++| A5 | relay worker | `Trigger::RelayReconnected { url }` | socket re-established after backoff |
++| A6 | operator | `AppAction::InvalidateCompile { reason }` | external force-recompile |
++| A7 | config | `Trigger::UserConfiguredRelaysChanged { generation }` | added/removed relay in local config |
++| A8 | config | `Trigger::IndexerSetChanged { generation }` | indexer relay list edited |
++| A9 | auth | `Trigger::RelayAuthStateChanged { url, state }` | NIP-42 transition (M5+) |
++| A10 | session | `Trigger::SignerAvailable { account, signer_id }` | signer-loss-then-recovery (M6+) |
++
++A1–A3 are M2 scope; A4–A10 are interface seams that M2 establishes so later milestones do not have to re-plumb. The compiler treats unknown triggers as `Trigger::Generic`.
++
++## 4.1 Actor message shapes
++
++```rust
++// crates/nmp-core/src/kernel/planner/trigger.rs (proposed)
++
++#[derive(Clone, Debug)]
++pub enum CompileTrigger {
++    Nip65Arrived {
++        pubkey: Pubkey,
++        created_at: UnixSeconds,    // for replay-window skew detection
++    },
++    ViewOpened {
++        interest_ids: Vec<InterestId>,
++    },
++    ViewClosed {
++        interest_ids: Vec<InterestId>,
++        warmth_expired_at_ms: u64,
++    },
++    ActiveAccountChanged {
++        from: Option<AccountId>,
++        to: Option<AccountId>,
++    },
++    RelayReconnected {
++        url: RelayUrl,
++        prior_state: RelayConnectionState,  // for diagnostics
++    },
++    InvalidateCompile {
++        reason: InvalidateReason,
++    },
++    UserConfiguredRelaysChanged {
++        generation: u64,                    // monotonic config rev
++    },
++    IndexerSetChanged {
++        generation: u64,
++    },
++    RelayAuthStateChanged {
++        url: RelayUrl,
++        state: RelayAuthState,              // re-exported from ADR-0007
++    },
++    SignerAvailable {
++        account: AccountId,
++        signer_id: SignerId,
++    },
++}
++
++#[derive(Clone, Debug)]
++pub enum InvalidateReason {
++    DiagnosticsManualRefresh,               // operator UI button
++    TestForceRecompile,                     // nmp-testing harness
++    External(String),                       // catch-all with diagnostic string
++}
++```
++
++`InvalidateReason::TestForceRecompile` is the seam the wire-frame audit gate (§9) drives so the test does not have to fake a kind:10002 arrival to exercise the recompile path.
++
++## 4.2 Trigger semantics
++
++### A1 — Nip65Arrived
++
++Emitted from `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`) when **and only when** the parser decides to replace the prior mailbox entry (the `should_replace` branch at line 218–222). Stale arrivals do not trigger recompilation.
++
++Compiler effect: re-runs Stages 1–4 for every interest that touches `pubkey` either as a member of `shape.authors` or as a member of `shape.tags[#p]`. Other interests stay assigned to their current plan-id slot; the merged plan-id changes (because the mailbox snapshot's `created_at` for `pubkey` advanced) but its `per_relay` content may be identical.
++
++Outbox routing implication: if `pubkey` was previously routed to the indexer fallback set (Stage 2 read-fallback), the compiler now reassigns to the author's declared write relays. The wire-emitter closes the indexer REQ for that author's slice and opens a new REQ on the declared relays. ADR-0007 diagnostics reflect the route source flipping from `Indexer` to `Nip65`.
++
++### A2 — ViewOpened
++
++Emitted by the view registry when a `ViewModule::open` returns a non-empty `Vec<LogicalInterest>` (per [intro.md](intro.md) §2.2). May fire in batches when a screen opens many rows at once.
++
++Batching contract: the actor's planner inbox coalesces consecutive `ViewOpened` triggers within one actor tick into a single recompile pass. This is the existing reactivity batching (`docs/design/reactivity/scheduling-and-data-model.md`) extended to the planner; the M2 implementation respects the same `≤60Hz/view` budget from ADR-0002 by capping recompiles at one per tick regardless of trigger fan-in.
++
++### A3 — ViewClosed
++
++Emitted by the view registry after the warmth grace expires for an interest with refcount = 0. The warmth window is configurable (`AppConfig.view_warmth_ms`, default 30,000 — matching the doctrine in `docs/product-spec/subsystems.md` §7.6 "View warmth"). Closing an interest mid-warmth (e.g. account switch invalidates the prior account's interests) is a separate `ActiveAccountChanged` trigger, not this one.
++
++### A4 — ActiveAccountChanged
++
++M2 establishes the trigger; M8 wires the multi-account state machine that actually emits it. For M2, the trigger fires once at startup with `from: None, to: Some(active)` so the test surface can exercise account-scope binding without waiting for M8.
++
++Compiler effect: every `InterestScope::ActiveAccount` interest is re-resolved as if newly opened. `InterestScope::Account(specific)` interests are untouched. `InterestScope::Global` interests are untouched.
++
++### A5 — RelayReconnected
++
++Emitted by the relay worker (`crates/nmp-core/src/relay_worker.rs`) after a successful re-handshake. Compiler effect: the wire-emitter re-issues the relay's `SubShape` set as REQs to restore tail subscriptions; the compiler does *not* re-merge or re-resolve. This is a pure "replay current plan to one relay" operation, not a real recompilation, but it routes through the same trigger queue so the diagnostic stream sees it.
++
++Per `docs/product-spec/subsystems.md` §7.2 "Reconnect": "the planner restores live REQs and schedules a coverage-aware gap fill. View payloads do not reset." The gap-fill schedule is M4 (NIP-77); for M2 the gap is implicit (live tail resumes without backfill).
++
++### A6 — InvalidateCompile
++
++The single external `AppAction` variant. Useful for:
++
++- Operator diagnostics screens — "Force re-route now."
++- Test harnesses — see §9.
++- Future debugging tools that change runtime config.
++
++Compiler effect: full recompile from scratch, ignoring incremental caches. Plan-id will change iff any input changed since last compile.
++
++### A7 / A8 — User/indexer config changes
++
++Both bump a `generation: u64` so the plan-id picks up the change (per [compiler.md](compiler.md) §3.4). M2 binds the generation counters but does not yet implement a settings UI to mutate them; v1 ships the seams.
++
++### A9 — RelayAuthStateChanged
++
++M5 wires this fully. M2 only models the trigger so the compiler's data-flow shape does not need to change in M5. Compiler effect: marks the relay as "auth-paused" in its `RelayPlan` so the wire-emitter knows to hold REQs until `RelayAuthState::Authenticated`. Open question 6 in the parent index covers where the gate physically lives.
++
++### A10 — SignerAvailable
++
++M6+ trigger. Some interests (private DMs in M9, NIP-42 challenge response in M5) only become routable once a signer is loaded for their account. M2 records the trigger shape; behaviour is no-op pre-M6.
++
++## 4.3 Trigger ordering and idempotence
++
++The actor's planner inbox is a FIFO queue. Order matters only at the granularity of a tick: within a tick, all queued triggers are folded into the compile inputs and one compile runs. Across ticks, recompiles happen in order received.
++
++Idempotence: running the compiler twice in a row with the same inputs yields identical outputs (same `plan_id`, same `per_relay`). The wire-emitter's diff of two identical plans is empty. This is the contract the audit gate in §9 leans on.
++
++## 4.4 What does *not* trigger recompilation
++
++Explicit non-triggers (so future code does not accidentally over-couple):
++
++- **An EVENT arrival on an existing REQ.** The compiler does not care; the view-modules' projections do.
++- **An EOSE on a one-shot interest.** The interest closes via lifecycle; that flows through `ViewClosed`-equivalent path (the registry drops the interest, fires `ViewClosed`).
++- **A profile-claim refcount delta that does not cross 0↔1.** Going from refcount 5 → 4 is invisible to the compiler.
++- **A relay's RTT or bytes-rx counter ticking.** Diagnostics-only.
++- **A new event id surfacing inside a `ThreadView`'s reduce.** The view module re-invokes `interests()` and returns the augmented set; that emits `ViewOpened` for the *new* `InterestId`s, not a full thread-view recompile. The compiler sees only the additive delta.
++
++These non-triggers keep the recompile cadence aligned with material routing changes, not with event throughput. That is what protects against the "subscription churn under firehose load" failure mode the NDK/Applesauce lessons explicitly warn against (`docs/design/ndk-applesauce-lessons.md` §7 "should recompile" paragraph, lines 92–94).
+diff --git a/docs/design/subscription-compilation/tests.md b/docs/design/subscription-compilation/tests.md
+new file mode 100644
+index 0000000..f6b54b9
+--- /dev/null
++++ b/docs/design/subscription-compilation/tests.md
+@@ -0,0 +1,273 @@
++# Subscription Compilation §9 — Wire-Frame Audit Gate
++
++> Parent: `docs/design/subscription-compilation.md`.
++> Read first: `docs/plan.md` lines 134–137 (M2 exit gates); `docs/design/firehose-bench.md` (the modeled bench harness this test does *not* duplicate).
++
++The M2 exit gate is a single integration test that asserts on the *shape and identity* of the compiler's wire output, not on perf budgets. It is the structural-correctness counterpart to firehose-bench's perf-correctness suite.
++
++## 9.1 Test file location
++
++```
++crates/nmp-testing/tests/m2_subscription_compilation_audit.rs
++```
++
++The `crates/nmp-testing/tests/` directory does not exist yet; M2 creates it. This is intentional — it establishes the convention that planner-correctness gates live as Cargo integration tests under `nmp-testing/tests/`, distinct from the modeled benches under `nmp-testing/bin/`.
++
++`Cargo.toml` for `nmp-testing` needs the standard `[[test]]` block:
++
++```toml
++[[test]]
++name = "m2_subscription_compilation_audit"
++path = "tests/m2_subscription_compilation_audit.rs"
++```
++
++`cargo test -p nmp-testing --test m2_subscription_compilation_audit` is the M2 exit-gate invocation. CI adds this to the `cargo test --workspace` pre-merge gate per `docs/plan.md` §6.
++
++## 9.2 What the test asserts
++
++Four assertions corresponding to the four M2 exit-gate bullets in `docs/plan.md` lines 134–137:
++
++### Assertion 1 — Bug-extinction #3 surface check
++
++> "No public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning."
++
++> **Codegen dependency.** The test below introspects an `AppActionMeta` reflection
++> helper that lives in the per-app generated crate (ADR-0010). It does not exist
++> yet; ADR-0010's codegen must emit it before this assertion can compile. M2's
++> implementation PR is responsible for adding that emission. Until then the test
++> body below is the design contract; the implementing PR can choose either to
++> generate `AppActionMeta` or to back-stop with a `proc-macro`-driven enum walker.
++
++```rust
++#[test]
++fn no_public_publish_action_carries_relay_list() {
++    // Compile-time-ish check: introspect the AppAction enum's generated variants
++    // (per ADR-0010, the per-app generated enum is a closed enum we can match
++    // exhaustively in tests). For every variant whose namespace starts with
++    // "kernel.publish" or "nip01.send" or "nip17.send", assert that no field
++    // is of type Vec<RelayUrl> EXCEPT the one variant `PublishWithOverride`.
++    let variants = AppActionMeta::all_variants();
++    for v in variants {
++        if v.is_publish_action() {
++            let has_relay_field = v.fields().any(|f| f.ty == "Vec<RelayUrl>");
++            if v.namespace == "kernel.publish_override" {
++                assert!(has_relay_field, "override must carry override_relays");
++            } else {
++                assert!(!has_relay_field,
++                    "{} must not expose a relays parameter", v.namespace);
++            }
++        }
++    }
++}
++```
++
++This is a "shape of the API" assertion, not a behaviour assertion. If a future PR adds a relay field to `SendNote`, the test fails.
++
++### Assertion 2 — Per-author wire fan-out for a 1000-author timeline
++
++> "For a timeline of 1000 authors, the compiled plan opens REQs only against the union of those authors' write relays (de-duplicated). Test asserts on the wire frame count."
++
++```rust
++#[test]
++fn timeline_compiles_to_per_relay_union() {
++    let mut harness = PlannerHarness::new();
++
++    // Seed mailbox cache with 1000 authors, deliberately overlapping relay sets:
++    //   - 600 authors use { wss://relay.damus.io, wss://nos.lol }
++    //   - 300 authors use { wss://nostr.wine, wss://nos.lol }
++    //   - 100 authors use { wss://operator-niche.example }
++    let authors = make_authors_with_overlapping_mailboxes(1000);
++    for (pk, mb) in &authors { harness.mailbox_cache_mut().put(pk.clone(), mb.clone()); }
++
++    // Register one Timeline interest containing all 1000 authors.
++    let interest_id = harness.register_interest(LogicalInterest {
++        scope: InterestScope::ActiveAccount,
++        shape: InterestShape::timeline_for(authors.iter().map(|(pk, _)| pk.clone()).collect()),
++        lifecycle: InterestLifecycle::Tailing,
++        ..LogicalInterest::default()
++    });
++
++    let plan = harness.compile().expect("compile");
++
++    // Assert: exactly one REQ per relay in the union of write relays.
++    let expected_relays: BTreeSet<RelayUrl> = authors.iter()
++        .flat_map(|(_, mb)| mb.write.iter().cloned())
++        .collect();
++    let actual_relays: BTreeSet<RelayUrl> = plan.per_relay.iter()
++        .map(|rp| rp.relay_url.clone()).collect();
++    assert_eq!(actual_relays, expected_relays);
++
++    // Assert: each relay carries exactly one SubShape (merge happened).
++    for rp in &plan.per_relay {
++        assert_eq!(rp.sub_shapes.len(), 1,
++            "relay {} should have one merged sub-shape, has {}",
++            rp.relay_url, rp.sub_shapes.len());
++    }
++
++    // Assert: each relay's authors are exactly the subset that declared it.
++    for rp in &plan.per_relay {
++        let expected_authors: BTreeSet<Pubkey> = authors.iter()
++            .filter(|(_, mb)| mb.write.contains(&rp.relay_url))
++            .map(|(pk, _)| pk.clone())
++            .collect();
++        let actual_authors = &rp.sub_shapes[0].shape.authors;
++        assert_eq!(actual_authors, &expected_authors,
++            "relay {} should serve only its declared authors", rp.relay_url);
++    }
++
++    // Assert: plan-id is deterministic — running compile twice yields the same id.
++    let plan2 = harness.compile().expect("compile #2");
++    assert_eq!(plan.plan_id, plan2.plan_id, "recompile with no input changes ≠ same plan_id");
++}
++```
++
++This is the single most load-bearing test in M2. It assert on:
++
++- **Relay count** = size of union of declared write relays (no extras, no misses).
++- **Per-relay author partition** = exact subset semantics.
++- **Sub-shape merge** = one REQ per relay (merge lattice worked).
++- **Plan-id stability** = re-compile is idempotent.
++
++### Assertion 3 — Late-arriving kind:10002 triggers recompilation
++
++> "An author whose mailbox was unknown gets re-routed once their kind:10002 lands, without the platform observing protocol churn."
++
++```rust
++#[test]
++fn late_nip65_arrival_reroutes_without_churn() {
++    let mut harness = PlannerHarness::new();
++    let target = pubkey("alice");
++
++    // Seed: no mailbox for alice. Register an interest that needs her.
++    harness.register_interest(LogicalInterest::timeline_for(vec![target.clone()]));
++    let plan_v1 = harness.compile().unwrap();
++
++    // The author should be routed via indexer fallback.
++    let alice_relay_v1 = plan_v1.per_relay.iter()
++        .find(|rp| rp.sub_shapes[0].shape.authors.contains(&target))
++        .expect("alice routed somewhere");
++    assert!(alice_relay_v1.role_tags.contains(&RoutingSource::Indexer));
++
++    // Now alice's kind:10002 arrives.
++    harness.ingest_nip65(&target, ["wss://alice-relay.example"]);
++
++    // The ingest emits Trigger::Nip65Arrived → recompile happens internally.
++    harness.flush_pending_triggers();
++    let plan_v2 = harness.last_compiled_plan();
++
++    // Assert: plan-id changed.
++    assert_ne!(plan_v1.plan_id, plan_v2.plan_id);
++
++    // Assert: alice now routes to her declared relay, not the indexer.
++    let alice_relay_v2 = plan_v2.per_relay.iter()
++        .find(|rp| rp.sub_shapes[0].shape.authors.contains(&target))
++        .expect("alice still routed");
++    assert_eq!(alice_relay_v2.relay_url, "wss://alice-relay.example".into());
++    assert!(alice_relay_v2.role_tags.contains(&RoutingSource::Nip65));
++
++    // Assert: the audit stream contains exactly ONE planner re-emission for alice;
++    // the platform sees one transition, not a thrash of N intermediate states.
++    let audit = harness.compile_audit_log();
++    let alice_transitions = audit.iter()
++        .filter(|a| a.affected_authors.contains(&target))
++        .count();
++    assert_eq!(alice_transitions, 1, "expected exactly one recompile for late NIP-65");
++}
++```
++
++This assertion is what `docs/design/ndk-applesauce-lessons.md` §2 line 19 calls out as NDK's important operational truth: "metadata can arrive late… the system should be able to refresh or expand active work without the app tearing down and recreating views."
++
++### Assertion 4 — Four-lane diagnostic distinctness
++
++> "The diagnostics screen shows the four relay-fact lanes (NIP-65 / hint / provenance / user-configured) separately."
++
++```rust
++#[test]
++fn four_lanes_stay_distinct_in_diagnostic_payload() {
++    let mut harness = PlannerHarness::new();
++    let author = pubkey("alice");
++
++    // Set up evidence in all four lanes for the same relay url.
++    let url: RelayUrl = "wss://r.example".into();
++    harness.ingest_nip65(&author, [url.clone()]);              // Nip65 fact
++    harness.observe_hint(&author, url.clone(),                 // Hint fact
++        HintSource::EventTag { event_id: eid("e1"), tag: TagKey::E, position: 2 });
++    harness.observe_provenance(&author, url.clone(), eid("e2")); // Provenance fact
++    harness.user_configured_relay(url.clone(),                 // UserConfigured fact
++        UserConfiguredCategory::Indexer);
++
++    let coverage = harness.open_view::<RelayCoverageView>(
++        RelayCoverageSpec { relay_url: url.clone() });
++
++    assert_eq!(coverage.by_lane.nip65, 1);
++    assert_eq!(coverage.by_lane.hint,  1);
++    assert_eq!(coverage.by_lane.user_configured, 1);
++    // Provenance count is the rolling 60s counter; alice's event landed once.
++    assert_eq!(coverage.provenance_count_last_minute, 1);
++
++    // Structural: no compiler output collapses lanes.
++    let plan = harness.compile().unwrap();
++    let alice_assignment = plan.per_relay.iter()
++        .find(|rp| rp.relay_url == url).unwrap();
++    // role_tags is a SET, not a single value — lanes are preserved.
++    assert!(alice_assignment.role_tags.len() >= 1);
++    assert!(matches!(alice_assignment.role_tags.iter().next().unwrap(),
++        RoutingSource::Nip65 | RoutingSource::UserConfigured));
++}
++```
++
++This assertion encodes the doctrine: a single relay may be in the plan for multiple reasons; the plan must say which reasons, not collapse them.
++
++## 9.3 The `PlannerHarness`
++
++The test harness is itself part of `nmp-testing`:
++
++```rust
++// crates/nmp-testing/src/planner_harness.rs (proposed)
++
++pub struct PlannerHarness {
++    cache: InMemoryMailboxCache,
++    user_config: UserConfiguredRelays,
++    indexer_set: Vec<RelayUrl>,
++    interests: InterestRegistry,
++    compiler: SubscriptionCompiler,
++    audit_log: Vec<CompileAuditEntry>,
++}
++
++impl PlannerHarness {
++    pub fn new() -> Self;
++    pub fn mailbox_cache_mut(&mut self) -> &mut dyn MailboxCache;
++    pub fn register_interest(&mut self, i: LogicalInterest) -> InterestId;
++    pub fn drop_interest(&mut self, id: InterestId);
++    pub fn ingest_nip65(&mut self, author: &Pubkey, relays: impl IntoIterator<Item = RelayUrl>);
++    pub fn observe_hint(&mut self, author: &Pubkey, url: RelayUrl, source: HintSource);
++    pub fn observe_provenance(&mut self, author: &Pubkey, url: RelayUrl, event: EventId);
++    pub fn user_configured_relay(&mut self, url: RelayUrl, cat: UserConfiguredCategory);
++    pub fn force_recompile(&mut self, reason: InvalidateReason);
++    pub fn flush_pending_triggers(&mut self);
++    pub fn compile(&mut self) -> Result<CompiledPlan, CompileError>;
++    pub fn last_compiled_plan(&self) -> &CompiledPlan;
++    pub fn compile_audit_log(&self) -> &[CompileAuditEntry];
++    pub fn open_view<V: ViewModule>(&mut self, spec: V::Spec) -> V::Payload;
++}
++```
++
++The harness is the *minimum* surface required for the four assertions above. It is deliberately small so it does not become its own moving target.
++
++## 9.4 What this test does *not* cover
++
++By design (these belong to other M2 gates or later milestones):
++
++- **Real wire frames against a relay.** This is `firehose-bench live` per `docs/plan.md` line 110; the audit test is offline and synthetic.
++- **Wire-emitter diff correctness across two plans.** That is a separate unit test inside `nmp-core::kernel::wire`, not the milestone-exit gate.
++- **NIP-77 watermarks.** M4.
++- **Per-account auth state.** M5.
++- **The publish path running end-to-end.** M6.
++
++The audit gate's job is exactly the four assertions: API shape, fan-out structure, recompilation on late NIP-65, and four-lane diagnostic distinctness. Those are the four exit-gate bullets the milestone document lists; this test is the verification surface for all four.
++
++## 9.5 CI integration
++
++The test runs in the default `cargo test --workspace` job and takes < 1 second on standard hardware (no networking, no LMDB, in-memory cache only). It is the canonical regression test for "did someone re-introduce the hardcoded two-role planner?" and as such must never be skipped or `#[ignore]`d.
++
++If the M3 (LMDB) milestone graduates the mailbox cache to a real backend, this test continues to exercise the trait surface via the `InMemoryMailboxCache` impl — no changes required. That is the seam `nmp-nip65::cache::MailboxCache` exists for.
+
+Review for: (1) doctrine compliance (in particular D0 — does nmp-nip65 propose any kernel-side type leaks? D4 — single source for relay-set facts?); (2) any TODO/FIXME/unimplemented; (3) file-size compliance (any new file > 300 LOC?); (4) internal consistency across the 8 sub-docs; (5) compatibility with the upcoming M3 LMDB schema (e.g. watermark table); (6) the planned test (m2_subscription_compilation_audit) exit criteria; (7) any hidden shortcuts. Be terse; name file:line + fix for any concern.
+codex
+I’ll review this as a docs/spec merge against the actual repo state, first checking the new files, their line counts, and the referenced milestone/doctrine anchors so the findings are tied to current line numbers.
+exec
+/bin/zsh -lc 'git status -sb' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+## master...origin/master
+?? .claude/
+?? docs/perf/codex-reviews/031fc07.md
+
+exec
+/bin/zsh -lc 'rg -n "nostr-multi-platform|subscription compilation|M2|031fc07|D0|D4|nmp-nip65|outbox" /Users/pablofernandez/.codex/memories/MEMORY.md' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+1384:- the compile-safe verification path here was `xcodebuild build -scheme Podcastr -project Podcastr.xcodeproj -destination 'platform=iOS Simulator,id=18354708-738D-4D15-B86C-4FDED4D43A93'`, followed by `xcrun simctl launch booted io.f7z.podcast` [Task 1]
+1491:- the useful focused verification command was `xcodebuild -project Podcastr.xcodeproj -scheme Podcastr -destination 'id=18354708-738D-4D15-B86C-4FDED4D43A93' -derivedDataPath Derived -only-testing:PodcastrTests/PersistenceDurabilityTests -only-testing:PodcastrTests/OPMLImportTests -only-testing:PodcastrTests/DataExportTests -only-testing:PodcastrTests/AppStateStorePerformanceTests test`, which passed with `32 tests, 0 failures` in the clean worktree [Task 2]
+2943:- rust migration, tenex-daemon, daemon.ts, M1, M2, M3, M8, M9, quality gates, publish_outbox, worker_protocol, can I use it yet, test:rust:publish-interop
+3018:- early status truth in this branch family was: Rust had a large library/test surface and a publish-outbox binary, but the live daemon entrypoint was still `src/commands/daemon.ts`; implemented support modules did not mean the Rust daemon was already usable as the operator-facing daemon [Task 2]
+3035:- Symptom: `cargo test -p tenex-daemon` fails during a status check in the publish-outbox area. Cause: branch-local compile breakage such as missing helpers in `publish_outbox.rs`. Fix: treat the compile failure as the concrete blocker instead of describing the milestone as compile-clean [Task 2]
+
+exec
+/bin/zsh -lc 'git rev-parse --short HEAD && git branch --show-current' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+75d18ff
+master
+
+codex
+The worktree is on `master`, with `031fc07` already behind the current HEAD. I found unrelated untracked local files, so I’ll keep this review read-only and scoped to the merge’s documentation files plus the referenced plan/doctrine docs.
+exec
+/bin/zsh -lc 'git show --name-only --format= 031fc07' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+docs/design/subscription-compilation.md
+docs/design/subscription-compilation/compiler.md
+docs/design/subscription-compilation/diagnostics.md
+docs/design/subscription-compilation/intro.md
+docs/design/subscription-compilation/nip65.md
+docs/design/subscription-compilation/outbox.md
+docs/design/subscription-compilation/recompilation.md
+docs/design/subscription-compilation/tests.md
+
+exec
+/bin/zsh -lc 'git show --stat --oneline --decorate --no-renames 031fc07' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+031fc07 design(m2): subscription compilation + outbox routing spec
+ docs/design/subscription-compilation.md            |  43 ++++
+ docs/design/subscription-compilation/compiler.md   | 193 +++++++++++++++
+ .../design/subscription-compilation/diagnostics.md | 188 ++++++++++++++
+ docs/design/subscription-compilation/intro.md      | 132 ++++++++++
+ docs/design/subscription-compilation/nip65.md      | 221 +++++++++++++++++
+ docs/design/subscription-compilation/outbox.md     | 224 +++++++++++++++++
+ .../subscription-compilation/recompilation.md      | 159 ++++++++++++
+ docs/design/subscription-compilation/tests.md      | 273 +++++++++++++++++++++
+ 8 files changed, 1433 insertions(+)
+
+exec
+/bin/zsh -lc 'rg -n "TODO|FIXME|unimplemented|panic!|todo!|unwrap'"\\(|TBD|stub|placeholder|does not exist yet|proposed|deferred|open question|future|post-M2|M6|M3|M4|M5|M8|M9\" docs/design/subscription-compilation.md docs/design/subscription-compilation" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc 'wc -l docs/design/subscription-compilation.md docs/design/subscription-compilation/*.md' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+      43 docs/design/subscription-compilation.md
+     193 docs/design/subscription-compilation/compiler.md
+     188 docs/design/subscription-compilation/diagnostics.md
+     132 docs/design/subscription-compilation/intro.md
+     221 docs/design/subscription-compilation/nip65.md
+     224 docs/design/subscription-compilation/outbox.md
+     159 docs/design/subscription-compilation/recompilation.md
+     273 docs/design/subscription-compilation/tests.md
+    1433 total
+
+ succeeded in 0ms:
+docs/design/subscription-compilation.md:6:> **Scope:** Replace the "hardcoded two-role relay set" planner in `crates/nmp-core/src/kernel/{requests,ingest,mod}.rs` with a **subscription compilation stage** that turns logical interests into per-relay plans driven by NIP-65 mailboxes, and graduates outbox routing to a first-class planner subsystem. v1 is in-memory; M3 plugs it into LMDB. This is a design doc; no implementation lands in this PR.
+docs/design/subscription-compilation.md:39:3. **Per-author indexer-fallback ledger row?** Today the compiler treats indexer fallback as an inline relay assignment. If the kind:10002 fetch is its own durable action (M6 ledger), the fallback becomes a tracked action with retry/cancel semantics. Cleaner for diagnostics; heavier for M2. Defer.
+docs/design/subscription-compilation.md:42:6. **Auth-paused relays in compiled plans.** If a relay is in `RelayAuthState::ChallengeReceived`, the compiler still produces a plan that assigns interests to it (so reconnect-after-auth resumes correctly), but emission must pause. Is the pause modeled inside the compiler (per-relay gate) or inside the wire-emitter (consumes plans, applies pause)? Bias: wire-emitter, but the compiler must surface the pause as a fact for `LogicalInterestStatus`. Resolve before M5.
+docs/design/subscription-compilation.md:43:7. **NSE crate compilation surface.** `nmp-nip17-nse` (M9) runs in iOS Notification Service Extension with bounded memory; it needs to compile a single-author single-relay plan without the full planner. Confirm in an ADR that the compiler exposes a `compile_one(spec, mailbox_cache_snapshot) -> Plan` pure function suitable for NSE use, and that the function does not require a live actor.
+docs/design/subscription-compilation/nip65.md:31:`nmp-nip65` implements exactly one extension trait family: `ViewModule`. It does *not* implement `ActionModule` (kind:10002 publish is the user's own "update my relay list" action, deferred to M6's action ledger; in this milestone it has no write surface). It does *not* implement `DomainModule` (mailbox records live in the kernel-owned mailbox cache; they are queryable Nostr events, not durable app-defined records).
+docs/design/subscription-compilation/nip65.md:150:The `MailboxCache` trait is the seam M3 (LMDB persistence) replaces with a backing-store-aware implementation. The compiler does not know which backend it is reading.
+docs/design/subscription-compilation/nip65.md:185:- **No publish action.** Updating a user's own kind:10002 is `nmp-nip01::UpdateRelayList` (M6); that action depends on `nmp-nip65::parse` to validate the local copy before publishing.
+docs/design/subscription-compilation/nip65.md:200:- Future `nmp-nip01::UpdateRelayList` (M6).
+docs/design/subscription-compilation/nip65.md:201:- Future `nmp-nip17` (M9) — DM publish path imports `resolve_author_inbox` for recipient lookups.
+docs/design/subscription-compilation/diagnostics.md:22:// crates/nmp-core/src/kernel/diagnostics/lanes.rs (proposed)
+docs/design/subscription-compilation/diagnostics.md:75:Emitted by the pointer loader (post-M2; for M2 the field exists but is rarely populated — only `e`/`a`-tag third-slot hints from thread-view hydration fill it). Per-event hints are de-duplicated; an event whose `e` tag contains a hint URL produces one `HintRelayFact` per (relay_url, source) pair.
+docs/design/subscription-compilation/diagnostics.md:126:- `Provenance → NIP-65 hint refresh.` If we observe many `ProvenanceRelayFact { relay_url: R, author: A }` records but no `Nip65RelayFact { pubkey: A, relay_url: R }`, the operator diagnostic can suggest "we are receiving A's events from R but A has not declared R; their kind:10002 may be stale." This is a future operator-UI affordance, not a behaviour.
+docs/design/subscription-compilation/tests.md:14:The `crates/nmp-testing/tests/` directory does not exist yet; M2 creates it. This is intentional — it establishes the convention that planner-correctness gates live as Cargo integration tests under `nmp-testing/tests/`, distinct from the modeled benches under `nmp-testing/bin/`.
+docs/design/subscription-compilation/tests.md:64:This is a "shape of the API" assertion, not a behaviour assertion. If a future PR adds a relay field to `SendNote`, the test fails.
+docs/design/subscription-compilation/tests.md:143:    let plan_v1 = harness.compile().unwrap();
+docs/design/subscription-compilation/tests.md:209:    let plan = harness.compile().unwrap();
+docs/design/subscription-compilation/tests.md:211:        .find(|rp| rp.relay_url == url).unwrap();
+docs/design/subscription-compilation/tests.md:214:    assert!(matches!(alice_assignment.role_tags.iter().next().unwrap(),
+docs/design/subscription-compilation/tests.md:226:// crates/nmp-testing/src/planner_harness.rs (proposed)
+docs/design/subscription-compilation/tests.md:263:- **NIP-77 watermarks.** M4.
+docs/design/subscription-compilation/tests.md:264:- **Per-account auth state.** M5.
+docs/design/subscription-compilation/tests.md:265:- **The publish path running end-to-end.** M6.
+docs/design/subscription-compilation/tests.md:273:If the M3 (LMDB) milestone graduates the mailbox cache to a real backend, this test continues to exercise the trait surface via the `InMemoryMailboxCache` impl — no changes required. That is the seam `nmp-nip65::cache::MailboxCache` exists for.
+docs/design/subscription-compilation/recompilation.md:24:| A4 | session | `Trigger::ActiveAccountChanged { from, to }` | account switch (M8) |
+docs/design/subscription-compilation/recompilation.md:29:| A9 | auth | `Trigger::RelayAuthStateChanged { url, state }` | NIP-42 transition (M5+) |
+docs/design/subscription-compilation/recompilation.md:30:| A10 | session | `Trigger::SignerAvailable { account, signer_id }` | signer-loss-then-recovery (M6+) |
+docs/design/subscription-compilation/recompilation.md:37:// crates/nmp-core/src/kernel/planner/trigger.rs (proposed)
+docs/design/subscription-compilation/recompilation.md:111:M2 establishes the trigger; M8 wires the multi-account state machine that actually emits it. For M2, the trigger fires once at startup with `from: None, to: Some(active)` so the test surface can exercise account-scope binding without waiting for M8.
+docs/design/subscription-compilation/recompilation.md:119:Per `docs/product-spec/subsystems.md` §7.2 "Reconnect": "the planner restores live REQs and schedules a coverage-aware gap fill. View payloads do not reset." The gap-fill schedule is M4 (NIP-77); for M2 the gap is implicit (live tail resumes without backfill).
+docs/design/subscription-compilation/recompilation.md:137:M5 wires this fully. M2 only models the trigger so the compiler's data-flow shape does not need to change in M5. Compiler effect: marks the relay as "auth-paused" in its `RelayPlan` so the wire-emitter knows to hold REQs until `RelayAuthState::Authenticated`. Open question 6 in the parent index covers where the gate physically lives.
+docs/design/subscription-compilation/recompilation.md:141:M6+ trigger. Some interests (private DMs in M9, NIP-42 challenge response in M5) only become routable once a signer is loaded for their account. M2 records the trigger shape; behaviour is no-op pre-M6.
+docs/design/subscription-compilation/recompilation.md:151:Explicit non-triggers (so future code does not accidentally over-couple):
+docs/design/subscription-compilation/intro.md:13:- **Startup REQs ignore mailboxes by construction.** `crates/nmp-core/src/kernel/requests.rs:50-106` (`startup_requests`) issues six fixed REQs, each pinned to `Content` or `Indexer`. The seed-bootstrap timeline (line 65–70) fans seven hundred-author future timelines through one relay. The exit-gate test for M2 (`docs/plan.md` lines 134–137) requires that this fan exactly equal the union of those authors' write relays.
+docs/design/subscription-compilation/intro.md:17:- **No publish path exists yet.** `crates/nmp-core/src/kernel/requests.rs:30` (no occurrences of `EVENT` outbound) and `crates/nmp-core/src/relay.rs:42-45` (`OutboundMessage` carries only role + text). The first publish action (M6 `SendNote`) will hit this same `req()`-style seam. M2 must establish the planner shape before M6 builds the first user of it; the doctrine "no developer-supplied relays for a publish" (`docs/aim.md` §6 doctrine 5; `docs/product-spec/subsystems.md` §7.3 row "Publish leaked to wrong relays") needs a structural enforcement point.
+docs/design/subscription-compilation/intro.md:28:// crates/nmp-core/src/kernel/planner/interest.rs (proposed)
+docs/design/subscription-compilation/intro.md:62:    /// Bound to a specific account (multi-account UIs, M8). Re-routes on that
+docs/design/subscription-compilation/intro.md:119:The kernel `SessionState` (`docs/product-spec/subsystems.md` §7.4; M8 graduates this to multi-account) carries an active account id. `InterestScope::ActiveAccount` resolves at compile time, not at registration time. On account switch (§4 trigger A4), the compiler re-evaluates every `ActiveAccount`-scoped interest against the new active account's mailbox view. This is the structural enforcement of bug-extinction #5 (account-context overlap) referenced in `docs/plan.md` line 284 — the kernel cannot "forget" to re-route because every plan re-derives from the active scope.
+docs/design/subscription-compilation/outbox.md:6:This section defines the **publish-side seam** the M2 milestone lands so the M6 write path has a ready surface. There is no publish code in the repo today (`crates/nmp-core/src/kernel/requests.rs` contains no `EVENT` outbound; the relay worker has no publish channel). M2 lands the trait and the override action; M6 writes the first concrete consumer (`SendNoteAction`).
+docs/design/subscription-compilation/outbox.md:8:This is the framing the parent index calls out as "design seam now, first concrete consumer in M6." Without this seam, M6 risks reinventing outbox routing inline.
+docs/design/subscription-compilation/outbox.md:13:// crates/nmp-core/src/kernel/planner/publish.rs (proposed)
+docs/design/subscription-compilation/outbox.md:81:// crates/nmp-core/src/kernel/planner/publish_default.rs (proposed)
+docs/design/subscription-compilation/outbox.md:97:This is the only `PublishPlanner` impl shipped in v1. The trait exists so a future `Wot​PublishPlanner` (M13 WoT subsystem) or a sandbox planner used in tests can replace it without touching action-ledger code.
+docs/design/subscription-compilation/outbox.md:141:2. **One-shot** — does not persist as a default for future publishes.
+docs/design/subscription-compilation/outbox.md:148:// crates/nmp-core/src/kernel/actions/publish_override.rs (proposed)
+docs/design/subscription-compilation/outbox.md:212:The bug-extinction #7 test (`docs/plan.md` line 234) — "publish OK / store fail and store OK / publish fail both roll back atomically" — runs against the M6 implementation. The seam M2 lands here must make that test possible. Specifically:
+docs/design/subscription-compilation/outbox.md:217:## 7.6 What M2 does not cover (deferred)
+docs/design/subscription-compilation/outbox.md:219:- **Action ledger schema** — `docs/design/kernel-substrate.md` §4 is the design; M6 implements.
+docs/design/subscription-compilation/outbox.md:220:- **Retry policy** — exponential backoff parameters land in M6.
+docs/design/subscription-compilation/outbox.md:221:- **Concurrent publish coalescing** — if two actions publish the same event (a republish), the planner can dedupe to one wire EVENT per relay. Defer to M6 / M7 stress test.
+docs/design/subscription-compilation/outbox.md:222:- **NIP-42 auth challenge during publish** — relays may demand AUTH before accepting an EVENT. Wires up in M5.
+docs/design/subscription-compilation/outbox.md:224:The publish-planner trait is intentionally finished enough that the M6 implementation does not need to extend it. That is the seam the milestone gates against.
+docs/design/subscription-compilation/compiler.md:80:`docs/product-spec/subsystems.md` §7.3 specifies one explicit override: DMs (NIP-17 gift-wraps, M9) fail closed if recipient inbox relays are missing. The compiler enforces this by refusing to produce a plan for an interest tagged `privacy = FailClosed` if any tagged-pubkey inbox lookup returns `Indexer` source. §7 details the publish-side enforcement.
+docs/design/subscription-compilation/compiler.md:84:The indexer set is a kernel-configured `Vec<RelayUrl>` (default: a small curated list; user-configurable in `AppConfig`). Today's `crates/nmp-core/src/relay.rs:2` is the placeholder for one indexer relay (`purplepag.es`); the v1 indexer set lives in `AppConfig.indexer_relays`.
+docs/design/subscription-compilation/compiler.md:137:Definition (this design picks the "interest-set + mailbox-snapshot" formulation; open question 1 in the parent index notes the alternative):
+docs/design/subscription-compilation/compiler.md:173:| `pending_view_requests` (requests.rs:333-355) | Becomes `compiler.flush_deferred_for_relay(role, url)`: called when a relay reconnects (§4 trigger A3). The compiler resubmits its current plan against that relay's slot. |
+docs/design/subscription-compilation/compiler.md:186:What this migration does **not** do (deferred per parent index open questions 3, 6, 7):
+docs/design/subscription-compilation/compiler.md:188:- It does not move the action ledger into M2 — `SendNote` lands in M6.
+docs/design/subscription-compilation/compiler.md:189:- It does not implement LMDB persistence for the mailbox cache — M3.
+docs/design/subscription-compilation/compiler.md:190:- It does not implement NIP-77 watermarks — M4.
+docs/design/subscription-compilation/compiler.md:191:- It does not add a per-author indexer-fallback ledger row — open question 3.
+
+exec
+/bin/zsh -lc 'rg -n "D0|D1|D2|D3|D4|D5|Doctrine|kernel never grows|single writer|capabilities report|M2|subscription compiler|subscription compilation|outbox|NIP-65|watermark|LMDB|relay-set|relay set" docs/plan.md docs/aim.md docs/product-spec/subsystems.md docs/design/kernel-substrate.md docs/design/app-extension-kernel.md docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md docs/design/ndk-applesauce-lessons.md docs/design/reactivity/*.md' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+docs/design/reactivity/validation-harness.md:39:- Spawns a configurable `EventStore` (in-memory, LMDB, or SQLite backend).
+docs/design/reactivity/scheduling-and-data-model.md:92:│   - LMDB / SQLite / IndexedDB / nostrdb                  │
+docs/design/reactivity/scheduling-and-data-model.md:95:│     tombstones, sync watermarks                          │
+docs/design/ndk-applesauce-lessons.md:5:> **Scope:** High-level product and architecture lessons to preserve before NMP's outbox, relay policy, event store, and action layers are built. This is not an implementation plan.
+docs/design/ndk-applesauce-lessons.md:9:NDK and Applesauce both solve real Nostr client problems that NMP must eventually solve: relay discovery, outbox routing, cache-backed reads, action-driven writes, derived models, and sane application defaults. They do it with different API philosophies and runtime assumptions.
+docs/design/ndk-applesauce-lessons.md:17:NDK's strongest lesson is that outbox routing should feel automatic to application developers. A developer should not normally decide which relays to use for a profile, timeline, reply, or publish. The framework should infer the right relay set from NIP-65 metadata, event tags, relay hints, and fallback policy.
+docs/design/ndk-applesauce-lessons.md:21:NDK's relay-set abstraction is useful as a product concept. Once routing policy decides where an operation belongs, the transport layer should receive a clear set of relays and carry out the operation. That keeps relay choice distinct from socket mechanics.
+docs/design/ndk-applesauce-lessons.md:37:Applesauce's outbox-map idea is especially important at the conceptual level: author-scoped reads should be grouped by the relays that can actually serve those authors. A timeline for many authors should not become one large author filter blasted to every relay. The routing layer should split work by relay responsibility.
+docs/design/ndk-applesauce-lessons.md:39:Applesauce also keeps pointer relay hints as first-class information. Relay hints from events, tags, and NIP-19 pointers are not the same as NIP-65 relay lists, but they are useful evidence. NMP should preserve that distinction:
+docs/design/ndk-applesauce-lessons.md:41:- NIP-65 describes a user's declared relay preferences.
+docs/design/ndk-applesauce-lessons.md:70:The sync engine should share relay policy with the subscription planner. If live reads for an author go to the author's outbox relays, historical reconciliation for that same view should not quietly use a different relay universe. Sync, live REQ, cache reads, and fallback discovery are different execution modes for the same logical interest.
+docs/design/ndk-applesauce-lessons.md:76:NDK's subscription grouping and Applesauce's relay/filter maps both point to a broader design lesson: NMP should treat subscription compilation as a first-class planner stage.
+docs/design/ndk-applesauce-lessons.md:103:This also keeps performance policy centralized. The same planner that owns outbox routing can enforce limits, deduplicate page loads, avoid duplicate boundary events, and prevent multiple components from opening equivalent pagination work.
+docs/design/ndk-applesauce-lessons.md:109:Outbox support belongs in the actor-owned routing and planning layer. Relay sockets should not decide whether a relay is an inbox, outbox, indexer, fallback, or hint. They should connect, subscribe, publish, report status, and nothing more.
+docs/design/ndk-applesauce-lessons.md:133:- which relay set was chosen,
+docs/design/ndk-applesauce-lessons.md:153:- From NDK, take the ambition that outbox routing is automatic by default.
+docs/design/ndk-applesauce-lessons.md:159:NMP should therefore make outbox invisible in ordinary app code but visible in diagnostics and tests. It should use explicit internal routing artifacts, but expose intent-oriented product APIs. It should route efficiently by author and recipient, but still render cached state immediately. It should support fallback for public reads, but fail closed for private delivery. It should make NIP-77 a coverage and backfill policy, not a blocking fetch API. It should compile logical interests into cache, sync, live, and fallback work without exposing that machinery to platform code.
+docs/design/ndk-applesauce-lessons.md:161:That is the durable lesson: outbox support is not a feature bolted onto subscriptions. It is a framework-level correctness policy that touches storage, planning, publishing, diagnostics, privacy, and the developer API.
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:185:This validates the shape without requiring the full outbox planner, NIP-77 sync engine, or FFI.
+docs/design/kernel-substrate.md:17:| `DomainModule` | Durable non-Nostr records | Schema, migrations, indexes, record types | Per-domain typed records + LMDB-backed CRUD |
+docs/design/kernel-substrate.md:60:- LMDB sub-database per `NAMESPACE`.
+docs/design/kernel-substrate.md:173:- Pre-formatted display fields per doctrine D1.
+docs/design/kernel-substrate.md:427:`TestHarness` mocks the actor, the storage backend, and the FFI emission. Modules can be tested without a real relay, without LMDB on disk, without UniFFI bindings.
+docs/design/kernel-substrate.md:439:- `DomainModule` trait + `DomainRegistry` + LMDB backing.
+docs/design/kernel-substrate.md:452:- `nmp-nip65`: Mailboxes view module + outbox routing helper.
+docs/aim.md:80:- A **database trait** with multiple swappable backends: in-memory, LMDB, nostrdb, SQLite (native and WASM via OPFS/IndexedDB VFS).
+docs/aim.md:81:- A **gossip/outbox trait** with in-memory and SQLite backends, implementing the NIP-65 relay-list metadata model and per-pubkey relay discovery.
+docs/aim.md:89:These crates are **dependencies, not forks**. The framework's job is to compose them into an opinionated whole; their authors do the protocol correctness work, we do the application-layer work. Where they have gaps relative to the framework's goals (reactive queries, models, sessions, web-of-trust, opinionated outbox routing on every operation), the framework adds those layers above — it does not push them down into the protocol layer.
+docs/aim.md:125:### 4.4 Outbox / smart relay routing (NIP-65)
+docs/aim.md:127:The framework implements the outbox model **by default and automatically**. Subscriptions with `authors` filters automatically route reads to those authors' write relays. Publishes for an event automatically go to the author's write relays plus inbox relays of any `p`-tagged recipients (for DMs and notifications). The developer does not pick relays per operation; the framework does. They can override, but the override is the exception.
+docs/aim.md:129:Per-pubkey relay lists are fetched lazily via a gossip layer, cached in a swappable backend (in-memory or SQLite), and refreshed when a fresher kind-10002 arrives. When a user's outbox changes, dependent subscriptions automatically re-resolve their relay sets.
+docs/aim.md:184:│   │                              # outbox routing. Pure Rust, no FFI.
+docs/aim.md:219:## 6. Doctrine — the rules the API must make hard to violate
+docs/aim.md:254:- Two pre-existing TypeScript Nostr libraries — intentionally unnamed here — supply the high-level application architecture (event store, models, actions, sessions, outbox routing, NIP-77 sync, wallet, messaging, web-of-trust, developer guardrails) being translated into Rust idiom under the RMP architectural skeleton.
+docs/product-spec/subsystems.md:27:Storage backend is configurable via `AppConfig.storage_backend` (LMDB or SQLite-style native backend, IndexedDB/OPFS strategy for web, final choice resolved before v1). The store wraps the Rust Nostr SDK protocol types, but NMP owns the application-kernel storage traits because the app kernel needs typed provenance, action ledger rows, relay metadata, domain records, and bounded-view indexes in addition to raw events.
+docs/product-spec/subsystems.md:31:**Sync watermarks.** The store maintains a per-`(filter_signature, relay_url)` table:
+docs/product-spec/subsystems.md:34:watermarks {
+docs/product-spec/subsystems.md:52:- Authoritative absence: only a complete coverage record/watermark can turn a miss into "not found." A non-empty cache result is not proof that a query is complete.
+docs/product-spec/subsystems.md:63:- **Coverage-aware backfill.** Before issuing historical traffic, the planner consults cache coverage/watermarks (§7.1). Complete coverage serves from cache; partial coverage schedules a gap fill; unknown coverage triggers bounded fetch/REQ or NIP-77 if supported. A non-empty cache result is never treated as complete by itself.
+docs/product-spec/subsystems.md:74:Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
+docs/product-spec/subsystems.md:86:| Discovery (kind-10002 fetch for unknown pubkeys) | Configurable indexer relay set (default: a curated list of high-coverage relays). |
+docs/product-spec/subsystems.md:98:- Fresher kind-10002 arrives → invalidate dependent subscriptions, recompute relay sets, re-issue REQs as needed.
+docs/product-spec/subsystems.md:101:The gossip cache is the `nostr-gossip` crate; backend selection (in-memory vs SQLite) follows the storage backend choice. Watermarks (§7.1) intersect with outbox: a sync watermark is keyed by `(filter, relay)` and naturally tracks per-author per-relay coverage.
+docs/product-spec/subsystems.md:181:**Best-effort field contract (per doctrine D1).** Every display-bearing field in every view payload is **non-optional** and has a defined placeholder when the underlying data is missing:
+docs/product-spec/subsystems.md:242:Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.
+docs/product-spec/subsystems.md:252:**Watermarks as a first-class type.** The engine reads and writes the `watermarks` table introduced in §7.1. A watermark answers two questions:
+docs/product-spec/subsystems.md:262:2. **View open.** When a view opens whose filter has a gap (per watermark/coverage), the engine reconciles the gap concurrently with the live REQ tail. Progress is visible in `SyncState`; the view payload streams in as events land.
+docs/product-spec/subsystems.md:287:    pub watermarks_summary: WatermarksSummary,  // coverage stats per relay
+docs/product-spec/subsystems.md:402:- Sync watermarks coverage (per relay: % of opened filters fully synced).
+docs/product-spec/subsystems.md:447:| Relay policy / outbox | Relay-target resolver, relay metadata store, provenance, canonical filters, action ledger |
+docs/plan.md:9:> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
+docs/plan.md:20:- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
+docs/plan.md:29:- LMDB / IndexedDB persistent storage (in-memory only today).
+docs/plan.md:30:- NIP-65 outbox routing (hardcoded content + indexer relays today).
+docs/plan.md:65:4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
+docs/plan.md:66:5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
+docs/plan.md:100:- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
+docs/plan.md:118:### M2 — Subscription compilation + outbox routing
+docs/plan.md:120:**Demo product:** Same iOS app as M1, but timeline subscriptions are routed per-author to those authors' write relays per NIP-65, not to the hardcoded primal/purplepag.es pair. Diagnostics screen visibly shows the per-relay fan-out and which authors each relay covers.
+docs/plan.md:122:**Scope.** The planner becomes a **subscription compilation stage** per the NDK/Applesauce lessons doc. Logical interests get compiled into per-relay plans; recompilation happens when relay metadata arrives late. NIP-65 routing is the default for both reads and writes. Provenance / NIP-65 / relay hints / user-configured relays are four distinct facts that inform each other but never collapse.
+docs/plan.md:126:- `nmp-nip65` protocol module: Mailboxes view module (parsed kind:10002); outbox routing as a planner subsystem; recompilation triggers (kind:10002 arrival, view open, relay reconnect).
+docs/plan.md:127:- Planner refactored from "hardcoded relay set" to "compiler of logical interests → per-relay plans." See `docs/design/ndk-applesauce-lessons.md` §7 (subscription compilation lessons).
+docs/plan.md:128:- Per-pubkey relay-list cache (durable, even before LMDB lands — keep it in-memory until M3, but the data model is correct).
+docs/plan.md:129:- Indexer fallback when a pubkey's kind:10002 is unknown: opportunistic discovery from a configurable indexer relay set.
+docs/plan.md:137:- Distinct-source visibility: the diagnostics screen shows the four relay-fact lanes (NIP-65 / hint / provenance / user-configured) separately.
+docs/plan.md:139:**Runnable artifact.** iOS app with measurably different relay-fan-out behavior; demo screenshot in `docs/perf/m2/outbox-routing.md` showing per-relay coverage.
+docs/plan.md:143:### M3 — Persistence (LMDB) + full insert invariants
+docs/plan.md:147:**Scope.** Swap in-memory `EventStore` for LMDB via `Box<dyn EventStore>`. Implement the full insert invariants from `product-spec.md` §7.1: parameterized replaceable events (kind 30000–39999 by `(pubkey, kind, d-tag)`), kind:5 delete handling with tombstone persistence, NIP-40 expiration scheduling, dedup with provenance merge, claim-based GC running.
+docs/plan.md:151:- LMDB schema design doc (`docs/design/lmdb-schema.md`) — key encoding, secondary indexes, tombstones, watermarks table (populated in M4), backup/export format.
+docs/plan.md:152:- `EventStore` trait abstracted; LMDB backend; in-memory backend kept for tests.
+docs/plan.md:158:- Cold-start with primed LMDB: time-to-first-painted-timeline ≤ 1.5 s on iPhone 12.
+docs/plan.md:176:- Sync watermarks table active per-`(filter, relay)`.
+docs/plan.md:177:- Planner consults watermarks before issuing historical REQ; sync-first backfill with REQ as fallback (when relay doesn't support NIP-77).
+docs/plan.md:187:- Relay reconnect after 10 min resumes from watermark; gap filled by sync.
+docs/plan.md:271:**Scope.** Per spec doctrine D4 (single writer per fact) extended to account scope:
+docs/plan.md:299:- `nmp-nip17` protocol module: Conversation view module + ConversationList view module; SendDm action module; NIP-44 encryption / NIP-59 gift-wrapping; outbox routing for DMs (recipient inbox relays only — never public).
+docs/plan.md:335:- Profile picture update through compose → kind:0 republish with new Blossom URL → in-place refinement across all open Profile / Timeline payloads (per doctrine D1).
+docs/plan.md:356:  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
+docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Hard zero — no deferral escape, no "tracking issue" carve-out. Every pre-existing one is resolved in M10.5. If something genuinely cannot be done in M10.5 because it belongs to a later milestone (e.g. NIP-65 outbox work), then it is not a TODO/FIXME in the scoped files — it lives as a milestone task in `docs/plan.md`, not as a code marker.
+docs/plan.md:380:- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
+docs/plan.md:647:| **Outbox routing (NIP-65)** | M2 | First-class as a planner stage, not a side feature. Diagnostics show per-relay coverage. |
+docs/plan.md:648:| **NDK-style subscription aggregation** | M2 | Per `docs/design/ndk-applesauce-lessons.md` §7, the planner becomes a subscription compiler. Logical interests → per-relay plans → wire REQs, semantics-preserving merge/split. |
+docs/plan.md:656:| **NIP-77 negentropy** | M4 | Sync engine with watermarks; planner consults before REQ; capability negotiation; bytes-saved diagnostic. |
+docs/plan.md:664:| 02 | nmp-nip02 | M2 | follow-list parsing (contacts view) |
+docs/plan.md:685:| 65 | nmp-nip65 | M2 | mailboxes + outbox |
+docs/plan.md:697:- **M2 (outbox), M3 (LMDB), M4 (negentropy)** can pipeline tightly: M3 + M4 are almost mechanically pluggable once M2's compiled-plan abstraction exists.
+docs/plan.md:704:A team of two could run M5 alongside the M2–M4 sequence with no integration risk. With parallel-agent execution (this session's mode), the practical limit is conflict surface: independent crates, independent docs, and independent platform shells fan out cleanly; shared mutable files (e.g. `nmp.toml`, the codegen output, `Cargo.toml`) serialize.
+docs/plan.md:776:- **Runtime evidence.** Live mode (or replay mode with real adapters substituted for modeled segments) runs against actual LMDB, actual WebSockets, actual UniFFI marshaling. Passing here is real evidence.
+
+exec
+/bin/zsh -lc 'rg -n "struct AuthorRelayList|author_relay_lists|ingest_relay_list|parse_relay_list|CONTENT_RELAY_URL|INDEXER_RELAY_URL|RelayRole|OutboundMessage|struct LogicalInterestStatus|struct WireSubscriptionStatus|should_store_event" crates/nmp-core/src' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+crates/nmp-core/src/kernel/requests.rs:4:    pub(crate) fn relay_connecting(&mut self, role: RelayRole) {
+crates/nmp-core/src/kernel/requests.rs:11:    pub(crate) fn relay_connected(&mut self, role: RelayRole) {
+crates/nmp-core/src/kernel/requests.rs:20:    pub(crate) fn relay_failed(&mut self, role: RelayRole, error: String) {
+crates/nmp-core/src/kernel/requests.rs:40:    pub(crate) fn relay_closed(&mut self, role: RelayRole) {
+crates/nmp-core/src/kernel/requests.rs:50:    pub(crate) fn startup_requests(&mut self) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:66:            RelayRole::Content,
+crates/nmp-core/src/kernel/requests.rs:72:            RelayRole::Indexer,
+crates/nmp-core/src/kernel/requests.rs:78:            RelayRole::Indexer,
+crates/nmp-core/src/kernel/requests.rs:84:            RelayRole::Indexer,
+crates/nmp-core/src/kernel/requests.rs:90:            RelayRole::Indexer,
+crates/nmp-core/src/kernel/requests.rs:96:            RelayRole::Indexer,
+crates/nmp-core/src/kernel/requests.rs:108:    pub(crate) fn active_subscriptions(&self, role: RelayRole) -> Vec<String> {
+crates/nmp-core/src/kernel/requests.rs:118:    pub(crate) fn open_author(&mut self, pubkey: String, can_send: bool) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:142:    pub(crate) fn open_thread(&mut self, event_id: String, can_send: bool) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:174:    ) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:207:    ) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:243:    ) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:265:    pub(crate) fn close_author(&mut self, pubkey: &str) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:289:    pub(crate) fn close_thread(&mut self, event_id: &str) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:316:    ) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:324:                closes.push(OutboundMessage {
+crates/nmp-core/src/kernel/requests.rs:333:    pub(crate) fn pending_view_requests(&mut self) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:357:    pub(super) fn firehose_requests(&mut self) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:367:            RelayRole::Content,
+crates/nmp-core/src/kernel/requests.rs:374:    pub(super) fn pending_profile_claim_requests(&mut self) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:390:    pub(super) fn profile_claim_request(&mut self, pubkey: String) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:397:            RelayRole::Indexer,
+crates/nmp-core/src/kernel/requests.rs:404:    pub(super) fn author_requests(&mut self) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:419:                RelayRole::Indexer,
+crates/nmp-core/src/kernel/requests.rs:425:                RelayRole::Indexer,
+crates/nmp-core/src/kernel/requests.rs:431:                RelayRole::Content,
+crates/nmp-core/src/kernel/requests.rs:441:    pub(super) fn prepare_thread_requests(&mut self) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:483:    pub(super) fn maybe_open_thread_hydration(&mut self) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/requests.rs:499:                RelayRole::Content,
+crates/nmp-core/src/kernel/requests.rs:520:                RelayRole::Content,
+crates/nmp-core/src/kernel/requests.rs:532:        role: RelayRole,
+crates/nmp-core/src/kernel/requests.rs:536:    ) -> OutboundMessage {
+crates/nmp-core/src/kernel/requests.rs:552:        OutboundMessage {
+crates/nmp-core/src/kernel/requests.rs:558:    pub(crate) fn defer_outbound(&mut self, message: OutboundMessage) {
+crates/nmp-core/src/kernel/requests.rs:570:    pub(crate) fn record_tx(&mut self, role: RelayRole, bytes: usize) {
+crates/nmp-core/src/kernel/tests.rs:18:        .any(|request| request.role == RelayRole::Indexer));
+crates/nmp-core/src/kernel/tests.rs:21:        .any(|request| request.role == RelayRole::Content));
+crates/nmp-core/src/kernel/tests.rs:150:fn parse_relay_list_splits_nip65_markers() {
+crates/nmp-core/src/kernel/tests.rs:151:    let parsed = parse_relay_list(
+crates/nmp-core/src/kernel/status.rs:5:        self.relay_status_for(RelayRole::Content)
+crates/nmp-core/src/kernel/status.rs:9:        RelayRole::all()
+crates/nmp-core/src/kernel/status.rs:15:    pub(super) fn relay_status_for(&self, role: RelayRole) -> RelayStatus {
+crates/nmp-core/src/kernel/status.rs:46:            } else if self.relay(RelayRole::Indexer).connection == "connected" {
+crates/nmp-core/src/kernel/status.rs:52:            relay_urls: vec![INDEXER_RELAY_URL.to_string()],
+crates/nmp-core/src/kernel/status.rs:66:            relay_urls: vec![CONTENT_RELAY_URL.to_string(), INDEXER_RELAY_URL.to_string()],
+crates/nmp-core/src/kernel/status.rs:118:                relay_urls: vec![INDEXER_RELAY_URL.to_string()],
+crates/nmp-core/src/kernel/status.rs:130:            relay_urls: vec![CONTENT_RELAY_URL.to_string(), INDEXER_RELAY_URL.to_string()],
+crates/nmp-core/src/kernel/status.rs:172:                relay_urls: vec![CONTENT_RELAY_URL.to_string()],
+crates/nmp-core/src/kernel/status.rs:194:                relay_urls: vec![CONTENT_RELAY_URL.to_string()],
+crates/nmp-core/src/kernel/status.rs:222:    pub(super) fn relay(&self, role: RelayRole) -> &RelayHealth {
+crates/nmp-core/src/kernel/status.rs:228:    pub(super) fn relay_mut(&mut self, role: RelayRole) -> &mut RelayHealth {
+crates/nmp-core/src/kernel/status.rs:249:        match self.author_relay_lists.get(pubkey) {
+crates/nmp-core/src/kernel/status.rs:261:        let mut relays = vec![CONTENT_RELAY_URL.to_string(), INDEXER_RELAY_URL.to_string()];
+crates/nmp-core/src/kernel/status.rs:262:        if let Some(list) = self.author_relay_lists.get(pubkey) {
+crates/nmp-core/src/kernel/mod.rs:10:    OutboundMessage, RelayRole, CONTENT_RELAY_URL, DEFAULT_EMIT_HZ, FIATJAF_PUBKEY,
+crates/nmp-core/src/kernel/mod.rs:11:    INDEXER_RELAY_URL, JB55_PUBKEY, TEST_NPUB, TEST_PUBKEY, TIMELINE_AUTHOR_LIMIT,
+crates/nmp-core/src/kernel/mod.rs:134:struct WireSubscriptionStatus {
+crates/nmp-core/src/kernel/mod.rs:147:struct LogicalInterestStatus {
+crates/nmp-core/src/kernel/mod.rs:235:    role: RelayRole,
+crates/nmp-core/src/kernel/mod.rs:270:struct AuthorRelayList {
+crates/nmp-core/src/kernel/mod.rs:292:    relays: HashMap<RelayRole, RelayHealth>,
+crates/nmp-core/src/kernel/mod.rs:311:    deferred_outbound: VecDeque<OutboundMessage>,
+crates/nmp-core/src/kernel/mod.rs:313:    author_relay_lists: HashMap<String, AuthorRelayList>,
+crates/nmp-core/src/kernel/mod.rs:342:            relays: RelayRole::all()
+crates/nmp-core/src/kernel/mod.rs:366:            author_relay_lists: HashMap::new(),
+crates/nmp-core/src/kernel/ingest.rs:6:        role: RelayRole,
+crates/nmp-core/src/kernel/ingest.rs:8:    ) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/ingest.rs:35:    pub(super) fn handle_text(&mut self, role: RelayRole, text: &str) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/ingest.rs:79:                    outbound.push(OutboundMessage {
+crates/nmp-core/src/kernel/ingest.rs:134:    pub(super) fn handle_event(&mut self, role: RelayRole, sub_id: &str, value: &Value) {
+crates/nmp-core/src/kernel/ingest.rs:160:            10002 => self.ingest_relay_list(event),
+crates/nmp-core/src/kernel/ingest.rs:209:    pub(super) fn ingest_relay_list(&mut self, event: NostrEvent) {
+crates/nmp-core/src/kernel/ingest.rs:210:        let relay_list = parse_relay_list(event.created_at, &event.tags);
+crates/nmp-core/src/kernel/ingest.rs:219:            .author_relay_lists
+crates/nmp-core/src/kernel/ingest.rs:231:            self.author_relay_lists.insert(event.pubkey, relay_list);
+crates/nmp-core/src/kernel/ingest.rs:243:        if !self.should_store_event(sub_id, &event) {
+crates/nmp-core/src/kernel/ingest.rs:268:    pub(super) fn should_store_event(&self, sub_id: &str, event: &NostrEvent) -> bool {
+crates/nmp-core/src/kernel/ingest.rs:329:    pub(super) fn maybe_open_timeline(&mut self) -> Vec<OutboundMessage> {
+crates/nmp-core/src/kernel/ingest.rs:356:                RelayRole::Content,
+crates/nmp-core/src/relay.rs:1:pub(crate) const CONTENT_RELAY_URL: &str = "wss://relay.primal.net";
+crates/nmp-core/src/relay.rs:2:pub(crate) const INDEXER_RELAY_URL: &str = "wss://purplepag.es";
+crates/nmp-core/src/relay.rs:16:pub(crate) enum RelayRole {
+crates/nmp-core/src/relay.rs:21:impl RelayRole {
+crates/nmp-core/src/relay.rs:35:            Self::Content => CONTENT_RELAY_URL,
+crates/nmp-core/src/relay.rs:36:            Self::Indexer => INDEXER_RELAY_URL,
+crates/nmp-core/src/relay.rs:42:pub(crate) struct OutboundMessage {
+crates/nmp-core/src/relay.rs:43:    pub(crate) role: RelayRole,
+crates/nmp-core/src/actor.rs:2:use crate::relay::{OutboundMessage, RelayRole, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
+crates/nmp-core/src/actor.rs:43:    let mut relay_controls: HashMap<RelayRole, RelayControl> = HashMap::new();
+crates/nmp-core/src/actor.rs:294:fn all_relays_connected(connected_relays: &HashSet<RelayRole>) -> bool {
+crates/nmp-core/src/actor.rs:295:    RelayRole::all()
+crates/nmp-core/src/actor.rs:301:    relay_controls: &mut HashMap<RelayRole, RelayControl>,
+crates/nmp-core/src/actor.rs:306:    for role in RelayRole::all() {
+crates/nmp-core/src/actor.rs:325:    connected_relays: &HashSet<RelayRole>,
+crates/nmp-core/src/actor.rs:326:    relay_controls: &HashMap<RelayRole, RelayControl>,
+crates/nmp-core/src/actor.rs:342:    relay_controls: &HashMap<RelayRole, RelayControl>,
+crates/nmp-core/src/actor.rs:344:    outbound: Vec<OutboundMessage>,
+crates/nmp-core/src/actor.rs:352:    relay_controls: &HashMap<RelayRole, RelayControl>,
+crates/nmp-core/src/actor.rs:354:    message: OutboundMessage,
+crates/nmp-core/src/actor.rs:368:    relay_controls: &mut HashMap<RelayRole, RelayControl>,
+crates/nmp-core/src/actor.rs:369:    connected_relays: &mut HashSet<RelayRole>,
+crates/nmp-core/src/actor.rs:372:    for role in RelayRole::all() {
+crates/nmp-core/src/relay_worker.rs:1:use crate::relay::RelayRole;
+crates/nmp-core/src/relay_worker.rs:13:        role: RelayRole,
+crates/nmp-core/src/relay_worker.rs:17:        role: RelayRole,
+crates/nmp-core/src/relay_worker.rs:22:        role: RelayRole,
+crates/nmp-core/src/relay_worker.rs:26:        role: RelayRole,
+crates/nmp-core/src/relay_worker.rs:33:    pub(crate) fn role(&self) -> RelayRole {
+crates/nmp-core/src/relay_worker.rs:67:    role: RelayRole,
+crates/nmp-core/src/relay_worker.rs:77:    role: RelayRole,
+crates/nmp-core/src/relay_worker.rs:119:    role: RelayRole,
+crates/nmp-core/src/relay_worker.rs:177:    role: RelayRole,
+crates/nmp-core/src/relay_worker.rs:219:fn open_relay_socket(role: RelayRole) -> Result<RelaySocket, String> {
+crates/nmp-core/src/kernel/nostr.rs:97:pub(super) fn parse_relay_list(created_at: u64, tags: &[Vec<String>]) -> AuthorRelayList {
+crates/nmp-core/src/kernel/update.rs:32:            relay_url: CONTENT_RELAY_URL,
+
+exec
+/bin/zsh -lc 'rg -n "nmp-nip65|nmp-core|protocol module|DomainModule|ViewModule|ActionModule|extension|module composition|LMDB|watermark|EventStore|mailbox|relay" docs/design docs/product-spec docs/decisions | head -n 200' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+docs/decisions/0009-app-extension-kernel-boundary.md:1:# ADR 0009: App extension kernel boundary
+docs/decisions/0009-app-extension-kernel-boundary.md:5:**Adopts:** `docs/design/app-extension-kernel.md` (the design proposal)
+docs/decisions/0009-app-extension-kernel-boundary.md:6:**Modifies:** ADR-0006 (slice positioning), ADR-0008 (Twitter clone repositioned as first extension module)
+docs/decisions/0009-app-extension-kernel-boundary.md:11:The product spec as originally written placed 15 social-client-shaped view kinds (Profile, Timeline, Thread, Reactions, Conversation, ...) directly in `nmp-core`, alongside a closed `AppAction` enum and a closed `AppUpdate` enum. The first developer to build something that isn't a social client — Highlighter, TENEX, Win the Day, Cut Tracker, podcast apps — has two bad options:
+docs/decisions/0009-app-extension-kernel-boundary.md:13:- Add their app nouns (`Highlight`, `Project`, `Episode`, `WeightLog`, `DailyPlan`) to `nmp-core`. The framework becomes a junk drawer of every consumer's domain concepts. Untenable.
+docs/decisions/0009-app-extension-kernel-boundary.md:16:The proposal in `docs/design/app-extension-kernel.md` identifies this as a fundamental abstraction error. It argues for reframing NMP as a **Nostr-native app kernel with first-class extension modules** rather than a framework with closed built-ins.
+docs/decisions/0009-app-extension-kernel-boundary.md:22:`nmp-core` provides **generic infrastructure only**:
+docs/decisions/0009-app-extension-kernel-boundary.md:30:- Typed view registry (driven by `ViewModule` trait).
+docs/decisions/0009-app-extension-kernel-boundary.md:31:- Durable action ledger (driven by `ActionModule` trait).
+docs/decisions/0009-app-extension-kernel-boundary.md:36:`nmp-core` does **not** contain:
+docs/decisions/0009-app-extension-kernel-boundary.md:48:| `nmp-core` kernel | actor, store substrate, planner, ledger, registries, codegen, diagnostics | No |
+docs/decisions/0009-app-extension-kernel-boundary.md:49:| NMP protocol modules (`nmp-nip01`, `nmp-nip17`, `nmp-nip29`, `nmp-nip65`, `nmp-blossom`, `nmp-nwc`, …) | reusable Nostr protocol concepts: Event, Filter, Keys, gift-wrap, groups, mailboxes, blossom, NWC | Only protocol nouns |
+docs/decisions/0009-app-extension-kernel-boundary.md:53:**Five extension trait families** (concrete signatures in `docs/design/kernel-substrate.md`):
+docs/decisions/0009-app-extension-kernel-boundary.md:55:- `DomainModule` — durable non-Nostr records with migrations and indexes.
+docs/decisions/0009-app-extension-kernel-boundary.md:56:- `ViewModule` — typed reactive projections with payloads and deltas.
+docs/decisions/0009-app-extension-kernel-boundary.md:57:- `ActionModule` — durable workflows on the action ledger.
+docs/decisions/0009-app-extension-kernel-boundary.md:61:**The rule.** If implementing Highlighter, TENEX, Win the Day, Cut Tracker, or a podcast app requires adding domain nouns to `nmp-core`, the extension boundary is wrong and the kernel must change, not the app.
+docs/decisions/0009-app-extension-kernel-boundary.md:65:- **ADR-0006 (vertical-slice-first):** the slice's discipline survives — running code at every checkpoint, one architectural ingredient per sub-phase. The slice's *target* changes: the kind:0 Profile path is now built as the canonical `ViewModule` in a Nostr-protocol module, not as a built-in feature of `nmp-core`. The slice now proves the extension boundary first, then the protocol module on top.
+docs/decisions/0009-app-extension-kernel-boundary.md:66:- **ADR-0008 (Twitter clone):** the Twitter clone is repositioned as **the first canonical extension module** demonstrating the kernel boundary at scale, not as the framework's set of built-in features. The sub-phase plan grows by one or two phases (extension-boundary prototype with a tiny non-Nostr fixture module lands before the Twitter slice begins).
+docs/decisions/0009-app-extension-kernel-boundary.md:67:- **`product-spec.md` §4 (crate roster), §6.2–§6.6 (state/action/update/capabilities/views), §7 (most subsystem specs), §12 (phasing):** rewritten to reflect kernel + protocol-module + app-module layering. Built-in view kinds become "reference protocol modules with their own view modules." Built-in wallet / messages / blossom become protocol modules (`nmp-nwc`, `nmp-nip17`, `nmp-blossom`).
+docs/decisions/0009-app-extension-kernel-boundary.md:68:- **`view-catalog.md`:** reframed as the catalog of *reference Nostr extension modules* shipped with the framework (Profile, Contacts, Timeline, Thread, Reactions, Conversation, ...). Apps can use them, ignore them, or replace them. They are not in `nmp-core`.
+docs/decisions/0009-app-extension-kernel-boundary.md:79:- ADR-0007 (diagnostics bridge): applies to any extension's relay status, action ledger entries, domain records, and capability reports.
+docs/decisions/0009-app-extension-kernel-boundary.md:85:- **Smaller kernel, larger ecosystem surface.** `nmp-core` shrinks substantially. The ecosystem grows: `nmp-nip01`, `nmp-nip02`, `nmp-nip17`, `nmp-nip25`, `nmp-nip29`, `nmp-nip65`, `nmp-nip77`, `nmp-blossom`, `nmp-nwc`, `nmp-cashu` become first-class protocol modules.
+docs/decisions/0009-app-extension-kernel-boundary.md:87:- **Phase 1a takes longer.** ADR-0008's 8-week estimate grows to roughly 12–15 weeks. The kernel substrate (1a.1) and tiny fixture module land before the first Nostr-shaped extension module. Twitter clone follows on top.
+docs/decisions/0009-app-extension-kernel-boundary.md:88:- **Two fixture apps prove the boundary in v1.** A tiny non-Nostr-shaped fixture (e.g., a TODO/notes module with an app-local identity) lands in 1a.1; the Twitter clone is the first Nostr-shaped extension module. Together they prove the kernel works in both directions.
+docs/decisions/0009-app-extension-kernel-boundary.md:90:- **Future protocol-spec evolution is cleaner.** A new NIP (say NIP-100) becomes a new crate, not a `nmp-core` patch.
+docs/decisions/0009-app-extension-kernel-boundary.md:97:2. The Twitter clone (Phase 1a.2 onward) is implemented entirely as extension modules over `nmp-core` + protocol modules. `nmp-core` does not gain any of: `Profile`, `Timeline`, `Thread`, `Reactions`, `Conversation`, `Tweet`, `Compose` types or actions.
+docs/decisions/0009-app-extension-kernel-boundary.md:98:3. A future hypothetical Highlighter-lite module can be added without changes to `nmp-core`. (Not built in v1; the design must support it.)
+docs/decisions/0009-app-extension-kernel-boundary.md:102:- **Keep current spec; punt extension boundary to v2.** Rejected — shipping v1 with the wrong abstraction creates a major-version migration within a year. Worse than ~4 extra weeks of design now.
+docs/decisions/0009-app-extension-kernel-boundary.md:115:| 5. Minimum v1 extension API before social-client proof app | The 1a.1 substrate (5 trait families + codegen for one fixture) is the minimum. Twitter clone consumes it from 1a.2 onward |
+docs/product-spec/cli-toolchain-phasing.md:15:nmp gen view <name>                Scaffold a ViewModule in the app core crate.
+docs/product-spec/cli-toolchain-phasing.md:16:nmp gen action <name>              Scaffold an ActionModule in the app core crate.
+docs/product-spec/cli-toolchain-phasing.md:50:- Settings (relay config, account switcher, debug diagnostics)
+docs/product-spec/cli-toolchain-phasing.md:84:| §7.2 Where views live | Materialized lazily in `nmp-core`; surfaced as snapshots in `AppState.views` and as `ViewBatch` deltas. Opt-in opaque handles deferred. | §6.6 |
+docs/product-spec/cli-toolchain-phasing.md:86:| §7.4 NIP-46 bunker as capability | Internal to `nmp-core`; not exposed as a `CallbackInterface`. Pairing flow surfaces as `Effect::BunkerPairingReady` for native rendering of QR/URI. | §6.4, §7.4 |
+docs/product-spec/cli-toolchain-phasing.md:97:- Server-side: no relay implementation beyond test doubles.
+docs/product-spec/cli-toolchain-phasing.md:114:| 0. Foundations | Workspace, `nmp-core` kernel skeleton, `nmp-codegen` skeleton, empty per-app generated crate, headless test harness | Actor starts/stops; `nmp gen modules --check` deterministic; `cargo test --workspace` green |
+docs/product-spec/cli-toolchain-phasing.md:118:| 4. Action and ledger primitive | Fire-and-forget action pipeline, signer/publisher traits, generalized ledger, per-relay publish status, no untracked side-effect path | Ledger rows exist before relay/HTTP side effects; partial publish and repair states are renderable |
+docs/product-spec/cli-toolchain-phasing.md:120:| 6. v1 release | Publish primitive kernel crates, minimal examples, migration notes, explicit future extension points | A developer can build a small event viewer without writing cleanup, dedup, replaceable handling, or FFI batching |
+docs/product-spec/cli-toolchain-phasing.md:126:| 7. Relay policy | NIP-65 outbox, gossip relay lists, relay liveness, fail-closed DM routing | Implemented as policy over the v1 resolver, metadata store, provenance, and action ledger |
+docs/product-spec/cli-toolchain-phasing.md:127:| 8. Sync | NIP-77 backfill, watermarks, relay capability probing, progress diagnostics | Live views tail before sync completes; authoritative absence requires coverage proof |
+docs/product-spec/cli-toolchain-phasing.md:130:| 11. Messaging | NIP-17/NIP-44/NIP-59, conversation store, background decrypt crate | Missing DM inbox relays fail closed; module owns no separate relay pool or persistence engine |
+docs/product-spec/cli-toolchain-phasing.md:156:3. **Signer policy for sub-actions.** When an action composes multiple sub-actions each requiring signing (e.g., publishing a note and a relay-list update in one user step), how is the bunker prompted — one prompt or N?
+docs/product-spec/cli-toolchain-phasing.md:159:6. **Pluggable view kinds.** Are project-specific `ViewSpec` variants first-class (enum extension is awkward in Rust), or are they added by string-keyed payloads with consumer-side decoding?
+docs/product-spec/cli-toolchain-phasing.md:174:- **EventStore.** The reactive single source of truth for all Nostr events. Owned by the actor; not exposed at FFI.
+docs/product-spec/cli-toolchain-phasing.md:175:- **Outbox routing.** NIP-65-based automatic relay selection for both reads and writes.
+docs/product-spec/cli-toolchain-phasing.md:177:- **View.** A pre-built derived projection of `EventStore` contents. Opened by `OpenView` action; payload arrives via `AppState.views` / `ViewBatch`.
+docs/product-spec/cli-toolchain-phasing.md:180:- **Watermark.** A `(filter, relay) → time` record indicating how much of that filter we have already reconciled from that relay. The basis of coverage-aware backfill (§7.1, §7.8).
+docs/product-spec/subsystems.md:7:### 7.1 EventStore
+docs/product-spec/subsystems.md:17:| Duplicate id | Merge relay provenance set; keep earliest `received_at`; do not overwrite. |
+docs/product-spec/subsystems.md:24:| Provenance | Every event records typed sidecar provenance: relay URL, first seen, last seen, source, and deterministic primary relay. |
+docs/product-spec/subsystems.md:27:Storage backend is configurable via `AppConfig.storage_backend` (LMDB or SQLite-style native backend, IndexedDB/OPFS strategy for web, final choice resolved before v1). The store wraps the Rust Nostr SDK protocol types, but NMP owns the application-kernel storage traits because the app kernel needs typed provenance, action ledger rows, relay metadata, domain records, and bounded-view indexes in addition to raw events.
+docs/product-spec/subsystems.md:29:GC: a claim-based collector tracks `view_id → Vec<event_id>` references. View close drops claims. A periodic `prune()` removes events with zero claims that are also absent from declared "pinned" sets (sessions' contact-list events, sessions' relay-list events).
+docs/product-spec/subsystems.md:31:**Sync watermarks.** The store maintains a per-`(filter_signature, relay_url)` table:
+docs/product-spec/subsystems.md:34:watermarks {
+docs/product-spec/subsystems.md:36:  relay_url: String,
+docs/product-spec/subsystems.md:37:  synced_up_to: u64,           // unix seconds; "we have everything matching this filter on this relay up to T"
+docs/product-spec/subsystems.md:46:A cache-miss query against a fully-synced `(filter, relay)` pair is **authoritative**: the answer is "this event does not exist on that relay." A cache-miss against an unsynced pair triggers either a sync (if NIP-77 supported) or a fallback fetch.
+docs/product-spec/subsystems.md:50:- Pointer/address misses: cache-first lookup for event id or replaceable address, batched and deduped across waiting views, then relay hints, then configured fallback sources.
+docs/product-spec/subsystems.md:52:- Authoritative absence: only a complete coverage record/watermark can turn a miss into "not found." A non-empty cache result is not proof that a query is complete.
+docs/product-spec/subsystems.md:54:The default loader queries open relays and configured sources. Users can add custom sources (CDN cache, local mirror, etc.) through app-kernel extension points, but loaded events still enter through the same verified insert path.
+docs/product-spec/subsystems.md:63:- **Coverage-aware backfill.** Before issuing historical traffic, the planner consults cache coverage/watermarks (§7.1). Complete coverage serves from cache; partial coverage schedules a gap fill; unknown coverage triggers bounded fetch/REQ or NIP-77 if supported. A non-empty cache result is never treated as complete by itself.
+docs/product-spec/subsystems.md:64:- **Logical vs. wire subscriptions.** A logical subscription belongs to a view/action/monitor. A wire subscription belongs to a relay. Many logical consumers may share one wire REQ, and each consumer still receives only events matching its canonical filter.
+docs/product-spec/subsystems.md:65:- **Coalescing.** Filters that are equal or safely subsumable into a single broader filter share one REQ per relay. The planner maintains a formal merge lattice for `limit`, `since`, `until`, multi-filter arrays, and tag operators.
+docs/product-spec/subsystems.md:66:- **Loader integration.** Pointer/address/tag/timeline misses go through the pointer loader registry with cache-first batching, dedupe, relay hints, cancellation on view close, and explicit missing-window state.
+docs/product-spec/subsystems.md:70:- **Reconnect.** On relay reconnect, the planner restores live REQs and schedules a coverage-aware gap fill. View payloads do not reset.
+docs/product-spec/subsystems.md:74:Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
+docs/product-spec/subsystems.md:80:| Subscription with `authors` filter | Union of each pubkey's write relays (kind-10002), deduplicated. Pubkeys without known mailboxes trigger an opportunistic kind-10002 fetch from indexer relays. |
+docs/product-spec/subsystems.md:81:| Subscription with `p` tag filter or notifications | Union of each tagged pubkey's inbox relays. |
+docs/product-spec/subsystems.md:82:| Subscription with neither | Active session's read relays. |
+docs/product-spec/subsystems.md:83:| Publish of any signed event | Author's write relays. |
+docs/product-spec/subsystems.md:84:| Publish with `p` tags (DMs, mentions, reactions) | Author's write relays **plus** each tagged pubkey's inbox relays. |
+docs/product-spec/subsystems.md:85:| DM (NIP-17 gift-wrapped) | **Only** resolved recipient inbox relays. Never the author's write relays. Never the active session's "default" relays. Missing recipient inbox relays fail closed. |
+docs/product-spec/subsystems.md:86:| Discovery (kind-10002 fetch for unknown pubkeys) | Configurable indexer relay set (default: a curated list of high-coverage relays). |
+docs/product-spec/subsystems.md:90:- "Publish leaked to wrong relays" → ruled out by the safe API. The developer cannot supply a relay list to `SendNote`. Explicit overrides are named, one-shot, and debug-flagged in logs.
+docs/product-spec/subsystems.md:91:- "DM accidentally public" → ruled out by the safe API. The DM publish path consults only resolved inbox relays; there is no fallback-to-all-relays path for gift wraps.
+docs/product-spec/subsystems.md:92:- "Reads missing an author's actual relays" → bounded and surfaced. If the author's kind-10002 is reachable it is opportunistically fetched on first contact; if not, coverage and diagnostic state expose the miss risk and configured fallback policy.
+docs/product-spec/subsystems.md:95:**Per-pubkey relay-list lifecycle.**
+docs/product-spec/subsystems.md:97:- First contact with an unknown pubkey → enqueue kind-10002 fetch from indexer relays.
+docs/product-spec/subsystems.md:98:- Fresher kind-10002 arrives → invalidate dependent subscriptions, recompute relay sets, re-issue REQs as needed.
+docs/product-spec/subsystems.md:101:The gossip cache is the `nostr-gossip` crate; backend selection (in-memory vs SQLite) follows the storage backend choice. Watermarks (§7.1) intersect with outbox: a sync watermark is keyed by `(filter, relay)` and naturally tracks per-author per-relay coverage.
+docs/product-spec/subsystems.md:121:    pub mailboxes_view_id: ViewId,
+docs/product-spec/subsystems.md:127:Signers are managed entirely in `nmp-core`. The initial product signer catalog is:
+docs/product-spec/subsystems.md:135:The signer abstraction inside `nmp-core` is a Rust trait with `sign(unsigned_event) -> Future<signed_event>`. Adding a signer kind is an internal task; external developers do not implement signers.
+docs/product-spec/subsystems.md:155:The ledger is general, not relay-only. It can represent local optimistic commit, signer prompt, per-relay publish attempt, HTTP upload, NWC request, retry, repair, partial failure, timeout, and final status. Relay publishes additionally track attempted/acked/failed/timed-out by relay plus required success count.
+docs/product-spec/subsystems.md:226:View warmth: a view stays cached for 30 seconds after its last claim is dropped (configurable). Re-opening within the window costs zero relay traffic and zero re-sync.
+docs/product-spec/subsystems.md:242:Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.
+docs/product-spec/subsystems.md:247:View opens → Live REQ handler starts → Planner consults coverage → Sync engine reconciles gaps → EventStore inserts → ViewBatch emits
+docs/product-spec/subsystems.md:252:**Watermarks as a first-class type.** The engine reads and writes the `watermarks` table introduced in §7.1. A watermark answers two questions:
+docs/product-spec/subsystems.md:254:- Has this `(filter, relay)` pair ever been synced?
+docs/product-spec/subsystems.md:261:1. **App foreground.** On `AppAction::Foreground`, the engine schedules an incremental sync for the active user's home filter (kind:1, kind:6, kind:7 matching followed authors) against their write relays. Runs in the tokio runtime; emits `SyncState` updates as it progresses; no UI blocking.
+docs/product-spec/subsystems.md:262:2. **View open.** When a view opens whose filter has a gap (per watermark/coverage), the engine reconciles the gap concurrently with the live REQ tail. Progress is visible in `SyncState`; the view payload streams in as events land.
+docs/product-spec/subsystems.md:270:    pub relay: String,
+docs/product-spec/subsystems.md:277:**Per-relay capability negotiation.** Not every relay implements NIP-77. The engine maintains per-relay capability metadata, probed lazily on first contact. Unsupported relays cause the planner to fall back to bounded fetch/REQ scanning for that relay only — other relays in the same fan-out may still use sync.
+docs/product-spec/subsystems.md:287:    pub watermarks_summary: WatermarksSummary,  // coverage stats per relay
+docs/product-spec/subsystems.md:318:The module is intentionally stricter than common fallback patterns: if recipient inbox relays are unknown, a DM send action fails closed with diagnostic state instead of publishing gift wraps to default/public relays. Read state is local conversation state unless and until an explicit protocol-level receipt policy is added.
+docs/product-spec/subsystems.md:330:- Subscription opened with no relays resolvable
+docs/product-spec/subsystems.md:342:- `MockRelay` (re-exported from `nostr-relay-builder`).
+docs/product-spec/subsystems.md:345:- `NetworkChaos` for injecting drops/latency at the relay-pool layer.
+docs/product-spec/subsystems.md:373:iOS Notification Service Extension and Android background workers link only this crate. Memory and time budgets (iOS NSE ~24MB / 30s) are observed by design: no relay connections, no full event store load, only the minimal state needed to decrypt and format a preview.
+docs/product-spec/subsystems.md:384:- On successful relay-side acknowledgement (OK message), the record is deleted.
+docs/product-spec/subsystems.md:402:- Sync watermarks coverage (per relay: % of opened filters fully synced).
+docs/product-spec/subsystems.md:404:- Cache hit rate (event store reads served without relay traffic).
+docs/product-spec/subsystems.md:406:- Relay provenance rows and primary-relay selection counts.
+docs/product-spec/subsystems.md:408:- Action ledger counts by status, including per-relay publish status.
+docs/product-spec/subsystems.md:412:- Outstanding subscriptions per relay.
+docs/product-spec/subsystems.md:441:Post-v1 modules must be thin policy layers over the v1 kernel. A module fails review if it implements its own relay pool, persistence engine, signer lifecycle, subscription scheduler, action runner, or platform cache. It may define policy, view payloads, domain-store records, and actions.
+docs/product-spec/subsystems.md:446:| Sync | Canonical filter matcher, relay capabilities, cache coverage, monitor progress, domain store |
+docs/product-spec/subsystems.md:447:| Relay policy / outbox | Relay-target resolver, relay metadata store, provenance, canonical filters, action ledger |
+docs/product-spec/subsystems.md:448:| Messaging | Conversation domain store, signer/decrypt capabilities, relay resolver, action ledger |
+docs/product-spec/subsystems.md:454:Diagnostics must expose monitor status, relay capabilities, sync progress, action ledger rows, domain-store sizes, and per-account active capabilities so module behavior is inspectable without platform-specific debugging.
+docs/product-spec/appendices.md:17:The event store, gossip cache, sync watermarks, working set, and signer state all live in the actor and **never cross FFI**.
+docs/product-spec/appendices.md:32:| **Local relay / localhost IPC.** Rust runs an in-process Nostr relay (e.g. `LocalRelay` from `nostr-relay-builder`); platform talks Nostr over WebSocket to it. | Some Tauri apps; Citrine-style Android setups conceptually | WebSocket+JSON tax for in-process IPC. Outbox routing semantics get weird (the "relay" is local but represents many remote relays). The framework's value-add (views, actions, sessions as state) gets obscured behind a protocol that wasn't designed for it. |
+docs/design/view-catalog/template-and-enumeration.md:7:> **Audience:** Framework contributors building reference Nostr protocol modules. Each view kind below is a `ViewModule` shipped in a reusable protocol crate such as `nmp-nip01`, `nmp-nip10`, `nmp-nip25`, `nmp-nip65`, or `nmp-nip17`.
+docs/design/view-catalog/template-and-enumeration.md:9:> **Status:** Rev 2, reframed per ADR-0009. These view kinds are not in `nmp-core`; apps consume them by adding the owning module crate to `nmp.toml` and regenerating the per-app FFI crate.
+docs/design/view-catalog/template-and-enumeration.md:17:Every reference Nostr view module lives in a `nmp-nip*` crate and implements `ViewModule` from `nmp-core::substrate`:
+docs/design/view-catalog/template-and-enumeration.md:28:impl ViewModule for <Kind>Module {
+docs/design/view-catalog/template-and-enumeration.md:90:| 3 | Mailboxes | `nmp-nip65` | stub | 1a.4 |
+docs/decisions/0005-domain-keyed-platform-shadow.md:54:- **Rust side does not change.** Projections are already domain-keyed (`Projections.author_display[pubkey]`); the planner already dedups identical specs; view warmth is already the TTL. The reverse index already routes by domain attributes. Nothing in `nmp-core` shifts.
+docs/product-spec/api-surface.md:9:The concrete FFI API is per-app generated. `nmp-core` defines kernel primitives and extension traits; `nmp gen modules` composes the selected kernel, protocol modules, and app modules into a generated `nmp-app-<name>` crate that exposes closed typed enums to Swift/Kotlin/TypeScript.
+docs/product-spec/api-surface.md:22:    /// `config` carries data directory, default relays, storage backend choice,
+docs/product-spec/api-surface.md:48:`AppConfig` is a `uniffi::Record` containing only platform-resolved primitives (paths, lists of relay URLs, feature-flag booleans). No `Arc<dyn ...>` types in the config — capabilities are registered separately via setters so each can be bridged on its own schedule.
+docs/product-spec/api-surface.md:82:`AppAction` is a generated per-app `uniffi::Enum`, not a closed enum in `nmp-core`. The generated enum composes kernel variants, selected Nostr protocol module variants, and app-specific module variants:
+docs/product-spec/api-surface.md:93:The long-term action catalog below is illustrative. Each item belongs in the relevant module crate, not in `nmp-core`.
+docs/product-spec/api-surface.md:149:    OverrideRelaysForNext { relays: Vec<String> },
+docs/product-spec/api-surface.md:180:    NipAuthChallenge { relay: String, challenge: String },
+docs/product-spec/api-surface.md:235:**Views are opened via `dispatch(OpenView)` with a platform-generated `ViewId`, and updates arrive as `ViewBatch` entries keyed by that id.** Materialization is lazy in `nmp-core` — view payloads live in the actor and are projected into `ViewSnapshots`/`ViewBatch` on every change.
+docs/product-spec/api-surface.md:246:`ViewSpec` is an enum of supported view kinds. v1 covers the primitive view protocol plus minimal built-in views for testing: raw filter, event detail, and timeline-like bounded list. The long-term product catalog covers profile, contacts, mailboxes, mutes, blossom-servers, timeline, thread, replies, reactions, conversation-list, conversation, zap-history, wallet-balance, wot-rank, and search. Each maps to a typed payload variant.
+docs/design/view-catalog/stubs-validation-next.md:11:Parsed kind:3 follow list for one pubkey. Payload: `ContactsView { pubkey, follows: Vec<ContactEntry>, raw_event_id }`. ContactEntry has pubkey + (resolved via projection) display name + relay hint. Phase 1.
+docs/design/view-catalog/stubs-validation-next.md:15:Parsed kind:10002 for one pubkey. Payload: `MailboxesView { pubkey, inbox_relays: Vec<String>, outbox_relays: Vec<String>, raw_event_id }`. Phase 1.
+docs/design/view-catalog/stubs-validation-next.md:53:- **Consumer-defined view kinds.** v1 owns the enum. Per spec §13 open question 6, this may relax in v2 via either enum extension (awkward) or string-keyed payloads (consumer-decoded). Decide post-v1.
+docs/design/view-catalog/profile-timeline-thread-reactions.md:78:    kinds: vec![0, 10002],                      // kind:0 (profile) + mailbox-tagged refresh
+docs/design/view-catalog/profile-timeline-thread-reactions.md:248:    pub root_event: EventCoord,           // id + author + relay hint
+docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md:5:**Resolves:** `docs/design/app-extension-kernel.md` open question 1
+docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md:10:ADR-0009 establishes that NMP is a kernel with extension modules. Apps assemble themselves from a kernel (`nmp-core`), a set of protocol modules (`nmp-nip01`, `nmp-nip17`, ...), and an app-specific core crate (`twitter-core`, `highlighter-core`, ...). Each layer can define typed `AppAction` variants, `AppUpdate` variants, `ViewSpec` variants, and so on.
+docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md:14:1. **Generated app enum.** Each app's build runs a codegen step that produces concrete per-app `AppAction = KernelAction | Nip01Action | Nip17Action | TwitterAction | ...` enums by composing variants from the kernel, the chosen protocol modules, and the app's own core crate. The FFI exposes the per-app concrete types. Compile-time type safety end-to-end.
+docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md:36:kernel = "nmp-core"
+docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md:51:├── Cargo.toml             # depends on nmp-core, the chosen protocol modules, twitter-core
+docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md:74:    // Kernel variants (from nmp-core)
+docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md:103:Inside `nmp-core`'s actor, message handling is generic over module identifier:
+docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md:164:If a future need for runtime module loading emerges (e.g., shipping a Highlighter "extension" that adds functionality to an existing app), it can be added as a layer on top of the generated enum (e.g., a `KernelAction::Plugin { namespace, bytes }` variant that intentionally falls back to type-erased dispatch for plugin code). The default path stays typed.
+docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md:171:- Adding a new protocol module (`nmp-nip-XX`) becomes a standard pattern: create the crate, define the `Action`/`Update`/`ViewSpec`/`Capability` types, add to `nmp.toml`, run codegen.
+docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md:184:- Phase 1a.1 (kernel substrate prototype) generates a tiny `nmp-app-fixture` crate with one module set demonstrating DomainModule, ViewModule, ActionModule, CapabilityModule, and IdentityModule. Bindings compile and round-trip a dispatch in the reference shell.
+docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md:185:- Phase 1a.2 (Profile module on desktop) generates `nmp-app-twitter` with `nmp-core` + `nmp-nip01` + `twitter-core`. UniFFI bindings compile.
+docs/product-spec/overview-and-dx.md:17:A Cargo workspace shipping a Nostr-native **app kernel** (`nmp-core`), reusable **Nostr protocol modules** (`nmp-nip01`, `nmp-nip17`, `nmp-nip65`, etc.), app-owned extension modules, a codegen tool (`nmp gen modules`) that produces per-app concrete FFI enums/wrappers, FFI bindings for Swift/Kotlin/TypeScript, a wasm target, a scaffolding CLI, and reference platform shells.
+docs/product-spec/overview-and-dx.md:19:The kernel composes the `rust-nostr` crate family plus OS capability crates into a substrate. It owns actor runtime, verified event store, subscription planner, relay routing pipeline, signer/session plumbing, durable action ledger, domain-store substrate, typed view registry, capability bridge, platform shadow/codegen machinery, diagnostics, and test harnesses.
+docs/product-spec/overview-and-dx.md:21:The kernel does **not** own Profile, Timeline, Thread, Reactions, Conversation, Wallet, DM, Blossom, or app-specific domain concepts. Those live in reusable protocol modules or app crates. Platform code renders state and dispatches user intents — nothing else.
+docs/product-spec/overview-and-dx.md:31:### D0. Kernel + extension modules — no app nouns in `nmp-core`
+docs/product-spec/overview-and-dx.md:33:Per ADR-0009, NMP is a Nostr-native app kernel plus extension modules. The kernel provides substrate; protocol modules and app modules contribute typed variants via `ViewModule`, `ActionModule`, `DomainModule`, `CapabilityModule`, and `IdentityModule`. If implementing a real app requires adding domain nouns to `nmp-core`, the kernel boundary is wrong and must change.
+docs/product-spec/overview-and-dx.md:37:- `nmp-core` becoming a junk drawer of every consumer's domain concepts.
+docs/product-spec/overview-and-dx.md:56:NIP-77 negentropy reconciliation is the default backfill mechanism. Every `(filter, relay)` pair the app touches is treated as a tracked sync target with a watermark. Live REQ remains the tailing path, but historical gaps consult coverage first and prefer sync over REQ scans when relays support it.
+docs/product-spec/overview-and-dx.md:60:### D3. Outbox routing is automatic; manual relay selection is the opt-out
+docs/product-spec/overview-and-dx.md:62:Per NIP-65, reads and writes are routed to the relevant relays by framework policy without normal app code specifying them. Subscriptions with `authors` filters route to those authors' write relays; publishes go to the author's write relays plus tagged recipients' inbox relays; discovery falls back to a configurable indexer set.
+docs/product-spec/overview-and-dx.md:64:The safe public path does not ask the developer to pick relays per operation. Explicit override and diagnostic/test paths exist, but they are named, observable, and excluded from the default app-building flow.
+docs/product-spec/overview-and-dx.md:68:- Posts to relays the author hasn't declared as write relays.
+docs/product-spec/overview-and-dx.md:69:- DMs leaked to public relays.
+docs/product-spec/overview-and-dx.md:70:- Silent reads against a default relay set that miss an author's actual relays; unknown relay lists surface as coverage/diagnostic state and use a bounded fallback policy.
+docs/product-spec/overview-and-dx.md:102:- Relay implementations (we depend on `relay-builder` for tests; we do not ship a production relay).
+docs/product-spec/overview-and-dx.md:144:3. Publish of an event to relays the author has not declared as write relays, without explicit override.
+docs/product-spec/overview-and-dx.md:145:4. DM published to public relays.
+docs/product-spec/overview-and-dx.md:149:8. Two concurrent UI subscriptions for the same filter producing two relay REQs.
+docs/product-spec/overview-and-dx.md:160:- correctly routes to the right relays automatically,
+docs/product-spec/overview-and-dx.md:180:| `nmp-core` | Kernel substrate: actor, store, planner, ledger, registries, extension traits, diagnostics | Pure Rust |
+docs/product-spec/overview-and-dx.md:189:| `nmp-nip65` | Mailboxes view and outbox routing helpers | Pure Rust |
+docs/product-spec/overview-and-dx.md:195:| `nmp-testing` | Mock relay, factories, simulated time, perf-replay harness | Pure Rust |
+docs/product-spec/overview-and-dx.md:257:$ npx @nmp/cli init relay-cat
+docs/product-spec/overview-and-dx.md:260:? Storage backend (default for non-web): ◉ LMDB  ○ SQLite  ○ nostrdb  ○ In-memory
+docs/product-spec/overview-and-dx.md:262:? Default relays (comma-separated): wss://relay.damus.io,wss://nos.lol
+docs/product-spec/overview-and-dx.md:266:✓ Scaffolded relay-cat in ./relay-cat
+docs/product-spec/overview-and-dx.md:272:$ cd relay-cat && nix develop
+docs/product-spec/overview-and-dx.md:291:The developer should not be writing relay code, subscription bookkeeping, cache invalidation, or replaceable-event handling. Ever. If they catch themselves doing so, that is the symptom of either a missing built-in or a framework bug.
+docs/product-spec/overview-and-dx.md:297:- Pick relays per subscription or publish (outbox handles it).
+docs/product-spec/overview-and-dx.md:299:- De-duplicate events across relays.
+docs/product-spec/overview-and-dx.md:306:- Schedule background relay reconnection.
+docs/decisions/0006-vertical-slice-first.md:5:**Modified by:** ADR-0009 (the slice is now built on top of the kernel substrate; the kind:0 path is a Profile `ViewModule` in `nmp-nip01`, not a built-in feature of `nmp-core`)
+docs/decisions/0006-vertical-slice-first.md:13:The classic failure mode at this stage is **horizontal expansion** — building "the EventStore" comprehensively, then "the planner" comprehensively, then "the views" comprehensively, then finally stitching them together at the end, only to discover that the FFI surface or the relay adapter or the storage backend doesn't actually compose the way the model assumed.
+docs/decisions/0006-vertical-slice-first.md:15:The walking-skeleton / tracer-bullet pattern argues for the opposite: build one **narrow vertical** through every layer first, validate it works end-to-end against a real relay and real storage, *then* expand.
+docs/decisions/0006-vertical-slice-first.md:38:                       │ direct fn calls into nmp-core)
+docs/decisions/0006-vertical-slice-first.md:40:│  nmp-core actor (minimal)                                    │
+docs/decisions/0006-vertical-slice-first.md:48:│  EventStore (minimal)                                        │
+docs/decisions/0006-vertical-slice-first.md:49:│  - in-memory only (no LMDB yet)                              │
+docs/decisions/0006-vertical-slice-first.md:54:                       │ events from relay
+docs/decisions/0006-vertical-slice-first.md:57:│  - one WebSocket via nostr-sdk to one relay                  │
+docs/decisions/0006-vertical-slice-first.md:59:│  - no outbox routing yet (hardcoded relay)                   │
+docs/decisions/0006-vertical-slice-first.md:66:- **LMDB / durable storage.** In-memory only; cold restart loses everything. Wire LMDB in *after* the slice works.
+docs/decisions/0006-vertical-slice-first.md:67:- **Outbox routing.** Hardcoded single-relay configuration. NIP-65 fan-out comes after.
+docs/decisions/0006-vertical-slice-first.md:80:3. When the relay delivers a kind:0, the avatar updates in place to the real picture / name / NIP-05.
+docs/decisions/0006-vertical-slice-first.md:81:4. Closing the window CLOSEs the relay subscription after the 30s grace.
+docs/decisions/0006-vertical-slice-first.md:82:5. The same component instance mounted N times in the UI shares one underlying relay REQ (per the wrapper's refcount).
+docs/decisions/0006-vertical-slice-first.md:84:7. `firehose-bench live` mode can run the same flow against a real relay and report measured (not modeled) numbers for cold_start and a tiny version of profile_thrashing.
+docs/decisions/0006-vertical-slice-first.md:93:- The actor's synchronous fan-out hits the latency budget against real relay frame arrival, not just `mem::replace` calls in a benchmark.
+docs/decisions/0006-vertical-slice-first.md:94:- A real WebSocket → real EventStore → real DeltaBuffer → real component update is measurable end-to-end.
+docs/decisions/0006-vertical-slice-first.md:98:- LMDB performance (slice is in-memory).
+docs/decisions/0006-vertical-slice-first.md:100:- Outbox routing fan-out (slice is single-relay).
+docs/decisions/0006-vertical-slice-first.md:110:- **Phase 1 produces a runnable desktop demo at its first checkpoint**, not just passing unit tests. Anyone can `cargo run` and see an avatar appear from a real relay.
+docs/decisions/0006-vertical-slice-first.md:111:- **The firehose-bench live mode becomes unblocked** for the slice's narrow scope (cold_start with a single Profile view, profile_thrashing with mount/unmount churn). Other scenarios stay blocked until their dependencies (LMDB, multi-relay, NSE, etc.) land.
+docs/decisions/0006-vertical-slice-first.md:112:- **Subsequent expansion has a working substrate to build on.** Adding LMDB is a `Box<dyn EventStore>` swap. Adding multi-relay is a planner change. Adding negentropy is a planner change. Adding iOS is a UniFFI wrap of the existing actor. None require redesigning the architecture.
+docs/decisions/0006-vertical-slice-first.md:123:- Firehose-bench live mode runs cold_start and a slice version of profile_thrashing against a real relay.
+docs/decisions/0006-vertical-slice-first.md:124:- A 5-minute manual demo: launch the desktop app, see the avatar appear, kill the relay connection and watch the reconnect, mount/unmount the avatar 100 times rapidly, check no leaks.
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:12:1. How can an app show what is happening at the networking level: relay connection state, wire subscriptions, logical view interests, cache coverage, reconnects, EOSE/CLOSED, and relay capabilities?
+
+codex
+I’ve got the first concrete problem: the docs make `nmp-core` consume `nmp-nip65` while `nmp-nip65` also depends on `nmp-core`, which would be a crate cycle and also muddies the kernel/module boundary. I’m checking the rest for similar “looks clean in prose but fails at implementation seam” issues.
+exec
+/bin/zsh -lc "nl -ba docs/design/subscription-compilation/outbox.md | sed -n '1,235p'" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc "nl -ba docs/design/subscription-compilation/nip65.md | sed -n '1,230p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# Subscription Compilation §6 — `nmp-nip65` Crate Layout
+     2	
+     3	> Parent: `docs/design/subscription-compilation.md`.
+     4	> Read first: `docs/design/kernel-substrate.md` §3 (`ViewModule`) for the trait this crate implements; `docs/design/app-extension-kernel.md` §3 layering table — `nmp-nip65` is a **protocol module**, not an app module.
+     5	
+     6	`nmp-nip65` is the first NMP protocol module (per the v1 reference-modules list in `docs/design/kernel-substrate.md` §11) whose job is *not* to expose product views. It exists primarily as **the canonical source of the mailbox cache** the compiler consults. It contributes one `ViewModule` (for app-side rendering of "this user's relay list") and a small public API the compiler imports directly without going through the FFI surface.
+     7	
+     8	## 6.1 File structure
+     9	
+    10	```
+    11	crates/nmp-nip65/
+    12	├── Cargo.toml
+    13	├── README.md
+    14	├── src/
+    15	│   ├── lib.rs              # re-exports + crate-level documentation
+    16	│   ├── module.rs           # impl ViewModule for MailboxesView
+    17	│   ├── parse.rs            # kind:10002 tag parsing (extracted from kernel)
+    18	│   ├── cache.rs            # MailboxCache trait + InMemory impl
+    19	│   ├── routing.rs          # AuthorRouting, RoutingSource, mailbox lookup API
+    20	│   └── tests/
+    21	│       ├── parse.rs        # round-trip + edge-case tag parsing
+    22	│       ├── routing.rs      # mailbox → relay-set resolution scenarios
+    23	│       └── snapshot.rs     # cache snapshot/restore for compiler-input tests
+    24	└── tests/                  # integration tests against the in-memory cache
+    25	```
+    26	
+    27	Soft target per file: ≤ 300 LOC (AGENTS.md). The crate stays small; everything heavier (filter compilation, indexer probes) lives in `nmp-core::kernel::planner`, not here.
+    28	
+    29	## 6.2 Traits implemented
+    30	
+    31	`nmp-nip65` implements exactly one extension trait family: `ViewModule`. It does *not* implement `ActionModule` (kind:10002 publish is the user's own "update my relay list" action, deferred to M6's action ledger; in this milestone it has no write surface). It does *not* implement `DomainModule` (mailbox records live in the kernel-owned mailbox cache; they are queryable Nostr events, not durable app-defined records).
+    32	
+    33	### `MailboxesView` (`impl ViewModule`)
+    34	
+    35	```rust
+    36	// crates/nmp-nip65/src/module.rs
+    37	
+    38	pub struct MailboxesView;
+    39	
+    40	#[derive(Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
+    41	pub struct MailboxesSpec {
+    42	    pub pubkey: Pubkey,
+    43	}
+    44	
+    45	#[derive(Clone, Serialize)]
+    46	pub struct MailboxesPayload {
+    47	    pub pubkey: Pubkey,
+    48	    pub read:  Vec<RelayUrl>,
+    49	    pub write: Vec<RelayUrl>,
+    50	    pub both:  Vec<RelayUrl>,
+    51	    pub created_at: UnixSeconds,           // 0 if unknown
+    52	    pub source: MailboxSource,
+    53	}
+    54	
+    55	pub enum MailboxSource {
+    56	    Cached    { freshness: FreshnessHint },
+    57	    Fetching,
+    58	    Unknown,
+    59	}
+    60	
+    61	impl ViewModule for MailboxesView {
+    62	    const NAMESPACE: &'static str = "nip65.mailboxes";
+    63	    type Spec    = MailboxesSpec;
+    64	    type Payload = MailboxesPayload;
+    65	    type Delta   = MailboxesPayload;       // payloads are small; emit whole snapshots
+    66	    type Key     = Pubkey;
+    67	    type State   = MailboxesPayload;
+    68	
+    69	    fn key(spec: &MailboxesSpec) -> Pubkey {
+    70	        spec.pubkey.clone()
+    71	    }
+    72	
+    73	    fn dependencies(spec: &MailboxesSpec) -> ViewDependencies {
+    74	        ViewDependencies::author_kind(&spec.pubkey, 10002)
+    75	    }
+    76	
+    77	    fn interests(spec: &MailboxesSpec, ctx: &InterestContext) -> Vec<LogicalInterest> {
+    78	        vec![LogicalInterest {
+    79	            id: ctx.fresh_id(),
+    80	            scope: InterestScope::Global,
+    81	            shape: InterestShape {
+    82	                authors: btreeset![spec.pubkey.clone()],
+    83	                kinds:   btreeset![10002],
+    84	                limit:   Some(1),
+    85	                ..Default::default()
+    86	            },
+    87	            hints: vec![],
+    88	            lifecycle: InterestLifecycle::OneShot,
+    89	        }]
+    90	    }
+    91	
+    92	    fn open(ctx: &ViewContext, spec: MailboxesSpec) -> (Self::State, Self::Payload) {
+    93	        let snapshot = ctx.mailbox_cache().get(&spec.pubkey);
+    94	        let payload = MailboxesPayload::from_snapshot(spec.pubkey, snapshot);
+    95	        (payload.clone(), payload)
+    96	    }
+    97	
+    98	    fn on_event_inserted(ctx: &ViewContext, st: &mut Self::State, ev: &Event)
+    99	        -> Option<Self::Delta>
+   100	    {
+   101	        if ev.kind != 10002 || ev.pubkey != st.pubkey { return None; }
+   102	        let parsed = parse::parse_relay_list(ev.created_at, &ev.tags);
+   103	        if parsed.created_at < st.created_at { return None; }
+   104	        *st = MailboxesPayload::from_parsed(st.pubkey.clone(), parsed);
+   105	        Some(st.clone())
+   106	    }
+   107	
+   108	    // on_event_removed / replaced / projection_changed / on_tick: defaults
+   109	    fn snapshot(_ctx: &ViewContext, st: &Self::State) -> Self::Payload {
+   110	        st.clone()
+   111	    }
+   112	}
+   113	```
+   114	
+   115	The view exists so platform code can render "alice@example uses these relays" using the same path as any other view (`useMailboxes(pubkey)`); it is *not* the compiler's source of truth. The compiler reads `MailboxCache` directly.
+   116	
+   117	## 6.3 Public surface (compiler-facing, not FFI-facing)
+   118	
+   119	```rust
+   120	// crates/nmp-nip65/src/cache.rs
+   121	
+   122	pub trait MailboxCache: Send + Sync {
+   123	    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot>;
+   124	    fn put(&mut self, pubkey: Pubkey, snapshot: MailboxSnapshot)
+   125	        -> CachePutResult;
+   126	    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)>;
+   127	    fn generation(&self) -> u64;           // monotonic; advances on every accepted put
+   128	}
+   129	
+   130	pub enum CachePutResult {
+   131	    Inserted,
+   132	    ReplacedNewer { prior_created_at: UnixSeconds },
+   133	    RejectedStale { current_created_at: UnixSeconds },
+   134	}
+   135	
+   136	#[derive(Clone, Debug)]
+   137	pub struct MailboxSnapshot {
+   138	    pub kind10002_event_id: EventId,
+   139	    pub created_at: UnixSeconds,
+   140	    pub read:  Vec<RelayUrl>,
+   141	    pub write: Vec<RelayUrl>,
+   142	    pub both:  Vec<RelayUrl>,
+   143	    pub seen_from: Vec<RelayUrl>,          // ProvenanceRelayFact seed
+   144	}
+   145	
+   146	pub struct InMemoryMailboxCache { /* HashMap<Pubkey, MailboxSnapshot> */ }
+   147	impl MailboxCache for InMemoryMailboxCache { /* ... */ }
+   148	```
+   149	
+   150	The `MailboxCache` trait is the seam M3 (LMDB persistence) replaces with a backing-store-aware implementation. The compiler does not know which backend it is reading.
+   151	
+   152	```rust
+   153	// crates/nmp-nip65/src/routing.rs
+   154	
+   155	pub fn resolve_author_outbox(
+   156	    cache: &dyn MailboxCache,
+   157	    user_configured: &UserConfiguredRelays,
+   158	    indexer_set: &[RelayUrl],
+   159	    author: &Pubkey,
+   160	) -> AuthorRouting { /* ... */ }
+   161	
+   162	pub fn resolve_author_inbox(
+   163	    cache: &dyn MailboxCache,
+   164	    user_configured: &UserConfiguredRelays,
+   165	    indexer_set: &[RelayUrl],
+   166	    author: &Pubkey,
+   167	) -> AuthorRouting { /* ... */ }
+   168	```
+   169	
+   170	These are the two pure functions [compiler.md](compiler.md) Stage 1 calls per author. They return `AuthorRouting` with the `RoutingSource` tag set per the four-lane discipline ([diagnostics.md](diagnostics.md) §5.2). Test fixtures live in `crates/nmp-nip65/src/tests/routing.rs`; the same fixtures plug into the audit gate (§9).
+   171	
+   172	```rust
+   173	// crates/nmp-nip65/src/parse.rs
+   174	
+   175	pub fn parse_relay_list(created_at: UnixSeconds, tags: &[Vec<String>])
+   176	    -> ParsedRelayList;
+   177	```
+   178	
+   179	This is the function currently inlined as a free fn in `crates/nmp-core/src/kernel/nostr.rs` (referenced by `kernel/ingest.rs:210` and tested in `kernel/tests.rs:150`). M2 moves it here and re-exports from `nmp-core` for compatibility during the migration.
+   180	
+   181	## 6.4 What `nmp-nip65` does *not* contain
+   182	
+   183	By design, to keep the kernel boundary clean (per `docs/design/app-extension-kernel.md` §3):
+   184	
+   185	- **No publish action.** Updating a user's own kind:10002 is `nmp-nip01::UpdateRelayList` (M6); that action depends on `nmp-nip65::parse` to validate the local copy before publishing.
+   186	- **No outbox routing policy.** The decision "publish goes to author write relays + recipient inbox relays" is the publish planner ([outbox.md](outbox.md) §7), not this crate. This crate provides the lookups; the policy lives in `nmp-core::kernel::planner::publish`.
+   187	- **No indexer-probe scheduling.** Probes are kernel-side; this crate is unaware of probe lifecycle.
+   188	- **No FFI types.** `MailboxesPayload` is exposed at FFI via the per-app generated enum (per ADR-0010 codegen); the crate itself ships pure Rust.
+   189	
+   190	## 6.5 Module composition (per `docs/design/kernel-substrate.md` §8)
+   191	
+   192	`nmp-nip65` consumes:
+   193	
+   194	- `nmp-core::substrate::{ViewModule, ViewContext, InterestContext, LogicalInterest, ...}` — kernel trait surface.
+   195	- `nmp-core::kernel::projections` — for reading kind:10002 events out of the event store (the compiler's input).
+   196	
+   197	`nmp-nip65` is consumed by:
+   198	
+   199	- `nmp-core::kernel::planner` — for `MailboxCache`, `resolve_author_outbox/inbox`, `parse_relay_list`.
+   200	- Future `nmp-nip01::UpdateRelayList` (M6).
+   201	- Future `nmp-nip17` (M9) — DM publish path imports `resolve_author_inbox` for recipient lookups.
+   202	- The per-app generated enum — `MailboxesView` becomes one variant of `ViewSpec` in `nmp-app-<name>` per ADR-0010.
+   203	
+   204	## 6.6 Cargo manifest sketch
+   205	
+   206	```toml
+   207	[package]
+   208	name = "nmp-nip65"
+   209	version = "0.0.1"
+   210	edition = "2021"
+   211	
+   212	[dependencies]
+   213	nmp-core   = { path = "../nmp-core" }
+   214	serde      = { workspace = true, features = ["derive"] }
+   215	serde_json = { workspace = true }
+   216	
+   217	[dev-dependencies]
+   218	nmp-testing = { path = "../nmp-testing" }
+   219	```
+   220	
+   221	No `nostr-sdk` dependency: this crate operates on parsed `Event` structs from `nmp-core`'s already-vetted ingest path. Avoiding a duplicate parse dependency keeps the surface auditable.
+
+ succeeded in 0ms:
+     1	# Subscription Compilation §7 — Outbox Routing as a Planner Subsystem
+     2	
+     3	> Parent: `docs/design/subscription-compilation.md`.
+     4	> Read first: `docs/product-spec/subsystems.md` §7.3 (outbox routing algorithm); `docs/aim.md` §4.4 ("outbox / smart relay routing") and §6 doctrines 5 and 10.
+     5	
+     6	This section defines the **publish-side seam** the M2 milestone lands so the M6 write path has a ready surface. There is no publish code in the repo today (`crates/nmp-core/src/kernel/requests.rs` contains no `EVENT` outbound; the relay worker has no publish channel). M2 lands the trait and the override action; M6 writes the first concrete consumer (`SendNoteAction`).
+     7	
+     8	This is the framing the parent index calls out as "design seam now, first concrete consumer in M6." Without this seam, M6 risks reinventing outbox routing inline.
+     9	
+    10	## 7.1 The `PublishPlanner` trait
+    11	
+    12	```rust
+    13	// crates/nmp-core/src/kernel/planner/publish.rs (proposed)
+    14	
+    15	#[async_trait]
+    16	pub trait PublishPlanner: Send + Sync {
+    17	    /// Compute the relay set for publishing a signed event. Pure (no side
+    18	    /// effects); the caller (action ledger) feeds the result into the
+    19	    /// per-relay publish state machine.
+    20	    fn plan_publish(
+    21	        &self,
+    22	        event: &Event,
+    23	        privacy: PublishPrivacy,
+    24	        override_: Option<PublishOverride>,
+    25	    ) -> Result<PublishPlan, PublishPlanError>;
+    26	}
+    27	
+    28	pub struct PublishPlan {
+    29	    pub plan_id: PublishPlanId,                // hashes event coords + chosen relays
+    30	    pub assignments: Vec<PublishAssignment>,
+    31	    pub required_success_count: u8,            // ledger acceptance threshold
+    32	    pub deadline_ms: u64,
+    33	}
+    34	
+    35	pub struct PublishAssignment {
+    36	    pub relay_url: RelayUrl,
+    37	    pub reason: PublishRouteReason,            // which lane motivated this relay
+    38	    pub privacy_role: PrivacyRole,             // for the audit log; not policy
+    39	}
+    40	
+    41	pub enum PublishRouteReason {
+    42	    AuthorWriteRelay  { lane: RelayFactLane },     // Nip65 or UserConfigured
+    43	    RecipientInbox    { recipient: Pubkey, lane: RelayFactLane },
+    44	    Hint              { source: HintSource },
+    45	    Override          { audit: String },           // see §7.4
+    46	}
+    47	
+    48	pub enum PrivacyRole {
+    49	    Author,        // this relay is in the plan as the author's outbox
+    50	    Recipient,     // this relay is in the plan as a recipient's inbox
+    51	    Both,          // single relay is both
+    52	}
+    53	
+    54	#[derive(Clone, Debug)]
+    55	pub enum PublishPrivacy {
+    56	    /// Public events (kind:1, kind:0, kind:3, kind:10002, kind:7, ...).
+    57	    /// Falls back to indexer set if author has no write relays.
+    58	    Public,
+    59	    /// Private/gift-wrapped events (kind:1059 wrapping NIP-44). Fails closed
+    60	    /// if any recipient has no inbox relays.
+    61	    PrivateToRecipients { recipients: Vec<Pubkey> },
+    62	    /// Notifications (kind:1 with `#p` tags, reactions, zaps, replies that
+    63	    /// the author wants to surface to the tagged pubkey). Combines author
+    64	    /// write + each `#p` inbox.
+    65	    PublicWithNotifications { notify: Vec<Pubkey> },
+    66	}
+    67	
+    68	#[derive(Clone, Debug)]
+    69	pub enum PublishPlanError {
+    70	    NoAuthorRelays { author: Pubkey, lane_facts: ByLaneCounts },
+    71	    PrivateRecipientUnroutable { recipient: Pubkey },
+    72	    OverrideRejected { reason: String },
+    73	}
+    74	```
+    75	
+    76	The trait is consumed by the action ledger (per `docs/design/kernel-substrate.md` §4 — the kernel owns "per-relay publish attempts" provenance). When an `ActionModule::reduce` reaches its publish step, it calls the planner, gets a `PublishPlan`, and the kernel fans out to relays with the standard ledger-correlated retry/cancel semantics.
+    77	
+    78	## 7.2 Default implementation: `Nip65PublishPlanner`
+    79	
+    80	```rust
+    81	// crates/nmp-core/src/kernel/planner/publish_default.rs (proposed)
+    82	
+    83	pub struct Nip65PublishPlanner<'a> {
+    84	    pub mailbox_cache:    &'a dyn MailboxCache,
+    85	    pub user_configured:  &'a UserConfiguredRelays,
+    86	    pub indexer_set:      &'a [RelayUrl],
+    87	    pub active_account:   Option<AccountId>,
+    88	}
+    89	
+    90	impl PublishPlanner for Nip65PublishPlanner<'_> {
+    91	    fn plan_publish(&self, event: &Event, privacy: PublishPrivacy,
+    92	                    override_: Option<PublishOverride>) -> Result<PublishPlan, PublishPlanError>
+    93	    { /* algorithm in §7.3 */ }
+    94	}
+    95	```
+    96	
+    97	This is the only `PublishPlanner` impl shipped in v1. The trait exists so a future `Wot​PublishPlanner` (M13 WoT subsystem) or a sandbox planner used in tests can replace it without touching action-ledger code.
+    98	
+    99	## 7.3 Write fan-out algorithm (per `docs/product-spec/subsystems.md` §7.3)
+   100	
+   101	Inputs: a signed `event`, a `PublishPrivacy` mode, an optional `PublishOverride`.
+   102	
+   103	```
+   104	1. If `override_` is Some:
+   105	     return plan from override (see §7.4); set every PublishRouteReason::Override.
+   106	2. Resolve author write relays:
+   107	     author_outbox = resolve_author_outbox(cache, user_configured, indexer, event.pubkey)
+   108	     If author_outbox.relays is empty:
+   109	         return Err(NoAuthorRelays { ... })  // never fall back to indexer for writes
+   110	3. Match on privacy:
+   111	   a. Public:
+   112	        assignments = [each author_outbox.relays → AuthorWriteRelay { lane }]
+   113	        required_success_count = max(1, ceil(N/3))   // configurable
+   114	   b. PrivateToRecipients { recipients }:
+   115	        For each recipient r:
+   116	            inbox = resolve_author_inbox(cache, user_configured, indexer, r)
+   117	            If inbox.source == Indexer or inbox.relays is empty:
+   118	                return Err(PrivateRecipientUnroutable { recipient: r })
+   119	        assignments = union(each recipient's inbox.relays → RecipientInbox { recipient, lane })
+   120	        // intentionally NO author-write inclusion: private events do not go to public outbox
+   121	        required_success_count = recipients.len() as u8  // at least one per recipient
+   122	   c. PublicWithNotifications { notify }:
+   123	        assignments = author_outbox ∪ union(each notify pubkey's inbox)
+   124	        required_success_count = max(1, ceil(author_outbox.len() / 3))
+   125	4. plan_id = blake3(event.id, sorted assignments)
+   126	5. deadline_ms = now + AppConfig.publish_deadline_ms (default 30_000)
+   127	6. Return PublishPlan { plan_id, assignments, required_success_count, deadline_ms }
+   128	```
+   129	
+   130	Notes on the algorithm:
+   131	
+   132	- **Step 2's "no indexer fallback for writes"** is the structural enforcement of the doctrine `docs/product-spec/subsystems.md` §7.3 line 99: "fall back to indexer set for reads only; do not publish to indexers." A failed Step 2 surfaces in the action ledger as `Failed { reason: NoAuthorRelays }`, which the UI renders as a toast per ADR-0007's `SideEffect` lane.
+   133	- **Step 3(b)'s `Indexer` check** is the structural enforcement of bug-extinction #4 (`docs/plan.md` line 306 — "DM to public: no API path can send a DM to a non-inbox relay"). Indexer-sourced inbox means we have no NIP-65-declared inbox; for private events that is fail-closed. The recipient gets nothing rather than getting a public broadcast.
+   134	- **`required_success_count`** is the threshold below which the ledger marks the publish `PartiallyFailed`. The default ⅓-of-fan-out is tunable per `AppConfig.publish_quorum_ratio`.
+   135	
+   136	## 7.4 The `PublishOverride` escape hatch
+   137	
+   138	The override exists for tests, migration tools, and operator power-user flows. Per `docs/aim.md` §6 doctrine 5 ("manual relay selection is the opt-out, not the default") and `docs/product-spec/subsystems.md` §7.3 line 90 ("explicit overrides are named, one-shot, and debug-flagged in logs"), the override must be:
+   139	
+   140	1. **Named** — its own typed `AppAction` variant, not a hidden parameter on `SendNote`.
+   141	2. **One-shot** — does not persist as a default for future publishes.
+   142	3. **Audited** — emits a `Diagnostic::PublishOverrideUsed { reason, action_id }` on the `SideEffect` lane and writes a debug-level log line on every dispatch.
+   143	4. **Refused for privacy-sensitive modes** — `PublishPrivacy::PrivateToRecipients` rejects an override that adds non-inbox relays. The override may *narrow* a private fan-out to a subset of declared inboxes; it may not *widen* to public relays.
+   144	
+   145	### The override action
+   146	
+   147	```rust
+   148	// crates/nmp-core/src/kernel/actions/publish_override.rs (proposed)
+   149	
+   150	#[derive(Clone, Serialize, Deserialize)]
+   151	pub struct PublishWithOverride {
+   152	    pub inner: AppAction,                    // the underlying publish action
+   153	    pub override_relays: Vec<RelayUrl>,
+   154	    pub override_audit: String,              // human-readable justification
+   155	}
+   156	
+   157	pub struct PublishOverride {
+   158	    pub relays: Vec<RelayUrl>,
+   159	    pub audit:  String,
+   160	}
+   161	
+   162	// In the action ledger:
+   163	impl ActionModule for PublishWithOverride {
+   164	    const NAMESPACE: &'static str = "kernel.publish_override";
+   165	    type Action = PublishWithOverride;
+   166	    type Step   = PublishOverrideStep;
+   167	    type Output = PublishResult;
+   168	
+   169	    fn start(cx: &mut ActionContext, a: Self::Action)
+   170	        -> Result<ActionPlan<Self::Step>, ActionRejection>
+   171	    {
+   172	        // Emit the debug warning immediately. This is the audit trail.
+   173	        cx.emit_side_effect(SideEffect::Diagnostic(
+   174	            Diagnostic::PublishOverrideUsed {
+   175	                action_id: cx.id(),
+   176	                reason: a.override_audit.clone(),
+   177	                relays: a.override_relays.clone(),
+   178	            },
+   179	        ));
+   180	        cx.log_warn(format!(
+   181	            "OUTBOX OVERRIDE used by action {} → {} relays: {}",
+   182	            cx.id(), a.override_relays.len(), a.override_audit
+   183	        ));
+   184	        // ... validate that inner action's privacy mode permits override ...
+   185	    }
+   186	
+   187	    fn reduce(...) { /* delegate to inner action, but pass `override_` to PublishPlanner */ }
+   188	}
+   189	```
+   190	
+   191	The override action's existence is what test #2 in the bug-extinction list (`docs/plan.md` line 134) asserts: "no public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning." The `PublishWithOverride` variant is the *only* `AppAction` that carries a relay set; the audit string is required (compile-time non-optional); the warning fires unconditionally on dispatch.
+   192	
+   193	### Diagnostic shape
+   194	
+   195	```rust
+   196	pub enum Diagnostic {
+   197	    PublishOverrideUsed {
+   198	        action_id: ActionId,
+   199	        reason: String,
+   200	        relays: Vec<RelayUrl>,
+   201	    },
+   202	    // ... other variants ...
+   203	}
+   204	```
+   205	
+   206	This is the SideEffect-lane payload per ADR-0007. The platform diagnostic UI renders an entry on every override; the count over a session is a soft metric (Cargo dependents using overrides routinely should re-examine their flow).
+   207	
+   208	## 7.5 Atomicity contract
+   209	
+   210	Per `docs/design/kernel-substrate.md` §4 ("Atomicity"): the action ledger ensures the action's local store insert (for the signed event) happens in the same actor message as the ledger transition. The publish plan's per-relay attempts are *not* atomic with the local insert — relays may NACK over a long window — but the ledger correlates them.
+   211	
+   212	The bug-extinction #7 test (`docs/plan.md` line 234) — "publish OK / store fail and store OK / publish fail both roll back atomically" — runs against the M6 implementation. The seam M2 lands here must make that test possible. Specifically:
+   213	
+   214	- The publish-fanout step in `PublishWithOverride::reduce` is `AwaitCapability { request: CapabilityRequest::Publish { ... }, next_step }` per the `ActionTransition` enum in `docs/design/kernel-substrate.md` §4. The kernel owns the publish attempts and reports per-relay outcomes back into the next `reduce`.
+   215	- The local store insert happens *before* the publish step (optimistic insert), with rollback on `PartiallyFailed` if `required_success_count` is not met. This matches the "atomic with reversibility" reading of doctrine D4 (single writer per fact).
+   216	
+   217	## 7.6 What M2 does not cover (deferred)
+   218	
+   219	- **Action ledger schema** — `docs/design/kernel-substrate.md` §4 is the design; M6 implements.
+   220	- **Retry policy** — exponential backoff parameters land in M6.
+   221	- **Concurrent publish coalescing** — if two actions publish the same event (a republish), the planner can dedupe to one wire EVENT per relay. Defer to M6 / M7 stress test.
+   222	- **NIP-42 auth challenge during publish** — relays may demand AUTH before accepting an EVENT. Wires up in M5.
+   223	
+   224	The publish-planner trait is intentionally finished enough that the M6 implementation does not need to extend it. That is the seam the milestone gates against.
+
+exec
+/bin/zsh -lc "nl -ba docs/design/subscription-compilation/compiler.md | sed -n '1,210p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# Subscription Compilation §3 — The Compilation Pipeline
+     2	
+     3	> Parent: `docs/design/subscription-compilation.md`.
+     4	> Read first: [intro.md](intro.md) for the `LogicalInterest` shape this stage consumes.
+     5	
+     6	The compiler is a pure function plus a small amount of state (the mailbox cache and the active plan registry). It runs whenever a recompilation trigger fires (§4) and produces a `CompiledPlan` that the wire-emitter applies as a diff against the relay sockets.
+     7	
+     8	## 3.0 Pipeline overview
+     9	
+    10	```
+    11	   logical_interests:                 mailbox_cache + relay_config
+    12	   Vec<LogicalInterest>                       │
+    13	            │                                 │
+    14	            ▼                                 ▼
+    15	   ┌───────────────────────────────────────────────────┐
+    16	   │ Stage 1: Resolve authors → mailboxes              │  (§3.1)
+    17	   │   each author → { write, read, both, missing }    │
+    18	   └───────────────────────────────────────────────────┘
+    19	            │
+    20	            ▼
+    21	   ┌───────────────────────────────────────────────────┐
+    22	   │ Stage 2: Indexer fallback for missing mailboxes   │  (§3.2)
+    23	   │   missing → enqueue kind:10002 probe              │
+    24	   │   missing-author reads → indexer set (read only)  │
+    25	   └───────────────────────────────────────────────────┘
+    26	            │
+    27	            ▼
+    28	   ┌───────────────────────────────────────────────────┐
+    29	   │ Stage 3: Per-relay shape merge                    │  (§3.3)
+    30	   │   group interests by target relay URL             │
+    31	   │   merge compatible shapes inside each relay       │
+    32	   │   refuse merges that would change semantics       │
+    33	   └───────────────────────────────────────────────────┘
+    34	            │
+    35	            ▼
+    36	   ┌───────────────────────────────────────────────────┐
+    37	   │ Stage 4: Plan-id binding                          │  (§3.4)
+    38	   │   compute plan_id = hash(interest_set,            │
+    39	   │                          mailbox_snapshot,        │
+    40	   │                          merge_lattice_version)   │
+    41	   │   stable across no-op recompilations              │
+    42	   └───────────────────────────────────────────────────┘
+    43	            │
+    44	            ▼
+    45	   CompiledPlan { plan_id, per_relay: Vec<RelayPlan> }
+    46	```
+    47	
+    48	The wire-emitter (`crates/nmp-core/src/kernel/wire.rs`, to be added) diffs the new plan against the current wire-sub registry: opens new REQs, closes orphaned ones, leaves stable assignments untouched.
+    49	
+    50	## 3.1 Stage 1 — Resolve authors to mailboxes
+    51	
+    52	Inputs: every `LogicalInterest` with non-empty `shape.authors` or non-empty `shape.tags[#p]`; the mailbox cache populated by `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`).
+    53	
+    54	Output: an `AuthorRouting` per author per direction:
+    55	
+    56	```rust
+    57	pub struct AuthorRouting {
+    58	    pub author: Pubkey,
+    59	    pub direction: RoutingDirection,        // Outbox or Inbox
+    60	    pub relays: BTreeSet<RelayUrl>,         // resolved write/read/both
+    61	    pub source: RoutingSource,              // Nip65 | UserConfigured | Indexer | Hint
+    62	    pub freshness_ms: Option<u64>,          // age of the kind:10002 record
+    63	}
+    64	
+    65	pub enum RoutingDirection {
+    66	    Outbox,    // for `authors:` filters — the author's *write* relays
+    67	    Inbox,     // for `#p:` filters    — the tagged author's *read* relays
+    68	}
+    69	```
+    70	
+    71	Direction is decided by the interest's filter shape per `docs/product-spec/subsystems.md` §7.3:
+    72	
+    73	| Interest shape | Direction | Source per author |
+    74	|---|---|---|
+    75	| Non-empty `authors`, no `#p` | Outbox | author's `write_relays ∪ both_relays` |
+    76	| Empty `authors`, non-empty `#p` | Inbox | tagged author's `read_relays ∪ both_relays` |
+    77	| Both populated | Outbox primarily; Inbox interests split (see §3.3) | both |
+    78	| Neither populated | (handled by stage 3 as "use active-account read relays") | — |
+    79	
+    80	`docs/product-spec/subsystems.md` §7.3 specifies one explicit override: DMs (NIP-17 gift-wraps, M9) fail closed if recipient inbox relays are missing. The compiler enforces this by refusing to produce a plan for an interest tagged `privacy = FailClosed` if any tagged-pubkey inbox lookup returns `Indexer` source. §7 details the publish-side enforcement.
+    81	
+    82	## 3.2 Stage 2 — Indexer fallback for unknown mailboxes
+    83	
+    84	The indexer set is a kernel-configured `Vec<RelayUrl>` (default: a small curated list; user-configurable in `AppConfig`). Today's `crates/nmp-core/src/relay.rs:2` is the placeholder for one indexer relay (`purplepag.es`); the v1 indexer set lives in `AppConfig.indexer_relays`.
+    85	
+    86	Two distinct behaviours:
+    87	
+    88	1. **Mailbox probe.** For every author with `mailbox_cache.get(author) == None`, the compiler emits a `IndexerProbe { author }` side effect on the plan. The probe registers as its own short-lived `LogicalInterest { shape: { kinds: [10002], authors: [author], limit: 1 }, lifecycle: OneShot, scope: Global }`. Recompilation triggers (§4 trigger A1) re-route the original interest once the kind:10002 lands.
+    89	2. **Read fallback.** For a `RoutingDirection::Outbox` interest whose author has no known mailboxes, the compiler routes the interest to the indexer set **as read-only fallback**. Per `docs/product-spec/subsystems.md` §7.3 line 99: "fall back to indexer set for reads only; do not publish to indexers." The `RoutingSource::Indexer` tag on the resulting `AuthorRouting` flows through to the diagnostic surface so the four-lane view (§5) can render "author X is being served by indexer Y because we have no mailbox for them."
+    90	
+    91	Bounded: a single author's indexer probe is enqueued at most once per `compiler_probe_window_secs` (default 60s) to prevent thundering-herd probes if a screen of N rows all claim the same unknown pubkey.
+    92	
+    93	## 3.3 Stage 3 — Per-relay shape merge
+    94	
+    95	After Stage 1, every interest has one or more `(relay_url, sub_shape)` assignments. Stage 3 groups by relay URL and merges shapes where merging preserves semantics.
+    96	
+    97	### Merge rules (the lattice)
+    98	
+    99	Two `InterestShape`s `A` and `B` are **mergeable on relay R** iff:
+   100	
+   101	1. `A.kinds == B.kinds` **or** one is empty (wildcard absorbs).
+   102	2. `A.tags.keys() == B.tags.keys()` (same tag dimensions) **and** the union of values per dimension stays under the relay's per-filter limit (default 1000).
+   103	3. `A.since` and `B.since`: merged `since = min(A.since, B.since)` *only if* both are present or both absent. Mixing a bounded interest with an unbounded one is **not** merged (would broaden the bounded one's window).
+   104	4. `A.until` and `B.until`: same rule, mirror of (3) with `max`.
+   105	5. `A.limit` and `B.limit`: mergeable iff both are absent. If either has a `limit`, **do not merge** — broadening would mask the limit's intent.
+   106	6. `A.lifecycle == B.lifecycle`. Tailing and one-shot do not merge (one-shot would never close).
+   107	7. `A.event_ids` and `B.event_ids`: merge by union, capped at the relay's per-filter `ids` limit.
+   108	
+   109	When mergeable, the merged shape is `{ authors: A.authors ∪ B.authors, ... }`. The merged interest tracks both originating `InterestId`s so per-event dispatch back to consumers stays correct.
+   110	
+   111	When not mergeable, the two interests get distinct sub-shapes on the same relay, producing two distinct REQs. That is fine and expected.
+   112	
+   113	Open question 2 in the parent index (`subscription-compilation.md`) covers the `limit`-only corner case formally.
+   114	
+   115	### Per-relay output
+   116	
+   117	```rust
+   118	pub struct RelayPlan {
+   119	    pub relay_url: RelayUrl,
+   120	    pub role_tags: BTreeSet<RoutingSource>,   // why this relay is in the plan
+   121	    pub sub_shapes: Vec<SubShape>,            // each emits one REQ
+   122	}
+   123	
+   124	pub struct SubShape {
+   125	    pub shape: InterestShape,                  // canonical, post-merge
+   126	    pub originating_interests: Vec<InterestId>,
+   127	    pub canonical_filter_hash: String,         // for ADR-0007 WireSubscriptionStatus
+   128	}
+   129	```
+   130	
+   131	The wire-emitter renders each `SubShape` as exactly one `REQ` on `relay_url` with a sub-id of `c{plan_id}-r{relay_idx}-s{shape_idx}`. The sub-id is meaningful only to the kernel; diagnostics use `canonical_filter_hash` for stable identity across re-emission.
+   132	
+   133	## 3.4 Stage 4 — Plan-id binding
+   134	
+   135	`plan_id` is the **stable identity** the platform observes for diagnostic continuity. It answers: "did this recompilation actually change anything observable?"
+   136	
+   137	Definition (this design picks the "interest-set + mailbox-snapshot" formulation; open question 1 in the parent index notes the alternative):
+   138	
+   139	```
+   140	plan_id = blake3(
+   141	    sorted(interest_set.iter().map(|i| (i.id, i.shape, i.scope, i.lifecycle))),
+   142	    sorted(mailbox_snapshot.iter().map(|(pk, ml)| (pk, ml.created_at,
+   143	                                                    sorted(ml.write),
+   144	                                                    sorted(ml.read)))),
+   145	    INDEXER_SET_VERSION,
+   146	    USER_CONFIGURED_RELAYS_VERSION,
+   147	    MERGE_LATTICE_VERSION,
+   148	)
+   149	```
+   150	
+   151	Properties:
+   152	
+   153	- **Recompilation with no change ⇒ same plan-id.** If `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:218-221`) deduplicates and decides not to replace a stale mailbox, no plan-id churn.
+   154	- **Adding an interest changes plan-id even if no new wire REQ results.** Two interests can merge into the same SubShape; the plan-id changes because the *interest set* changed. The platform diagnostic correctly reports "logical-interest count went up; wire-sub count did not."
+   155	- **A new kind:10002 for a covered author changes plan-id.** Even if the author's new write relays overlap entirely with the old set (e.g. relay added then removed), the snapshot's `created_at` advanced, so the hash changes. ADR-0007 diagnostics will reflect this; the wire-emitter's diff will be a no-op if the actual relay assignment is unchanged.
+   156	- **Indexer set change changes plan-id.** Operator config edits surface immediately.
+   157	
+   158	The `plan_id` is stored on `CompiledPlan` and rendered into `LogicalInterestStatus` (extending the record at `crates/nmp-core/src/kernel/mod.rs:147-154` with `plan_id: String, plan_generation: u64`). Tests in §9 assert plan-id stability across no-op recompilations.
+   159	
+   160	## 3.5 Migration of existing functions
+   161	
+   162	This is the binding contract: each function in `crates/nmp-core/src/kernel/requests.rs` and `crates/nmp-core/src/kernel/ingest.rs` either disappears, becomes thin glue over the compiler, or graduates into a typed module. The compiler does not coexist with the old planner; M2 replaces it.
+   163	
+   164	| Current function (file:line) | M2 replacement |
+   165	|---|---|
+   166	| `startup_requests` (requests.rs:50-106) | Becomes `register_startup_interests()`: pushes 4 `LogicalInterest`s into the registry (seed timeline; account kind:0; account kind:10002; seed contacts kind:3). The compiler runs once and produces wire REQs. The TEST_PUBKEY-specific bootstrap (line 71-82) becomes an `InterestScope::Global` interest with hardcoded indexer routing flagged `RoutingSource::UserConfigured` (it is operator-debug seed data). |
+   167	| `open_author` (requests.rs:118-140) | Registers three `LogicalInterest`s scoped `ActiveAccount` (kind:10002, kind:0, kinds:1+6 for author); calls `compiler.recompile(Trigger::ViewOpen)`. Refcount stays — but it lives on `InterestId` now, not on `ViewInterest { key, refcount }`. The `can_send` gate disappears: the compiler always produces a plan; the wire-emitter is the only thing that may queue deferrals. |
+   168	| `open_thread` (requests.rs:142-168) | Registers a `Thread { event_id }` view-module spec; the view module returns interests with `event_ids` and `#e`-tag shapes. Hydration cascade in `prepare_thread_requests` (requests.rs:441-466) becomes part of the view module's `reduce` returning new interests when new event ids surface. |
+   169	| `open_firehose_tag` (requests.rs:170-200) | Registers one `LogicalInterest { shape: { kinds: [1], tags: { #t: [tag] } }, scope: ActiveAccount, lifecycle: Tailing }`. Routes to active-account read relays per §3.1 table. |
+   170	| `claim_profile` / `release_profile` (requests.rs:202-263) | Registers/unregisters one `LogicalInterest { shape: { authors: [pk], kinds: [0], limit: 1 }, lifecycle: OneShot }` per claim. Refcount of distinct consumers becomes the `InterestId` claim set inside the registry. **Dedup of (pk, kinds=[0]) across N timeline rows yields one merged SubShape and one wire REQ** — this is what bug-extinction "1000 avatars do not produce 1000 REQs" verifies (already true today via dedup; the compiler preserves it). |
+   171	| `close_author` / `close_thread` (requests.rs:265-311) | Drop interests by `InterestId`; recompile with `Trigger::ViewClose`. Wire-emitter closes orphaned REQs. The "warm-close" grace from the view-warmth doctrine (`docs/design/kernel-substrate.md` §3 "lifecycle") is the compiler's, not the view's — interests stay registered for the warmth window after their last claim. |
+   172	| `close_subscriptions_with_prefixes` (requests.rs:313-331) | **Deleted.** The wire-emitter closes by `WireSubId`, which is the compiler's diff output. String-prefix matching of sub-ids is a 2026-05-period scaffold that the compiler removes. |
+   173	| `pending_view_requests` (requests.rs:333-355) | Becomes `compiler.flush_deferred_for_relay(role, url)`: called when a relay reconnects (§4 trigger A3). The compiler resubmits its current plan against that relay's slot. |
+   174	| `firehose_requests` (requests.rs:357-372) | Replaced as described above for `open_firehose_tag`. The `diag-firehose-N` sub-id scheme goes away — `canonical_filter_hash` plus `plan_id` give stable identity. |
+   175	| `pending_profile_claim_requests` (requests.rs:374-388) | Disappears. Claims are interests; the compiler is the only thing that decides "this interest needs a REQ." |
+   176	| `profile_claim_request` (requests.rs:390-402) | Disappears. The compiler routes claimed-profile interests through Stage 1; indexer fallback (Stage 2) handles the no-mailbox case. |
+   177	| `author_requests` (requests.rs:404-439) | Disappears (replaced by `open_author`'s interest registration). |
+   178	| `prepare_thread_requests` / `enqueue_thread_*` / `maybe_open_thread_hydration` (requests.rs:441-528) | Move to a `ThreadViewModule` in `nmp-nip10`. The hydration cascade is `view_module.reduce(...)` returning additional interests as new event ids surface in store. |
+   179	| `req` (requests.rs:530-556) | **Deleted.** Replaced by the wire-emitter's `emit_req(relay_url, sub_id, filter)`. No call site outside the wire-emitter is permitted to construct a REQ. |
+   180	| `defer_outbound` (requests.rs:558-568) | Moves to the wire-emitter; deferral is per-relay, keyed by URL, not by role. |
+   181	| `ingest_relay_list` (ingest.rs:209-233) | Stays, but emits a `Trigger::Nip65Arrived { pubkey }` event (§4 trigger A1) on a material update. Becomes the producer side of the recompilation cycle. |
+   182	| `ingest_profile` / `ingest_contacts` / `ingest_timeline_event` (ingest.rs:166-279) | Unchanged in storage shape. Their relevance to compilation is that they feed the view modules' projections (per `docs/design/reactivity/view-deltas-and-projections.md`). |
+   183	| `should_store_event` (ingest.rs:268-279) | Unchanged. Per-sub-id string filtering goes away when sub-ids become `c{plan}-r{relay}-s{shape}`, but the predicate switches to "is this event id covered by an active interest?" — a `compiler.is_covered(event)` call. |
+   184	| `maybe_open_timeline` (ingest.rs:329-365) | The "seed-contacts arrive → open union timeline" logic becomes a small `TimelineSeedModule`: it watches `seed_contacts` projection and registers a `Timeline { authors: union }` interest once the seed cohort is complete. |
+   185	
+   186	What this migration does **not** do (deferred per parent index open questions 3, 6, 7):
+   187	
+   188	- It does not move the action ledger into M2 — `SendNote` lands in M6.
+   189	- It does not implement LMDB persistence for the mailbox cache — M3.
+   190	- It does not implement NIP-77 watermarks — M4.
+   191	- It does not add a per-author indexer-fallback ledger row — open question 3.
+   192	
+   193	The compiler is **in-memory v1** by design. The mailbox cache is the existing `HashMap<String, AuthorRelayList>` (`crates/nmp-core/src/kernel/mod.rs:313`); it just gets a new consumer.
+
+exec
+/bin/zsh -lc "nl -ba docs/design/subscription-compilation/diagnostics.md | sed -n '1,205p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# Subscription Compilation §5 + §8 — Four-Lane Diagnostics
+     2	
+     3	> Parent: `docs/design/subscription-compilation.md`.
+     4	> Read first: ADR-0007 (`docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md`) — every record here extends ADR-0007 types; it does not replace them.
+     5	
+     6	The compiler's routing decisions are the most subtle correctness surface in the M2 milestone. They are also the easiest to silently get wrong (`docs/design/ndk-applesauce-lessons.md` §3, "automatic behaviour also needs strong tests"). Diagnostics make the four sources of relay knowledge legible — separately, never collapsed.
+     7	
+     8	## 5.0 The four lanes
+     9	
+    10	Per `docs/design/ndk-applesauce-lessons.md` §4 (lines 39–46) and `docs/aim.md` §6 doctrine 10 ("provenance preserved"), the four relay-fact lanes are:
+    11	
+    12	1. **NIP-65** — a pubkey's declared relay preferences (kind:10002).
+    13	2. **Hint** — relay URLs embedded in events or NIP-19 pointers (`e`/`a` tag third slot, `nevent`'s relay vector, etc.).
+    14	3. **Provenance** — relays we have actually observed an event arriving from.
+    15	4. **User-configured** — local-policy relays added by the user/operator, plus the kernel-configured indexer fallback set.
+    16	
+    17	Each lane is its own record stream. They never merge into a single "relays" field — that collapse is exactly the bug `docs/design/ndk-applesauce-lessons.md` §4 line 46 forbids. They may be displayed side-by-side in a diagnostic view; the actor stores them apart.
+    18	
+    19	This is structurally enforced: there is no `Vec<RelayUrl>` field on any compiler output type. Every relay-bearing field carries a `lane: RelayFactLane` discriminator.
+    20	
+    21	```rust
+    22	// crates/nmp-core/src/kernel/diagnostics/lanes.rs (proposed)
+    23	
+    24	#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
+    25	pub enum RelayFactLane {
+    26	    Nip65,
+    27	    Hint,
+    28	    Provenance,
+    29	    UserConfigured,
+    30	}
+    31	```
+    32	
+    33	## 5.1 Per-lane record schemas
+    34	
+    35	Each lane has one record type. All four are exposed to the platform via the existing ADR-0007 `ViewBatch` lane (low-cadence, coalesced to 1–4 Hz per ADR-0007 "How status crosses the bridge"). They feed into the diagnostics screen, not into normal product UI.
+    36	
+    37	### Lane 1 — `Nip65RelayFact`
+    38	
+    39	```rust
+    40	pub struct Nip65RelayFact {
+    41	    pub pubkey: Pubkey,
+    42	    pub relay_url: RelayUrl,
+    43	    pub roles: Nip65Roles,                    // read | write | both
+    44	    pub kind10002_event_id: EventId,           // provenance of the kind:10002
+    45	    pub kind10002_created_at: UnixSeconds,
+    46	    pub kind10002_seen_from: Vec<RelayUrl>,    // which relays delivered it
+    47	    pub freshness: FreshnessHint,              // recent / hours_old / days_old / never_verified
+    48	}
+    49	
+    50	pub struct Nip65Roles {
+    51	    pub read: bool,
+    52	    pub write: bool,
+    53	}
+    54	```
+    55	
+    56	Emitted whenever `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`) replaces a mailbox entry. One record per `(pubkey, relay_url)` pair; an author with 4 declared relays produces 4 records on each update.
+    57	
+    58	### Lane 2 — `HintRelayFact`
+    59	
+    60	```rust
+    61	pub struct HintRelayFact {
+    62	    pub relay_url: RelayUrl,
+    63	    pub source: HintSource,
+    64	    pub freshness_ms: u64,                     // monotonic from observation
+    65	    pub recently_succeeded: bool,              // last attempt produced ≥1 EVENT
+    66	}
+    67	
+    68	pub enum HintSource {
+    69	    EventTag    { event_id: EventId, tag: TagKey, position: u8 },
+    70	    Nip19       { pointer: String /* nevent1.../naddr1... */ },
+    71	    UserConfig  { config_path: String },        // for hints injected via config
+    72	}
+    73	```
+    74	
+    75	Emitted by the pointer loader (post-M2; for M2 the field exists but is rarely populated — only `e`/`a`-tag third-slot hints from thread-view hydration fill it). Per-event hints are de-duplicated; an event whose `e` tag contains a hint URL produces one `HintRelayFact` per (relay_url, source) pair.
+    76	
+    77	### Lane 3 — `ProvenanceRelayFact`
+    78	
+    79	```rust
+    80	pub struct ProvenanceRelayFact {
+    81	    pub relay_url: RelayUrl,
+    82	    pub event_id: EventId,
+    83	    pub seen_at_ms: u64,
+    84	    pub wire_sub_id: String,                   // which REQ delivered it
+    85	    pub kind: u32,
+    86	    pub author: Pubkey,
+    87	}
+    88	```
+    89	
+    90	Emitted by `handle_event` (`crates/nmp-core/src/kernel/ingest.rs:134-164`) for every EVENT arrival. This is the highest-cardinality lane and the only one where coalescing matters at the ADR-0007 boundary: the platform diagnostic view consumes a summarised projection (`ProvenanceSummary` per author or per event), not the raw fact stream.
+    91	
+    92	### Lane 4 — `UserConfiguredRelayFact`
+    93	
+    94	```rust
+    95	pub struct UserConfiguredRelayFact {
+    96	    pub relay_url: RelayUrl,
+    97	    pub category: UserConfiguredCategory,
+    98	    pub generation: u64,                       // config version; matches Trigger::*Changed
+    99	    pub added_at_ms: u64,
+   100	}
+   101	
+   102	pub enum UserConfiguredCategory {
+   103	    AccountRead,                                // user's own read relays (overrides NIP-65 read)
+   104	    AccountWrite,                               // user's own write relays
+   105	    Indexer,                                    // kernel indexer set member
+   106	    Debug,                                      // operator-injected for testing
+   107	}
+   108	```
+   109	
+   110	Emitted on `Trigger::UserConfiguredRelaysChanged` / `Trigger::IndexerSetChanged`. Low-cardinality, low-cadence.
+   111	
+   112	## 5.2 What the compiler may *combine*; what stays distinct
+   113	
+   114	The compiler may *use* facts from multiple lanes to compute a routing decision; it may **not** present them as one. Concretely:
+   115	
+   116	- **Routing decision** (`AuthorRouting.source` from [compiler.md](compiler.md) §3.1): records *which lane* the relay set was derived from. Single-valued; one of `Nip65 | Hint | Indexer | UserConfigured`. The author may have facts in three lanes; the compiler picks one, says so, and the other lane records remain visible.
+   117	- **`RelayPlan.role_tags`** is a `BTreeSet<RoutingSource>` because a single relay may be in the plan for multiple reasons (e.g. NIP-65 for author A + user-configured fallback for everyone). The set discriminates, it does not collapse.
+   118	- **The platform diagnostic view** receives all four lanes as separate `ViewBatch` records. The UI may render them in one screen with four side-by-side columns, but the data path is four lanes.
+   119	
+   120	A test (§9) asserts that no compiler output type has a field of type `Vec<RelayUrl>` without an adjacent `RelayFactLane`. That is the structural enforcement.
+   121	
+   122	## 5.3 Lane interactions
+   123	
+   124	The lanes inform each other through these well-defined hooks:
+   125	
+   126	- `Provenance → NIP-65 hint refresh.` If we observe many `ProvenanceRelayFact { relay_url: R, author: A }` records but no `Nip65RelayFact { pubkey: A, relay_url: R }`, the operator diagnostic can suggest "we are receiving A's events from R but A has not declared R; their kind:10002 may be stale." This is a future operator-UI affordance, not a behaviour.
+   127	- `Hint → planner suggestion.` `HintRelayFact` with `recently_succeeded: true` may surface in the diagnostic view as "you might want to add this to your indexer set." Again, not automatic.
+   128	- `User-configured` is the **only** lane the compiler treats as authoritative-by-policy (the user said so). Open question 5 in the parent index resolves the augment-vs-override precedence between NIP-65 and user-configured for the active account.
+   129	
+   130	The lesson the four-lane discipline preserves: routing is **derivable but contested** evidence. Collapsing the lanes loses information; preserving them lets the diagnostic answer "why did we route this REQ to that relay?" months after the decision.
+   131	
+   132	---
+   133	
+   134	# §8 — Reverse-relay-coverage diagnostic view
+   135	
+   136	> The inverse question. For any relay we are talking to, *whose* timeline does it serve?
+   137	
+   138	This is one specific `ViewModule` that consumes the four-lane fact streams plus the compiler's `RelayPlan`s and produces a per-relay summary.
+   139	
+   140	## 8.1 Spec, payload, dependencies
+   141	
+   142	```rust
+   143	pub struct RelayCoverageSpec {
+   144	    pub relay_url: RelayUrl,
+   145	}
+   146	
+   147	pub struct RelayCoveragePayload {
+   148	    pub relay_url: RelayUrl,
+   149	    pub serving_authors: u32,
+   150	    pub author_examples: Vec<Pubkey>,      // first N (configurable, default 16)
+   151	    pub by_lane: ByLaneCounts,
+   152	    pub wire_sub_count: u32,
+   153	    pub last_event_at_ms: Option<u64>,
+   154	    pub provenance_count_last_minute: u32,
+   155	}
+   156	
+   157	pub struct ByLaneCounts {
+   158	    pub nip65: u32,             // authors for whom relay is in their NIP-65 set
+   159	    pub hint: u32,              // authors for whom we routed here via hints
+   160	    pub user_configured: u32,   // authors served via user-config
+   161	    pub indexer_fallback: u32,  // authors with no mailbox, served via indexer
+   162	}
+   163	
+   164	// `ViewModule::dependencies` returns:
+   165	//   - Mailbox cache updates touching any author in our timeline
+   166	//   - RelayPlan updates touching `relay_url`
+   167	//   - Provenance facts on `relay_url` (rate-limited; only the count, not individual events)
+   168	```
+   169	
+   170	## 8.2 Implementation outline
+   171	
+   172	The view's `reduce` consumes three input streams:
+   173	
+   174	1. `Nip65RelayFact` records — increments/decrements `by_lane.nip65` per (relay_url, pubkey) membership.
+   175	2. `CompiledPlan` re-emissions — every plan recompile produces a `(plan_id, relay_url) → authors` projection that this view subscribes to. The compiler exposes this projection as `RelayAuthorCoverage` in the kernel's projection cache (per `docs/design/reactivity/view-deltas-and-projections.md`).
+   176	3. `ProvenanceRelayFact` records — feeds the rolling 60-second counter for `provenance_count_last_minute`.
+   177	
+   178	This is the M2 exit-gate diagnostic listed in `docs/plan.md` line 130 ("Reverse-relay-coverage view for diagnostics: 'this relay is serving N authors of our timeline.'").
+   179	
+   180	## 8.3 Cardinality and emission cadence
+   181	
+   182	One `RelayCoverageSpec`/relay → ≤ N records, where N is the number of relays currently in the planner's union of `RelayPlan`s. For typical Nostr usage that is in the low tens; rendering all of them on one diagnostic screen is fine.
+   183	
+   184	Emission cadence follows ADR-0007's diagnostic-view rule: material-transition immediately, otherwise 1–4 Hz. The provenance counter ticks every second; the `by_lane` counts only emit on `CompiledPlan` recompiles or new mailbox arrivals.
+   185	
+   186	## 8.4 Why it lives in diagnostics, not in product UI
+   187	
+   188	Per `docs/aim.md` §4.4 ("the developer does not pick relays per operation; the framework does") and ADR-0007's domain-of-diagnostics separation, end-user product UIs do not show "relay X is serving 12 authors." That is operator/debug surface. Normal apps consume the `LogicalInterestStatus` summaries; `RelayCoveragePayload` is for the diagnostics screen (proof iOS app screenshot in `docs/perf/m2/outbox-routing.md` per `docs/plan.md` line 139).
+
+exec
+/bin/zsh -lc "nl -ba docs/design/subscription-compilation/recompilation.md | sed -n '1,180p'" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc "nl -ba docs/design/subscription-compilation/tests.md | sed -n '1,290p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# Subscription Compilation §4 — Recompilation Triggers
+     2	
+     3	> Parent: `docs/design/subscription-compilation.md`.
+     4	> Read first: [compiler.md](compiler.md) §3.4 for plan-id semantics.
+     5	
+     6	The compiler is idempotent and pure given `(interest_set, mailbox_snapshot, indexer_set, user_configured_relays)`. Recompilation is therefore safe to run on every trigger; the wire-emitter's diff turns no-op recompilations into zero wire effect.
+     7	
+     8	This section enumerates **every trigger that may cause recompilation** and the actor message shape each one carries. Triggers fan in from three sources: relay ingest, view registry mutations, and operator/user state changes. All of them route through the same actor mailbox (`docs/design/reactivity/loop-and-reverse-index.md`).
+     9	
+    10	## 4.0 Internal vs external triggers
+    11	
+    12	Two trigger classes exist:
+    13	
+    14	- **Internal** triggers are emitted by the actor itself in response to an `InternalEvent`. They are `Trigger::*` enum variants; the planner consumes them off its own internal queue. They have no public dispatch surface.
+    15	- **External** triggers are `AppAction` variants the platform may dispatch directly. There is exactly one — `AppAction::InvalidateCompile { reason }` — to keep the public surface minimal per `docs/aim.md` §6 doctrine 5.
+    16	
+    17	The full list:
+    18	
+    19	| ID | Source | Trigger | Carries |
+    20	|---|---|---|---|
+    21	| A1 | ingest | `Trigger::Nip65Arrived { pubkey, created_at }` | kind:10002 just landed |
+    22	| A2 | view registry | `Trigger::ViewOpened { interest_ids }` | one or more interests just registered |
+    23	| A3 | view registry | `Trigger::ViewClosed { interest_ids }` | warmth grace expired; interests dropped |
+    24	| A4 | session | `Trigger::ActiveAccountChanged { from, to }` | account switch (M8) |
+    25	| A5 | relay worker | `Trigger::RelayReconnected { url }` | socket re-established after backoff |
+    26	| A6 | operator | `AppAction::InvalidateCompile { reason }` | external force-recompile |
+    27	| A7 | config | `Trigger::UserConfiguredRelaysChanged { generation }` | added/removed relay in local config |
+    28	| A8 | config | `Trigger::IndexerSetChanged { generation }` | indexer relay list edited |
+    29	| A9 | auth | `Trigger::RelayAuthStateChanged { url, state }` | NIP-42 transition (M5+) |
+    30	| A10 | session | `Trigger::SignerAvailable { account, signer_id }` | signer-loss-then-recovery (M6+) |
+    31	
+    32	A1–A3 are M2 scope; A4–A10 are interface seams that M2 establishes so later milestones do not have to re-plumb. The compiler treats unknown triggers as `Trigger::Generic`.
+    33	
+    34	## 4.1 Actor message shapes
+    35	
+    36	```rust
+    37	// crates/nmp-core/src/kernel/planner/trigger.rs (proposed)
+    38	
+    39	#[derive(Clone, Debug)]
+    40	pub enum CompileTrigger {
+    41	    Nip65Arrived {
+    42	        pubkey: Pubkey,
+    43	        created_at: UnixSeconds,    // for replay-window skew detection
+    44	    },
+    45	    ViewOpened {
+    46	        interest_ids: Vec<InterestId>,
+    47	    },
+    48	    ViewClosed {
+    49	        interest_ids: Vec<InterestId>,
+    50	        warmth_expired_at_ms: u64,
+    51	    },
+    52	    ActiveAccountChanged {
+    53	        from: Option<AccountId>,
+    54	        to: Option<AccountId>,
+    55	    },
+    56	    RelayReconnected {
+    57	        url: RelayUrl,
+    58	        prior_state: RelayConnectionState,  // for diagnostics
+    59	    },
+    60	    InvalidateCompile {
+    61	        reason: InvalidateReason,
+    62	    },
+    63	    UserConfiguredRelaysChanged {
+    64	        generation: u64,                    // monotonic config rev
+    65	    },
+    66	    IndexerSetChanged {
+    67	        generation: u64,
+    68	    },
+    69	    RelayAuthStateChanged {
+    70	        url: RelayUrl,
+    71	        state: RelayAuthState,              // re-exported from ADR-0007
+    72	    },
+    73	    SignerAvailable {
+    74	        account: AccountId,
+    75	        signer_id: SignerId,
+    76	    },
+    77	}
+    78	
+    79	#[derive(Clone, Debug)]
+    80	pub enum InvalidateReason {
+    81	    DiagnosticsManualRefresh,               // operator UI button
+    82	    TestForceRecompile,                     // nmp-testing harness
+    83	    External(String),                       // catch-all with diagnostic string
+    84	}
+    85	```
+    86	
+    87	`InvalidateReason::TestForceRecompile` is the seam the wire-frame audit gate (§9) drives so the test does not have to fake a kind:10002 arrival to exercise the recompile path.
+    88	
+    89	## 4.2 Trigger semantics
+    90	
+    91	### A1 — Nip65Arrived
+    92	
+    93	Emitted from `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`) when **and only when** the parser decides to replace the prior mailbox entry (the `should_replace` branch at line 218–222). Stale arrivals do not trigger recompilation.
+    94	
+    95	Compiler effect: re-runs Stages 1–4 for every interest that touches `pubkey` either as a member of `shape.authors` or as a member of `shape.tags[#p]`. Other interests stay assigned to their current plan-id slot; the merged plan-id changes (because the mailbox snapshot's `created_at` for `pubkey` advanced) but its `per_relay` content may be identical.
+    96	
+    97	Outbox routing implication: if `pubkey` was previously routed to the indexer fallback set (Stage 2 read-fallback), the compiler now reassigns to the author's declared write relays. The wire-emitter closes the indexer REQ for that author's slice and opens a new REQ on the declared relays. ADR-0007 diagnostics reflect the route source flipping from `Indexer` to `Nip65`.
+    98	
+    99	### A2 — ViewOpened
+   100	
+   101	Emitted by the view registry when a `ViewModule::open` returns a non-empty `Vec<LogicalInterest>` (per [intro.md](intro.md) §2.2). May fire in batches when a screen opens many rows at once.
+   102	
+   103	Batching contract: the actor's planner inbox coalesces consecutive `ViewOpened` triggers within one actor tick into a single recompile pass. This is the existing reactivity batching (`docs/design/reactivity/scheduling-and-data-model.md`) extended to the planner; the M2 implementation respects the same `≤60Hz/view` budget from ADR-0002 by capping recompiles at one per tick regardless of trigger fan-in.
+   104	
+   105	### A3 — ViewClosed
+   106	
+   107	Emitted by the view registry after the warmth grace expires for an interest with refcount = 0. The warmth window is configurable (`AppConfig.view_warmth_ms`, default 30,000 — matching the doctrine in `docs/product-spec/subsystems.md` §7.6 "View warmth"). Closing an interest mid-warmth (e.g. account switch invalidates the prior account's interests) is a separate `ActiveAccountChanged` trigger, not this one.
+   108	
+   109	### A4 — ActiveAccountChanged
+   110	
+   111	M2 establishes the trigger; M8 wires the multi-account state machine that actually emits it. For M2, the trigger fires once at startup with `from: None, to: Some(active)` so the test surface can exercise account-scope binding without waiting for M8.
+   112	
+   113	Compiler effect: every `InterestScope::ActiveAccount` interest is re-resolved as if newly opened. `InterestScope::Account(specific)` interests are untouched. `InterestScope::Global` interests are untouched.
+   114	
+   115	### A5 — RelayReconnected
+   116	
+   117	Emitted by the relay worker (`crates/nmp-core/src/relay_worker.rs`) after a successful re-handshake. Compiler effect: the wire-emitter re-issues the relay's `SubShape` set as REQs to restore tail subscriptions; the compiler does *not* re-merge or re-resolve. This is a pure "replay current plan to one relay" operation, not a real recompilation, but it routes through the same trigger queue so the diagnostic stream sees it.
+   118	
+   119	Per `docs/product-spec/subsystems.md` §7.2 "Reconnect": "the planner restores live REQs and schedules a coverage-aware gap fill. View payloads do not reset." The gap-fill schedule is M4 (NIP-77); for M2 the gap is implicit (live tail resumes without backfill).
+   120	
+   121	### A6 — InvalidateCompile
+   122	
+   123	The single external `AppAction` variant. Useful for:
+   124	
+   125	- Operator diagnostics screens — "Force re-route now."
+   126	- Test harnesses — see §9.
+   127	- Future debugging tools that change runtime config.
+   128	
+   129	Compiler effect: full recompile from scratch, ignoring incremental caches. Plan-id will change iff any input changed since last compile.
+   130	
+   131	### A7 / A8 — User/indexer config changes
+   132	
+   133	Both bump a `generation: u64` so the plan-id picks up the change (per [compiler.md](compiler.md) §3.4). M2 binds the generation counters but does not yet implement a settings UI to mutate them; v1 ships the seams.
+   134	
+   135	### A9 — RelayAuthStateChanged
+   136	
+   137	M5 wires this fully. M2 only models the trigger so the compiler's data-flow shape does not need to change in M5. Compiler effect: marks the relay as "auth-paused" in its `RelayPlan` so the wire-emitter knows to hold REQs until `RelayAuthState::Authenticated`. Open question 6 in the parent index covers where the gate physically lives.
+   138	
+   139	### A10 — SignerAvailable
+   140	
+   141	M6+ trigger. Some interests (private DMs in M9, NIP-42 challenge response in M5) only become routable once a signer is loaded for their account. M2 records the trigger shape; behaviour is no-op pre-M6.
+   142	
+   143	## 4.3 Trigger ordering and idempotence
+   144	
+   145	The actor's planner inbox is a FIFO queue. Order matters only at the granularity of a tick: within a tick, all queued triggers are folded into the compile inputs and one compile runs. Across ticks, recompiles happen in order received.
+   146	
+   147	Idempotence: running the compiler twice in a row with the same inputs yields identical outputs (same `plan_id`, same `per_relay`). The wire-emitter's diff of two identical plans is empty. This is the contract the audit gate in §9 leans on.
+   148	
+   149	## 4.4 What does *not* trigger recompilation
+   150	
+   151	Explicit non-triggers (so future code does not accidentally over-couple):
+   152	
+   153	- **An EVENT arrival on an existing REQ.** The compiler does not care; the view-modules' projections do.
+   154	- **An EOSE on a one-shot interest.** The interest closes via lifecycle; that flows through `ViewClosed`-equivalent path (the registry drops the interest, fires `ViewClosed`).
+   155	- **A profile-claim refcount delta that does not cross 0↔1.** Going from refcount 5 → 4 is invisible to the compiler.
+   156	- **A relay's RTT or bytes-rx counter ticking.** Diagnostics-only.
+   157	- **A new event id surfacing inside a `ThreadView`'s reduce.** The view module re-invokes `interests()` and returns the augmented set; that emits `ViewOpened` for the *new* `InterestId`s, not a full thread-view recompile. The compiler sees only the additive delta.
+   158	
+   159	These non-triggers keep the recompile cadence aligned with material routing changes, not with event throughput. That is what protects against the "subscription churn under firehose load" failure mode the NDK/Applesauce lessons explicitly warn against (`docs/design/ndk-applesauce-lessons.md` §7 "should recompile" paragraph, lines 92–94).
+
+ succeeded in 0ms:
+     1	# Subscription Compilation §9 — Wire-Frame Audit Gate
+     2	
+     3	> Parent: `docs/design/subscription-compilation.md`.
+     4	> Read first: `docs/plan.md` lines 134–137 (M2 exit gates); `docs/design/firehose-bench.md` (the modeled bench harness this test does *not* duplicate).
+     5	
+     6	The M2 exit gate is a single integration test that asserts on the *shape and identity* of the compiler's wire output, not on perf budgets. It is the structural-correctness counterpart to firehose-bench's perf-correctness suite.
+     7	
+     8	## 9.1 Test file location
+     9	
+    10	```
+    11	crates/nmp-testing/tests/m2_subscription_compilation_audit.rs
+    12	```
+    13	
+    14	The `crates/nmp-testing/tests/` directory does not exist yet; M2 creates it. This is intentional — it establishes the convention that planner-correctness gates live as Cargo integration tests under `nmp-testing/tests/`, distinct from the modeled benches under `nmp-testing/bin/`.
+    15	
+    16	`Cargo.toml` for `nmp-testing` needs the standard `[[test]]` block:
+    17	
+    18	```toml
+    19	[[test]]
+    20	name = "m2_subscription_compilation_audit"
+    21	path = "tests/m2_subscription_compilation_audit.rs"
+    22	```
+    23	
+    24	`cargo test -p nmp-testing --test m2_subscription_compilation_audit` is the M2 exit-gate invocation. CI adds this to the `cargo test --workspace` pre-merge gate per `docs/plan.md` §6.
+    25	
+    26	## 9.2 What the test asserts
+    27	
+    28	Four assertions corresponding to the four M2 exit-gate bullets in `docs/plan.md` lines 134–137:
+    29	
+    30	### Assertion 1 — Bug-extinction #3 surface check
+    31	
+    32	> "No public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning."
+    33	
+    34	> **Codegen dependency.** The test below introspects an `AppActionMeta` reflection
+    35	> helper that lives in the per-app generated crate (ADR-0010). It does not exist
+    36	> yet; ADR-0010's codegen must emit it before this assertion can compile. M2's
+    37	> implementation PR is responsible for adding that emission. Until then the test
+    38	> body below is the design contract; the implementing PR can choose either to
+    39	> generate `AppActionMeta` or to back-stop with a `proc-macro`-driven enum walker.
+    40	
+    41	```rust
+    42	#[test]
+    43	fn no_public_publish_action_carries_relay_list() {
+    44	    // Compile-time-ish check: introspect the AppAction enum's generated variants
+    45	    // (per ADR-0010, the per-app generated enum is a closed enum we can match
+    46	    // exhaustively in tests). For every variant whose namespace starts with
+    47	    // "kernel.publish" or "nip01.send" or "nip17.send", assert that no field
+    48	    // is of type Vec<RelayUrl> EXCEPT the one variant `PublishWithOverride`.
+    49	    let variants = AppActionMeta::all_variants();
+    50	    for v in variants {
+    51	        if v.is_publish_action() {
+    52	            let has_relay_field = v.fields().any(|f| f.ty == "Vec<RelayUrl>");
+    53	            if v.namespace == "kernel.publish_override" {
+    54	                assert!(has_relay_field, "override must carry override_relays");
+    55	            } else {
+    56	                assert!(!has_relay_field,
+    57	                    "{} must not expose a relays parameter", v.namespace);
+    58	            }
+    59	        }
+    60	    }
+    61	}
+    62	```
+    63	
+    64	This is a "shape of the API" assertion, not a behaviour assertion. If a future PR adds a relay field to `SendNote`, the test fails.
+    65	
+    66	### Assertion 2 — Per-author wire fan-out for a 1000-author timeline
+    67	
+    68	> "For a timeline of 1000 authors, the compiled plan opens REQs only against the union of those authors' write relays (de-duplicated). Test asserts on the wire frame count."
+    69	
+    70	```rust
+    71	#[test]
+    72	fn timeline_compiles_to_per_relay_union() {
+    73	    let mut harness = PlannerHarness::new();
+    74	
+    75	    // Seed mailbox cache with 1000 authors, deliberately overlapping relay sets:
+    76	    //   - 600 authors use { wss://relay.damus.io, wss://nos.lol }
+    77	    //   - 300 authors use { wss://nostr.wine, wss://nos.lol }
+    78	    //   - 100 authors use { wss://operator-niche.example }
+    79	    let authors = make_authors_with_overlapping_mailboxes(1000);
+    80	    for (pk, mb) in &authors { harness.mailbox_cache_mut().put(pk.clone(), mb.clone()); }
+    81	
+    82	    // Register one Timeline interest containing all 1000 authors.
+    83	    let interest_id = harness.register_interest(LogicalInterest {
+    84	        scope: InterestScope::ActiveAccount,
+    85	        shape: InterestShape::timeline_for(authors.iter().map(|(pk, _)| pk.clone()).collect()),
+    86	        lifecycle: InterestLifecycle::Tailing,
+    87	        ..LogicalInterest::default()
+    88	    });
+    89	
+    90	    let plan = harness.compile().expect("compile");
+    91	
+    92	    // Assert: exactly one REQ per relay in the union of write relays.
+    93	    let expected_relays: BTreeSet<RelayUrl> = authors.iter()
+    94	        .flat_map(|(_, mb)| mb.write.iter().cloned())
+    95	        .collect();
+    96	    let actual_relays: BTreeSet<RelayUrl> = plan.per_relay.iter()
+    97	        .map(|rp| rp.relay_url.clone()).collect();
+    98	    assert_eq!(actual_relays, expected_relays);
+    99	
+   100	    // Assert: each relay carries exactly one SubShape (merge happened).
+   101	    for rp in &plan.per_relay {
+   102	        assert_eq!(rp.sub_shapes.len(), 1,
+   103	            "relay {} should have one merged sub-shape, has {}",
+   104	            rp.relay_url, rp.sub_shapes.len());
+   105	    }
+   106	
+   107	    // Assert: each relay's authors are exactly the subset that declared it.
+   108	    for rp in &plan.per_relay {
+   109	        let expected_authors: BTreeSet<Pubkey> = authors.iter()
+   110	            .filter(|(_, mb)| mb.write.contains(&rp.relay_url))
+   111	            .map(|(pk, _)| pk.clone())
+   112	            .collect();
+   113	        let actual_authors = &rp.sub_shapes[0].shape.authors;
+   114	        assert_eq!(actual_authors, &expected_authors,
+   115	            "relay {} should serve only its declared authors", rp.relay_url);
+   116	    }
+   117	
+   118	    // Assert: plan-id is deterministic — running compile twice yields the same id.
+   119	    let plan2 = harness.compile().expect("compile #2");
+   120	    assert_eq!(plan.plan_id, plan2.plan_id, "recompile with no input changes ≠ same plan_id");
+   121	}
+   122	```
+   123	
+   124	This is the single most load-bearing test in M2. It assert on:
+   125	
+   126	- **Relay count** = size of union of declared write relays (no extras, no misses).
+   127	- **Per-relay author partition** = exact subset semantics.
+   128	- **Sub-shape merge** = one REQ per relay (merge lattice worked).
+   129	- **Plan-id stability** = re-compile is idempotent.
+   130	
+   131	### Assertion 3 — Late-arriving kind:10002 triggers recompilation
+   132	
+   133	> "An author whose mailbox was unknown gets re-routed once their kind:10002 lands, without the platform observing protocol churn."
+   134	
+   135	```rust
+   136	#[test]
+   137	fn late_nip65_arrival_reroutes_without_churn() {
+   138	    let mut harness = PlannerHarness::new();
+   139	    let target = pubkey("alice");
+   140	
+   141	    // Seed: no mailbox for alice. Register an interest that needs her.
+   142	    harness.register_interest(LogicalInterest::timeline_for(vec![target.clone()]));
+   143	    let plan_v1 = harness.compile().unwrap();
+   144	
+   145	    // The author should be routed via indexer fallback.
+   146	    let alice_relay_v1 = plan_v1.per_relay.iter()
+   147	        .find(|rp| rp.sub_shapes[0].shape.authors.contains(&target))
+   148	        .expect("alice routed somewhere");
+   149	    assert!(alice_relay_v1.role_tags.contains(&RoutingSource::Indexer));
+   150	
+   151	    // Now alice's kind:10002 arrives.
+   152	    harness.ingest_nip65(&target, ["wss://alice-relay.example"]);
+   153	
+   154	    // The ingest emits Trigger::Nip65Arrived → recompile happens internally.
+   155	    harness.flush_pending_triggers();
+   156	    let plan_v2 = harness.last_compiled_plan();
+   157	
+   158	    // Assert: plan-id changed.
+   159	    assert_ne!(plan_v1.plan_id, plan_v2.plan_id);
+   160	
+   161	    // Assert: alice now routes to her declared relay, not the indexer.
+   162	    let alice_relay_v2 = plan_v2.per_relay.iter()
+   163	        .find(|rp| rp.sub_shapes[0].shape.authors.contains(&target))
+   164	        .expect("alice still routed");
+   165	    assert_eq!(alice_relay_v2.relay_url, "wss://alice-relay.example".into());
+   166	    assert!(alice_relay_v2.role_tags.contains(&RoutingSource::Nip65));
+   167	
+   168	    // Assert: the audit stream contains exactly ONE planner re-emission for alice;
+   169	    // the platform sees one transition, not a thrash of N intermediate states.
+   170	    let audit = harness.compile_audit_log();
+   171	    let alice_transitions = audit.iter()
+   172	        .filter(|a| a.affected_authors.contains(&target))
+   173	        .count();
+   174	    assert_eq!(alice_transitions, 1, "expected exactly one recompile for late NIP-65");
+   175	}
+   176	```
+   177	
+   178	This assertion is what `docs/design/ndk-applesauce-lessons.md` §2 line 19 calls out as NDK's important operational truth: "metadata can arrive late… the system should be able to refresh or expand active work without the app tearing down and recreating views."
+   179	
+   180	### Assertion 4 — Four-lane diagnostic distinctness
+   181	
+   182	> "The diagnostics screen shows the four relay-fact lanes (NIP-65 / hint / provenance / user-configured) separately."
+   183	
+   184	```rust
+   185	#[test]
+   186	fn four_lanes_stay_distinct_in_diagnostic_payload() {
+   187	    let mut harness = PlannerHarness::new();
+   188	    let author = pubkey("alice");
+   189	
+   190	    // Set up evidence in all four lanes for the same relay url.
+   191	    let url: RelayUrl = "wss://r.example".into();
+   192	    harness.ingest_nip65(&author, [url.clone()]);              // Nip65 fact
+   193	    harness.observe_hint(&author, url.clone(),                 // Hint fact
+   194	        HintSource::EventTag { event_id: eid("e1"), tag: TagKey::E, position: 2 });
+   195	    harness.observe_provenance(&author, url.clone(), eid("e2")); // Provenance fact
+   196	    harness.user_configured_relay(url.clone(),                 // UserConfigured fact
+   197	        UserConfiguredCategory::Indexer);
+   198	
+   199	    let coverage = harness.open_view::<RelayCoverageView>(
+   200	        RelayCoverageSpec { relay_url: url.clone() });
+   201	
+   202	    assert_eq!(coverage.by_lane.nip65, 1);
+   203	    assert_eq!(coverage.by_lane.hint,  1);
+   204	    assert_eq!(coverage.by_lane.user_configured, 1);
+   205	    // Provenance count is the rolling 60s counter; alice's event landed once.
+   206	    assert_eq!(coverage.provenance_count_last_minute, 1);
+   207	
+   208	    // Structural: no compiler output collapses lanes.
+   209	    let plan = harness.compile().unwrap();
+   210	    let alice_assignment = plan.per_relay.iter()
+   211	        .find(|rp| rp.relay_url == url).unwrap();
+   212	    // role_tags is a SET, not a single value — lanes are preserved.
+   213	    assert!(alice_assignment.role_tags.len() >= 1);
+   214	    assert!(matches!(alice_assignment.role_tags.iter().next().unwrap(),
+   215	        RoutingSource::Nip65 | RoutingSource::UserConfigured));
+   216	}
+   217	```
+   218	
+   219	This assertion encodes the doctrine: a single relay may be in the plan for multiple reasons; the plan must say which reasons, not collapse them.
+   220	
+   221	## 9.3 The `PlannerHarness`
+   222	
+   223	The test harness is itself part of `nmp-testing`:
+   224	
+   225	```rust
+   226	// crates/nmp-testing/src/planner_harness.rs (proposed)
+   227	
+   228	pub struct PlannerHarness {
+   229	    cache: InMemoryMailboxCache,
+   230	    user_config: UserConfiguredRelays,
+   231	    indexer_set: Vec<RelayUrl>,
+   232	    interests: InterestRegistry,
+   233	    compiler: SubscriptionCompiler,
+   234	    audit_log: Vec<CompileAuditEntry>,
+   235	}
+   236	
+   237	impl PlannerHarness {
+   238	    pub fn new() -> Self;
+   239	    pub fn mailbox_cache_mut(&mut self) -> &mut dyn MailboxCache;
+   240	    pub fn register_interest(&mut self, i: LogicalInterest) -> InterestId;
+   241	    pub fn drop_interest(&mut self, id: InterestId);
+   242	    pub fn ingest_nip65(&mut self, author: &Pubkey, relays: impl IntoIterator<Item = RelayUrl>);
+   243	    pub fn observe_hint(&mut self, author: &Pubkey, url: RelayUrl, source: HintSource);
+   244	    pub fn observe_provenance(&mut self, author: &Pubkey, url: RelayUrl, event: EventId);
+   245	    pub fn user_configured_relay(&mut self, url: RelayUrl, cat: UserConfiguredCategory);
+   246	    pub fn force_recompile(&mut self, reason: InvalidateReason);
+   247	    pub fn flush_pending_triggers(&mut self);
+   248	    pub fn compile(&mut self) -> Result<CompiledPlan, CompileError>;
+   249	    pub fn last_compiled_plan(&self) -> &CompiledPlan;
+   250	    pub fn compile_audit_log(&self) -> &[CompileAuditEntry];
+   251	    pub fn open_view<V: ViewModule>(&mut self, spec: V::Spec) -> V::Payload;
+   252	}
+   253	```
+   254	
+   255	The harness is the *minimum* surface required for the four assertions above. It is deliberately small so it does not become its own moving target.
+   256	
+   257	## 9.4 What this test does *not* cover
+   258	
+   259	By design (these belong to other M2 gates or later milestones):
+   260	
+   261	- **Real wire frames against a relay.** This is `firehose-bench live` per `docs/plan.md` line 110; the audit test is offline and synthetic.
+   262	- **Wire-emitter diff correctness across two plans.** That is a separate unit test inside `nmp-core::kernel::wire`, not the milestone-exit gate.
+   263	- **NIP-77 watermarks.** M4.
+   264	- **Per-account auth state.** M5.
+   265	- **The publish path running end-to-end.** M6.
+   266	
+   267	The audit gate's job is exactly the four assertions: API shape, fan-out structure, recompilation on late NIP-65, and four-lane diagnostic distinctness. Those are the four exit-gate bullets the milestone document lists; this test is the verification surface for all four.
+   268	
+   269	## 9.5 CI integration
+   270	
+   271	The test runs in the default `cargo test --workspace` job and takes < 1 second on standard hardware (no networking, no LMDB, in-memory cache only). It is the canonical regression test for "did someone re-introduce the hardcoded two-role planner?" and as such must never be skipped or `#[ignore]`d.
+   272	
+   273	If the M3 (LMDB) milestone graduates the mailbox cache to a real backend, this test continues to exercise the trait surface via the `InMemoryMailboxCache` impl — no changes required. That is the seam `nmp-nip65::cache::MailboxCache` exists for.
+
+exec
+/bin/zsh -lc "nl -ba docs/design/subscription-compilation/intro.md | sed -n '1,150p'" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc "nl -ba docs/design/subscription-compilation.md | sed -n '1,70p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# Design: Subscription Compilation + Outbox Routing (M2)
+     2	
+     3	> **Status:** Draft (M2 design + impl-prep).
+     4	> **Date:** 2026-05-18.
+     5	> **Companion docs:** `docs/plan.md` §M2; `docs/aim.md` §4.4–§4.5; `docs/design/ndk-applesauce-lessons.md` §7; ADR-0007 (diagnostic lanes); `docs/design/kernel-substrate.md` §3 (`ViewModule`) and §4 (`ActionModule`); `docs/product-spec/subsystems.md` §7.2–§7.3.
+     6	> **Scope:** Replace the "hardcoded two-role relay set" planner in `crates/nmp-core/src/kernel/{requests,ingest,mod}.rs` with a **subscription compilation stage** that turns logical interests into per-relay plans driven by NIP-65 mailboxes, and graduates outbox routing to a first-class planner subsystem. v1 is in-memory; M3 plugs it into LMDB. This is a design doc; no implementation lands in this PR.
+     7	
+     8	This document is split into focused sub-files to stay under the 500 LOC ceiling (`AGENTS.md`).
+     9	
+    10	- [Intro — problem statement + logical interest model](subscription-compilation/intro.md) (§1, §2)
+    11	- [Compiler — pipeline + plan-id contract + function migration table](subscription-compilation/compiler.md) (§3)
+    12	- [Recompilation — triggers and actor message shapes](subscription-compilation/recompilation.md) (§4)
+    13	- [Diagnostics — four-lane records + reverse-coverage view](subscription-compilation/diagnostics.md) (§5, §8)
+    14	- [nmp-nip65 — crate layout, traits, public surface](subscription-compilation/nip65.md) (§6)
+    15	- [Outbox — publish-planner seam + override action](subscription-compilation/outbox.md) (§7)
+    16	- [Tests — wire-frame audit gate](subscription-compilation/tests.md) (§9)
+    17	
+    18	## Section map
+    19	
+    20	| § | Topic | File |
+    21	|---|---|---|
+    22	| 1 | What is wrong with the current planner (cited line refs) | intro.md |
+    23	| 2 | Logical interest — formal definition + how views express them | intro.md |
+    24	| 3 | Compilation pipeline: authors → mailboxes → per-relay plans + plan-id | compiler.md |
+    25	| 4 | Recompilation triggers (kind:10002, view open/close, reconnect, account switch, manual, user-configured change) | recompilation.md |
+    26	| 5 | Four-lane diagnostic records (NIP-65 / hint / provenance / user-configured) | diagnostics.md |
+    27	| 6 | `nmp-nip65` file layout, `MailboxesViewModule`, public surface | nip65.md |
+    28	| 7 | `PublishPlanner` trait, write fan-out policy, override + debug warning | outbox.md |
+    29	| 8 | Reverse-relay-coverage diagnostic view ("this relay serves N authors of our timeline") | diagnostics.md |
+    30	| 9 | M2 exit-gate audit test path + assertions | tests.md |
+    31	| 10 | Open questions for follow-up ADRs | this file (below) |
+    32	
+    33	## 10. Open questions
+    34	
+    35	These remain to be resolved by ADRs after design review, not in this design pass.
+    36	
+    37	1. **Plan-id stability under perturbation.** The intro/compiler picks "logical-interest set + author-mailbox snapshot ⇒ plan-id." That ties plan-id to mailbox membership, so a single new kind:10002 arrival reshuffles plan-ids for every interest including that author. An alternative scopes plan-id to the *logical-interest set only* and tracks per-relay assignment as a separate stable identifier. Pick one in an ADR; the test contracts in §9 assume the former.
+    38	2. **Filter-merge lattice formal-isation.** §3 step 3 lists which filter fields are safely mergeable across logical interests (`authors`, `kinds`, `since`, `until`, `limit`, tag operators). It does not yet specify the merge algebra for the corner case where two interests differ only in `limit` but agree on authors and kinds. Worth an ADR-level note before the lattice is coded; `docs/product-spec/subsystems.md` §7.2 promises "a formal merge lattice for `limit`, `since`, `until`, multi-filter arrays, and tag operators."
+    39	3. **Per-author indexer-fallback ledger row?** Today the compiler treats indexer fallback as an inline relay assignment. If the kind:10002 fetch is its own durable action (M6 ledger), the fallback becomes a tracked action with retry/cancel semantics. Cleaner for diagnostics; heavier for M2. Defer.
+    40	4. **Read-relay vs write-relay use for subscriptions on the same author.** NIP-65 defines write relays (outbox) for the author's own events and read relays (inbox) for events directed *at* the author. For a `Timeline { authors: [...] }` we want write relays. For a `Notifications { p: [author] }` we want inbox relays. The compiler distinguishes them by filter shape (`authors` vs `#p`), matching the `docs/product-spec/subsystems.md` §7.3 routing table. Document a corner case: kind-1 filtered by both `authors` and `#p` is rare but real (replies to the author from the author). Pick a precedence in an ADR; current bias is `authors` wins (write relays).
+    41	5. **User-configured relay precedence vs NIP-65.** A user adds `wss://my-private.example` to local config. Does it *augment* (union) or *override* (replace) NIP-65 routing for the active account? `subsystems.md` §7.3 default-resolves by NIP-65; user-configured is "fallback" in the indexer sense. ADR needs to spell out the augment/override question for the active account specifically.
+    42	6. **Auth-paused relays in compiled plans.** If a relay is in `RelayAuthState::ChallengeReceived`, the compiler still produces a plan that assigns interests to it (so reconnect-after-auth resumes correctly), but emission must pause. Is the pause modeled inside the compiler (per-relay gate) or inside the wire-emitter (consumes plans, applies pause)? Bias: wire-emitter, but the compiler must surface the pause as a fact for `LogicalInterestStatus`. Resolve before M5.
+    43	7. **NSE crate compilation surface.** `nmp-nip17-nse` (M9) runs in iOS Notification Service Extension with bounded memory; it needs to compile a single-author single-relay plan without the full planner. Confirm in an ADR that the compiler exposes a `compile_one(spec, mailbox_cache_snapshot) -> Plan` pure function suitable for NSE use, and that the function does not require a live actor.
+
+ succeeded in 0ms:
+     1	# Subscription Compilation §1–§2 — Problem and Logical Interest Model
+     2	
+     3	> Parent: `docs/design/subscription-compilation.md`.
+     4	> Read first: `docs/design/ndk-applesauce-lessons.md` §7 (subscription compilation lessons), §2 (NDK outbox lessons), §9 (NMP principles).
+     5	
+     6	## 1. Problem — what is wrong with the current planner
+     7	
+     8	The kernel today encodes "where a REQ goes" as a two-valued enum and resolves it at the call site of every request. Concretely:
+     9	
+    10	- **Two hardcoded relays.** `crates/nmp-core/src/relay.rs:1-2` declares `CONTENT_RELAY_URL = "wss://relay.primal.net"` and `INDEXER_RELAY_URL = "wss://purplepag.es"` as module-level constants. There is no per-author routing.
+    11	- **Relay choice is a 2-variant enum, not a URL set.** `crates/nmp-core/src/relay.rs:15-39` defines `RelayRole::{Content, Indexer}` with a `.url() -> &'static str` that returns one of the two literals. This shape cannot express "this REQ should go to the union of these N authors' write relays."
+    12	- **The seam that emits REQs is parameterized by `RelayRole`.** `crates/nmp-core/src/kernel/requests.rs:530-556` (`req()`) inserts a `WireSub { role, .. }` keyed by a string sub-id and emits `OutboundMessage { role, text }`. The role *is* the routing decision; there is no relay-URL field on `WireSub` or `OutboundMessage`. Any compiler that fans an interest out across N URLs has to replace this helper.
+    13	- **Startup REQs ignore mailboxes by construction.** `crates/nmp-core/src/kernel/requests.rs:50-106` (`startup_requests`) issues six fixed REQs, each pinned to `Content` or `Indexer`. The seed-bootstrap timeline (line 65–70) fans seven hundred-author future timelines through one relay. The exit-gate test for M2 (`docs/plan.md` lines 134–137) requires that this fan exactly equal the union of those authors' write relays.
+    14	- **View-open REQs ignore mailboxes too.** `crates/nmp-core/src/kernel/requests.rs:404-439` (`author_requests`) hardcodes a three-REQ shape — `author-relays-N` on Indexer, `author-profile-N` on Indexer, `author-notes-N` on Content. The author's notes are fetched from the global content relay even though by the time the view opens we may already have that author's kind:10002 in cache (see next bullet).
+    15	- **Mailbox cache exists but no consumer.** `crates/nmp-core/src/kernel/ingest.rs:209-233` (`ingest_relay_list`) already parses kind:10002 into `self.author_relay_lists: HashMap<String, AuthorRelayList>` (declared at `crates/nmp-core/src/kernel/mod.rs:269-275` and reserved at `mod.rs:313`). The cache is written; **nothing reads it for routing**. This is the bug doctrine D5 ("capabilities report, never decide") inverted: we have the data, we ignore it.
+    16	- **Profile claim path is single-relay.** `crates/nmp-core/src/kernel/requests.rs:390-402` (`profile_claim_request`) sends a kind:0 fetch to `RelayRole::Indexer` unconditionally. It cannot consult mailboxes for the claimed author.
+    17	- **No publish path exists yet.** `crates/nmp-core/src/kernel/requests.rs:30` (no occurrences of `EVENT` outbound) and `crates/nmp-core/src/relay.rs:42-45` (`OutboundMessage` carries only role + text). The first publish action (M6 `SendNote`) will hit this same `req()`-style seam. M2 must establish the planner shape before M6 builds the first user of it; the doctrine "no developer-supplied relays for a publish" (`docs/aim.md` §6 doctrine 5; `docs/product-spec/subsystems.md` §7.3 row "Publish leaked to wrong relays") needs a structural enforcement point.
+    18	
+    19	The summary diagnosis: **the planner is a string formatter, not a compiler.** Every REQ is a per-call-site decision; routing is one of two literals; recompilation is impossible because nothing is compiled. The diagnostics in `crates/nmp-core/src/kernel/mod.rs:117-154` already type `RelayStatus` / `WireSubscriptionStatus` / `LogicalInterestStatus` per ADR-0007 — but the planner currently emits at most one `LogicalInterestStatus` per view kind because there is no logical-interest object to scope it against.
+    20	
+    21	## 2. The logical interest model
+    22	
+    23	A **logical interest** is the actor-internal, semantics-preserving description of what a view, action, or monitor wants the kernel to keep alive on the wire. It is the input to compilation. It is *not* a Nostr filter (a filter is one possible wire artifact a plan can produce — `docs/design/ndk-applesauce-lessons.md` §7 lines 89–90).
+    24	
+    25	### 2.1 Formal shape
+    26	
+    27	```rust
+    28	// crates/nmp-core/src/kernel/planner/interest.rs (proposed)
+    29	
+    30	/// A logical interest is what a kernel-side consumer (view, action, monitor,
+    31	/// sync job, pointer loader) wants alive on the wire. The compiler turns N
+    32	/// logical interests into M ≤ N per-relay plans.
+    33	#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
+    34	pub struct LogicalInterest {
+    35	    /// Stable identity assigned by the registry on first insertion. Survives
+    36	    /// recompilation. Two interests with identical content collide on hash but
+    37	    /// keep distinct ids if they were registered by distinct claims.
+    38	    pub id: InterestId,
+    39	
+    40	    /// Scope decides how mailbox resolution and indexer fallback behave.
+    41	    /// Account-scoped interests resolve via the active account's mailbox view
+    42	    /// for ambiguous filters (e.g. interests with no `authors` and no `#p`).
+    43	    pub scope: InterestScope,
+    44	
+    45	    /// What the consumer wants. This is a normalised filter set, not a Nostr
+    46	    /// wire filter. Tags use sorted vec representation so equality is stable.
+    47	    pub shape: InterestShape,
+    48	
+    49	    /// Routing hints the consumer wants honoured. The compiler may ignore
+    50	    /// hints if they conflict with policy (e.g. private-publish privacy).
+    51	    pub hints: Vec<RelayHint>,
+    52	
+    53	    /// Lifecycle: tailing means "stay open after EOSE"; one_shot closes on
+    54	    /// EOSE. Window is the planner's intent, not necessarily the relay
+    55	    /// `since`/`until` it ends up emitting.
+    56	    pub lifecycle: InterestLifecycle,
+    57	}
+    58	
+    59	pub enum InterestScope {
+    60	    /// Bound to the active account in SessionState. Re-routes on account switch.
+    61	    ActiveAccount,
+    62	    /// Bound to a specific account (multi-account UIs, M8). Re-routes on that
+    63	    /// account's mailbox refresh; ignored on account switch.
+    64	    Account(AccountId),
+    65	    /// No account context. Used for global pointer loaders, NIP-19-driven
+    66	    /// fetches, and indexer-direct probes.
+    67	    Global,
+    68	}
+    69	
+    70	pub struct InterestShape {
+    71	    pub authors:    BTreeSet<Pubkey>,        // empty = wildcard
+    72	    pub kinds:      BTreeSet<u32>,           // empty = wildcard (rare)
+    73	    pub tags:       BTreeMap<TagKey, BTreeSet<String>>,  // sorted for hash stability
+    74	    pub since:      Option<UnixSeconds>,
+    75	    pub until:      Option<UnixSeconds>,
+    76	    pub limit:      Option<u32>,
+    77	    pub event_ids:  BTreeSet<EventId>,       // for pointer/thread hydration
+    78	}
+    79	
+    80	pub enum InterestLifecycle {
+    81	    Tailing,                                   // stays open after EOSE
+    82	    OneShot,                                   // CLOSE on EOSE
+    83	    BoundedTime { until_ms: u64 },             // CLOSE on EOSE or deadline
+    84	}
+    85	```
+    86	
+    87	`InterestShape` mirrors the Nostr filter shape closely on purpose: most logical interests correspond directly to a single filter, and the kernel ships canonical normalisation (sort, dedup, fold ranges) so equality and hashing are deterministic. The compiler is then free to merge two shapes (or refuse to) on the basis of structural compatibility (§3 step 3).
+    88	
+    89	### 2.2 How view modules express interests
+    90	
+    91	Every `ViewModule` (per `docs/design/kernel-substrate.md` §3) declares its dependencies via the existing `ViewDependencies` mechanism. M2 adds one new method:
+    92	
+    93	```rust
+    94	pub trait ViewModule {
+    95	    // ... existing methods ...
+    96	
+    97	    /// Translate a view spec into the logical interests required to keep it
+    98	    /// live. Called by the planner when the view opens; re-called on
+    99	    /// recompilation triggers that the view module opts into (account-switch,
+   100	    /// mailbox refresh).
+   101	    fn interests(spec: &Self::Spec, ctx: &InterestContext)
+   102	        -> Vec<LogicalInterest>;
+   103	}
+   104	```
+   105	
+   106	`InterestContext` exposes read access to the mailbox cache and to the active account but **not** to the relay set; the view module never names relays. That keeps `nmp-aim.md` doctrine 5 ("outbox routing automatic; manual relay selection is the opt-out, not the default") structurally enforced — there is no place for a view module to write a URL.
+   107	
+   108	Concrete examples for the existing seed-timeline path:
+   109	
+   110	- `TimelineView { authors: [pablof7z, fiatjaf, jb55, ...follows] }` returns one `LogicalInterest { shape: { authors, kinds: {1, 6}, limit: 200 }, lifecycle: Tailing }`.
+   111	- `AuthorView { pubkey }` returns three interests: kind:10002 (Indexer fallback policy, see §3), kind:0 (one-shot), and `{ authors: [pubkey], kinds: {1, 6}, limit: 100 }` (Tailing).
+   112	- `ProfileClaim { pubkey }` (the refcounted UI path from `crates/nmp-core/src/kernel/requests.rs:202-237`) returns one interest: `{ authors: [pubkey], kinds: {0}, limit: 1, lifecycle: OneShot }`.
+   113	- `ThreadView { event_id }` returns up to two interests: `{ ids: [...] }` for context, `{ kinds: {1, 6}, tags: { #e: [...] } }` for replies.
+   114	
+   115	The seed-bootstrap path (`crates/nmp-core/src/kernel/requests.rs:50-106`) becomes one `LogicalInterest` per concern, registered at actor `Start` rather than emitted as raw REQs. The compiler produces the wire artifacts.
+   116	
+   117	### 2.3 Account scope binding
+   118	
+   119	The kernel `SessionState` (`docs/product-spec/subsystems.md` §7.4; M8 graduates this to multi-account) carries an active account id. `InterestScope::ActiveAccount` resolves at compile time, not at registration time. On account switch (§4 trigger A4), the compiler re-evaluates every `ActiveAccount`-scoped interest against the new active account's mailbox view. This is the structural enforcement of bug-extinction #5 (account-context overlap) referenced in `docs/plan.md` line 284 — the kernel cannot "forget" to re-route because every plan re-derives from the active scope.
+   120	
+   121	Account-scoped interests with empty `authors` and empty `#p` (e.g. a free-form hashtag firehose) resolve against the active account's *read relays* (NIP-65 read side) — the user's own subscription preferences, not a globally hardcoded relay. Today's `firehose_requests()` at `crates/nmp-core/src/kernel/requests.rs:357-372` hardcodes `RelayRole::Content`; under the compiler this becomes "active-account read relays, falling back to indexer set if the active account has no kind:10002."
+   122	
+   123	### 2.4 What is *not* a logical interest
+   124	
+   125	To keep the surface small, the following are explicitly **not** logical interests:
+   126	
+   127	- A **wire REQ**. Wire REQs are produced by the compiler; they live in `WireSubscriptionStatus` per ADR-0007.
+   128	- A **publish**. Publishes are durable actions on the action ledger (`docs/design/kernel-substrate.md` §4); they consult the `PublishPlanner` (§7) but they are not interests because they do not stay alive.
+   129	- A **diagnostic record**. ADR-0007 lanes are facts derived from the planner's state, not inputs.
+   130	- An **HTTP fetch** (Blossom upload, indexer JSON probe). Those are `CapabilityModule` requests.
+   131	
+   132	The boundary is intentional: an interest is anything that asks the planner to *keep a REQ open*. Everything else routes through a different seam.
+
+exec
+/bin/zsh -lc "nl -ba docs/decisions/0009-app-extension-kernel-boundary.md | sed -n '20,65p'" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc "nl -ba docs/product-spec/subsystems.md | sed -n '55,110p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+    20	## Decision
+    21	
+    22	`nmp-core` provides **generic infrastructure only**:
+    23	
+    24	- Actor runtime and unidirectional state flow.
+    25	- Verified Nostr event store with replaceable/delete/expiration semantics.
+    26	- Subscription planner with composite-keyed reverse index.
+    27	- Relay routing and publish pipeline.
+    28	- Signer/session plumbing (identity-scope agnostic).
+    29	- Domain-store substrate for non-Nostr records.
+    30	- Typed view registry (driven by `ViewModule` trait).
+    31	- Durable action ledger (driven by `ActionModule` trait).
+    32	- Capability bridge (driven by `CapabilityModule` trait).
+    33	- Platform-shadow + codegen machinery.
+    34	- Diagnostics and test harnesses.
+    35	
+    36	`nmp-core` does **not** contain:
+    37	
+    38	- Profile, Timeline, Thread, Reactions, Conversation, or any other view-kind business logic.
+    39	- Wallet, messaging, blossom, or any other domain feature.
+    40	- A closed `AppAction` enum or `AppUpdate` enum.
+    41	- A closed `ViewSpec` enum.
+    42	- App-specific identity concepts (agent, feedback identity, coach, etc.).
+    43	
+    44	**Four layers, clear ownership.**
+    45	
+    46	| Layer | Owns | May contain app nouns? |
+    47	|---|---|---|
+    48	| `nmp-core` kernel | actor, store substrate, planner, ledger, registries, codegen, diagnostics | No |
+    49	| NMP protocol modules (`nmp-nip01`, `nmp-nip17`, `nmp-nip29`, `nmp-nip65`, `nmp-blossom`, `nmp-nwc`, …) | reusable Nostr protocol concepts: Event, Filter, Keys, gift-wrap, groups, mailboxes, blossom, NWC | Only protocol nouns |
+    50	| App core crate (`twitter-core`, `highlighter-core`, `tenex-core`, …) | app domain records, view modules, action modules, app-specific capability types, policies | Yes |
+    51	| Platform shell | rendering, OS handle execution, generated wrappers | No policy nouns beyond UI labels |
+    52	
+    53	**Five extension trait families** (concrete signatures in `docs/design/kernel-substrate.md`):
+    54	
+    55	- `DomainModule` — durable non-Nostr records with migrations and indexes.
+    56	- `ViewModule` — typed reactive projections with payloads and deltas.
+    57	- `ActionModule` — durable workflows on the action ledger.
+    58	- `CapabilityModule` — typed native fact reports.
+    59	- `IdentityModule` — signer scopes beyond "active Nostr account."
+    60	
+    61	**The rule.** If implementing Highlighter, TENEX, Win the Day, Cut Tracker, or a podcast app requires adding domain nouns to `nmp-core`, the extension boundary is wrong and the kernel must change, not the app.
+    62	
+    63	## What changes from prior ADRs
+    64	
+    65	- **ADR-0006 (vertical-slice-first):** the slice's discipline survives — running code at every checkpoint, one architectural ingredient per sub-phase. The slice's *target* changes: the kind:0 Profile path is now built as the canonical `ViewModule` in a Nostr-protocol module, not as a built-in feature of `nmp-core`. The slice now proves the extension boundary first, then the protocol module on top.
+
+ succeeded in 0ms:
+    55	
+    56	### 7.2 Subscription planner
+    57	
+    58	Owns the mapping from `ViewSpec` → `Vec<Filter>` → `Vec<RelayUrl>` → on-the-wire REQ.
+    59	
+    60	Behaviors:
+    61	
+    62	- **Live tail first.** Live subscriptions register their local handler and start REQ tailing immediately. Historical backfill runs beside it, not before it.
+    63	- **Coverage-aware backfill.** Before issuing historical traffic, the planner consults cache coverage/watermarks (§7.1). Complete coverage serves from cache; partial coverage schedules a gap fill; unknown coverage triggers bounded fetch/REQ or NIP-77 if supported. A non-empty cache result is never treated as complete by itself.
+    64	- **Logical vs. wire subscriptions.** A logical subscription belongs to a view/action/monitor. A wire subscription belongs to a relay. Many logical consumers may share one wire REQ, and each consumer still receives only events matching its canonical filter.
+    65	- **Coalescing.** Filters that are equal or safely subsumable into a single broader filter share one REQ per relay. The planner maintains a formal merge lattice for `limit`, `since`, `until`, multi-filter arrays, and tag operators.
+    66	- **Loader integration.** Pointer/address/tag/timeline misses go through the pointer loader registry with cache-first batching, dedupe, relay hints, cancellation on view close, and explicit missing-window state.
+    67	- **Auto-close.** REQs without consumers are CLOSE'd. One-shot filters (those with no live subscribers, only an `until` upper bound) are CLOSE'd on EOSE.
+    68	- **Buffering.** Inbound events are batched to ≤ 60Hz per view (configurable). Batches turn into one `ViewBatch` per tick.
+    69	- **Backpressure.** If platform-side rendering falls behind, the planner drops `ViewBatch` updates in favor of a single `FullState` catch-up. View payload semantics make this lossless.
+    70	- **Reconnect.** On relay reconnect, the planner restores live REQs and schedules a coverage-aware gap fill. View payloads do not reset.
+    71	
+    72	### 7.3 Outbox routing
+    73	
+    74	Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
+    75	
+    76	**Resolution algorithm.**
+    77	
+    78	| Operation | Relay set |
+    79	|---|---|
+    80	| Subscription with `authors` filter | Union of each pubkey's write relays (kind-10002), deduplicated. Pubkeys without known mailboxes trigger an opportunistic kind-10002 fetch from indexer relays. |
+    81	| Subscription with `p` tag filter or notifications | Union of each tagged pubkey's inbox relays. |
+    82	| Subscription with neither | Active session's read relays. |
+    83	| Publish of any signed event | Author's write relays. |
+    84	| Publish with `p` tags (DMs, mentions, reactions) | Author's write relays **plus** each tagged pubkey's inbox relays. |
+    85	| DM (NIP-17 gift-wrapped) | **Only** resolved recipient inbox relays. Never the author's write relays. Never the active session's "default" relays. Missing recipient inbox relays fail closed. |
+    86	| Discovery (kind-10002 fetch for unknown pubkeys) | Configurable indexer relay set (default: a curated list of high-coverage relays). |
+    87	
+    88	**Why this prevents specific failure modes.**
+    89	
+    90	- "Publish leaked to wrong relays" → ruled out by the safe API. The developer cannot supply a relay list to `SendNote`. Explicit overrides are named, one-shot, and debug-flagged in logs.
+    91	- "DM accidentally public" → ruled out by the safe API. The DM publish path consults only resolved inbox relays; there is no fallback-to-all-relays path for gift wraps.
+    92	- "Reads missing an author's actual relays" → bounded and surfaced. If the author's kind-10002 is reachable it is opportunistically fetched on first contact; if not, coverage and diagnostic state expose the miss risk and configured fallback policy.
+    93	- "Hand-rolled fan-out logic" → no API surface for it.
+    94	
+    95	**Per-pubkey relay-list lifecycle.**
+    96	
+    97	- First contact with an unknown pubkey → enqueue kind-10002 fetch from indexer relays.
+    98	- Fresher kind-10002 arrives → invalidate dependent subscriptions, recompute relay sets, re-issue REQs as needed.
+    99	- Kind-10002 missing for a pubkey after N seconds → fall back to indexer set for reads only; do not publish to indexers.
+   100	
+   101	The gossip cache is the `nostr-gossip` crate; backend selection (in-memory vs SQLite) follows the storage backend choice. Watermarks (§7.1) intersect with outbox: a sync watermark is keyed by `(filter, relay)` and naturally tracks per-author per-relay coverage.
+   102	
+   103	### 7.4 Sessions
+   104	
+   105	`SessionState` holds:
+   106	
+   107	```rust
+   108	pub struct SessionState {
+   109	    pub accounts: Vec<Account>,
+   110	    pub active: Option<String>,             // pubkey
+
+exec
+/bin/zsh -lc 'rg -n "''^### 7'"\\.|"'^##|''^#" docs/product-spec/subsystems.md docs/design/kernel-substrate.md docs/aim.md docs/plan.md docs/design/app-extension-kernel.md docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md docs/design/ndk-applesauce-lessons.md' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:1:# ADR 0007: Diagnostics and non-Nostr domain data over the app bridge
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:6:## Context
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:17:## Decision
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:29:## Network observability model
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:33:### 1. Relay status
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:84:### 2. Wire subscription status
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:114:### 3. Logical interest status
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:155:## How status crosses the bridge
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:175:## Vertical-slice implications
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:187:## Non-Nostr data model
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:208:## Platform shadow behavior
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:219:## Consequences
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:228:## Alternatives considered
+docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md:235:## Validation
+docs/aim.md:1:# Project Aim — Rust Multiplatform Nostr Application Framework
+docs/aim.md:3:## Purpose of this document
+docs/aim.md:9:## 1. The north star
+docs/aim.md:19:## 2. Architectural foundation: the RMP bible
+docs/aim.md:74:## 3. Protocol foundation: existing Rust primitives
+docs/aim.md:93:## 4. High-level functionality being synthesized
+docs/aim.md:101:### 4.1 Reactive single source of truth ("EventStore")
+docs/aim.md:113:### 4.2 Reactive models / derived views
+docs/aim.md:119:### 4.3 Action-based writes
+docs/aim.md:125:### 4.4 Outbox / smart relay routing (NIP-65)
+docs/aim.md:131:### 4.5 Subscription planner
+docs/aim.md:135:### 4.6 Multi-account sessions
+docs/aim.md:141:### 4.7 Web of Trust
+docs/aim.md:145:### 4.8 NIP-77 Negentropy sync
+docs/aim.md:149:### 4.9 Wallet integration
+docs/aim.md:153:### 4.10 Messaging
+docs/aim.md:157:### 4.11 Blossom media
+docs/aim.md:161:### 4.12 Developer guardrails
+docs/aim.md:165:### 4.13 Testing
+docs/aim.md:169:### 4.14 Scaffolding CLI
+docs/aim.md:175:## 5. Crate layout
+docs/aim.md:219:## 6. Doctrine — the rules the API must make hard to violate
+docs/aim.md:238:## 7. Open design questions (must resolve before substantive coding)
+docs/aim.md:250:## 8. References
+docs/aim.md:258:## 9. What this document is not
+docs/plan.md:1:# Build & Validation Plan
+docs/plan.md:13:## 0. Where we are right now
+docs/plan.md:17:### Implemented and running
+docs/plan.md:27:### Designed but not implemented
+docs/plan.md:45:### Gaps in the prior plan that this rewrite addresses
+docs/plan.md:60:## 1. Principles of execution
+docs/plan.md:71:## 2. The milestone ladder
+docs/plan.md:75:### M0 — Kernel substrate + non-Nostr fixture *(DONE)*
+docs/plan.md:89:### M1 — Read-only Twitter slice on iOS *(LARGELY DONE; final hardening in flight)*
+docs/plan.md:118:### M2 — Subscription compilation + outbox routing
+docs/plan.md:143:### M3 — Persistence (LMDB) + full insert invariants
+docs/plan.md:167:### M4 — NIP-77 negentropy sync engine
+docs/plan.md:194:### M5 — NIP-42 auth
+docs/plan.md:217:### M6 — Sessions + signers + write path
+docs/plan.md:244:### M7 — Reactions + Thread + Reply (the interaction loop)
+docs/plan.md:267:### M8 — Multi-session (multi-account) clients
+docs/plan.md:291:### M9 — NIP-17 DMs + NSE
+docs/plan.md:315:### M10 — Blossom + media + long-running capabilities
+docs/plan.md:341:### M10.5 — FFI hardening + iOS empirical proof *(hard gate before M11 starts)*
+docs/plan.md:386:### M11 — Podcast app (the `../podcast` rebuild on NMP — the kernel-boundary proof)
+docs/plan.md:484:### M12 — Wallet (NWC + zaps + Cashu + nutzaps)
+docs/plan.md:510:### M13 — Web-of-Trust
+docs/plan.md:535:### M14 — UniFFI migration
+docs/plan.md:559:### M15 — Cross-platform: Android + Desktop + Web
+docs/plan.md:601:### M16 — CLI + starter app + recipe book
+docs/plan.md:624:### M17 — v1 release
+docs/plan.md:641:## 3. Subsystem coverage matrix
+docs/plan.md:659:### NIP support roadmap at v1
+docs/plan.md:693:## 4. Parallelization opportunities
+docs/plan.md:706:### Worktree hygiene
+docs/plan.md:712:## 5. Test pyramid
+docs/plan.md:730:## 6. CI / pre-merge hygiene
+docs/plan.md:750:## 7. Decision log
+docs/plan.md:767:### The harness-first pattern
+docs/plan.md:771:### Modeled budget contract vs runtime evidence
+docs/plan.md:782:## 8. What this plan is not
+docs/design/app-extension-kernel.md:1:# Design Proposal: App Extension Kernel Boundary
+docs/design/app-extension-kernel.md:7:## 1. Problem
+docs/design/app-extension-kernel.md:23:## 2. Decision
+docs/design/app-extension-kernel.md:54:## 3. Layering
+docs/design/app-extension-kernel.md:71:## 4. Extension Points
+docs/design/app-extension-kernel.md:73:### 4.1 Domain Modules
+docs/design/app-extension-kernel.md:101:### 4.2 View Modules
+docs/design/app-extension-kernel.md:126:### 4.3 Action Modules And The Durable Ledger
+docs/design/app-extension-kernel.md:172:### 4.4 Capability Modules
+docs/design/app-extension-kernel.md:198:### 4.5 Identity Scopes
+docs/design/app-extension-kernel.md:215:### 4.6 Typed Nostr References
+docs/design/app-extension-kernel.md:234:### 4.7 Codegen Contract
+docs/design/app-extension-kernel.md:246:## 5. What Stays Out Of NMP Core
+docs/design/app-extension-kernel.md:269:## 6. Acceptance Tests For The Boundary
+docs/design/app-extension-kernel.md:273:### 6.1 Highlighter-lite
+docs/design/app-extension-kernel.md:286:### 6.2 Personal-coach-lite
+docs/design/app-extension-kernel.md:298:### 6.3 TENEX-lite
+docs/design/app-extension-kernel.md:310:### 6.4 Podcast-lite
+docs/design/app-extension-kernel.md:322:## 7. Consequences
+docs/design/app-extension-kernel.md:332:## 8. Open Design Questions
+docs/design/app-extension-kernel.md:340:## 9. Recommended Next Step
+docs/design/kernel-substrate.md:1:# Design: Kernel substrate (extension trait families)
+docs/design/kernel-substrate.md:11:## 1. The five extension trait families
+docs/design/kernel-substrate.md:27:## 2. `DomainModule` — durable non-Nostr records
+docs/design/kernel-substrate.md:82:#[derive(Serialize, Deserialize, Clone)]
+docs/design/kernel-substrate.md:113:## 3. `ViewModule` — typed reactive projections
+docs/design/kernel-substrate.md:180:## 4. `ActionModule` — durable workflows on the action ledger
+docs/design/kernel-substrate.md:245:#[derive(Clone, Serialize, Deserialize)]
+docs/design/kernel-substrate.md:251:#[derive(Clone, Serialize, Deserialize)]
+docs/design/kernel-substrate.md:287:## 5. `CapabilityModule` — typed native fact reports
+docs/design/kernel-substrate.md:326:## 6. `IdentityModule` — signer scopes
+docs/design/kernel-substrate.md:356:## 7. Codegen pipeline
+docs/design/kernel-substrate.md:386:## 8. Module composition rules
+docs/design/kernel-substrate.md:396:## 9. Diagnostics integration (ADR-0007)
+docs/design/kernel-substrate.md:410:## 10. Testing patterns
+docs/design/kernel-substrate.md:415:#[test]
+docs/design/kernel-substrate.md:435:## 11. What goes in v1 vs later
+docs/design/kernel-substrate.md:469:## 12. Open questions still to settle
+docs/design/kernel-substrate.md:479:## 13. Validation
+docs/design/ndk-applesauce-lessons.md:1:# Design Note: Lessons from NDK and Applesauce
+docs/design/ndk-applesauce-lessons.md:7:## 1. Purpose
+docs/design/ndk-applesauce-lessons.md:15:## 2. What NDK Gets Right
+docs/design/ndk-applesauce-lessons.md:25:## 3. What NDK Warns Us About
+docs/design/ndk-applesauce-lessons.md:33:## 4. What Applesauce Gets Right
+docs/design/ndk-applesauce-lessons.md:50:## 5. What Applesauce Warns Us About
+docs/design/ndk-applesauce-lessons.md:60:## 6. NIP-77 And Sync Lessons
+docs/design/ndk-applesauce-lessons.md:74:## 7. Subscription Compilation Lessons
+docs/design/ndk-applesauce-lessons.md:97:## 8. Loading And Pagination Lessons
+docs/design/ndk-applesauce-lessons.md:105:## 9. NMP Principles To Preserve
+docs/design/ndk-applesauce-lessons.md:107:### 9.1 Outbox Is Policy, Not Transport
+docs/design/ndk-applesauce-lessons.md:111:### 9.2 Relay Metadata Is Durable Domain State
+docs/design/ndk-applesauce-lessons.md:115:### 9.3 Developer APIs Should Express Intent
+docs/design/ndk-applesauce-lessons.md:119:### 9.4 Reads Should Render Before Routing Is Perfect
+docs/design/ndk-applesauce-lessons.md:123:### 9.5 Privacy-Sensitive Routes Must Fail Closed
+docs/design/ndk-applesauce-lessons.md:127:### 9.6 Routing Decisions Must Be Observable
+docs/design/ndk-applesauce-lessons.md:141:### 9.7 Tests Should Target Policy Directly
+docs/design/ndk-applesauce-lessons.md:145:### 9.8 Coverage Is Different From Cache Presence
+docs/design/ndk-applesauce-lessons.md:149:## 10. Product Direction
+docs/product-spec/subsystems.md:1:# Product Spec: Subsystems
+docs/product-spec/subsystems.md:5:## 7. Subsystem specifications
+docs/product-spec/subsystems.md:7:### 7.1 EventStore
+docs/product-spec/subsystems.md:56:### 7.2 Subscription planner
+docs/product-spec/subsystems.md:72:### 7.3 Outbox routing
+docs/product-spec/subsystems.md:103:### 7.4 Sessions
+docs/product-spec/subsystems.md:137:### 7.5 Actions catalog
+docs/product-spec/subsystems.md:144:#[async_trait]
+docs/product-spec/subsystems.md:157:### 7.6 Views
+docs/product-spec/subsystems.md:200:#[derive(Clone, uniffi::Record)]
+docs/product-spec/subsystems.md:207:#[derive(Clone, uniffi::Record)]
+docs/product-spec/subsystems.md:230:### 7.7 Web of Trust
+docs/product-spec/subsystems.md:240:### 7.8 Sync engine (live REQ plus NIP-77 backfill)
+docs/product-spec/subsystems.md:293:### 7.9 Wallet
+docs/product-spec/subsystems.md:308:### 7.10 Messaging
+docs/product-spec/subsystems.md:320:### 7.11 Blossom
+docs/product-spec/subsystems.md:324:### 7.12 Guardrails
+docs/product-spec/subsystems.md:338:### 7.13 Testing surface
+docs/product-spec/subsystems.md:351:### 7.14 Background notification decryption
+docs/product-spec/subsystems.md:356:#[uniffi::export]
+docs/product-spec/subsystems.md:363:#[derive(uniffi::Record)]
+docs/product-spec/subsystems.md:377:### 7.15 Offline action queue
+docs/product-spec/subsystems.md:391:### 7.16 Performance instrumentation (`nmp-metrics`)
+docs/product-spec/subsystems.md:439:### 7.17 Future module integration contract
+
+exec
+/bin/zsh -lc "rg --files | rg 'docs/product-spec.md"'$|nmp-aim.md$|docs/aim.md$|docs/product-spec/subsystems.md$'"'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+docs/aim.md
+docs/product-spec/subsystems.md
+docs/product-spec.md
+
+exec
+/bin/zsh -lc "nl -ba docs/product-spec.md | sed -n '1,80p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# Product Specification - Nostr Multi-Platform Framework
+     2	
+     3	This document is split into focused sections to keep each hand-authored file under the repository LOC ceiling.
+     4	
+     5	- [Product Spec: Overview And Developer Experience](product-spec/overview-and-dx.md)
+     6	- [Product Spec: API Surface](product-spec/api-surface.md)
+     7	- [Product Spec: Subsystems](product-spec/subsystems.md)
+     8	- [Product Spec: CLI, Toolchain, Phasing](product-spec/cli-toolchain-phasing.md)
+     9	- [Product Spec: Appendices](product-spec/appendices.md)
+    10	
+    11	## Section Map
+    12	
+    13	- Sections 1-5: [overview and developer experience](product-spec/overview-and-dx.md)
+    14	- Section 6: [API surface](product-spec/api-surface.md)
+    15	- Section 7: [subsystems](product-spec/subsystems.md)
+    16	- Sections 8-14: [CLI, toolchain, phasing, open questions, glossary](product-spec/cli-toolchain-phasing.md)
+    17	- Appendices: [FFI details and NIP glossary](product-spec/appendices.md)
+
+exec
+/bin/zsh -lc "rg -n 'docs/product-spec/subsystems\\.md"'` §7'"\\.6|docs/aim\\.md"'` §6 doctrine 10|docs/aim'"\\.md"'` §6 doctrine 5|nmp-aim'"\\.md|docs/product-spec\\.md|docs/plan\\.md"'` line|docs/product-spec/subsystems'"\\.md"'` §7'"\\.3 line|docs/design/ndk-applesauce-lessons\\.md"'` §[0-9] line|line [0-9]'"' docs/design/subscription-compilation.md docs/design/subscription-compilation" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+docs/design/subscription-compilation/diagnostics.md:10:Per `docs/design/ndk-applesauce-lessons.md` §4 (lines 39–46) and `docs/aim.md` §6 doctrine 10 ("provenance preserved"), the four relay-fact lanes are:
+docs/design/subscription-compilation/diagnostics.md:17:Each lane is its own record stream. They never merge into a single "relays" field — that collapse is exactly the bug `docs/design/ndk-applesauce-lessons.md` §4 line 46 forbids. They may be displayed side-by-side in a diagnostic view; the actor stores them apart.
+docs/design/subscription-compilation/diagnostics.md:178:This is the M2 exit-gate diagnostic listed in `docs/plan.md` line 130 ("Reverse-relay-coverage view for diagnostics: 'this relay is serving N authors of our timeline.'").
+docs/design/subscription-compilation/diagnostics.md:188:Per `docs/aim.md` §4.4 ("the developer does not pick relays per operation; the framework does") and ADR-0007's domain-of-diagnostics separation, end-user product UIs do not show "relay X is serving 12 authors." That is operator/debug surface. Normal apps consume the `LogicalInterestStatus` summaries; `RelayCoveragePayload` is for the diagnostics screen (proof iOS app screenshot in `docs/perf/m2/outbox-routing.md` per `docs/plan.md` line 139).
+docs/design/subscription-compilation/recompilation.md:15:- **External** triggers are `AppAction` variants the platform may dispatch directly. There is exactly one — `AppAction::InvalidateCompile { reason }` — to keep the public surface minimal per `docs/aim.md` §6 doctrine 5.
+docs/design/subscription-compilation/recompilation.md:93:Emitted from `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`) when **and only when** the parser decides to replace the prior mailbox entry (the `should_replace` branch at line 218–222). Stale arrivals do not trigger recompilation.
+docs/design/subscription-compilation/recompilation.md:107:Emitted by the view registry after the warmth grace expires for an interest with refcount = 0. The warmth window is configurable (`AppConfig.view_warmth_ms`, default 30,000 — matching the doctrine in `docs/product-spec/subsystems.md` §7.6 "View warmth"). Closing an interest mid-warmth (e.g. account switch invalidates the prior account's interests) is a separate `ActiveAccountChanged` trigger, not this one.
+docs/design/subscription-compilation/compiler.md:89:2. **Read fallback.** For a `RoutingDirection::Outbox` interest whose author has no known mailboxes, the compiler routes the interest to the indexer set **as read-only fallback**. Per `docs/product-spec/subsystems.md` §7.3 line 99: "fall back to indexer set for reads only; do not publish to indexers." The `RoutingSource::Indexer` tag on the resulting `AuthorRouting` flows through to the diagnostic surface so the four-lane view (§5) can render "author X is being served by indexer Y because we have no mailbox for them."
+docs/design/subscription-compilation/compiler.md:166:| `startup_requests` (requests.rs:50-106) | Becomes `register_startup_interests()`: pushes 4 `LogicalInterest`s into the registry (seed timeline; account kind:0; account kind:10002; seed contacts kind:3). The compiler runs once and produces wire REQs. The TEST_PUBKEY-specific bootstrap (line 71-82) becomes an `InterestScope::Global` interest with hardcoded indexer routing flagged `RoutingSource::UserConfigured` (it is operator-debug seed data). |
+docs/design/subscription-compilation/tests.md:4:> Read first: `docs/plan.md` lines 134–137 (M2 exit gates); `docs/design/firehose-bench.md` (the modeled bench harness this test does *not* duplicate).
+docs/design/subscription-compilation/tests.md:28:Four assertions corresponding to the four M2 exit-gate bullets in `docs/plan.md` lines 134–137:
+docs/design/subscription-compilation/tests.md:178:This assertion is what `docs/design/ndk-applesauce-lessons.md` §2 line 19 calls out as NDK's important operational truth: "metadata can arrive late… the system should be able to refresh or expand active work without the app tearing down and recreating views."
+docs/design/subscription-compilation/tests.md:261:- **Real wire frames against a relay.** This is `firehose-bench live` per `docs/plan.md` line 110; the audit test is offline and synthetic.
+docs/design/subscription-compilation/intro.md:13:- **Startup REQs ignore mailboxes by construction.** `crates/nmp-core/src/kernel/requests.rs:50-106` (`startup_requests`) issues six fixed REQs, each pinned to `Content` or `Indexer`. The seed-bootstrap timeline (line 65–70) fans seven hundred-author future timelines through one relay. The exit-gate test for M2 (`docs/plan.md` lines 134–137) requires that this fan exactly equal the union of those authors' write relays.
+docs/design/subscription-compilation/intro.md:17:- **No publish path exists yet.** `crates/nmp-core/src/kernel/requests.rs:30` (no occurrences of `EVENT` outbound) and `crates/nmp-core/src/relay.rs:42-45` (`OutboundMessage` carries only role + text). The first publish action (M6 `SendNote`) will hit this same `req()`-style seam. M2 must establish the planner shape before M6 builds the first user of it; the doctrine "no developer-supplied relays for a publish" (`docs/aim.md` §6 doctrine 5; `docs/product-spec/subsystems.md` §7.3 row "Publish leaked to wrong relays") needs a structural enforcement point.
+docs/design/subscription-compilation/intro.md:23:A **logical interest** is the actor-internal, semantics-preserving description of what a view, action, or monitor wants the kernel to keep alive on the wire. It is the input to compilation. It is *not* a Nostr filter (a filter is one possible wire artifact a plan can produce — `docs/design/ndk-applesauce-lessons.md` §7 lines 89–90).
+docs/design/subscription-compilation/intro.md:106:`InterestContext` exposes read access to the mailbox cache and to the active account but **not** to the relay set; the view module never names relays. That keeps `nmp-aim.md` doctrine 5 ("outbox routing automatic; manual relay selection is the opt-out, not the default") structurally enforced — there is no place for a view module to write a URL.
+docs/design/subscription-compilation/intro.md:119:The kernel `SessionState` (`docs/product-spec/subsystems.md` §7.4; M8 graduates this to multi-account) carries an active account id. `InterestScope::ActiveAccount` resolves at compile time, not at registration time. On account switch (§4 trigger A4), the compiler re-evaluates every `ActiveAccount`-scoped interest against the new active account's mailbox view. This is the structural enforcement of bug-extinction #5 (account-context overlap) referenced in `docs/plan.md` line 284 — the kernel cannot "forget" to re-route because every plan re-derives from the active scope.
+docs/design/subscription-compilation/outbox.md:132:- **Step 2's "no indexer fallback for writes"** is the structural enforcement of the doctrine `docs/product-spec/subsystems.md` §7.3 line 99: "fall back to indexer set for reads only; do not publish to indexers." A failed Step 2 surfaces in the action ledger as `Failed { reason: NoAuthorRelays }`, which the UI renders as a toast per ADR-0007's `SideEffect` lane.
+docs/design/subscription-compilation/outbox.md:133:- **Step 3(b)'s `Indexer` check** is the structural enforcement of bug-extinction #4 (`docs/plan.md` line 306 — "DM to public: no API path can send a DM to a non-inbox relay"). Indexer-sourced inbox means we have no NIP-65-declared inbox; for private events that is fail-closed. The recipient gets nothing rather than getting a public broadcast.
+docs/design/subscription-compilation/outbox.md:138:The override exists for tests, migration tools, and operator power-user flows. Per `docs/aim.md` §6 doctrine 5 ("manual relay selection is the opt-out, not the default") and `docs/product-spec/subsystems.md` §7.3 line 90 ("explicit overrides are named, one-shot, and debug-flagged in logs"), the override must be:
+docs/design/subscription-compilation/outbox.md:191:The override action's existence is what test #2 in the bug-extinction list (`docs/plan.md` line 134) asserts: "no public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning." The `PublishWithOverride` variant is the *only* `AppAction` that carries a relay set; the audit string is required (compile-time non-optional); the warning fires unconditionally on dispatch.
+docs/design/subscription-compilation/outbox.md:212:The bug-extinction #7 test (`docs/plan.md` line 234) — "publish OK / store fail and store OK / publish fail both roll back atomically" — runs against the M6 implementation. The seam M2 lands here must make that test possible. Specifically:
+
+exec
+/bin/zsh -lc "nl -ba docs/aim.md | sed -n '219,238p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+   219	## 6. Doctrine — the rules the API must make hard to violate
+   220	
+   221	These rules are the framework's identity. They derive from the RMP bible and from the protocol-correctness lessons of the libraries we are synthesizing:
+   222	
+   223	1. **One event store per application.** Singleton enforced at the FFI boundary.
+   224	2. **All reads through the store.** No "fetch from relay, return to caller" API exists. Relay results land in the store; callers subscribe to the store.
+   225	3. **All writes through actions.** No "build event, sign, publish" sequence the developer assembles manually.
+   226	4. **Replaceable-event invariants enforced on insert.** Stale kind-0/3/10002/parameterized-replaceable events are impossible to retain.
+   227	5. **Outbox routing automatic.** Manual relay selection is the opt-out, not the default.
+   228	6. **Subscriptions auto-group, auto-close, auto-dedup, auto-buffer.** The developer never writes grouping/dedup/cleanup code.
+   229	7. **Sessions are state, switching is an action.** No imperative "log out, then log in, then reload" dance.
+   230	8. **No errors cross FFI.** All operational failure surfaces as state fields.
+   231	9. **No business logic in native code.** Enforced by docs, examples, and an architectural lint where feasible.
+   232	10. **Provenance preserved.** Every event in the store remembers which relays delivered it; private events cannot be accidentally republished to public relays.
+   233	11. **Capabilities, not callbacks.** Native↔Rust interactions go through bounded, idempotent capability bridges modeled exactly on the RMP bible's pattern.
+   234	12. **Snapshots by default, granular updates as optimization.** Start with `AppUpdate::FullState`; add granular `AppUpdate::*` variants only where profiling demands.
+   235	
+   236	---
+   237	
+   238	## 7. Open design questions (must resolve before substantive coding)
+
+exec
+/bin/zsh -lc "nl -ba docs/design/view-catalog/template-and-enumeration.md | sed -n '1,35p'" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc "nl -ba docs/design/kernel-substrate.md | sed -n '435,460p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# View Catalog: Template And Enumeration
+     2	
+     3	[Back to Design: View Catalog](../view-catalog.md)
+     4	
+     5	# Design: View Catalog
+     6	
+     7	> **Audience:** Framework contributors building reference Nostr protocol modules. Each view kind below is a `ViewModule` shipped in a reusable protocol crate such as `nmp-nip01`, `nmp-nip10`, `nmp-nip25`, `nmp-nip65`, or `nmp-nip17`.
+     8	
+     9	> **Status:** Rev 2, reframed per ADR-0009. These view kinds are not in `nmp-core`; apps consume them by adding the owning module crate to `nmp.toml` and regenerating the per-app FFI crate.
+    10	
+    11	> **Prerequisites:** `product-spec.md` §7.6, `reactivity.md`, `kernel-substrate.md` §3, ADR-0005, ADR-0010.
+    12	
+    13	---
+    14	
+    15	## 1. Per-view-kind template
+    16	
+    17	Every reference Nostr view module lives in a `nmp-nip*` crate and implements `ViewModule` from `nmp-core::substrate`:
+    18	
+    19	```
+    20	crates/nmp-<protocol>/src/views/<kind>.rs
+    21	```
+    22	
+    23	with this public shape:
+    24	
+    25	```rust
+    26	pub struct <Kind>Module;
+    27	
+    28	impl ViewModule for <Kind>Module {
+    29	    const NAMESPACE: &'static str = "nipXX.<kind>";
+    30	
+    31	    type Spec = <Kind>Spec;
+    32	    type Payload = <Kind>View;
+    33	    type Delta = <Kind>Delta;
+    34	    type Key = <Kind>Key;
+    35	    type State = <Kind>State;
+
+ succeeded in 0ms:
+   435	## 11. What goes in v1 vs later
+   436	
+   437	**v1 kernel substrate** (Phase 1a.1):
+   438	
+   439	- `DomainModule` trait + `DomainRegistry` + LMDB backing.
+   440	- `ViewModule` trait + view registry + reverse-index integration + delta buffer integration.
+   441	- `ActionModule` trait + durable ledger + restart recovery.
+   442	- `CapabilityModule` trait + bridge plumbing.
+   443	- `IdentityModule` trait + secure-store binding.
+   444	- `nmp gen modules` codegen with output for one fixture app.
+   445	
+   446	**v1 reference modules:**
+   447	
+   448	- `nmp-nip01`: Event types, Filter, Profile / Contacts / Timeline view modules, SendNote / DeleteEvent actions.
+   449	- `nmp-nip02`: Contacts module (re-exported for convenience; structure overlaps with nip01).
+   450	- `nmp-nip10`: Reply marker handling for thread building.
+   451	- `nmp-nip25`: Reactions view module + React action.
+   452	- `nmp-nip65`: Mailboxes view module + outbox routing helper.
+   453	- `nmp-nip77`: Sync engine (per spec §7.8, now packaged as a module).
+   454	- `nmp-blossom`: Blossom upload action + upload view module.
+   455	- `nmp-nip17`: Conversation view module + SendDm action + NSE crate.
+   456	
+   457	**v1 app modules:**
+   458	
+   459	- `twitter-core`: the demo app (compose UI state, settings).
+   460	- `fixture-todo-core`: the non-Nostr fixture module proving the boundary.
+
+exec
+/bin/zsh -lc "nl -ba docs/design/app-extension-kernel.md | sed -n '246,268p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+   246	## 5. What Stays Out Of NMP Core
+   247	
+   248	These concepts should not be added to `nmp-core`:
+   249	
+   250	- agent approvals,
+   251	- daily plans,
+   252	- weight cuts,
+   253	- podcast episode ingest,
+   254	- transcript chunking,
+   255	- highlighter capture policy,
+   256	- OCR selection rules,
+   257	- TENEX project/report semantics,
+   258	- app-specific friend/whitelist rules,
+   259	- coach prompts,
+   260	- provider/model settings beyond generic credential capability plumbing.
+   261	
+   262	If multiple apps repeat a pattern, extract the deterministic substrate, not the product noun. For example:
+   263	
+   264	- Extract a durable action ledger, not "agent approval."
+   265	- Extract media upload/draft primitives, not "highlight capture."
+   266	- Extract domain-store migrations/export/redaction, not "weight logs."
+   267	- Extract signer/session scopes, not "coach identity."
+   268	
+
+exec
+/bin/zsh -lc "nl -ba docs/design/ndk-applesauce-lessons.md | sed -n '107,118p'" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc "nl -ba docs/product-spec/subsystems.md | sed -n '1,40p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+   107	### 9.1 Outbox Is Policy, Not Transport
+   108	
+   109	Outbox support belongs in the actor-owned routing and planning layer. Relay sockets should not decide whether a relay is an inbox, outbox, indexer, fallback, or hint. They should connect, subscribe, publish, report status, and nothing more.
+   110	
+   111	### 9.2 Relay Metadata Is Durable Domain State
+   112	
+   113	Relay lists, relay hints, seen-relay provenance, relay health, and coverage records are part of the app's durable understanding of the Nostr network. They should survive restart and be available to diagnostics. They should not be treated as incidental cache entries.
+   114	
+   115	### 9.3 Developer APIs Should Express Intent
+   116	
+   117	The safe public surface should be phrased in product terms: open a timeline, open a profile, publish a reply, update my relay list, send a message. Relay choice is framework policy. Manual relay selection can exist for tests, diagnostics, migration tools, and advanced overrides, but it should be visibly outside the default path.
+   118	
+
+ succeeded in 0ms:
+     1	# Product Spec: Subsystems
+     2	
+     3	[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)
+     4	
+     5	## 7. Subsystem specifications
+     6	
+     7	### 7.1 EventStore
+     8	
+     9	Single instance per `FfiApp`, owned by the actor. Public to the framework (not to native).
+    10	
+    11	Behaviors guaranteed at insert time:
+    12	
+    13	| Concern | Behavior |
+    14	|---|---|
+    15	| Insert API | Every event enters through one actor-owned insert path returning `InsertOutcome`; no caller mutates indexes or storage directly. |
+    16	| Signature/delegation validity | Verified before any tombstone, provenance, replaceable-index, or durable-storage mutation. |
+    17	| Duplicate id | Merge relay provenance set; keep earliest `received_at`; do not overwrite. |
+    18	| Replaceable kinds (0, 3, 10000-19999) | Compare `(pubkey, kind)` against existing; keep newest `created_at`; tie-break by lexicographically smallest `id`. |
+    19	| Parameterized replaceable (30000-39999) | Compare `(pubkey, kind, d-tag)`; same supersession rule. |
+    20	| Kind 5 (delete) | After verification, scan referenced `e` and `a` tags and remove matching events authored by the deleter. Persisted as tombstone so later re-insertion is suppressed. Tombstone timestamp is the maximum delete timestamp observed for that target. |
+    21	| NIP-40 expiration | Schedule a tokio timer to remove the event at the expiration timestamp; on actor restart, scan and re-schedule. |
+    22	| NIP-26 delegation | Validate delegation tag at insert; reject malformed. |
+    23	| Ephemeral events | Delivered to live consumers but not durably stored. |
+    24	| Provenance | Every event records typed sidecar provenance: relay URL, first seen, last seen, source, and deterministic primary relay. |
+    25	| Query matching | Storage backends may return candidates; every result is re-run through the canonical matcher before it affects state or views. |
+    26	
+    27	Storage backend is configurable via `AppConfig.storage_backend` (LMDB or SQLite-style native backend, IndexedDB/OPFS strategy for web, final choice resolved before v1). The store wraps the Rust Nostr SDK protocol types, but NMP owns the application-kernel storage traits because the app kernel needs typed provenance, action ledger rows, relay metadata, domain records, and bounded-view indexes in addition to raw events.
+    28	
+    29	GC: a claim-based collector tracks `view_id → Vec<event_id>` references. View close drops claims. A periodic `prune()` removes events with zero claims that are also absent from declared "pinned" sets (sessions' contact-list events, sessions' relay-list events).
+    30	
+    31	**Sync watermarks.** The store maintains a per-`(filter_signature, relay_url)` table:
+    32	
+    33	```
+    34	watermarks {
+    35	  filter_sig: Hash,            // canonicalized filter
+    36	  relay_url: String,
+    37	  synced_up_to: u64,           // unix seconds; "we have everything matching this filter on this relay up to T"
+    38	  last_sync_method: SyncMethod, // Negentropy | ReqScan | Manual
+    39	  bytes_saved_vs_req: u64,     // cumulative, for diagnostics
+    40	  updated_at: u64,
+
+exec
+/bin/zsh -lc "nl -ba docs/plan.md | sed -n '118,140p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+   118	### M2 — Subscription compilation + outbox routing
+   119	
+   120	**Demo product:** Same iOS app as M1, but timeline subscriptions are routed per-author to those authors' write relays per NIP-65, not to the hardcoded primal/purplepag.es pair. Diagnostics screen visibly shows the per-relay fan-out and which authors each relay covers.
+   121	
+   122	**Scope.** The planner becomes a **subscription compilation stage** per the NDK/Applesauce lessons doc. Logical interests get compiled into per-relay plans; recompilation happens when relay metadata arrives late. NIP-65 routing is the default for both reads and writes. Provenance / NIP-65 / relay hints / user-configured relays are four distinct facts that inform each other but never collapse.
+   123	
+   124	**Subsystem deliverables.**
+   125	
+   126	- `nmp-nip65` protocol module: Mailboxes view module (parsed kind:10002); outbox routing as a planner subsystem; recompilation triggers (kind:10002 arrival, view open, relay reconnect).
+   127	- Planner refactored from "hardcoded relay set" to "compiler of logical interests → per-relay plans." See `docs/design/ndk-applesauce-lessons.md` §7 (subscription compilation lessons).
+   128	- Per-pubkey relay-list cache (durable, even before LMDB lands — keep it in-memory until M3, but the data model is correct).
+   129	- Indexer fallback when a pubkey's kind:10002 is unknown: opportunistic discovery from a configurable indexer relay set.
+   130	- Reverse-relay-coverage view for diagnostics: "this relay is serving N authors of our timeline."
+   131	
+   132	**Exit gate.**
+   133	
+   134	- Bug-extinction test #3 (publish to wrong relays): no public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning.
+   135	- Subscription compilation correctness: for a timeline of 1000 authors, the compiled plan opens REQs only against the union of those authors' write relays (de-duplicated). Test asserts on the wire frame count.
+   136	- Late-arriving kind:10002 triggers recompilation: an author whose mailbox was unknown gets re-routed once their kind:10002 lands, without the platform observing protocol churn.
+   137	- Distinct-source visibility: the diagnostics screen shows the four relay-fact lanes (NIP-65 / hint / provenance / user-configured) separately.
+   138	
+   139	**Runnable artifact.** iOS app with measurably different relay-fan-out behavior; demo screenshot in `docs/perf/m2/outbox-routing.md` showing per-relay coverage.
+   140	
+
+exec
+/bin/zsh -lc "nl -ba crates/nmp-core/src/kernel/nostr.rs | sed -n '90,130p'" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc "nl -ba crates/nmp-core/src/kernel/ingest.rs | sed -n '205,235p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+    90	    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
+    91	}
+    92	
+    93	pub(crate) fn is_hex_id(value: &str) -> bool {
+    94	    is_hex_pubkey(value)
+    95	}
+    96	
+    97	pub(super) fn parse_relay_list(created_at: u64, tags: &[Vec<String>]) -> AuthorRelayList {
+    98	    let mut list = AuthorRelayList {
+    99	        created_at,
+   100	        ..AuthorRelayList::default()
+   101	    };
+   102	    let mut seen = HashSet::new();
+   103	
+   104	    for tag in tags {
+   105	        if tag.first().map(String::as_str) != Some("r") {
+   106	            continue;
+   107	        }
+   108	        let Some(url) = tag.get(1).filter(|url| url.starts_with("wss://")) else {
+   109	            continue;
+   110	        };
+   111	        let marker = tag.get(2).map(String::as_str).unwrap_or("both");
+   112	        let key = format!("{url}:{marker}");
+   113	        if !seen.insert(key) {
+   114	            continue;
+   115	        }
+   116	        match marker {
+   117	            "read" => list.read_relays.push(url.clone()),
+   118	            "write" => list.write_relays.push(url.clone()),
+   119	            _ => list.both_relays.push(url.clone()),
+   120	        }
+   121	    }
+   122	
+   123	    list
+   124	}
+   125	
+   126	pub(super) fn event_references(event: &StoredEvent, event_id: &str) -> bool {
+   127	    event.tags.iter().any(|tag| {
+   128	        tag.first().map(String::as_str) == Some("e") && tag.get(1).is_some_and(|id| id == event_id)
+   129	    })
+   130	}
+
+ succeeded in 0ms:
+   205	        ));
+   206	        self.seed_contacts.insert(event.pubkey, follows);
+   207	    }
+   208	
+   209	    pub(super) fn ingest_relay_list(&mut self, event: NostrEvent) {
+   210	        let relay_list = parse_relay_list(event.created_at, &event.tags);
+   211	        if relay_list.read_relays.is_empty()
+   212	            && relay_list.write_relays.is_empty()
+   213	            && relay_list.both_relays.is_empty()
+   214	        {
+   215	            return;
+   216	        }
+   217	
+   218	        let should_replace = self
+   219	            .author_relay_lists
+   220	            .get(&event.pubkey)
+   221	            .map(|current| relay_list.created_at >= current.created_at)
+   222	            .unwrap_or(true);
+   223	        if should_replace {
+   224	            self.log(format!(
+   225	                "NIP-65 {} read={} write={} both={}",
+   226	                short_hex(&event.pubkey),
+   227	                relay_list.read_relays.len(),
+   228	                relay_list.write_relays.len(),
+   229	                relay_list.both_relays.len()
+   230	            ));
+   231	            self.author_relay_lists.insert(event.pubkey, relay_list);
+   232	        }
+   233	    }
+   234	
+   235	    pub(super) fn ingest_timeline_event(&mut self, sub_id: &str, event: NostrEvent) {
+
+exec
+/bin/zsh -lc "nl -ba crates/nmp-core/src/kernel/mod.rs | sed -n '260,318p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+   260	            last_event_at: None,
+   261	            last_notice: None,
+   262	            last_error: None,
+   263	            reconnect_count: 0,
+   264	            counters: Counters::default(),
+   265	        }
+   266	    }
+   267	}
+   268	
+   269	#[derive(Clone, Debug, Default)]
+   270	struct AuthorRelayList {
+   271	    created_at: u64,
+   272	    read_relays: Vec<String>,
+   273	    write_relays: Vec<String>,
+   274	    both_relays: Vec<String>,
+   275	}
+   276	
+   277	#[derive(Clone, Debug)]
+   278	struct ViewInterest {
+   279	    key: String,
+   280	    refcount: u32,
+   281	}
+   282	
+   283	pub(crate) struct Kernel {
+   284	    rev: u64,
+   285	    visible_limit: usize,
+   286	    started_at: Option<Instant>,
+   287	    last_event_at: Option<Instant>,
+   288	    first_event_at: Option<Instant>,
+   289	    target_profile_loaded_at: Option<Instant>,
+   290	    timeline_opened_at: Option<Instant>,
+   291	    timeline_first_item_at: Option<Instant>,
+   292	    relays: HashMap<RelayRole, RelayHealth>,
+   293	    profiles: HashMap<String, Profile>,
+   294	    events: HashMap<String, StoredEvent>,
+   295	    timeline: VecDeque<String>,
+   296	    selected_author: Option<ViewInterest>,
+   297	    author_request_pending: bool,
+   298	    author_view_seq: u64,
+   299	    selected_thread: Option<ViewInterest>,
+   300	    thread_request_pending: bool,
+   301	    thread_view_seq: u64,
+   302	    diagnostic_firehose: Option<ViewInterest>,
+   303	    diagnostic_firehose_seq: u64,
+   304	    diagnostic_firehose_events: u64,
+   305	    pending_thread_ids: BTreeSet<String>,
+   306	    requested_thread_ids: HashSet<String>,
+   307	    thread_ids_inflight: bool,
+   308	    pending_thread_reply_targets: BTreeSet<String>,
+   309	    requested_thread_reply_targets: HashSet<String>,
+   310	    thread_replies_inflight: bool,
+   311	    deferred_outbound: VecDeque<OutboundMessage>,
+   312	    seed_contacts: HashMap<String, Vec<String>>,
+   313	    author_relay_lists: HashMap<String, AuthorRelayList>,
+   314	    timeline_authors: BTreeSet<String>,
+   315	    profile_claims: HashMap<String, BTreeSet<String>>,
+   316	    requested_profiles: HashSet<String>,
+   317	    pending_profiles: BTreeSet<String>,
+   318	    profile_req_seq: u64,
+
+exec
+/bin/zsh -lc "nl -ba docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md | sed -n '70,120p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+    70	// AUTO-GENERATED — do not edit by hand. Regenerate with `nmp gen modules`.
+    71	
+    72	#[derive(Clone, uniffi::Enum)]
+    73	pub enum AppAction {
+    74	    // Kernel variants (from nmp-core)
+    75	    Kernel(nmp_core::KernelAction),
+    76	
+    77	    // Protocol module variants
+    78	    Nip01(nmp_nip01::Action),
+    79	    Nip02(nmp_nip02::Action),
+    80	    Nip25(nmp_nip25::Action),
+    81	    Nip10(nmp_nip10::Action),
+    82	
+    83	    // App module variants
+    84	    Twitter(twitter_core::Action),
+    85	}
+    86	
+    87	impl AppAction {
+    88	    pub fn dispatch(self, app: &Arc<FfiApp>) {
+    89	        match self {
+    90	            AppAction::Kernel(a) => app.core_tx.send(CoreMsg::Kernel(a)).ok(),
+    91	            AppAction::Nip01(a) => app.core_tx.send(CoreMsg::Module("nip01", a.into_bytes())).ok(),
+    92	            AppAction::Nip02(a) => app.core_tx.send(CoreMsg::Module("nip02", a.into_bytes())).ok(),
+    93	            // ...
+    94	        };
+    95	    }
+    96	}
+    97	```
+    98	
+    99	`AppUpdate`, `ViewSpec`, and the capability traits are generated analogously.
+   100	
+   101	### How the actor dispatches
+   102	
+   103	Inside `nmp-core`'s actor, message handling is generic over module identifier:
+   104	
+   105	```rust
+   106	enum CoreMsg {
+   107	    Kernel(KernelAction),
+   108	    Module(&'static str, Vec<u8>),    // module namespace + serialized action
+   109	    Internal(InternalEvent),
+   110	}
+   111	
+   112	fn handle_message(&mut self, msg: CoreMsg) {
+   113	    match msg {
+   114	        CoreMsg::Kernel(a) => self.handle_kernel_action(a),
+   115	        CoreMsg::Module(ns, bytes) => {
+   116	            let handler = self.module_registry.get(ns).expect("module not registered");
+   117	            handler.dispatch(self, &bytes);
+   118	        }
+   119	        CoreMsg::Internal(e) => self.handle_internal(e),
+   120	    }
+
+exec
+/bin/zsh -lc "nl -ba docs/design/kernel-substrate.md | sed -n '180,240p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+   180	## 4. `ActionModule` — durable workflows on the action ledger
+   181	
+   182	Replaces the closed `AppAction` enum. Every user intent — `SendNote`, `React`, `Repost`, `CreateHighlight`, `UploadBlob`, `RunSync` — is an `ActionModule`.
+   183	
+   184	```rust
+   185	pub trait ActionModule: Send + Sync + 'static {
+   186	    const NAMESPACE: &'static str;
+   187	
+   188	    type Action: Clone + Serialize + DeserializeOwned + Send + 'static;
+   189	    type Step: Clone + Serialize + DeserializeOwned + Send + 'static;
+   190	    type Output: Clone + Serialize + Send + 'static;
+   191	
+   192	    /// Validate + plan the action. Returns initial step state or rejection.
+   193	    fn start(ctx: &mut ActionContext, action: Self::Action)
+   194	        -> Result<ActionPlan<Self::Step>, ActionRejection>;
+   195	
+   196	    /// Drive the state machine. Input could be a capability result, a relay OK,
+   197	    /// a timeout, a user approval, etc.
+   198	    fn reduce(ctx: &mut ActionContext, id: ActionId, input: ActionInput<Self::Step>)
+   199	        -> ActionTransition<Self::Step, Self::Output>;
+   200	}
+   201	
+   202	pub struct ActionPlan<Step> {
+   203	    pub initial_step: Step,
+   204	    pub initial_status: ActionStatus,
+   205	    pub deadline_ms: Option<u64>,
+   206	}
+   207	
+   208	pub enum ActionTransition<Step, Output> {
+   209	    Continue { step: Step, status: ActionStatus },
+   210	    Complete { output: Output },
+   211	    Fail { reason: String, transient: bool },
+   212	    AwaitCapability { request: CapabilityRequest, next_step: Step },
+   213	    AwaitUserApproval { prompt: ApprovalPrompt, next_step: Step },
+   214	}
+   215	```
+   216	
+   217	**What the kernel owns:**
+   218	
+   219	- Durable ledger rows (`actions` table in the storage backend).
+   220	- Action IDs (ULID).
+   221	- Status transitions: `Pending → Running → Completed | Failed | Cancelled`.
+   222	- Retries with exponential backoff (for transient failures).
+   223	- Cancellation correlation.
+   224	- Provenance (which relays it published to, when, with what response).
+   225	- Capability request/response correlation (the action awaits a `CapabilityResult`).
+   226	- Restart recovery (actor restart re-loads in-flight actions; modules' `reduce` is called with `ActionInput::ResumedAfterRestart`).
+   227	- Diagnostic rendering for ADR-0007.
+   228	
+   229	**What the module owns:**
+   230	
+   231	- Action types (e.g. `SendNote { content, reply_to }`, `React { target, emoji }`).
+   232	- Step machine (validate → sign → publish → confirm).
+   233	- Validation rules.
+   234	- Final domain writes (insert the signed event into the store, append to a domain table, etc.).
+   235	- Whether user approval is required (for sensitive actions).
+   236	
+   237	**Atomicity** (doctrine guarantee): the kernel ensures `module.reduce(...)`, ledger transitions, and local store writes happen as one actor message. External effects such as relay publishes cannot be rolled back after a relay accepts them, so publish steps must be ledger-correlated and restart-recoverable: a "publish accepted but local insert failed" path becomes an explicit failed/recovery ledger state, not silent divergence.
+   238	
+   239	**Example:**
+   240	
+
+codex
+I would not merge this as-is. Main blockers:
+
+- [outbox.md:54](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/outbox.md:54) and [outbox.md:106](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/outbox.md:106): public publish says it can fall back to the indexer path, and the algorithm calls `resolve_author_outbox(..., indexer, ...)`. That violates “do not publish to indexers.” Fix by splitting read fallback from publish resolution, or reject `RoutingSource::Indexer` for all publish modes.
+
+- [outbox.md:103](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/outbox.md:103): override returns before privacy validation, conflicting with [outbox.md:138](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/outbox.md:138). Fix by deriving the allowed base set first, then validate override as a subset for private routes.
+
+- [nip65.md:192](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:192) plus [nip65.md:197](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:197): `nmp-nip65` depends on `nmp-core`, while `nmp-core::kernel::planner` consumes `nmp-nip65`. That is a Rust crate cycle and blurs D0. Fix by moving mailbox traits/types into `nmp-core` or a tiny no-core crate, with `nmp-nip65` only registering module behavior.
+
+- [nip65.md:6](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:6), [nip65.md:98](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:98), [compiler.md:52](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:52): mailbox facts have too many apparent writers/read models: `ingest_relay_list`, `MailboxCache`, and `MailboxesView::on_event_inserted`. D4 wants one writer per fact. Fix by making one canonical mailbox projection/cache writer; views only read that projection.
+
+- [compiler.md:140](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:140) conflicts with [recompilation.md:95](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/recompilation.md:95): `plan_id` hashes the whole mailbox snapshot, so unrelated kind:10002 arrivals churn global plan ids. Fix by hashing only mailboxes referenced by the current interest set, or separate logical plan id from relay-assignment id.
+
+- [diagnostics.md:61](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/diagnostics.md:61) and [diagnostics.md:172](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/diagnostics.md:172): `HintRelayFact` has no author/subject key, and the coverage reducer does not consume hint or user-configured fact streams, yet [tests.md:202](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:202) asserts those counts. Add subject identity to hint facts and make the view consume all four lanes.
+
+- [diagnostics.md:15](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/diagnostics.md:15), [diagnostics.md:116](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/diagnostics.md:116), [diagnostics.md:157](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/diagnostics.md:157): indexer fallback is sometimes a fifth source and sometimes user-configured. Fix by modeling it as `UserConfigured { category: Indexer }`, with any indexer count as a subcategory, not a new lane.
+
+- [compiler.md:166](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:166) and [compiler.md:184](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:184): startup timeline/profile/contact behavior remains framed as `nmp-core` migration work. D0 fix: move social bootstrap modules to `nmp-nip01` / `nmp-nip02` / demo app code; core should only execute registered module interests.
+
+- [outbox.md:119](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/outbox.md:119): private publish uses a union of recipient inbox relays plus `required_success_count = recipients.len() as u8`; this neither proves one success per recipient nor avoids overflow. Fix with per-recipient delivery requirements and `usize`.
+
+Other checks: no literal `TODO`, `FIXME`, or `unimplemented!` in the added docs. File sizes are compliant: all new files are under 300 LOC, largest is `tests.md` at 273. The planned audit test still has weak spots: [tests.md:34](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:34) depends on non-existent `AppActionMeta`, and [tests.md:213](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:213) does not actually assert lane distinctness.
+tokens used
+128,648
+I would not merge this as-is. Main blockers:
+
+- [outbox.md:54](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/outbox.md:54) and [outbox.md:106](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/outbox.md:106): public publish says it can fall back to the indexer path, and the algorithm calls `resolve_author_outbox(..., indexer, ...)`. That violates “do not publish to indexers.” Fix by splitting read fallback from publish resolution, or reject `RoutingSource::Indexer` for all publish modes.
+
+- [outbox.md:103](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/outbox.md:103): override returns before privacy validation, conflicting with [outbox.md:138](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/outbox.md:138). Fix by deriving the allowed base set first, then validate override as a subset for private routes.
+
+- [nip65.md:192](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:192) plus [nip65.md:197](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:197): `nmp-nip65` depends on `nmp-core`, while `nmp-core::kernel::planner` consumes `nmp-nip65`. That is a Rust crate cycle and blurs D0. Fix by moving mailbox traits/types into `nmp-core` or a tiny no-core crate, with `nmp-nip65` only registering module behavior.
+
+- [nip65.md:6](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:6), [nip65.md:98](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:98), [compiler.md:52](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:52): mailbox facts have too many apparent writers/read models: `ingest_relay_list`, `MailboxCache`, and `MailboxesView::on_event_inserted`. D4 wants one writer per fact. Fix by making one canonical mailbox projection/cache writer; views only read that projection.
+
+- [compiler.md:140](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:140) conflicts with [recompilation.md:95](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/recompilation.md:95): `plan_id` hashes the whole mailbox snapshot, so unrelated kind:10002 arrivals churn global plan ids. Fix by hashing only mailboxes referenced by the current interest set, or separate logical plan id from relay-assignment id.
+
+- [diagnostics.md:61](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/diagnostics.md:61) and [diagnostics.md:172](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/diagnostics.md:172): `HintRelayFact` has no author/subject key, and the coverage reducer does not consume hint or user-configured fact streams, yet [tests.md:202](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:202) asserts those counts. Add subject identity to hint facts and make the view consume all four lanes.
+
+- [diagnostics.md:15](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/diagnostics.md:15), [diagnostics.md:116](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/diagnostics.md:116), [diagnostics.md:157](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/diagnostics.md:157): indexer fallback is sometimes a fifth source and sometimes user-configured. Fix by modeling it as `UserConfigured { category: Indexer }`, with any indexer count as a subcategory, not a new lane.
+
+- [compiler.md:166](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:166) and [compiler.md:184](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:184): startup timeline/profile/contact behavior remains framed as `nmp-core` migration work. D0 fix: move social bootstrap modules to `nmp-nip01` / `nmp-nip02` / demo app code; core should only execute registered module interests.
+
+- [outbox.md:119](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/outbox.md:119): private publish uses a union of recipient inbox relays plus `required_success_count = recipients.len() as u8`; this neither proves one success per recipient nor avoids overflow. Fix with per-recipient delivery requirements and `usize`.
+
+Other checks: no literal `TODO`, `FIXME`, or `unimplemented!` in the added docs. File sizes are compliant: all new files are under 300 LOC, largest is `tests.md` at 273. The planned audit test still has weak spots: [tests.md:34](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:34) depends on non-existent `AppActionMeta`, and [tests.md:213](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:213) does not actually assert lane distinctness.
diff --git a/docs/perf/codex-reviews/9fead0e.md b/docs/perf/codex-reviews/9fead0e.md
new file mode 100644
index 0000000..2ff38ec
--- /dev/null
+++ b/docs/perf/codex-reviews/9fead0e.md
@@ -0,0 +1,4443 @@
+Reading additional input from stdin...
+2026-05-17T22:45:39.677433Z ERROR codex_core::session: failed to load skill /Users/pablofernandez/.agents/skills/voice-capture-sheet/SKILL.md: invalid YAML: mapping values are not allowed in this context at line 2 column 116
+OpenAI Codex v0.129.0 (research preview)
+--------
+workdir: /Users/pablofernandez/Work/nostr-multi-platform
+model: gpt-5.5
+provider: openai
+approval: never
+sandbox: workspace-write [workdir, /tmp, $TMPDIR, /Users/pablofernandez/.codex/memories]
+reasoning effort: xhigh
+reasoning summaries: none
+session id: 019e381d-d4df-7121-8616-c362ec3bdeae
+--------
+user
+You are reviewing merge 9fead0e (M3 LMDB schema + EventStore trait + GC policy design) on master in nostr-multi-platform. Doctrine D0-D5. File size: 300 LOC soft, 500 hard.
+
+M3 goal (per docs/plan.md §M3): swap in-memory EventStore for LMDB; implement full insert invariants (replaceable kinds, kind:5 delete, NIP-40 expiration, dedup w/ provenance merge); claim-based GC; watermark table populated in M4.
+
+Merge:
+
+=== M3 design merge: 9fead0e ===
+ docs/design/lmdb-schema.md     | 234 +++++++++++++++++++++++++++++++
+ docs/design/lmdb/gc.md         | 195 ++++++++++++++++++++++++++
+ docs/design/lmdb/keys.md       | 150 ++++++++++++++++++++
+ docs/design/lmdb/tests.md      | 223 +++++++++++++++++++++++++++++
+ docs/design/lmdb/trait.md      | 312 +++++++++++++++++++++++++++++++++++++++++
+ docs/design/lmdb/watermarks.md | 191 +++++++++++++++++++++++++
+ 6 files changed, 1305 insertions(+)
+
+9fead0e design(m3): LMDB schema + EventStore trait + GC policy
+Adds the M3 design: docs/design/lmdb-schema.md (master decision +
+perf budget + open questions) and split sub-docs under
+docs/design/lmdb/ for the trait surface, key encodings, GC policy,
+watermarks/provenance/migrations, and the test plan mapping every
+§7.1 insert invariant to a concrete test file.
+
+Decision: adopt nostr-lmdb as the on-disk byte store, wrap behind a
+NMP-owned EventStore trait, add NMP sub-databases for the rows
+nostr-lmdb does not model (claim-pinning, watermarks, projection
+caches, domain-module rows, secondary indexes for kernel-side queries).
+
+Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
+
+---
+
+diff --git a/docs/design/lmdb-schema.md b/docs/design/lmdb-schema.md
+new file mode 100644
+index 0000000..6b84216
+--- /dev/null
++++ b/docs/design/lmdb-schema.md
+@@ -0,0 +1,234 @@
++# Design: LMDB schema + EventStore trait + GC policy (M3)
++
++> **Audience:** kernel implementers landing M3 (persistence).
++> **Status:** rev 0 — proposed; opens ADR slot for any open-question resolution.
++> **Companion docs:** [`lmdb/trait.md`](lmdb/trait.md), [`lmdb/keys.md`](lmdb/keys.md), [`lmdb/gc.md`](lmdb/gc.md), [`lmdb/watermarks.md`](lmdb/watermarks.md), [`lmdb/tests.md`](lmdb/tests.md).
++> **Prerequisites:** `docs/product-spec/subsystems.md` §7.1 (insert invariants), `docs/decisions/0003-working-set-memory.md` (GC policy intent), `docs/decisions/0009-app-extension-kernel-boundary.md` (DomainModule storage), `docs/design/kernel-substrate.md` §2 (DomainModule trait).
++> **Plan reference:** [`docs/plan.md`](../plan.md) §M3.
++
++---
++
++## 1. Decision: wrap `nostr-lmdb` behind our own `EventStore` trait
++
++**Adopt `nostr-lmdb` as the on-disk byte store. Wrap it behind the NMP `EventStore` trait. Add NMP-owned LMDB sub-databases for the rows `nostr-lmdb` does not model.**
++
++The competing options were (1) use `nostr-lmdb` directly via its concrete `NostrLMDB` type (or via `nostr_database::NostrEventsDatabase`), (2) wrap behind our own trait, or (3) hand-roll an LMDB layer.
++
++**`nostr-lmdb` gives us** (per `docs.rs/nostr-lmdb`): `save_event(&Event)`, `event_by_id(&EventId)`, `check_id(&EventId) -> DatabaseEventStatus`, `query(Filter) -> Events`, `count(Filter)`, `delete(Filter)`, `wipe()`, `negentropy_items(Filter) -> Vec<(EventId, Timestamp)>`. It owns the page allocator, the LMDB environment, primary by-id store, indexes derived from `Filter`, replaceable / parameterized-replaceable supersession, and NIP-09 delete handling. It is the only mature Rust LMDB store for Nostr events with proven NIP-77 integration; reinventing it is a year of work that we will not recoup.
++
++**What `nostr-lmdb` does *not* model** (the gap that justifies a wrapper):
++
++| Concern | Why `nostr-lmdb` doesn't cover it | Where NMP needs it |
++|---|---|---|
++| Per-relay provenance (which relays delivered each event; first seen / last seen) | Out of scope; the crate models events, not their wire history | `subsystems.md` §7.1 "Duplicate id → merge relay provenance set"; ADR-0007 diagnostics; outbox routing scoring in M2+ |
++| Sync watermarks `(filter_hash, relay) → synced_up_to` | Out of scope; the crate does not know about logical filters or relay identity | `subsystems.md` §7.1 + §7.8; M4 NIP-77 engine needs them to be authoritative |
++| Claim register / release for view-driven GC | Out of scope; the crate has no concept of an "open view" | ADR-0003; current in-memory analogue in `kernel/mod.rs:315` `profile_claims: HashMap<String, BTreeSet<String>>` |
++| Working-set hot/cold split with eviction LRU | LMDB is OS-paged; the crate trusts the kernel page cache | ADR-0003 numeric gate (≤ 100 MB at 100 views / 10k hot) |
++| Kernel-side secondary indexes for query shapes our planner uses (e.g. `(p-tag, timestamp)` desc scan, `(e-tag, timestamp)` desc scan, `(expires_at, event_id)` for NIP-40 wakeups) | The `Filter` API recomputes per call; not optimal for our planner's repeat shapes | Planner cache-coverage queries (§7.2); NIP-40 expiration scheduling (§7.1) |
++| Tombstone-as-row that survives independent of the deleted event | The crate suppresses re-insert via its own delete index; we want it exposed for export / restoring across re-syncs | `subsystems.md` §7.1 kind:5 row "persisted as tombstone so later re-insertion is suppressed" |
++| `DomainModule` rows (drafts, settings, action ledger, projection caches) | Entirely out of scope; the crate stores Nostr `Event` only | ADR-0009 + `kernel-substrate.md` §2 — kernel hosts non-Nostr typed rows |
++| Migrations versioned per namespace | Out of scope | `kernel-substrate.md` §2: `DomainModule::migrations() -> Vec<DomainMigration>` |
++| `nmp dump` deterministic export | Out of scope | M3 exit criteria; M11 cross-app proof |
++
++**Therefore.** `EventStore` is a NMP-owned trait, with one production impl `LmdbEventStore` that holds (a) a `NostrLMDB` for the canonical event store and Nostr-shaped queries, and (b) NMP-owned secondary LMDB sub-databases under the same `lmdb::Environment` for the gap rows. The in-memory backend (`MemEventStore`) remains, both for tests and as the web-pre-M15 fallback. See [`lmdb/trait.md`](lmdb/trait.md) for the exact trait shape and the relayed-vs-owned method split.
++
++**Rejected alternatives.**
++
++- *Use `NostrLMDB` directly, no wrapper.* Loses every gap row above. Forces the kernel actor to know about LMDB transactions and a non-NMP concrete type, breaking the `Box<dyn EventStore>` substitutability M3 requires.
++- *Roll our own.* Reinvents NIP-09 / replaceable handling that `nostr-lmdb` already gets right. ~2,000 LOC of avoidable code with a worse bug surface than upstream.
++- *SQLite-backed `nostr-sdk` store.* Larger memory footprint at our 10k-event hot working set; iOS-disk-WAL fsync cost not justified for this access pattern. Held in reserve for the web port (M15) if IndexedDB OPFS proves unworkable.
++
++## 2. Subsystem ownership map
++
++```
++crates/nmp-core/src/store/
++  mod.rs                — trait re-exports + factory
++  trait.rs              — `EventStore` (see lmdb/trait.md)
++  mem.rs                — in-memory backend (preserved from kernel/mod.rs current state)
++  lmdb/
++    mod.rs              — `LmdbEventStore` orchestrator
++    env.rs              — `lmdb::Environment` + sub-db handles + open()
++    events.rs           — wraps `nostr_lmdb::NostrLMDB`; primary-by-id, replaceable rules, kind:5 handling
++    secondary.rs        — NMP-owned secondary indexes (see lmdb/keys.md §3)
++    provenance.rs       — provenance sub-db (see lmdb/watermarks.md §2)
++    watermarks.rs       — watermark sub-db (see lmdb/watermarks.md §1)
++    claims.rs           — claim register / release + hot-set hints (see lmdb/gc.md §2)
++    gc.rs               — `gc_step()` algorithm (see lmdb/gc.md §3)
++    domain.rs           — per-DomainModule sub-db namespacing + migration runner
++    dump.rs             — `nmp dump` (see §9 below)
++```
++
++Each file is bounded ≤ 300 LOC per AGENTS.md. The trait module is read by the actor; backend modules are read only by the orchestrator.
++
++## 3. EventStore trait
++
++See [`lmdb/trait.md`](lmdb/trait.md) for the exact `pub trait EventStore` signature with all required methods, return types, and the `StoreError` enum. Summary:
++
++- **Reads:** `get_by_id`, `scan_by_author_kind`, `scan_by_kind_dtag`, `scan_by_etag`, `scan_by_ptag`, `scan_by_kind_time`, `scan_expiring_before`. All `scan_*` methods return a streaming `EventIter` so the planner pages without materialising. Cache-coverage queries take a `WatermarkKey` and answer authoritatively.
++- **Writes:** `insert(event, RelayUrl)` returns `InsertOutcome` matching §7.1's table. `delete_by_filter` for foreign-relay cleanups. `tombstones_for` for replay.
++- **Watermarks / sync:** `read_watermark`, `write_watermark`, `list_watermarks_for_relay`.
++- **GC:** `claim(ClaimerId, &[EventId])`, `release(ClaimerId)`, `hot_set_hint(&[EventId])`, `gc_step(GcBudget) -> GcReport`.
++- **Domain rows:** `domain_open(namespace) -> DomainHandle` returns a typed handle; `DomainHandle::put/get/scan_index` is the per-DomainModule API.
++- **Migration:** `run_migrations(&[DomainMigration])` runs at startup, transactional per migration.
++- **Export:** `dump(out: &mut dyn Write, format: DumpFormat) -> Result<DumpStats>`.
++
++**Error semantics.** All methods return `Result<T, StoreError>`. Per doctrine D3, store errors do **not** cross FFI — the actor maps every variant to either (a) a typed `Effect` (e.g. `StoreCorrupt`, surfaces via diagnostics + toast), (b) a `tracing::warn!` log + degraded state, or (c) a panic at startup if the LMDB environment refuses to open. The trait itself uses `Result` since it is internal to the actor process.
++
++## 4. Key encoding
++
++Full byte layout for primary + every secondary in [`lmdb/keys.md`](lmdb/keys.md). At a glance:
++
++- Primary `events`: `event_id[32]` → `Event` (CBOR via `nostr` crate's serialization). Owned by `nostr-lmdb`.
++- Secondary `idx_author_kind`: `pubkey[32] || kind_be[4] || created_at_be[8] || event_id[32]` → empty. NMP-owned.
++- Secondary `idx_kind_dtag`: `kind_be[4] || dtag_len_be[2] || dtag_bytes || pubkey[32]` → `event_id[32]`. NMP-owned. Parameterized replaceable address lookup.
++- Secondary `idx_etag_time`, `idx_ptag_time`: `tag_value[32] || created_at_desc_be[8] || event_id[32]` → empty. NMP-owned. `created_at_desc = u64::MAX - created_at` so a forward LMDB scan is newest-first.
++- Secondary `idx_kind_time`: `kind_be[4] || created_at_desc_be[8] || event_id[32]` → empty.
++- Secondary `idx_expires`: `expires_at_be[8] || event_id[32]` → empty. Scanned by the NIP-40 reaper.
++- `tombstones`: `target_id[32]` → `TombstoneRow { kind5_event_id, deleter_pubkey, deleted_at, sources: Vec<RelayUrl> }` (CBOR).
++
++`created_at_be` is big-endian so byte order matches numeric order; `created_at_desc_be = u64::MAX - created_at` then big-endian for newest-first scans without `MDB_LAST + MDB_PREV`.
++
++All secondaries are maintained inside the same `RwTxn` as the primary write — atomicity is achieved by LMDB transactionality, not by post-hoc reconciliation.
++
++## 5. Watermark table
++
++See [`lmdb/watermarks.md`](lmdb/watermarks.md) for full layout. Row shape (CBOR):
++
++```rust
++struct WatermarkRow {
++  filter_hash: [u8; 32],     // BLAKE3 of canonicalised filter (see watermarks.md §3)
++  relay_url: String,
++  synced_up_to: u64,         // unix seconds
++  last_sync_method: SyncMethod, // Negentropy | ReqScan | Manual
++  last_negentropy_state: Option<Vec<u8>>, // engine-opaque resume blob
++  bytes_saved_vs_req: u64,
++  updated_at: u64,
++}
++```
++
++Key: `filter_hash[32] || relay_url_bytes` (no length prefix needed — relay URL is the variable suffix; lookup uses exact key). Populated by M4 (NIP-77) and consulted by M2's planner (cache-coverage check before issuing backfill REQ). Survives restarts; loaded into the actor on startup as a `HashMap<(filter_hash, relay_url), WatermarkRow>` for hot lookups, with all writes going through `EventStore` for durability.
++
++## 6. Migration plumbing
++
++See [`lmdb/watermarks.md`](lmdb/watermarks.md) §4. A `DomainModule` (per `kernel-substrate.md` §2) declares `const NAMESPACE: &'static str` and `const SCHEMA_VERSION: u32` plus `fn migrations() -> Vec<DomainMigration>`. The store assigns one LMDB sub-database per `(namespace, "data")`, plus one per `(namespace, index_name)` for each declared index. A `_meta` sub-database tracks `(namespace, current_version)`.
++
++The current `ModuleRegistry` (`crates/nmp-core/src/substrate/mod.rs:41`) discards the concrete `M: DomainModule` type after `register_domain::<M>()` returns — only the `ModuleDescriptor` is retained. The store cannot get from a namespace string back to `M::SCHEMA_VERSION` or `M::migrations()` at runtime. M3 adds a `DomainFactories { schema_version: fn() -> u32, migrations: fn() -> Vec<DomainMigration>, indexes: fn() -> Vec<DomainIndex> }` struct attached per descriptor, populated by capturing the `M::*` consts and fns in `fn`-pointer closures at register time. This matches the existing `key_fn: fn(&[u8]) -> Option<Vec<u8>>` pattern in `DomainIndex` (`crates/nmp-core/src/substrate/domain.rs:18`) — no `Box<dyn DomainModule>` and no new trait object-safety constraints on `DomainModule`. The change is additive to the substrate module surface. See [`lmdb/watermarks.md`](lmdb/watermarks.md) §4.1 for the registry-side code shape.
++
++On startup:
++
++1. For every registered `DomainModule`, read its row from `_meta`.
++2. If absent, treat current as 0 and run all migrations from 0 to `SCHEMA_VERSION` in one `RwTxn` per step.
++3. If present and less than `SCHEMA_VERSION`, run the missing steps.
++4. If greater, refuse to start (downgrade not supported); surface as `Effect::DomainSchemaTooNew { namespace }`.
++
++Each `DomainMigration::apply` receives a `MigrationTx` with put/get/delete + index rebuild helpers. Rollback semantics: each migration step is its own LMDB write transaction; failure aborts the step cleanly. If migration N succeeds and N+1 fails, the store stays at version N — the actor refuses to start the affected module and the rest of the kernel runs in degraded mode (the module's actions return `ActionRejection::ModuleUnavailable`).
++
++## 7. GC working-set policy
++
++See [`lmdb/gc.md`](lmdb/gc.md) for the eviction algorithm. Formal statement (matches ADR-0003):
++
++```
++hot_resident = {e | e is in claim_pinned}
++             ∪ {e | e is in open_view_cover}
++             ∪ {e | e is among the ≤10k most-recently-touched events}
++
++cold = stored_events \ hot_resident
++```
++
++`hot_resident` lives in a `lru::LruCache<EventId, Arc<Event>>` capped at the configured hot ceiling (default 10,000) plus an unbounded pinned overlay holding events with non-zero claim count. `cold` lives only on disk; lookup pays one LMDB `get` (memory-mapped — typically already in OS page cache for recently-evicted items).
++
++**Eviction algorithm.** On any insert that pushes the LRU over its ceiling, the oldest non-pinned entry is dropped. `gc_step()` is called periodically by the actor (default every 60 s and on memory pressure callbacks from `MemoryWarningCapability`): it (a) reaps NIP-40 expired events using `idx_expires`, (b) trims the LRU to `target_hot_size`, (c) deletes tombstones older than `tombstone_retention` (default 90 days) whose target event is absent from the store, (d) returns a `GcReport` for diagnostics.
++
++ADR-0003's numbers are preserved as the M3 exit gate (§11 below): ≤ 100 MB working-set at 100 active views / 10k hot events / 1M cached on disk.
++
++## 8. Replaceable + tombstone semantics
++
++The `insert()` path implements exactly the §7.1 invariants:
++
++- **Replaceable (kinds 0, 3, 10000–19999).** Look up the existing event for `(pubkey, kind)` in `idx_author_kind` (most recent suffix). If incoming `created_at` is newer, replace; if equal, keep lexicographically smallest `id`; else drop. Replacement deletes the old primary row and all secondary entries in the same `RwTxn`.
++- **Parameterized replaceable (30000–39999).** Same algorithm keyed on `(pubkey, kind, d-tag)` via `idx_kind_dtag` (which holds `event_id` as value so we don't need a separate `idx_author_kind_dtag`; the dtag prefix is unique per author by Nostr semantics — see [`lmdb/keys.md`](lmdb/keys.md) §3.2 for the per-author scoping note).
++- **Kind:5 self-delete.** Verify signature, scan referenced `e` and `a` tags, for each target `e_id` that is authored by the deleter or whose `a` address matches `(deleter_pubkey, kind, d-tag)`: delete the primary + all secondaries + write the tombstone row. Tombstone timestamp = `max(existing.deleted_at, kind5.created_at)`. Re-insert of the deleted event id is suppressed at insert time by a `tombstones.contains(event_id)` check.
++- **Foreign kind:5.** A kind:5 referencing events not authored by the kind:5's `pubkey` is ignored (per spec) — the event is *still stored* as a kind:5 (so other clients can render it / dedup it), but it has no side effect on the targets. The tombstone row is **not** written.
++- **NIP-40 expiration.** On insert, parse `expiration` tag; if present, write `idx_expires`. On `gc_step()`, scan `idx_expires` for keys with `expires_at_be ≤ now`, delete them like kind:5 (full primary + secondaries + tombstone marker noting `kind: Expired`).
++
++The tombstone schema is in [`lmdb/keys.md`](lmdb/keys.md) §4.
++
++## 9. Provenance: per-row sidecar sub-database
++
++**Decision: separate `provenance` sub-database keyed by `event_id[32]`.** Value is CBOR `ProvenanceRow { sources: Vec<ProvenanceEntry> }` where `ProvenanceEntry = { relay_url, first_seen_ms, last_seen_ms, primary: bool }`.
++
++Rejected: stuffing provenance into the `Event` row. That requires re-serializing the full `Event` on every relay redelivery (high write amplification — popular events arrive 5–20× from the relay fan-out) and forks the `nostr-lmdb` row format, which we explicitly want to keep upstream-compatible. The sidecar is appended cheaply with a single CBOR re-encode of the (typically small) `sources` vector.
++
++On duplicate-id insert (§7.1 row 2), `insert()` does not touch the primary; it only updates the provenance sidecar (`last_seen_ms` bump on the matching `ProvenanceEntry`, or append). The "primary relay" — for outbox-routing scoring (M2) and ADR-0007 diagnostics — is deterministically the first relay observed (`sources[0]` after sort by `first_seen_ms`).
++
++The export format (§ next) includes the provenance row alongside each event so a `nmp dump` round-trip restores it.
++
++## 10. Backup / export format
++
++`nmp dump` writes line-delimited JSON to stdout (or a file). Each line is a single tagged record:
++
++```json
++{"type":"event","event": {...nostr event...},"provenance":[{"relay_url":"wss://relay.primal.net","first_seen_ms":1747000000000,"last_seen_ms":1747001234567,"primary":true}]}
++{"type":"tombstone","target_id":"abc...","kind5_event_id":"def...","deleter_pubkey":"...","deleted_at":1747000000,"sources":["wss://..."]}
++{"type":"watermark","filter_hash":"hex32","relay_url":"wss://...","synced_up_to":1747000000,"last_sync_method":"Negentropy","bytes_saved_vs_req":12345,"updated_at":1747000123}
++{"type":"domain","namespace":"twitter.drafts","schema_version":1,"key_hex":"...","value_b64":"..."}
++```
++
++JSONL is the chosen format because (a) it streams (no holding the full dump in memory; cold-events page in as scanned), (b) it diffs cleanly (one record per line), (c) any line is independently parsable for partial recovery, (d) `jq` works out of the box. Binary CBOR is faster but loses ad-hoc inspectability — JSONL is the right tradeoff for an export format.
++
++`nmp restore` is symmetric: read JSONL, replay through `insert()` for events (so all secondaries are rebuilt from scratch — provenance is restored separately by writing the sidecar row directly after each event), `write_watermark` for watermarks, `DomainHandle::put` for domain rows. Restore is idempotent: replaying the same dump twice produces the same store.
++
++## 11. Test plan
++
++See [`lmdb/tests.md`](lmdb/tests.md) for the full mapping of every spec §7.1 invariant to a concrete test file under `crates/nmp-testing/tests/`. Highlights:
++
++| Invariant (§7.1) | Test file |
++|---|---|
++| Insert API single path | `crates/nmp-testing/tests/store_insert_path.rs` |
++| Signature verification before persist | `crates/nmp-testing/tests/store_invalid_sig.rs` |
++| Duplicate id → merge provenance, keep earliest received_at | `crates/nmp-testing/tests/store_provenance_merge.rs` |
++| Replaceable supersession | `crates/nmp-testing/tests/store_replaceable.rs` |
++| Parameterized replaceable supersession | `crates/nmp-testing/tests/store_param_replaceable.rs` |
++| Kind:5 self-delete persists as tombstone | `crates/nmp-testing/tests/store_kind5_tombstone.rs` |
++| Foreign kind:5 ignored | `crates/nmp-testing/tests/store_kind5_foreign.rs` |
++| NIP-40 expiration scheduled + reaped | `crates/nmp-testing/tests/store_nip40_expiration.rs` |
++| Watermarks survive restart, authoritative cache-miss | `crates/nmp-testing/tests/store_watermarks.rs` |
++| Claim register / release; GC drops un-claimed cold | `crates/nmp-testing/tests/store_gc_claims.rs` |
++| `nmp dump` round-trip is byte-identical for second dump | `crates/nmp-testing/tests/store_dump_roundtrip.rs` |
++| Migration v0→v1 success; rollback on N+1 failure | `crates/nmp-testing/tests/store_domain_migration.rs` |
++| Domain isolation: module A cannot read module B's sub-db | `crates/nmp-testing/tests/store_domain_isolation.rs` |
++| Working-set ≤ 100 MB at 100 views / 10k hot / 1M cached | `crates/nmp-testing/bin/reactivity-bench` (extended scenario) |
++
++## 12. Performance budget
++
++| Gate | Budget | Measurement |
++|---|---|---|
++| Cold-start time-to-first-painted-timeline on iPhone 12 (primed LMDB, last session's events on disk) | ≤ 1.5 s p99 | `firehose-bench live cold_start --device iphone12` |
++| Cold-start time-to-first-painted-timeline on simulator | ≤ 800 ms p99 (looser than device because no thermal envelope) | same harness, simulator scenario |
++| Working-set memory at 100 active views / 10k hot / 1M on disk | ≤ 100 MB resident | Instruments Allocations + `reactivity-bench` working-set scenario |
++| Single `insert()` for an unseen kind:1 with 4 secondaries | ≤ 250 µs p99 on iPhone 12 | criterion bench in `crates/nmp-testing/benches/store_insert.rs` |
++| `scan_by_author_kind` returning 200 newest events | ≤ 5 ms p99 | criterion bench in `crates/nmp-testing/benches/store_scan.rs` |
++| `gc_step()` work-batch ceiling (single call) | ≤ 50 ms total wall time | bounded by `GcBudget { max_events, max_duration_ms }` |
++| `nmp dump` of 1M events | sustained ≥ 50k events/sec on M-series Mac | wall-clock measurement in dump-roundtrip test |
++
++Each gate is measurable; any miss revises the design via an ADR before M3 is declared complete (per `plan.md` §1.6 "no silent endings").
++
++## 13. Open questions for ADR after review
++
++1. **`nostr-lmdb` LMDB environment sharing.** Can we open the same `lmdb::Environment` for both `NostrLMDB`'s sub-databases and our own NMP sub-databases (provenance, watermarks, claims, domain rows)? If yes, we get atomic cross-sub-db transactions for free (a single `RwTxn` covers event + provenance + secondary indexes). If `nostr-lmdb` insists on opening its own `Environment`, we lose that and the insert path needs a two-phase write with crash-recovery logic. Investigate before implementation — may require an upstream PR exposing `Environment` access.
++2. **Watermark `filter_hash` canonicalisation.** Two `Filter`s that are semantically identical but field-ordered differently must hash the same. The canonicalisation rule (likely: sort all tag-value arrays, sort kinds, sort authors, lexicographic field order before BLAKE3) needs to be specified once and shared with the planner so cache-coverage lookups hit. Candidate: a single `fn canonical_filter_hash(&Filter) -> [u8; 32]` in `nmp-core::store::watermarks`.
++3. **Projection cache durability.** Currently in-memory in the existing kernel (`kernel/mod.rs:293` `profiles: HashMap`). Do we persist projection caches as a `DomainModule` or rebuild from events at cold-start? Rebuild is simpler and avoids cache-staleness bugs but adds startup cost; persistence is faster but requires invalidation logic on kind:0 replacement. Recommended default: rebuild on cold-start, measure, decide whether to add the persistence layer in M3.x or M4.
++4. **Domain-module per-record encoding.** CBOR via `serde_cbor` vs serde-json vs bincode. CBOR is upstream-compatible (matches `nostr` crate); bincode is faster but stratifies the format. Default: CBOR for cross-language readability; revisit if benchmarks show >5% insert-time cost.
++5. **iOS keychain-stored encryption-at-rest key for LMDB.** Out of scope for M3 (mentioned for M6 keychain work) but the schema must not assume cleartext-on-disk forever; reserve a `meta` row for `encryption_version: u32` so a future migration can wrap pages.
++6. **`ModuleRegistry::register_domain` API stability.** Adding `DomainFactories` to `ModuleDescriptor` is a non-breaking additive change to the public substrate API (existing callers using only the generic `register_domain::<M>()` continue to compile), but it commits us to keeping `DomainModule::SCHEMA_VERSION` and `DomainModule::migrations` as compile-time-resolvable items rather than object-safe methods. Confirm this with the substrate maintainer before M3 lands — if `DomainModule` is expected to support runtime composition (e.g., plugin loading), we need option (c): the actor passes the live `&[Box<dyn DomainModule>]` to `EventStore::open` instead. Recommended default: stay with `fn`-pointer factories; revisit if a plugin-loading use case appears.
++
++## 14. Citations to current code
++
++- In-memory event store: `crates/nmp-core/src/kernel/mod.rs:294` (`events: HashMap<String, StoredEvent>`), `kernel/mod.rs:46` (`StoredEvent` struct).
++- Insert path under wrap: `crates/nmp-core/src/kernel/ingest.rs:166` (`ingest_profile`), `ingest.rs:235` (`ingest_timeline_event`), `ingest.rs:209` (`ingest_relay_list`).
++- Replaceable supersession (current scattered logic to be centralised in `EventStore::insert`): `kernel/ingest.rs:166-185` (profile replace by `(pubkey, kind)`), `ingest.rs:218-233` (NIP-65 list replace by `(pubkey, 10002)`).
++- Profile claim refcounting (current in-memory analogue of `EventStore::claim/release`): `kernel/mod.rs:315` (`profile_claims: HashMap<String, BTreeSet<String>>`), `kernel/requests.rs:202` (`claim_profile`), `requests.rs:239` (`release_profile`).
++- Substrate `DomainModule` trait the store backs: `crates/nmp-core/src/substrate/domain.rs:1` (current shape, lines 1–49).
++- Module registry the store consumes at startup: `crates/nmp-core/src/substrate/mod.rs:41` (`ModuleRegistry::register_domain`).
++
++The M3 implementation deletes none of the existing files outright — the kernel's `events: HashMap` and `profiles: HashMap` are replaced by calls to `Box<dyn EventStore>` held inside the `Kernel` struct, and the per-field tests (`kernel/tests.rs`) shift to the new trait via `MemEventStore`. No public FFI surface changes.
+diff --git a/docs/design/lmdb/gc.md b/docs/design/lmdb/gc.md
+new file mode 100644
+index 0000000..ce885c3
+--- /dev/null
++++ b/docs/design/lmdb/gc.md
+@@ -0,0 +1,195 @@
++# LMDB sub-design: GC working-set policy
++
++> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Formalises the hot-resident / cold-on-disk split required by ADR-0003 (`docs/decisions/0003-working-set-memory.md`).
++
++## 1. Definitions
++
++```
++stored_events = every event currently in `events` (primary), not tombstoned
++
++claim_pinned  = ⋃ { ids | ids ∈ claims[claimer] for each registered claimer }
++                where each `claimer` is an open ViewHandle / open ActionHandle
++
++open_view_cover = ⋃ { dependency_target_ids(spec)
++                       | (view_id, spec) ∈ open_views }
++                  computed from the composite reverse-index per ADR-0001
++
++recently_touched = top-N by `last_touched_ms` (default N = 10,000)
++
++hot_resident = claim_pinned ∪ open_view_cover ∪ recently_touched
++cold         = stored_events \ hot_resident
++```
++
++`last_touched_ms` is bumped on every `get_by_id`, on every secondary scan that *materialises* the event body, and on `insert` for a fresh row. Scans that only return ids/timestamps (e.g., the early-filter pass in a view's planner) do **not** bump it — only the construction of a `Delta` payload that needs the body does.
++
++`hot_resident` is stored in memory; `cold` lives only on disk. The store still **knows** about every cold event via secondaries — the reverse index covers both per ADR-0003: "The reverse index indexes both hot and cold events. Lookup returns view ids immediately; event bodies for delta construction load lazily and synchronously via the storage backend."
++
++## 2. Hot data structure
++
++```rust
++pub(crate) struct HotSet {
++    // LRU bounded by `target_hot_size` (default 10,000), evicts non-pinned.
++    lru: lru::LruCache<EventId, Arc<nostr::Event>>,
++    // Strong-pin overlay; refcounted by ClaimerId.
++    pinned: HashMap<EventId, u32>,                   // event_id → refcount
++    // Reverse map for cheap release().
++    by_claimer: HashMap<ClaimerId, SmallVec<[EventId; 8]>>,
++    target_hot_size: usize,
++}
++
++impl HotSet {
++    pub fn claim(&mut self, c: ClaimerId, ids: &[EventId]) {
++        for id in ids {
++            *self.pinned.entry(*id).or_insert(0) += 1;
++        }
++        self.by_claimer.entry(c).or_default().extend_from_slice(ids);
++    }
++
++    pub fn release(&mut self, c: ClaimerId) {
++        if let Some(ids) = self.by_claimer.remove(&c) {
++            for id in ids {
++                if let Some(rc) = self.pinned.get_mut(&id) {
++                    *rc = rc.saturating_sub(1);
++                    if *rc == 0 { self.pinned.remove(&id); }
++                }
++            }
++        }
++    }
++
++    pub fn touch(&mut self, id: EventId, e: Arc<nostr::Event>) {
++        self.lru.put(id, e);                          // bumps LRU
++        self.trim();
++    }
++
++    fn trim(&mut self) {
++        while self.lru.len() > self.target_hot_size {
++            // pop_lru returns oldest; skip pinned ones until we find an evictable.
++            // (LruCache::pop_lru doesn't take a predicate; we rotate.)
++            let mut skipped: SmallVec<[(EventId, Arc<nostr::Event>); 8]> = SmallVec::new();
++            let evicted = loop {
++                match self.lru.pop_lru() {
++                    Some((id, e)) if self.pinned.contains_key(&id) => skipped.push((id, e)),
++                    Some(pair) => break Some(pair),
++                    None => break None,
++                }
++            };
++            for (id, e) in skipped.drain(..) { self.lru.put(id, e); }
++            if evicted.is_none() { break; }           // every entry is pinned
++        }
++    }
++}
++```
++
++`target_hot_size` is set from `AppConfig::hot_event_ceiling` (default 10,000) and may be lowered by `MemoryWarningCapability` events (iOS app suspend or low-memory warning → halve the ceiling, run `gc_step()` once, restore after the warning clears).
++
++## 3. `gc_step()` algorithm
++
++```rust
++pub fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError> {
++    let start = Instant::now();
++    let now_s = unix_now();
++    let mut report = GcReport::default();
++
++    // 3.1 — NIP-40 expired reaper.
++    let to_reap = self.scan_expiring_before(now_s, budget.max_events_per_step)?
++        .collect::<Result<Vec<_>, _>>()?;
++    for ev in to_reap {
++        if start.elapsed().as_millis() as u32 >= budget.max_duration_ms { break; }
++        self.reap_one(ev.raw.id.into(), TombstoneOrigin::NIP40Expiry, now_s)?;
++        report.expired_reaped += 1;
++    }
++
++    // 3.2 — Trim LRU back to target.
++    let lru_before = self.hot.lock().lru.len();
++    self.hot.lock().trim();
++    report.lru_evicted = lru_before.saturating_sub(self.hot.lock().lru.len());
++
++    // 3.3 — Purge old tombstones whose target event is absent.
++    let cutoff = now_s.saturating_sub(self.cfg.tombstone_retention_secs);
++    report.tombstones_purged = self.purge_old_tombstones(cutoff,
++        budget.max_events_per_step.saturating_sub(report.expired_reaped))?;
++
++    report.duration_ms = start.elapsed().as_millis() as u32;
++    Ok(report)
++}
++```
++
++Single `gc_step()` is bounded by `GcBudget { max_events_per_step, max_duration_ms }`. Defaults: `max_events_per_step = 2000`, `max_duration_ms = 50`. The actor calls `gc_step()`:
++
++- Every 60 seconds (cooperative; runs on the actor thread between mailbox messages).
++- On `MemoryWarningCapability::Pressure` (iOS / Android low-memory signals).
++- On any single `insert()` that observes `hot.lru.len() > 2 * target_hot_size` (safety net).
++
++`gc_step()` is **never** invoked from an FFI call path — it runs on the actor's own schedule so any latency it introduces is invisible to the platform.
++
++## 4. Claim / release wiring
++
++The kernel actor holds `view_claims: HashMap<ViewId, ClaimerId>`. On `open_view(spec)`:
++
++1. The view module's `dependencies(spec)` is consulted (per `kernel-substrate.md` §3).
++2. The composite reverse-index resolves the dependency set to a (small, bounded) set of currently-known event ids — the *view cover*.
++3. `store.claim(claimer_id, &cover_ids)` pins those events in hot.
++4. As events arrive matching the dependency, the actor calls `store.claim(claimer_id, &[new_id])` incrementally (claim is idempotent under increment).
++
++On `close_view(view_id)`:
++
++1. `store.release(claimer_id)` drops every pin in one call.
++2. The view module's `state` is dropped; its claim refcounts decay; the next `gc_step()` evicts any newly-unpinned cold from LRU.
++
++Restart recovery: `claims_meta` sub-db ([`keys.md`](keys.md) §1) holds the persisted per-`ClaimerId` pin set. On startup the actor rebuilds active views first (per the diagnostics replay sequence), then re-claims; entries in `claims_meta` whose `ClaimerId` is not associated with a re-opened view are dropped from the persisted map. This means the cold-start path always re-derives claims from open-view state, but the persistence is what lets the store survive an actor restart without losing hot-set protection mid-shutdown.
++
++## 5. Memory accounting (the ADR-0003 gate)
++
++The relevant figure for the M3 exit gate is **working-set RSS at the configuration described in ADR-0003 §Decision**: 100 active views, 10k hot events, 1M cached on disk, ≤ 100 MB.
++
++Components measured:
++
++| Source | Approx bytes | Notes |
++|---|---|---|
++| Hot LRU (10k × Arc<Event>) | ~30 MB | average kind:1 event with content ~800 B, profile/contacts can be 4–8 KB each; mix-weighted average ~3 KB; the `Arc` is shared with view module payloads so the same body isn't duplicated |
++| Claim refcount maps (10k entries) | ~0.5 MB | `HashMap<EventId, u32>` + reverse `by_claimer` |
++| Reverse index in-memory (composite keys for 100 views) | ~5 MB | from ADR-0001 — bounded by `~broad_axes_guardrail` per ADR-0001 |
++| Projection caches (author display, reaction counts) | ~10 MB | LRU-bounded by referenced-view count per ADR-0003 |
++| LMDB page cache (kernel-owned, *not* counted toward RSS budget) | 0 | OS-paged, evicted under pressure; counts against system memory but not app working set |
++| Watermarks (loaded as `HashMap` for hot lookups) | ~2 MB | M4 — assuming O(10k) watermarks (one per `(filter, relay)` pair) |
++| Tombstone bloom filter (if added — see open questions) | ~1 MB | accelerates the `tombstones.contains_key()` check on insert |
++| Action ledger in-flight rows | ~1 MB | bounded by spec §7.5 |
++| Slack / Rust allocator overhead | ~20 MB | empirical from reactivity-bench |
++| **Total target** | **~70 MB** | leaves ~30 MB headroom against the 100 MB gate |
++
++The 1M-events-on-disk dimension does **not** appear in the budget because LMDB does not page them into our heap; they exist in mmap'd pages the OS may evict at will. This is the design intent of ADR-0003.
++
++## 6. Failure modes and degraded behavior
++
++| Failure | Detection | Response |
++|---|---|---|
++| LMDB env out of space | LMDB `MDB_MAP_FULL` on a write | Run an emergency `gc_step()` with relaxed budget; if still full, surface `Effect::StoreOutOfSpace`, refuse new inserts, allow reads + deletes |
++| LRU evicted a still-pinned event (bug) | `trim()` would have skipped it; if observed, log + invariant violation | Pin reinstated from `claims_meta`; fire `tracing::error!`; flagged as critical bug class to investigate |
++| `gc_step()` over-budget | `start.elapsed() > max_duration_ms` mid-loop | Break out of current loop early; remaining work picked up next call (no state corruption — every reaped event is its own transaction) |
++| `release()` called for unknown `ClaimerId` | `by_claimer.remove` returns None | Silent no-op; logged at debug; not a bug (idempotent close) |
++| Memory warning during heavy insert burst | iOS `didReceiveMemoryWarning` → `MemoryWarningCapability` event | Actor lowers `target_hot_size` to 5k, runs `gc_step({max_events_per_step:5000, max_duration_ms:200})` once; restored after the warning clears |
++
++## 7. Diagnostics integration (ADR-0007)
++
++The store exposes a `StoreHealth` snapshot for the diagnostics bridge:
++
++```rust
++pub struct StoreHealth {
++    pub primary_event_count: u64,
++    pub tombstone_count: u64,
++    pub hot_lru_size: usize,
++    pub claim_pinned_count: usize,
++    pub watermark_count: usize,
++    pub on_disk_bytes: u64,
++    pub last_gc: Option<GcReport>,
++    pub last_gc_at_ms: Option<u128>,
++}
++```
++
++Surfaced in the diagnostics screen alongside relay status (ADR-0007 §1). The Phase 1a.7 proof app already has the rendering scaffold (`ios/NmpStress/NmpStress/DiagnosticsView.swift`); M3 adds the StoreHealth row to it.
++
++## 8. Why not a periodic full sweep?
++
++A full sweep is `O(stored_events)`. With 1M events on disk the LMDB scan alone is 100–500 ms wall-time on iPhone 12 NAND — well outside the actor's single-message budget. The bounded `gc_step()` with explicit budget is therefore the only correct shape; it composes with LMDB's natural mmap eviction model and never blocks the mailbox for long.
++
++A periodic vacuum/compact pass (LMDB's equivalent of `VACUUM`) **is** scheduled — once per app launch, at idle, after the first 30 seconds of quiescence. It is *not* part of `gc_step`'s budget envelope and runs as a separate low-priority actor message that yields between LMDB page boundaries.
+diff --git a/docs/design/lmdb/keys.md b/docs/design/lmdb/keys.md
+new file mode 100644
+index 0000000..61ccd10
+--- /dev/null
++++ b/docs/design/lmdb/keys.md
+@@ -0,0 +1,150 @@
++# LMDB sub-design: key encoding
++
++> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Fixes the byte layout of every sub-database the NMP store opens. Primary event storage is owned by `nostr-lmdb`; everything below is NMP-owned and lives in NMP-managed sub-databases under the same `lmdb::Environment` (per open question 1 in the master doc).
++
++## 1. LMDB environment layout
++
++One `lmdb::Environment` per app data directory. Sub-databases:
++
++| Sub-db | Owner | Key shape | Value | Notes |
++|---|---|---|---|---|
++| (multiple) | `nostr-lmdb` | upstream | upstream | event primary, internal filter indexes, kind:5 suppression |
++| `idx_author_kind` | NMP | `pubkey[32] ‖ kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` | empty | newest-first scans for `(author, kinds[])` |
++| `idx_kind_dtag` | NMP | `kind_be[4] ‖ pubkey[32] ‖ dtag_len_be[2] ‖ dtag_bytes` | `event_id[32]` | parameterized replaceable address lookup |
++| `idx_etag_time` | NMP | `target_event_id[32] ‖ created_at_desc_be[8] ‖ event_id[32]` | `kind_be[4]` | reaction/reply/thread view scans |
++| `idx_ptag_time` | NMP | `target_pubkey[32] ‖ created_at_desc_be[8] ‖ event_id[32]` | `kind_be[4]` | mentions / notifications |
++| `idx_kind_time` | NMP | `kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` | empty | global-by-kind backfills |
++| `idx_expires` | NMP | `expires_at_be[8] ‖ event_id[32]` | empty | NIP-40 reaper |
++| `tombstones` | NMP | `target_event_id[32]` | CBOR `TombstoneRow` | persists past delete |
++| `provenance` | NMP | `event_id[32]` | CBOR `ProvenanceRow` | per-relay sidecar (master doc §9) |
++| `watermarks` | NMP | `filter_hash[32] ‖ relay_url_bytes` | CBOR `WatermarkRow` | M4 NIP-77 sync state |
++| `claims_meta` | NMP | `claimer_id_be[8]` | CBOR `Vec<EventId>` | pinned set per ClaimerId; rebuilt on restart from open views |
++| `domain_<ns>_data` | NMP, per `DomainModule` | module-defined | module-defined | one sub-db per registered namespace |
++| `domain_<ns>_idx_<name>` | NMP, per `DomainModule` index | `index_key ‖ primary_key` | empty | secondary indexes per `DomainIndex` |
++| `_meta` | NMP | string namespace | `{ schema_version: u32, opened_with_nmp_version: String }` | migration tracking |
++
++Sub-databases are opened lazily on first access and cached on the `LmdbEventStore`.
++
++## 2. Endian + ordering conventions
++
++- All integers in keys are **big-endian** so LMDB's byte-wise comparator matches numeric order.
++- `created_at_desc_be = (u64::MAX - created_at).to_be_bytes()` so a forward scan returns newest-first without `MDB_PREV` gymnastics.
++- All pubkeys / event ids are fixed-width 32 bytes; the `nostr` crate's `EventId` and `PublicKey` give us byte arrays directly.
++
++## 3. Secondary index details
++
++### 3.1 `idx_author_kind`
++
++Key: `pubkey[32] ‖ kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` → empty value.
++
++Scan recipes:
++
++- *Newest N events by author* — `range(pubkey ‖ 0u32_be ‖ ..)` (kind=0 lower bound) up to `pubkey ‖ u32::MAX_be ‖ ..`, take N.
++- *Newest N events by `(author, kind=1)`* — `range(pubkey ‖ 1u32_be ‖ ..)` up to `pubkey ‖ 1u32_be ‖ u64::MAX_be`, take N.
++- *All kind:0 for author* — `range(pubkey ‖ 0u32_be ‖ ..)`, take 1 (because the replaceable index ensures only one).
++
++Replaceable supersession (§7.1): on insert of a new kind in [0, 3, 10000–19999], find existing row via this index with `(pubkey, kind)` prefix, compare `created_at`, if incoming wins delete old + write new. Both deletes happen in the same `RwTxn` as the new write so there is no half-state visible to readers.
++
++### 3.2 `idx_kind_dtag` (parameterized replaceable)
++
++Key: `kind_be[4] ‖ pubkey[32] ‖ dtag_len_be[2] ‖ dtag_bytes` → `event_id[32]`.
++
++The d-tag bytes go last so two events with the same `(kind, pubkey)` but different `d` tags don't collide; the explicit length prefix avoids `d="foo"` vs `d="foob"` aliasing under prefix scans. Lookup is exact-key: `get_param_replaceable(pubkey, kind, d_tag)` builds the key and reads.
++
++The value is the `event_id`; the primary event itself lives in the `nostr-lmdb` events sub-db. On supersession, the old event-id is fetched from this row, both primary and old `idx_*` rows are deleted, and the value is overwritten with the new id.
++
++### 3.3 `idx_etag_time` and `idx_ptag_time`
++
++Key: `target[32] ‖ created_at_desc_be[8] ‖ event_id[32]` → `kind_be[4]`.
++
++The value holds the kind so a reactions view can filter `(kinds == 7)` during scan without a primary-row fetch per candidate. Bookmark / repost / thread views similarly avoid the `get_by_id` round trip until they need the body.
++
++On insert, the kernel walks the event's `tags`: every `e` tag value goes into `idx_etag_time` and every `p` tag value goes into `idx_ptag_time`. Tag values must be 32-byte hex (validated at insert time); non-conformant tags are silently skipped from indexing (they are still stored in the event body).
++
++### 3.4 `idx_kind_time`
++
++Key: `kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` → empty.
++
++Used by *global-by-kind* backfills (e.g. "recent kind:0 across all authors" during diagnostics). Heavy index — populated for **all** kinds by default but the implementation may skip kinds in a configurable deny-list to keep write amplification down (default deny-list: kind:1 if config flag `index_kind1_globally=false`, which it is by default; M2's planner does not need a global kind:1 scan).
++
++### 3.5 `idx_expires`
++
++Key: `expires_at_be[8] ‖ event_id[32]` → empty.
++
++Populated **only** for events that have an `expiration` tag at insert (NIP-40). `gc_step()` opens a read cursor at `expires_at = 0`, walks forward up to the configured budget, and reaps any keys whose `expires_at ≤ now_unix_seconds()`. Each reaped event triggers a tombstone-of-origin `NIP40Expiry` write so re-insertions (from a re-sync) don't resurrect it.
++
++## 4. Tombstones
++
++Key: `target_event_id[32]` → CBOR `TombstoneRow`:
++
++```rust
++#[derive(Serialize, Deserialize)]
++struct TombstoneRow {
++    target_id: [u8; 32],
++    origin: TombstoneOrigin,             // Kind5 | NIP40Expiry | AdminPurge
++    kind5_event_id: Option<[u8; 32]>,    // None for non-Kind5 origins
++    deleter_pubkey: Option<[u8; 32]>,    // None for NIP40Expiry / AdminPurge
++    deleted_at: u64,                     // max observed across kind:5 redeliveries
++    sources: Vec<String>,                // relay urls that delivered the kind:5
++}
++```
++
++Insert pre-check: before any new event hits the primary store, `tombstones.contains_key(event.id)` is consulted. A hit yields `InsertOutcome::Tombstoned { target_kind5_id }` and the event is dropped. This is the "later re-insertion is suppressed" behavior of §7.1.
++
++Foreign kind:5 (where the kind:5 author did not author all targets) is **stored** as an ordinary event (so other clients can render the delete intent) but **does not** write a `TombstoneRow` for any of its targets — per §7.1 "foreign kind:5 ignored". The kind:5 event itself goes through the normal insert path including secondaries.
++
++## 5. Watermarks
++
++Key: `filter_hash[32] ‖ relay_url_bytes` — variable-length, exact-key lookups only. `filter_hash` is BLAKE3 of the canonical filter encoding (see `lmdb/watermarks.md` §3 for the canonicalisation algorithm).
++
++Value: CBOR `WatermarkRow` (same shape as the trait type in [`trait.md`](trait.md) §2).
++
++## 6. Provenance
++
++Key: `event_id[32]` → CBOR `ProvenanceRow { sources: Vec<ProvenanceEntry> }`. On duplicate insert: read, mutate (append or bump `last_seen_ms`), write back. Bounded growth — the kernel caps `sources.len()` at 32 (the 33rd unique relay overwrites the oldest non-primary entry); for nearly all events this is non-binding. The `primary: bool` flag is deterministic: `sources[0]` after sorting by `(first_seen_ms, relay_url)`.
++
++## 7. Domain rows (per `DomainModule`)
++
++For each `DomainModule` with namespace `"foo.bar"`:
++
++- `domain_foo.bar_data` — primary data sub-db. Module owns key + value encoding.
++- `domain_foo.bar_idx_<index>` — one sub-db per `DomainIndex` (per `crates/nmp-core/src/substrate/domain.rs:16`). Key = `index_key_fn(data_value) ‖ primary_key`; value = empty. The index is rewritten on every put (delete-old, write-new).
++
++The actor exposes them only via `DomainHandle` (see [`trait.md`](trait.md) §4); modules never see the sub-db handles directly. Module isolation per `kernel-substrate.md` §8 is preserved: the handle factory checks the caller's registered namespace.
++
++## 8. `_meta` sub-database
++
++Key: namespace string (e.g. `"twitter.drafts"`, `"_kernel"`). Value: CBOR `{ schema_version: u32, opened_with_nmp_version: String, last_migration_at_ms: u64 }`. Read at startup by the migration runner; written after every successful migration step.
++
++The reserved `_kernel` namespace tracks the LMDB store's own schema version (currently 1). A bumped `_kernel` version triggers store-wide migrations (e.g. re-encoding all `ProvenanceRow` values when the format changes).
++
++## 9. Worked example: inserting a kind:1 from `pablof7z` arriving from `wss://relay.primal.net`
++
++```
++event_id   = a3f1...   (32 bytes)
++pubkey     = 0461...   (32 bytes)
++kind       = 1
++created_at = 1747000000
++tags       = [["e","b21c...","","root"], ["p","0488..."]]
++```
++
++Inside one `RwTxn`:
++
++1. `tombstones.get(&event_id)` → None ⇒ proceed.
++2. `nostr_lmdb.save_event(&event)` → SaveEventStatus::Success.
++3. `idx_author_kind.put(0461... ‖ 0x00000001 ‖ desc(1747000000) ‖ a3f1..., &[])`.
++4. `idx_kind_time.put(0x00000001 ‖ desc(1747000000) ‖ a3f1..., &[])` (only if `index_kind1_globally`; default off).
++5. For `e:b21c...` → `idx_etag_time.put(b21c... ‖ desc(1747000000) ‖ a3f1..., 1u32_be)`.
++6. For `p:0488...` → `idx_ptag_time.put(0488... ‖ desc(1747000000) ‖ a3f1..., 1u32_be)`.
++7. `provenance.put(a3f1..., cbor({sources:[{relay:"wss://relay.primal.net", first_seen_ms:T, last_seen_ms:T, primary:true}]}))`.
++
++Total LMDB writes: 1 primary (delegated to upstream) + 3 NMP secondaries + 1 provenance = ~5 page writes for a typical kind:1. Within the 250 µs p99 budget (master doc §12) on iPhone 12 NAND.
++
++A second arrival of the same id from `wss://nos.lol`:
++
++1. `tombstones.get(&a3f1...)` → None.
++2. `nostr_lmdb.save_event` → SaveEventStatus::Duplicate (we don't re-process).
++3. Skip steps 3–6 (secondaries unchanged).
++4. `provenance.get(a3f1...)` → existing row; append `{relay:"wss://nos.lol", first_seen_ms:T2, last_seen_ms:T2, primary:false}`; put back.
++
++One read + one write. Returns `InsertOutcome::Duplicate { sources_after: 2 }`.
+diff --git a/docs/design/lmdb/tests.md b/docs/design/lmdb/tests.md
+new file mode 100644
+index 0000000..762b46b
+--- /dev/null
++++ b/docs/design/lmdb/tests.md
+@@ -0,0 +1,223 @@
++# LMDB sub-design: test plan
++
++> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Maps every insert invariant in `docs/product-spec/subsystems.md` §7.1 to a concrete test in `crates/nmp-testing/tests/`. Each test exists for both `MemEventStore` (always) and `LmdbEventStore` (under `#[cfg(feature = "lmdb-backend")]`).
++
++## 1. Test harness shape
++
++```rust
++// crates/nmp-testing/src/store_harness.rs
++pub struct StoreHarness {
++    pub store: Box<dyn EventStore>,
++    pub tmp: tempfile::TempDir,
++    pub keys: nostr::Keys,
++}
++
++impl StoreHarness {
++    pub fn mem() -> Self { /* MemEventStore */ }
++    pub fn lmdb() -> Self { /* LmdbEventStore in tmp dir */ }
++
++    pub fn insert(&self, builder: EventBuilder, source: &str) -> InsertOutcome { /* ... */ }
++    pub fn assert_present(&self, id: &EventId);
++    pub fn assert_tombstoned(&self, id: &EventId);
++    pub fn restart(&mut self);   // close + reopen the store; LMDB only
++}
++
++// Tests use a macro to run against both backends.
++macro_rules! for_each_backend {
++    ($name:ident, $body:expr) => {
++        #[test] fn $name() { let mut h = StoreHarness::mem(); $body(&mut h); }
++        #[cfg(feature = "lmdb-backend")]
++        #[test] fn paste::paste!([<$name _lmdb>])() {
++            let mut h = StoreHarness::lmdb(); $body(&mut h);
++        }
++    };
++}
++```
++
++The harness lives in `crates/nmp-testing/src/` so per-test files are short and declarative.
++
++## 2. Invariant → test mapping
++
++Every row of the §7.1 table:
++
++### 2.1 Insert API single path (§7.1 row "Insert API")
++
++File: `crates/nmp-testing/tests/store_insert_path.rs`
++
++```rust
++for_each_backend!(insert_returns_insert_outcome, |h: &mut StoreHarness| {
++    let event = h.signed(EventBuilder::text_note("hello", &[]));
++    let outcome = h.store.insert(event.clone(), &"wss://t/".into(), 0).unwrap();
++    assert!(matches!(outcome, InsertOutcome::Inserted { .. }));
++    assert!(h.store.get_by_id(&event.id.to_bytes()).unwrap().is_some());
++});
++```
++
++Plus a static-assertion-style test ensuring no other public function on `EventStore` writes to the primary store (compile-time check by inspecting trait method list via a build script — deferred to v1.x; v1 covers via review).
++
++### 2.2 Signature verification (§7.1 row "Signature/delegation validity")
++
++File: `crates/nmp-testing/tests/store_invalid_sig.rs`
++
++Builds an event, mutates the signature, inserts. Expects `InsertOutcome::Rejected { reason: RejectReason::BadSignature }` and no row in primary, secondaries, provenance, or tombstones. Also tests a malformed NIP-26 delegation tag (rejects with `BadDelegation`).
++
++### 2.3 Duplicate id → provenance merge (§7.1 row "Duplicate id")
++
++File: `crates/nmp-testing/tests/store_provenance_merge.rs`
++
++```rust
++for_each_backend!(duplicate_merges_provenance_keeps_earliest, |h| {
++    let ev = h.signed(EventBuilder::text_note("x", &[]));
++    let o1 = h.store.insert(ev.clone(), &"wss://a/".into(), 1000).unwrap();
++    let o2 = h.store.insert(ev.clone(), &"wss://b/".into(), 2000).unwrap();
++    assert!(matches!(o1, InsertOutcome::Inserted { .. }));
++    assert!(matches!(o2, InsertOutcome::Duplicate { sources_after: 2, .. }));
++    let p = h.store.provenance_for(&ev.id.to_bytes()).unwrap();
++    assert_eq!(p.len(), 2);
++    let primary = p.iter().find(|e| e.primary).unwrap();
++    assert_eq!(primary.relay_url, "wss://a/");
++    assert_eq!(primary.first_seen_ms, 1000); // earliest preserved
++});
++```
++
++### 2.4 Replaceable supersession (§7.1 row "Replaceable kinds")
++
++File: `crates/nmp-testing/tests/store_replaceable.rs`
++
++Inserts two kind:0 from same pubkey, second with later `created_at`. Asserts: `get_by_id(first_id)` returns None; `scan_by_author_kind(pk, &[0], None, None, 10)` returns one row; the row's id is the second. Tie-break test: two kind:0 with same `created_at` — keep the lexicographically smaller id.
++
++### 2.5 Parameterized replaceable (§7.1 row "Parameterized replaceable")
++
++File: `crates/nmp-testing/tests/store_param_replaceable.rs`
++
++Insert two kind:30023 with same `(pubkey, d=foo)`, second newer; assert only the second is returned by `get_param_replaceable(pk, 30023, b"foo")`. Insert a third with same kind+pubkey but `d=bar` — assert both `foo` and `bar` are independently retrievable. Assert that a kind:30024 with `d=foo` (different kind) does not collide with the kind:30023.
++
++### 2.6 Kind:5 self-delete + tombstone persistence (§7.1 row "Kind 5")
++
++File: `crates/nmp-testing/tests/store_kind5_tombstone.rs`
++
++- Insert kind:1 by Alice.
++- Insert kind:5 by Alice referencing the kind:1 via `e` tag.
++- Assert kind:1 gone from primary; tombstone row exists with `target_id == kind1.id`, `origin == Kind5`.
++- Insert the same kind:1 again — assert `InsertOutcome::Tombstoned`, no primary row created.
++- Restart store; repeat the re-insertion — assert tombstone persists across restart.
++
++### 2.7 Foreign kind:5 ignored (§7.1 row "Kind 5" — foreign clause)
++
++File: `crates/nmp-testing/tests/store_kind5_foreign.rs`
++
++- Insert kind:1 by Alice.
++- Insert kind:5 by Bob referencing Alice's kind:1.
++- Assert: kind:1 is still present in primary (Bob can't delete Alice's event); the kind:5 event itself is stored (so other clients can see it); no tombstone row was written.
++
++### 2.8 NIP-40 expiration scheduling (§7.1 row "NIP-40 expiration")
++
++File: `crates/nmp-testing/tests/store_nip40_expiration.rs`
++
++- Insert kind:1 with `expiration` tag at `now + 1 second`.
++- Assert `scan_expiring_before(now + 5, 10)` returns the event.
++- Call `gc_step(GcBudget { max_events_per_step: 10, max_duration_ms: 100 })` at `now + 2`.
++- Assert primary row gone; tombstone written with `origin == NIP40Expiry`.
++- Insert same event again — assert `InsertOutcome::Tombstoned`.
++- Insert an event with `expiration` already in the past — assert `InsertOutcome::Rejected { reason: ExpiredOnArrival }`.
++- Restart store; insert new event with `expiration` at `now + 1`; assert the reaper picks it up after restart (the `idx_expires` cursor scan is the source of truth — no separate timer needs to survive restart).
++
++### 2.9 Watermarks (§7.1 "Sync watermarks")
++
++File: `crates/nmp-testing/tests/store_watermarks.rs`
++
++- Write a watermark; read it back; assert equal.
++- Restart store; read again; assert preserved.
++- Test `coverage()`: row with `synced_up_to = now - 60s` → `Coverage::CompleteAsOf` (under default 300s staleness); row with `synced_up_to = now - 600s` → `Coverage::PartialUpTo`; missing row → `Coverage::Unknown`.
++- `list_watermarks_for_relay("wss://a/")` returns only rows for that relay.
++- Concurrent writes to the same key (simulated): last-writer-wins, no row corruption.
++
++### 2.10 Claims + GC (§7.1 "GC")
++
++File: `crates/nmp-testing/tests/store_gc_claims.rs`
++
++- Insert 100 events; all in hot LRU (under default 10k ceiling).
++- Claim 10 of them under `ClaimerId(1)`.
++- Configure `target_hot_size = 50`; insert another 50 events; call `gc_step`.
++- Assert: 10 claimed events still present in hot (`store.get_by_id` is a fast in-memory hit — measurable via a counter exposed for the test); 40 unclaimed events evicted from LRU but still readable from disk.
++- Release `ClaimerId(1)`; insert another 20 events; call `gc_step`.
++- Assert: previously claimed events now subject to LRU eviction.
++
++### 2.11 Dump round-trip (master doc §10)
++
++File: `crates/nmp-testing/tests/store_dump_roundtrip.rs`
++
++- Build a populated store: 1000 events, 50 tombstones, 100 watermarks, 200 domain rows across 3 namespaces.
++- `dump(&mut buf1, DumpFormat::Jsonl)`.
++- Open a fresh store; replay every line; `dump(&mut buf2, ...)`.
++- Assert `buf1 == buf2` byte-for-byte (sort by stable key first — the dump iterates sub-dbs in a deterministic order documented in the dump module).
++
++### 2.12 Domain migration success + failure (master doc §6)
++
++File: `crates/nmp-testing/tests/store_domain_migration.rs`
++
++- Register `TestModuleV1` with `SCHEMA_VERSION = 1` and no migrations; open store; assert `_meta.test_module.schema_version == 1`.
++- Close store; register `TestModuleV2` with `SCHEMA_VERSION = 2` and one migration v1→v2 that writes one key; open store; assert migration ran and key exists.
++- Close; register `TestModuleV3` with `SCHEMA_VERSION = 3` and a deliberately failing migration v2→v3; open store; assert `Effect::DomainSchemaTooNew { namespace: "test_module" }` (under degraded-mode rules) and `_meta` still at v2.
++- Close; remove the failing migration; reopen — assert successful catch-up to v3 (idempotent retry).
++
++### 2.13 Domain isolation (`kernel-substrate.md` §8)
++
++File: `crates/nmp-testing/tests/store_domain_isolation.rs`
++
++- Open `DomainHandle` for module A; write key `K`.
++- Open `DomainHandle` for module B; read key `K` — assert returns `None`.
++- Module B's `scan_prefix(b"")` returns only module B's rows.
++
++### 2.14 Cold-start performance (master doc §12)
++
++Scenario in `crates/nmp-testing/bin/firehose-bench/src/scenarios/cold_start.rs` (already exists in M1; extended here):
++
++- Pre-populate an LMDB store with a representative session (~20k events: 10k kind:1, 8k kind:0, 2k kind:3 / 10002).
++- Tar + ship the file with the test fixture.
++- Measure: open store, register modules, run the bootstrap sequence that the actor runs on app launch, until the first `AppUpdate::FullState` is emitted with non-empty timeline.
++- Gate: ≤ 1.5 s on iPhone 12 hardware; ≤ 800 ms on iPhone 16 Pro simulator.
++
++### 2.15 Working-set memory (ADR-0003)
++
++Scenario in `crates/nmp-testing/bin/reactivity-bench` — extended with a new `--scenario working_set_lmdb` mode:
++
++- Insert 1M synthetic events into the store.
++- Open 100 view subscriptions covering 10k events.
++- Run for 60 seconds with light churn (insert 10 events / sec).
++- Sample RSS every 5 seconds via `/proc/self/status` on Linux / `mach_task_basic_info` on iOS.
++- Gate: max RSS ≤ 100 MB over the run.
++
++### 2.16 Restart preserves replaceable semantics (`plan.md` §M3 exit gate)
++
++File: `crates/nmp-testing/tests/store_replaceable_restart.rs`
++
++- Write kind:0 v1; assert present.
++- Write kind:0 v2 (newer); assert v1 gone, v2 present.
++- Restart store; assert v2 still present, v1 still gone.
++- Write kind:0 v0 (older than v2); assert no change (`InsertOutcome::Superseded`).
++
++## 3. Property tests
++
++In `crates/nmp-testing/tests/store_props.rs` using `proptest`:
++
++- **Insert is total under random valid events.** Generate a vec of valid signed events, insert in any order, assert the store's `get_by_id` agrees with the model (a `HashMap` reference impl).
++- **Replaceable convergence.** For any sequence of replaceable inserts for the same `(pubkey, kind, [d])` key, the final stored event is the (max created_at, min id) winner regardless of insertion order.
++- **Provenance commutativity.** For any two relay sources `r1, r2` and identical event, the post-state of provenance is identical to inserting `r2` first then `r1`.
++- **`nmp dump` is a fixed point.** Round-trip equality after N random operations.
++
++## 4. Cross-test invariants (asserted in a `teardown` hook for every test)
++
++Every test ends with `harness.assert_invariants()`:
++
++1. Every event in the primary store has a `provenance` row with ≥ 1 entry.
++2. Every secondary index entry's `event_id` resolves to an existing primary row.
++3. Every tombstone's `target_id` does **not** exist in the primary store.
++4. The `_meta._kernel.schema_version` is at the latest version the binary knows.
++5. The hot LRU contains only events that exist in the primary store.
++
++Violation of any invariant fails the test with a precise diff of which sub-db is out of sync.
++
++## 5. CI integration
++
++`cargo test --workspace --features lmdb-backend` becomes part of the pre-merge gate from M3 onward (`plan.md` §6 will be updated). The criterion benches in `crates/nmp-testing/benches/store_*.rs` run nightly with regression checks against the previous week's median (>5% regression on any p99 fails the nightly).
+diff --git a/docs/design/lmdb/trait.md b/docs/design/lmdb/trait.md
+new file mode 100644
+index 0000000..2bbdc60
+--- /dev/null
++++ b/docs/design/lmdb/trait.md
+@@ -0,0 +1,312 @@
++# LMDB sub-design: `EventStore` trait
++
++> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). This file fixes the trait surface; the master doc fixes the decision.
++
++## 1. Crate placement
++
++`crates/nmp-core/src/store/events.rs` (filename note: `trait` is a Rust keyword, so the file is named `events.rs` and exposes `pub trait EventStore`). Re-exported from `nmp_core::store::EventStore`. The actor (`crates/nmp-core/src/actor.rs`) holds the store as `store: Box<dyn EventStore>`; backends are constructed by the factory in `store/mod.rs::open_event_store(&AppConfig) -> Result<Box<dyn EventStore>, StoreError>`.
++
++## 2. Supporting types
++
++```rust
++use std::sync::Arc;
++
++pub type EventId = [u8; 32];
++pub type PubKey = [u8; 32];
++pub type RelayUrl = String;
++
++#[derive(Clone, Debug)]
++pub struct StoredEvent {
++    pub raw: Arc<nostr::Event>,         // upstream nostr crate type
++    pub received_at_ms: u64,            // wall-clock first arrival across all relays
++}
++
++#[derive(Clone, Debug)]
++pub struct ProvenanceEntry {
++    pub relay_url: RelayUrl,
++    pub first_seen_ms: u64,
++    pub last_seen_ms: u64,
++    pub primary: bool,                  // first observed relay (deterministic)
++}
++
++#[derive(Clone, Debug)]
++pub enum InsertOutcome {
++    /// Fresh insert; secondary indexes written.
++    Inserted { id: EventId, sources_after: u32 },
++    /// Duplicate id; provenance updated, primary untouched.
++    Duplicate { id: EventId, sources_after: u32 },
++    /// Replaceable supersession: this event replaced an older one.
++    Replaced { new_id: EventId, replaced_id: EventId },
++    /// Replaceable supersession: incoming was older, dropped.
++    Superseded { id: EventId, current_id: EventId },
++    /// Suppressed because target is tombstoned.
++    Tombstoned { id: EventId, target_kind5_id: EventId },
++    /// Signature / delegation / structural validity failed.
++    Rejected { id: EventId, reason: RejectReason },
++    /// Ephemeral kind: delivered to live consumers, not stored.
++    Ephemeral { id: EventId },
++}
++
++#[derive(Clone, Debug)]
++pub enum RejectReason {
++    BadSignature,
++    BadDelegation(String),
++    Malformed(String),
++    ExpiredOnArrival,                   // NIP-40 expiration already in the past
++}
++
++#[derive(Clone, Debug)]
++pub struct TombstoneRow {
++    pub target_id: EventId,
++    pub kind5_event_id: Option<EventId>, // None for NIP-40 expiry tombstones
++    pub deleter_pubkey: Option<PubKey>,
++    pub deleted_at: u64,                 // unix seconds
++    pub sources: Vec<RelayUrl>,
++    pub origin: TombstoneOrigin,
++}
++
++#[derive(Clone, Copy, Debug, Eq, PartialEq)]
++pub enum TombstoneOrigin { Kind5, NIP40Expiry, AdminPurge }
++
++#[derive(Clone, Debug)]
++pub struct WatermarkKey {
++    pub filter_hash: [u8; 32],
++    pub relay_url: RelayUrl,
++}
++
++#[derive(Clone, Debug)]
++pub struct WatermarkRow {
++    pub key: WatermarkKey,
++    pub synced_up_to: u64,               // unix seconds
++    pub last_sync_method: SyncMethod,
++    pub last_negentropy_state: Option<Vec<u8>>,
++    pub bytes_saved_vs_req: u64,
++    pub updated_at: u64,
++}
++
++#[derive(Clone, Copy, Debug, Eq, PartialEq)]
++pub enum SyncMethod { Negentropy, ReqScan, Manual }
++
++#[derive(Clone, Copy, Debug)]
++pub struct ClaimerId(pub u64);           // opaque view-handle id from the actor
++
++#[derive(Clone, Copy, Debug)]
++pub struct GcBudget {
++    pub max_events_per_step: usize,
++    pub max_duration_ms: u32,
++}
++
++#[derive(Clone, Debug, Default)]
++pub struct GcReport {
++    pub expired_reaped: usize,
++    pub lru_evicted: usize,
++    pub tombstones_purged: usize,
++    pub duration_ms: u32,
++}
++
++#[derive(Clone, Copy, Debug)]
++pub enum DumpFormat { Jsonl, Cbor }
++
++#[derive(Clone, Debug, Default)]
++pub struct DumpStats {
++    pub events: u64,
++    pub tombstones: u64,
++    pub watermarks: u64,
++    pub domain_rows: u64,
++    pub bytes_written: u64,
++}
++
++#[derive(Debug, thiserror::Error)]
++pub enum StoreError {
++    #[error("backend i/o: {0}")] Io(String),
++    #[error("backend corruption: {0}")] Corrupt(String),
++    #[error("encoding: {0}")] Encoding(String),
++    #[error("schema too new: {namespace} on-disk={on_disk} expected={expected}")]
++    SchemaTooNew { namespace: String, on_disk: u32, expected: u32 },
++    #[error("schema migration failed: {namespace} v{from}->v{to}: {reason}")]
++    MigrationFailed { namespace: String, from: u32, to: u32, reason: String },
++    #[error("unknown namespace: {0}")] UnknownNamespace(String),
++}
++```
++
++The store iterates lazily for scans:
++
++```rust
++pub trait EventIter: Iterator<Item = Result<StoredEvent, StoreError>> + Send {}
++impl<T: Iterator<Item = Result<StoredEvent, StoreError>> + Send> EventIter for T {}
++```
++
++`StoredEvent::raw` is `Arc<nostr::Event>` so the hot LRU can hold reference-counted copies without cloning the event body on each `get_by_id`.
++
++## 3. The trait
++
++```rust
++pub trait EventStore: Send + Sync {
++    // ─────── Reads ───────
++
++    /// Primary lookup. Returns Ok(None) if absent; tombstones do not count as "present".
++    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError>;
++
++    /// `idx_author_kind` scan, newest-first. `kinds` empty = any kind.
++    fn scan_by_author_kind<'a>(
++        &'a self,
++        author: &PubKey,
++        kinds: &[u32],
++        since: Option<u64>,
++        until: Option<u64>,
++        limit: usize,
++    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
++
++    /// `idx_kind_dtag` lookup. Returns the current authoritative parameterized
++    /// replaceable for `(pubkey, kind, d_tag)`, or Ok(None).
++    fn get_param_replaceable(
++        &self,
++        pubkey: &PubKey,
++        kind: u32,
++        d_tag: &[u8],
++    ) -> Result<Option<StoredEvent>, StoreError>;
++
++    /// `idx_etag_time` scan, newest-first. Used by reaction / repost / thread views.
++    fn scan_by_etag<'a>(
++        &'a self,
++        target: &EventId,
++        kinds: &[u32],
++        limit: usize,
++    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
++
++    /// `idx_ptag_time` scan, newest-first. Used by notifications / mention views.
++    fn scan_by_ptag<'a>(
++        &'a self,
++        target: &PubKey,
++        kinds: &[u32],
++        limit: usize,
++    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
++
++    /// `idx_kind_time` scan, newest-first. Used by timeline backfills.
++    /// `kinds` empty = any kind (parity with `scan_by_author_kind`).
++    fn scan_by_kind_time<'a>(
++        &'a self,
++        kinds: &[u32],
++        since: Option<u64>,
++        until: Option<u64>,
++        limit: usize,
++    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
++
++    /// `idx_expires` scan, ascending — used by the NIP-40 reaper.
++    fn scan_expiring_before<'a>(
++        &'a self,
++        unix_seconds: u64,
++        limit: usize,
++    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
++
++    /// Tombstones referencing a target id (typically one row).
++    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError>;
++
++    /// Iterate all tombstones (used by `nmp dump`).
++    fn list_tombstones<'a>(&'a self)
++        -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>;
++
++    /// Provenance sidecar for an event.
++    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError>;
++
++    // ─────── Writes ───────
++
++    /// The single insert path. `source` is the relay that delivered this copy.
++    /// Verifies signature/delegation, applies §7.1 invariants, updates secondaries
++    /// + provenance + tombstones atomically. Returns InsertOutcome per §7.1.
++    fn insert(&self, event: nostr::Event, source: &RelayUrl, received_at_ms: u64)
++        -> Result<InsertOutcome, StoreError>;
++
++    /// Delete by a NMP-internal filter — for admin / GC / kind:5 application.
++    /// Returns the number of primary rows removed.
++    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError>;
++
++    // ─────── Watermarks ───────
++
++    fn read_watermark(&self, key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError>;
++    fn write_watermark(&self, row: WatermarkRow) -> Result<(), StoreError>;
++    fn list_watermarks_for_relay<'a>(
++        &'a self,
++        relay_url: &str,
++    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>;
++
++    // ─────── Hot-set / claims (GC) ───────
++
++    /// Register a claim: caller pins `ids` against eviction until `release`.
++    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError>;
++    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError>;
++
++    /// Soft hint: keep these in hot LRU on a best-effort basis.
++    fn hot_set_hint(&self, ids: &[EventId]) -> Result<(), StoreError>;
++
++    /// One bounded GC pass — reap expired, trim LRU, purge old tombstones.
++    fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError>;
++
++    // ─────── Domain rows (per-DomainModule typed namespace) ───────
++
++    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle<'_>, StoreError>;
++    fn run_migrations(&self, namespace: &'static str, target_version: u32,
++                      migrations: &[crate::substrate::DomainMigration])
++        -> Result<(), StoreError>;
++
++    // ─────── Export ───────
++
++    fn dump(&self, out: &mut dyn std::io::Write, format: DumpFormat)
++        -> Result<DumpStats, StoreError>;
++}
++```
++
++`DeleteFilter` mirrors the limited subset of admin operations the kernel needs (by-relay-only events, by-author, by-id-list, by-kind range); it is **not** a pass-through to `nostr::Filter` — we intentionally do not expose arbitrary remote filters as a delete vector.
++
++## 4. `DomainHandle`
++
++```rust
++pub struct DomainHandle<'env> {
++    pub(crate) namespace: &'static str,
++    pub(crate) inner: DomainHandleInner<'env>,  // backend-specific
++}
++
++impl<'env> DomainHandle<'env> {
++    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError>;
++    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;
++    pub fn delete(&self, key: &[u8]) -> Result<bool, StoreError>;
++    pub fn scan_prefix<'a>(&'a self, prefix: &[u8])
++        -> Result<Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>, StoreError>;
++    pub fn scan_index<'a>(&'a self, index: &'static str, key_prefix: &[u8])
++        -> Result<Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>, StoreError>;
++}
++```
++
++A handle is module-scoped; the kernel does not give a `DraftsModule` handle to `SettingsModule` (per `kernel-substrate.md` §8 "Domain stores are isolated"). The handle is `'env`-bounded so it cannot outlive the LMDB environment.
++
++## 5. Error semantics (doctrine D3)
++
++The trait returns `Result<T, StoreError>`. The actor's wrapper functions map them as:
++
++- `Io / Corrupt` at startup → panic (we cannot run without a store; surfaces to platform shell as a process restart).
++- `Io / Corrupt` mid-run → `Effect::StoreDegraded { details }` published on the diagnostics bridge (ADR-0007); the affected operation returns the closest-fit graceful default (empty iterator, drop-write); the next gc_step retries.
++- `Encoding` → `tracing::error!` with the offending key/namespace; the action that triggered it fails with a `toast: Some("internal storage error; please restart")` per D3.
++- `SchemaTooNew` at startup → publish `Effect::DomainSchemaTooNew { namespace }`, the affected module starts in degraded mode (its actions reject with `ActionRejection::ModuleUnavailable`), rest of the kernel runs.
++- `MigrationFailed` → same as above, plus a one-time toast on first action attempt.
++- `UnknownNamespace` → programming error; assert in debug, log + drop in release.
++
++No `StoreError` ever crosses FFI. The `AppUpdate` carries only successful state + optional `toast: Option<String>`.
++
++## 6. Two backends in v1
++
++```rust
++// In-memory backend, kept for tests + web-pre-M15.
++pub struct MemEventStore { /* HashMap-backed; preserves the current kernel state */ }
++
++// Production backend on iOS / Android / Desktop.
++pub struct LmdbEventStore { /* wraps nostr_lmdb::NostrLMDB + NMP sub-dbs */ }
++
++pub fn open_event_store(cfg: &AppConfig) -> Result<Box<dyn EventStore>, StoreError> {
++    match cfg.storage_backend {
++        StorageBackend::Memory => Ok(Box::new(MemEventStore::new())),
++        StorageBackend::Lmdb { ref path } => Ok(Box::new(LmdbEventStore::open(path)?)),
++    }
++}
++```
++
++`MemEventStore` implements every method using `HashMap` / `BTreeMap`. The same test suite runs against both backends with `#[cfg(feature = "lmdb-backend")]` gating only the LMDB-specific edge tests (corruption recovery, oversized values).
+diff --git a/docs/design/lmdb/watermarks.md b/docs/design/lmdb/watermarks.md
+new file mode 100644
+index 0000000..e6b4333
+--- /dev/null
++++ b/docs/design/lmdb/watermarks.md
+@@ -0,0 +1,191 @@
++# LMDB sub-design: watermarks, provenance, migrations
++
++> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Covers the three NMP-owned LMDB sub-databases that hold non-event durable state: `watermarks`, `provenance`, and the per-`DomainModule` sub-databases tracked by `_meta`.
++
++## 1. Watermark table
++
++Sub-db: `watermarks`. Populated by the M4 NIP-77 negentropy engine and consulted by the M2 subscription planner (per `docs/product-spec/subsystems.md` §7.2 "coverage-aware backfill").
++
++Key: `filter_hash[32] ‖ relay_url_bytes` — exact-key lookups only; no scans by `filter_hash` alone are needed (the planner always knows the relay it's about to query). The relay url is appended raw (no length prefix) because lookups are exact-key and the trailing bytes have no semantic role beyond identification.
++
++Value: CBOR `WatermarkRow`:
++
++```rust
++#[derive(Serialize, Deserialize, Clone, Debug)]
++pub struct WatermarkRow {
++    pub key: WatermarkKey,
++    pub synced_up_to: u64,                 // unix seconds
++    pub last_sync_method: SyncMethod,      // Negentropy | ReqScan | Manual
++    pub last_negentropy_state: Option<Vec<u8>>, // engine-opaque resume blob (M4)
++    pub bytes_saved_vs_req: u64,           // cumulative; for diagnostics
++    pub updated_at: u64,                   // unix seconds
++}
++```
++
++`last_negentropy_state` is an opaque byte blob written by the NIP-77 engine (M4) — the store does not interpret it. Empty for `ReqScan` / `Manual` rows.
++
++### 1.1 Authoritative cache-miss semantics
++
++Per §7.1 of the spec: "A cache-miss query against a fully-synced `(filter, relay)` pair is **authoritative**." The store implements this via the read path:
++
++```rust
++pub fn coverage(&self, key: &WatermarkKey, now_s: u64) -> Coverage {
++    match self.read_watermark(key) {
++        Ok(Some(row)) if row.synced_up_to >= now_s.saturating_sub(self.cfg.coverage_staleness_secs) =>
++            Coverage::CompleteAsOf(row.synced_up_to),
++        Ok(Some(row)) => Coverage::PartialUpTo(row.synced_up_to),
++        Ok(None) => Coverage::Unknown,
++        Err(_) => Coverage::Unknown,  // degraded; do not lie about completeness
++    }
++}
++```
++
++`coverage_staleness_secs` defaults to 300 s — a row that hasn't been re-confirmed in 5 minutes is treated as partial. The planner uses this signal to decide whether a missing-event query is "doesn't exist" (CompleteAsOf) or "need to fetch" (PartialUpTo / Unknown).
++
++### 1.2 Restart hydration
++
++On `LmdbEventStore::open()`, the store reads all `watermarks` rows and builds an in-memory `HashMap<WatermarkKey, WatermarkRow>` for hot lookups. Every `write_watermark` updates both the in-memory map and the LMDB row in a single `RwTxn`. Restart re-derives the map; we don't need a separate cache file.
++
++For installations with O(100k+) watermarks (an edge case — typical apps see O(100)–O(10k)), the in-memory map switches to a lazy-loaded variant that pages on demand. Threshold and switching logic deferred to M4 when the negentropy engine's real-world row count is measured.
++
++## 2. Provenance
++
++Sub-db: `provenance`. Per-event sidecar; the master doc §9 justifies the split-table choice.
++
++Key: `event_id[32]`. Value: CBOR `ProvenanceRow`:
++
++```rust
++#[derive(Serialize, Deserialize, Clone, Debug)]
++pub struct ProvenanceRow {
++    pub sources: SmallVec<[ProvenanceEntry; 4]>,    // bounded at 32; see master doc §9
++}
++
++#[derive(Serialize, Deserialize, Clone, Debug)]
++pub struct ProvenanceEntry {
++    pub relay_url: String,
++    pub first_seen_ms: u64,
++    pub last_seen_ms: u64,
++    pub primary: bool,
++}
++```
++
++The `primary: bool` is set deterministically: after every mutation, the `sources` vec is sorted by `(first_seen_ms, relay_url)` and the head element gets `primary = true`, all others `false`. This produces a stable "first observed relay" identifier regardless of the order in which redeliveries are processed.
++
++### 2.1 Mutation hot path
++
++For a duplicate-id insert, the per-event provenance write is the **only** LMDB write (per [`keys.md`](keys.md) §9 worked example). The store reads the existing row, mutates the matching entry's `last_seen_ms` (or appends), re-sorts + recomputes `primary`, and writes it back. Total cost: 1 read + 1 write on a 4-element CBOR row — well under 50 µs on iPhone 12 NAND.
++
++The bound of 32 distinct relays per event is empirical: in practice an event is delivered by 1–6 relays; events propagated to "everywhere" (popular kind:0 / kind:3) might hit 15–25. The 32nd entry overwrites the oldest non-primary slot, preserving the primary anchor.
++
++### 2.2 Reuse in outbox routing (M2)
++
++The M2 subscription planner consults `provenance_for(id)` to learn which relays have already delivered an event when scoring per-relay coverage in `LogicalInterestStatus::relay_urls`. This avoids re-fetching the same event from relays we already know carry it. The provenance sidecar is also part of the ADR-0007 diagnostics bridge — the diagnostics screen shows per-event source counts in the firehose tap view.
++
++## 3. Filter canonicalisation (for `filter_hash`)
++
++The `filter_hash` field in `WatermarkKey` is BLAKE3 of the canonical filter encoding. Canonicalisation rules:
++
++1. Within each tag-value array (`#e`, `#p`, `#a`, etc.), sort ascending bytewise.
++2. Sort the `kinds` array ascending numerically.
++3. Sort the `authors` array ascending bytewise.
++4. Sort the `ids` array ascending bytewise.
++5. Encode the filter as CBOR with map keys in this lexicographic order: `authors`, `ids`, `kinds`, `since`, `until`, `limit`, `search`, then `#<tag>` keys in ascending tag-letter order.
++6. BLAKE3-hash the resulting bytes.
++
++This produces a deterministic hash that is stable across `Filter` field-order variations and across Rust HashMap ordering randomness. The implementation lives at `crates/nmp-core/src/store/watermarks.rs::canonical_filter_hash(&Filter) -> [u8; 32]` and is the single source of truth for the planner + sync engine + dump format.
++
++A filter with `limit: Some(N)` produces a *different* hash than the same filter without `limit` — because their cache-coverage semantics genuinely differ. A planner that wants to share a watermark across "limit=200" and "limit=500" requests of the same shape strips `limit` before hashing (this is a planner-side optimisation, not a store-side one).
++
++## 4. Migration plumbing
++
++Per `kernel-substrate.md` §2: `DomainModule` declares `SCHEMA_VERSION` and `migrations()`. The store handles applying them at startup.
++
++### 4.1 Registry extension required
++
++The existing `ModuleRegistry` (`crates/nmp-core/src/substrate/mod.rs:36-79`) stores only `ModuleDescriptor { namespace, family, rust_type }` — the concrete `M: DomainModule` type is consumed by the generic `register_domain::<M>()` call and not retained, so the store has no runtime path from a namespace string back to `M::SCHEMA_VERSION` or `M::migrations()`. M3 extends `ModuleDescriptor` for the Domain family with two `fn`-pointer factories — matching the existing `DomainIndex::key_fn: fn(&[u8]) -> ...` pattern (`substrate/domain.rs:18`):
++
++```rust
++// Added in M3 — substrate/domain.rs
++pub struct DomainFactories {
++    pub schema_version: fn() -> u32,
++    pub migrations: fn() -> Vec<DomainMigration>,
++    pub indexes: fn() -> Vec<DomainIndex>,
++}
++
++// ModuleRegistry::register_domain becomes:
++pub fn register_domain<M: DomainModule>(&mut self) {
++    let factories = DomainFactories {
++        schema_version: || M::SCHEMA_VERSION,
++        migrations: M::migrations,
++        indexes: M::indexes,
++    };
++    self.push_domain::<M>(M::NAMESPACE, factories);
++}
++```
++
++The store reads these factories at open time. No `Box<dyn DomainModule>` is required, no trait object-safety constraints are imposed on `DomainModule`, and the change is additive to the existing trait.
++
++### 4.2 Startup sequence
++
++```rust
++pub fn open(path: &Path, modules: &ModuleRegistry) -> Result<Self, StoreError> {
++    let env = open_lmdb_environment(path)?;
++    let meta = env.open_db(Some("_meta"))?;
++    let mut store = Self::bootstrap(env)?;
++
++    // _kernel schema version
++    store.migrate_kernel_schema(&meta)?;
++
++    // each registered DomainModule
++    for (namespace, factories) in modules.domain_factories() {
++        let current = store.read_meta_schema_version(namespace)?;
++        let target = (factories.schema_version)();
++        let mut applied = current;
++        let mut steps = (factories.migrations)();
++        steps.retain(|m| m.from_version >= current && m.to_version <= target);
++        steps.sort_by_key(|m| m.from_version);
++        for step in steps {
++            store.run_migration_step(namespace, step)?;
++            applied = step.to_version;
++            store.write_meta_schema_version(namespace, applied)?;
++        }
++        if applied < target {
++            return Err(StoreError::MigrationFailed { /* missing step */ });
++        }
++        if applied > target {
++            return Err(StoreError::SchemaTooNew { /* downgrade */ });
++        }
++    }
++    Ok(store)
++}
++```
++
++Each `run_migration_step` opens its own `RwTxn`, calls `step.apply(&mut migration_tx)`, drains `migration_tx.writes()` into the relevant sub-db, and commits. Either the whole step lands atomically or LMDB rolls it back on commit failure.
++
++### 4.3 Rollback semantics
++
++LMDB does not support cross-process downgrade; once `_meta.<namespace>.schema_version` is bumped, there is no "undo." Therefore:
++
++- If migration step N fails: `_meta` is **not** bumped; module starts in degraded mode (per [`trait.md`](trait.md) §5); user-visible diagnostic surfaces the failure.
++- If migration step N succeeds but N+1 fails: `_meta` is at N (the highest successful step). The module is "partly migrated"; the same degraded-mode handling applies; on next startup the runner retries from N → N+1.
++- If the user actually needs to downgrade (a forensics use case), they delete the sub-db and re-sync from relays. The `nmp dump` format is the supported escape hatch.
++
++### 4.4 Cross-module migration coordination (deferred — see open questions)
++
++`kernel-substrate.md` §12 already flags "cross-module migration coordination" as an open question. M3 implements the per-module path; cross-module coordination is deferred to a future module that needs it. The trait surface does not preclude adding it later: a future `Vec<NamespaceDependency>` field on `DomainMigration` would let the runner topologically order modules' migrations.
++
++## 5. The kernel store's own schema
++
++The kernel's internal LMDB layout (provenance + secondaries + tombstones + watermarks) is itself versioned via the reserved `_kernel` namespace in `_meta`. Current version: `1`. A future schema change (e.g., a new secondary index) bumps this and runs a one-time rebuild from primary events.
++
++The schema migration for `_kernel` v1 → v2 is privileged: it can read every sub-db and rewrite as needed. It runs **before** any `DomainModule` migration so module code never sees a stale kernel schema.
++
++## 6. Diagnostics surface
++
++`WatermarkRow` and `ProvenanceRow` are read by the ADR-0007 diagnostics bridge:
++
++- `LogicalInterestStatus.cache_coverage` resolves from `coverage()` (§1.1 above) for the relevant `(filter_hash, relay_url)` pair.
++- The firehose-tap diagnostic surfaces per-event provenance: which relays contributed each row, with `primary` flagged.
++- `StoreHealth.watermark_count` (per [`gc.md`](gc.md) §7) summarises the global count.
++
++These are read-mostly paths; the only contention is between the actor's `write_watermark` calls (driven by M4 sync completions) and the diagnostics query, which is bounded by LMDB's MVCC reader semantics — no lock contention.
+
+Review: (1) doctrine compliance (in particular D0 + D2 — composite reverse index + working-set bound must remain enforced through the trait); (2) any TODO/FIXME/unimplemented; (3) file-size compliance; (4) consistency of EventStore trait surface across the 6 sub-docs; (5) the choice of nostr-lmdb as on-disk byte store vs roll-our-own — is the gap analysis honest?; (6) the GC eviction algorithm + budget; (7) the migration plumbing's testability; (8) any hidden shortcuts. Be terse; name file:line + fix for any concern.
+codex
+I’ll ground the review in the current tree rather than just the pasted diff: first the local doctrine/plan/spec docs, then the six new design files for line-level consistency and size issues.
+exec
+/bin/zsh -lc 'pwd && rg -n "Doctrine|D0|D1|D2|D3|D4|D5|composite reverse|working-set|working set|EventStore|nostr-lmdb|TODO|FIXME|unimplemented|todo!|panic!" docs crates AGENTS.md' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+/Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/orchestration-log.md:9:| 2026-05-18 01:55 | 1 | Wave 1 completed in ~30 min. Landed: T7 build-verifier f1e374b (all gates green, sim screenshot captured); T6 debt-auditor d660735 (0 TODO/FIXME, 0 unimplemented in FFI surface, 4 must-fix safety-comment items); T2 m2-designer 031fc07 (subscription compilation spec, 8 files); T3 m3-designer 9fead0e (LMDB schema spec, 6 files); T5 m11-designer 0dfb975 (M11 podcast design, 13 files); T4 m105-designer's ffi-hardening files (5) absorbed into orchestrator commit fcf8b43. Three codex post-merge reviews ran: review-1 on the wave-1 cumulative diff flagged 5 issues (debt-inventory exit-ready language, NIP-XX placeholder, ADR-deferral escape, file sizes); review-2 on fcf8b43 flagged 6 issues (D5 mismatch, pre-merge CI gate, freed-pointer UB, S10 conditional, file sizes again, symbol count); review-3 on 0dfb975 flagged 7 issues (Swift file sizes 898/642, MCP-in-CI, AudioPlayback policy, EmbeddingId cycle, rig.rs weakened, OPML pixel-parity, view count). All flagged issues are being addressed via dedicated fix-it tasks T8 (codex-fixer-1) and T11 (codex-fixer-2). T1 (m1-hardener) unblocked + dispatched. T9 (ffi-safety-commenter) dispatched for the 4 must-fix items. Post-merge codex review protocol established and entered memory. |
+docs/perf/m1/build-verification.md:139:- Simulator chosen: **iPhone 17 / iOS 26.5** (UUID `C380BA01-AD22-4F4A-8524-A309DA15144C`)
+docs/aim.md:101:### 4.1 Reactive single source of truth ("EventStore")
+docs/aim.md:219:## 6. Doctrine — the rules the API must make hard to violate
+docs/aim.md:253:- **`rust-nostr`** workspace on GitHub — the protocol foundation. We depend on its `nostr`, `nostr-sdk`, `nostr-database`, `nostr-lmdb`, `nostr-ndb`, `nostr-sqlite`, `nostr-gossip`, `nostr-connect`, `nostr-keyring`, `nostr-blossom`, `nostr-relay-builder`, and `nwc` crates.
+docs/perf/firehose-bench/2026-05-17-comprehensive-findings.md:73:- `working_set_100_views`: 1,000,000 cached events, 10,000 hot events, 100 open views, about 19.79 MB modeled hot working-set memory.
+docs/perf/firehose-bench/2026-05-17-comprehensive-findings.md:78:The reactivity model is directionally strong. Composite reverse indexes, per-view delta gates, batching, coalescing, and a hot/cold working-set split are the right defaults. The benchmark specifically supports the Applesauce/NDK-derived lesson that the UI should express interest declaratively while the core dedupes work and emits bounded deltas.
+docs/perf/firehose-bench/2026-05-17-comprehensive-findings.md:82:The memory figures are modeled as actor hot working set plus indexes and open views. Cold event bodies are treated as disk-resident. The allocation measurement uses a process-wide counting allocator and only samples the post-warmup hot path.
+docs/perf/firehose-bench/2026-05-17-comprehensive-findings.md:165:relay <-> rust actor <-> durable store / hot working set
+docs/perf/firehose-bench/2026-05-17-comprehensive-findings.md:203:3. Add focused unit tests around the composite reverse index, coalescer, and domain-keyed wrapper lifecycle.
+docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+docs/perf/m10.5/debt-inventory.md:169:**Issue:** Casts a `usize` (which can be >u32 on 64-bit systems) to `u32` after explicit saturation at `u32::MAX`. This is intentional: if there are more than 2^32 profile claims (which will not occur in practice given working-set bounds), cap the refcount at u32::MAX. No silent overflow.
+docs/perf/m10.5/debt-inventory.md:195:**Issue:** Casting collection `.count()` and arithmetic results to metric types. No overflow risk because counts are bounded by working set size constraints (max 5,000 stored events per ADR-0001; max visible 500 per FFI clamp).
+docs/perf/m10.5/debt-inventory.md:270:## 3. Doctrine Violations
+docs/perf/m10.5/debt-inventory.md:272:### D0 Audit: Kernel Never Grows App Nouns
+docs/perf/m10.5/debt-inventory.md:286:### D1 Audit: Best-Effort Rendering with Placeholders
+docs/perf/m10.5/debt-inventory.md:299:### D2 Audit: Reactivity Contract (Composite Reverse Index)
+docs/perf/m10.5/debt-inventory.md:313:### D3 Audit: Errors Never Cross FFI
+docs/perf/m10.5/debt-inventory.md:327:### D4 Audit: One Writer Per Fact
+docs/perf/m10.5/debt-inventory.md:341:### D5 Audit: Capabilities Report, Never Decide
+docs/perf/m10.5/debt-inventory.md:359:| 14 integer casts (count→metric types) (F6–F14) | Justified | All bounded by design constraints (metrics counters, working-set bounds); no overflow risk. Casts are intentional and safe. |
+docs/perf/m10.5/debt-inventory.md:361:| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
+docs/perf/m10.5/debt-inventory.md:362:| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
+docs/perf/m10.5/debt-inventory.md:383:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/m10.5/debt-inventory.md:396:The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/m10.5/debt-inventory.md:404:- Doctrine review signed in writing in `docs/perf/m10.5/doctrine-review.md`
+docs/perf/reactivity-bench/1779045194-run-002.json:655:        "working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100",
+docs/perf/reactivity-bench/1779045194-run-002.json:662:    "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.",
+crates/nmp-testing/bin/reactivity-bench/scenario.rs:281:                "working-set gate scenario: cached_events={}, hot_events={}, open_views={}",
+docs/perf/reactivity-bench/1779050935-run-002.md:44:- working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100
+docs/perf/reactivity-bench/1779050935-run-002.md:49:- Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.
+docs/plan.md:9:> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
+docs/plan.md:19:- **Kernel substrate** in `crates/nmp-core` (~3,800 LOC): actor on a dedicated OS thread, mailbox-driven (ADR feedback adopted — relay reads happen in tokio reader tasks, the actor blocks on its own channel with deadline timeouts), substrate trait families (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`) in `nmp-core/src/substrate/`, ingest pipeline (`kernel/ingest.rs`), claim/release refcounting for profile interest (commit `23ae829`), composite reverse-index dependency tracking.
+docs/plan.md:20:- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
+docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
+docs/plan.md:23:- **Benches** in `crates/nmp-testing`: `reactivity-bench` (composite-key reverse index + coalescer + working set; run 002 passed all ADR-0001..0004 gates) and `firehose-bench` (replay + capture + live modes; replay scenarios pass the modeled budget contract).
+docs/plan.md:65:4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
+docs/plan.md:66:5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
+docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
+docs/plan.md:100:- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
+docs/plan.md:147:**Scope.** Swap in-memory `EventStore` for LMDB via `Box<dyn EventStore>`. Implement the full insert invariants from `product-spec.md` §7.1: parameterized replaceable events (kind 30000–39999 by `(pubkey, kind, d-tag)`), kind:5 delete handling with tombstone persistence, NIP-40 expiration scheduling, dedup with provenance merge, claim-based GC running.
+docs/plan.md:152:- `EventStore` trait abstracted; LMDB backend; in-memory backend kept for tests.
+docs/plan.md:154:- GC working set policy per ADR-0003: hot ≤ 10k events resident + claim-pinned set; cold on disk.
+docs/plan.md:271:**Scope.** Per spec doctrine D4 (single writer per fact) extended to account scope:
+docs/plan.md:335:- Profile picture update through compose → kind:0 republish with new Blossom URL → in-place refinement across all open Profile / Timeline payloads (per doctrine D1).
+docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/plan.md:356:  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
+docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Hard zero — no deferral escape, no "tracking issue" carve-out. Every pre-existing one is resolved in M10.5. If something genuinely cannot be done in M10.5 because it belongs to a later milestone (e.g. NIP-65 outbox work), then it is not a TODO/FIXME in the scoped files — it lives as a milestone task in `docs/plan.md`, not as a code marker.
+docs/plan.md:380:- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
+docs/plan.md:717:| Subsystem integration | `cargo test --test '*'` in `nmp-testing` | EventStore + planner + sync against MockRelay | `crates/nmp-testing/tests/` |
+docs/plan.md:720:| Reactivity bench | `reactivity-bench --standard --fail-on-gate` | Composite reverse index, delta coalescing, working-set memory, allocation gates | `crates/nmp-testing/bin/reactivity-bench/` |
+docs/plan.md:769:Every design doc has measurable gates. Gates run on the reactivity-bench harness (or `firehose-bench` for end-to-end behavior). Failures revise the design **before** implementation. Pre-implementation measurement is cheaper than post-implementation rework. Run 001 of reactivity-bench established the pattern: the reverse-index direction was validated (100×–1000× headroom), one design refinement landed (composite keys), and two budget bugs surfaced (per-view delta, working-set memory) — all before any view-kind code shipped.
+crates/nmp-testing/bin/reactivity-bench/main.rs:23:            "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.".to_string(),
+docs/perf/codex-reviews/031fc07.md:15:You are reviewing merge 031fc07 (M2 subscription compilation + outbox routing design) on master in nostr-multi-platform. Doctrine D0-D5 (D0 kernel never grows app nouns, D1 best-effort rendering, D2 reactivity contract, D3 no errors across FFI, D4 one writer per fact, D5 capabilities report don't decide). File size: 300 LOC soft, 500 hard.
+docs/perf/codex-reviews/031fc07.md:547:+- **Mailbox cache exists but no consumer.** `crates/nmp-core/src/kernel/ingest.rs:209-233` (`ingest_relay_list`) already parses kind:10002 into `self.author_relay_lists: HashMap<String, AuthorRelayList>` (declared at `crates/nmp-core/src/kernel/mod.rs:269-275` and reserved at `mod.rs:313`). The cache is written; **nothing reads it for routing**. This is the bug doctrine D5 ("capabilities report, never decide") inverted: we have the data, we ignore it.
+docs/perf/codex-reviews/031fc07.md:1112:+- The local store insert happens *before* the publish step (optimistic insert), with rollback on `PartiallyFailed` if `required_success_count` is not met. This matches the "atomic with reversibility" reading of doctrine D4 (single writer per fact).
+docs/perf/codex-reviews/031fc07.md:1567:Review for: (1) doctrine compliance (in particular D0 — does nmp-nip65 propose any kernel-side type leaks? D4 — single source for relay-set facts?); (2) any TODO/FIXME/unimplemented; (3) file-size compliance (any new file > 300 LOC?); (4) internal consistency across the 8 sub-docs; (5) compatibility with the upcoming M3 LMDB schema (e.g. watermark table); (6) the planned test (m2_subscription_compilation_audit) exit criteria; (7) any hidden shortcuts. Be terse; name file:line + fix for any concern.
+docs/perf/codex-reviews/031fc07.md:1578:/bin/zsh -lc 'rg -n "nostr-multi-platform|subscription compilation|M2|031fc07|D0|D4|nmp-nip65|outbox" /Users/pablofernandez/.codex/memories/MEMORY.md' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/031fc07.md:1580:1384:- the compile-safe verification path here was `xcodebuild build -scheme Podcastr -project Podcastr.xcodeproj -destination 'platform=iOS Simulator,id=18354708-738D-4D15-B86C-4FDED4D43A93'`, followed by `xcrun simctl launch booted io.f7z.podcast` [Task 1]
+docs/perf/codex-reviews/031fc07.md:1581:1491:- the useful focused verification command was `xcodebuild -project Podcastr.xcodeproj -scheme Podcastr -destination 'id=18354708-738D-4D15-B86C-4FDED4D43A93' -derivedDataPath Derived -only-testing:PodcastrTests/PersistenceDurabilityTests -only-testing:PodcastrTests/OPMLImportTests -only-testing:PodcastrTests/DataExportTests -only-testing:PodcastrTests/AppStateStorePerformanceTests test`, which passed with `32 tests, 0 failures` in the clean worktree [Task 2]
+docs/perf/codex-reviews/031fc07.md:1621:/bin/zsh -lc 'rg -n "TODO|FIXME|unimplemented|panic!|todo!|unwrap'"\\(|TBD|stub|placeholder|does not exist yet|proposed|deferred|open question|future|post-M2|M6|M3|M4|M5|M8|M9\" docs/design/subscription-compilation.md docs/design/subscription-compilation" in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/031fc07.md:1698:/bin/zsh -lc 'rg -n "D0|D1|D2|D3|D4|D5|Doctrine|kernel never grows|single writer|capabilities report|M2|subscription compiler|subscription compilation|outbox|NIP-65|watermark|LMDB|relay-set|relay set" docs/plan.md docs/aim.md docs/product-spec/subsystems.md docs/design/kernel-substrate.md docs/design/app-extension-kernel.md docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md docs/design/ndk-applesauce-lessons.md docs/design/reactivity/*.md' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/031fc07.md:1700:docs/design/reactivity/validation-harness.md:39:- Spawns a configurable `EventStore` (in-memory, LMDB, or SQLite backend).
+docs/perf/codex-reviews/031fc07.md:1721:docs/design/kernel-substrate.md:173:- Pre-formatted display fields per doctrine D1.
+docs/perf/codex-reviews/031fc07.md:1732:docs/aim.md:219:## 6. Doctrine — the rules the API must make hard to violate
+docs/perf/codex-reviews/031fc07.md:1739:docs/product-spec/subsystems.md:74:Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
+docs/perf/codex-reviews/031fc07.md:1743:docs/product-spec/subsystems.md:181:**Best-effort field contract (per doctrine D1).** Every display-bearing field in every view payload is **non-optional** and has a defined placeholder when the underlying data is missing:
+docs/perf/codex-reviews/031fc07.md:1744:docs/product-spec/subsystems.md:242:Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.
+docs/perf/codex-reviews/031fc07.md:1750:docs/plan.md:9:> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
+docs/perf/codex-reviews/031fc07.md:1751:docs/plan.md:20:- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
+docs/perf/codex-reviews/031fc07.md:1754:docs/plan.md:65:4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
+docs/perf/codex-reviews/031fc07.md:1755:docs/plan.md:66:5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
+docs/perf/codex-reviews/031fc07.md:1756:docs/plan.md:100:- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
+docs/perf/codex-reviews/031fc07.md:1767:docs/plan.md:147:**Scope.** Swap in-memory `EventStore` for LMDB via `Box<dyn EventStore>`. Implement the full insert invariants from `product-spec.md` §7.1: parameterized replaceable events (kind 30000–39999 by `(pubkey, kind, d-tag)`), kind:5 delete handling with tombstone persistence, NIP-40 expiration scheduling, dedup with provenance merge, claim-based GC running.
+docs/perf/codex-reviews/031fc07.md:1769:docs/plan.md:152:- `EventStore` trait abstracted; LMDB backend; in-memory backend kept for tests.
+docs/perf/codex-reviews/031fc07.md:1774:docs/plan.md:271:**Scope.** Per spec doctrine D4 (single writer per fact) extended to account scope:
+docs/perf/codex-reviews/031fc07.md:1776:docs/plan.md:335:- Profile picture update through compose → kind:0 republish with new Blossom URL → in-place refinement across all open Profile / Timeline payloads (per doctrine D1).
+docs/perf/codex-reviews/031fc07.md:1777:docs/plan.md:356:  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
+docs/perf/codex-reviews/031fc07.md:1778:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Hard zero — no deferral escape, no "tracking issue" carve-out. Every pre-existing one is resolved in M10.5. If something genuinely cannot be done in M10.5 because it belongs to a later milestone (e.g. NIP-65 outbox work), then it is not a TODO/FIXME in the scoped files — it lives as a milestone task in `docs/plan.md`, not as a code marker.
+docs/perf/codex-reviews/031fc07.md:1779:docs/plan.md:380:- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
+docs/perf/codex-reviews/031fc07.md:1914:/bin/zsh -lc 'rg -n "nmp-nip65|nmp-core|protocol module|DomainModule|ViewModule|ActionModule|extension|module composition|LMDB|watermark|EventStore|mailbox|relay" docs/design docs/product-spec docs/decisions | head -n 200' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/031fc07.md:1940:docs/decisions/0009-app-extension-kernel-boundary.md:88:- **Two fixture apps prove the boundary in v1.** A tiny non-Nostr-shaped fixture (e.g., a TODO/notes module with an app-local identity) lands in 1a.1; the Twitter clone is the first Nostr-shaped extension module. Together they prove the kernel works in both directions.
+docs/perf/codex-reviews/031fc07.md:1960:docs/product-spec/cli-toolchain-phasing.md:174:- **EventStore.** The reactive single source of truth for all Nostr events. Owned by the actor; not exposed at FFI.
+docs/perf/codex-reviews/031fc07.md:1962:docs/product-spec/cli-toolchain-phasing.md:177:- **View.** A pre-built derived projection of `EventStore` contents. Opened by `OpenView` action; payload arrives via `AppState.views` / `ViewBatch`.
+docs/perf/codex-reviews/031fc07.md:1964:docs/product-spec/subsystems.md:7:### 7.1 EventStore
+docs/perf/codex-reviews/031fc07.md:1982:docs/product-spec/subsystems.md:74:Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
+docs/perf/codex-reviews/031fc07.md:2002:docs/product-spec/subsystems.md:242:Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.
+docs/perf/codex-reviews/031fc07.md:2003:docs/product-spec/subsystems.md:247:View opens → Live REQ handler starts → Planner consults coverage → Sync engine reconciles gaps → EventStore inserts → ViewBatch emits
+docs/perf/codex-reviews/031fc07.md:2027:docs/product-spec/appendices.md:17:The event store, gossip cache, sync watermarks, working set, and signer state all live in the actor and **never cross FFI**.
+docs/perf/codex-reviews/031fc07.md:2063:docs/product-spec/overview-and-dx.md:31:### D0. Kernel + extension modules — no app nouns in `nmp-core`
+docs/perf/codex-reviews/031fc07.md:2067:docs/product-spec/overview-and-dx.md:60:### D3. Outbox routing is automatic; manual relay selection is the opt-out
+docs/perf/codex-reviews/031fc07.md:2091:docs/decisions/0006-vertical-slice-first.md:13:The classic failure mode at this stage is **horizontal expansion** — building "the EventStore" comprehensively, then "the planner" comprehensively, then "the views" comprehensively, then finally stitching them together at the end, only to discover that the FFI surface or the relay adapter or the storage backend doesn't actually compose the way the model assumed.
+docs/perf/codex-reviews/031fc07.md:2095:docs/decisions/0006-vertical-slice-first.md:48:│  EventStore (minimal)                                        │
+docs/perf/codex-reviews/031fc07.md:2107:docs/decisions/0006-vertical-slice-first.md:94:- A real WebSocket → real EventStore → real DeltaBuffer → real component update is measurable end-to-end.
+docs/perf/codex-reviews/031fc07.md:2112:docs/decisions/0006-vertical-slice-first.md:112:- **Subsequent expansion has a working substrate to build on.** Adding LMDB is a `Box<dyn EventStore>` swap. Adding multi-relay is a planner change. Adding negentropy is a planner change. Adding iOS is a UniFFI wrap of the existing actor. None require redesigning the architecture.
+docs/perf/codex-reviews/031fc07.md:2561:   215	- The local store insert happens *before* the publish step (optimistic insert), with rollback on `PartiallyFailed` if `required_success_count` is not met. This matches the "atomic with reversibility" reading of doctrine D4 (single writer per fact).
+docs/perf/codex-reviews/031fc07.md:3465:    15	- **Mailbox cache exists but no consumer.** `crates/nmp-core/src/kernel/ingest.rs:209-233` (`ingest_relay_list`) already parses kind:10002 into `self.author_relay_lists: HashMap<String, AuthorRelayList>` (declared at `crates/nmp-core/src/kernel/mod.rs:269-275` and reserved at `mod.rs:313`). The cache is written; **nothing reads it for routing**. This is the bug doctrine D5 ("capabilities report, never decide") inverted: we have the data, we ignore it.
+docs/perf/codex-reviews/031fc07.md:3656:    74	Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
+docs/perf/codex-reviews/031fc07.md:3717:docs/aim.md:101:### 4.1 Reactive single source of truth ("EventStore")
+docs/perf/codex-reviews/031fc07.md:3732:docs/aim.md:219:## 6. Doctrine — the rules the API must make hard to violate
+docs/perf/codex-reviews/031fc07.md:3832:docs/product-spec/subsystems.md:7:### 7.1 EventStore
+docs/perf/codex-reviews/031fc07.md:3913:   219	## 6. Doctrine — the rules the API must make hard to violate
+docs/perf/codex-reviews/031fc07.md:4055:     7	### 7.1 EventStore
+docs/perf/codex-reviews/031fc07.md:4387:- [nip65.md:192](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:192) plus [nip65.md:197](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:197): `nmp-nip65` depends on `nmp-core`, while `nmp-core::kernel::planner` consumes `nmp-nip65`. That is a Rust crate cycle and blurs D0. Fix by moving mailbox traits/types into `nmp-core` or a tiny no-core crate, with `nmp-nip65` only registering module behavior.
+docs/perf/codex-reviews/031fc07.md:4389:- [nip65.md:6](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:6), [nip65.md:98](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:98), [compiler.md:52](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:52): mailbox facts have too many apparent writers/read models: `ingest_relay_list`, `MailboxCache`, and `MailboxesView::on_event_inserted`. D4 wants one writer per fact. Fix by making one canonical mailbox projection/cache writer; views only read that projection.
+docs/perf/codex-reviews/031fc07.md:4397:- [compiler.md:166](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:166) and [compiler.md:184](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:184): startup timeline/profile/contact behavior remains framed as `nmp-core` migration work. D0 fix: move social bootstrap modules to `nmp-nip01` / `nmp-nip02` / demo app code; core should only execute registered module interests.
+docs/perf/codex-reviews/031fc07.md:4401:Other checks: no literal `TODO`, `FIXME`, or `unimplemented!` in the added docs. File sizes are compliant: all new files are under 300 LOC, largest is `tests.md` at 273. The planned audit test still has weak spots: [tests.md:34](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:34) depends on non-existent `AppActionMeta`, and [tests.md:213](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:213) does not actually assert lane distinctness.
+docs/perf/codex-reviews/031fc07.md:4410:- [nip65.md:192](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:192) plus [nip65.md:197](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:197): `nmp-nip65` depends on `nmp-core`, while `nmp-core::kernel::planner` consumes `nmp-nip65`. That is a Rust crate cycle and blurs D0. Fix by moving mailbox traits/types into `nmp-core` or a tiny no-core crate, with `nmp-nip65` only registering module behavior.
+docs/perf/codex-reviews/031fc07.md:4412:- [nip65.md:6](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:6), [nip65.md:98](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:98), [compiler.md:52](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:52): mailbox facts have too many apparent writers/read models: `ingest_relay_list`, `MailboxCache`, and `MailboxesView::on_event_inserted`. D4 wants one writer per fact. Fix by making one canonical mailbox projection/cache writer; views only read that projection.
+docs/perf/codex-reviews/031fc07.md:4420:- [compiler.md:166](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:166) and [compiler.md:184](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:184): startup timeline/profile/contact behavior remains framed as `nmp-core` migration work. D0 fix: move social bootstrap modules to `nmp-nip01` / `nmp-nip02` / demo app code; core should only execute registered module interests.
+docs/perf/codex-reviews/031fc07.md:4424:Other checks: no literal `TODO`, `FIXME`, or `unimplemented!` in the added docs. File sizes are compliant: all new files are under 300 LOC, largest is `tests.md` at 273. The planned audit test still has weak spots: [tests.md:34](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:34) depends on non-existent `AppActionMeta`, and [tests.md:213](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:213) does not actually assert lane distinctness.
+docs/decisions/0009-app-extension-kernel-boundary.md:76:- ADR-0003 (working-set memory): applies to the kernel's hot/cold split regardless of module count.
+docs/decisions/0009-app-extension-kernel-boundary.md:81:The doctrines from `product-spec.md` §1.5 (D1 best-effort rendering, D2 negentropy first, D3 outbox automatic, D4 single writer per fact, D5 snapshots bounded by what's open) survive intact and apply across all modules.
+docs/decisions/0009-app-extension-kernel-boundary.md:88:- **Two fixture apps prove the boundary in v1.** A tiny non-Nostr-shaped fixture (e.g., a TODO/notes module with an app-local identity) lands in 1a.1; the Twitter clone is the first Nostr-shaped extension module. Together they prove the kernel works in both directions.
+docs/perf/reactivity-bench/1779051783-run-002.json:655:        "working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100",
+docs/perf/reactivity-bench/1779051783-run-002.json:662:    "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.",
+docs/decisions/0005-domain-keyed-platform-shadow.md:59:- **Three-tier data model becomes explicit.** Rust durable storage → Rust working set + projections → platform domain-keyed shadow. Each layer derives from the layer below; only Rust is source of truth.
+docs/perf/codex-reviews/fcf8b43.md:15:You are reviewing merge fcf8b431b8d86f1801fef2fad26e81fbf56506f4 on master in the nostr-multi-platform repo. Doctrine D0–D5 (kernel never grows app nouns; best-effort rendering; reactivity contract ≤60 Hz/view; errors never cross FFI; one writer per fact; capabilities report don't decide). File-size: 300 LOC soft, 500 hard.
+docs/perf/codex-reviews/fcf8b43.md:85:+   (`docs/product-spec/overview-and-dx.md` §1.5 D0–D5), and every ownership
+docs/perf/codex-reviews/fcf8b43.md:132:+| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
+docs/perf/codex-reviews/fcf8b43.md:201:+| S1 | Mount/unmount churn | actor recv + refcount | D5 (snapshot bounded), bible #5 |
+docs/perf/codex-reviews/fcf8b43.md:203:+| S3 | Snapshot pressure | listener serialization | bible #9 (≤60 Hz), D5 |
+docs/perf/codex-reviews/fcf8b43.md:204:+| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
+docs/perf/codex-reviews/fcf8b43.md:207:+| S7 | Error-shape exhaustion | every invalid input path | D3 (no errors cross FFI) |
+docs/perf/codex-reviews/fcf8b43.md:208:+| S8 | Subscription planner DOS | OpenView/CloseView storm | D2 (≤60 Hz/view), D5 |
+docs/perf/codex-reviews/fcf8b43.md:209:+| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
+docs/perf/codex-reviews/fcf8b43.md:263:+├── doctrine-review.md       # D0–D5 sign-off (M10.5 exit-gate artifact)
+docs/perf/codex-reviews/fcf8b43.md:283:+| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
+docs/perf/codex-reviews/fcf8b43.md:291:+grep -rEn '(TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit)' \
+docs/perf/codex-reviews/fcf8b43.md:301:+### 7.2 D3 structural gap (named, not hidden)
+docs/perf/codex-reviews/fcf8b43.md:307:+caller and without setting any state field. This is **D3-compliant in
+docs/perf/codex-reviews/fcf8b43.md:309:+D3-incomplete in the user-visible sense** (no toast surfaces in
+docs/perf/codex-reviews/fcf8b43.md:312:+The debt-inventory's D3 audit (lines 317–334) concludes the same:
+docs/perf/codex-reviews/fcf8b43.md:327:+## 8. Doctrine review checklist
+docs/perf/codex-reviews/fcf8b43.md:329:+Full D0–D5 line-item-to-scenario mapping in
+docs/perf/codex-reviews/fcf8b43.md:332:+| Doctrine | Proven by |
+docs/perf/codex-reviews/fcf8b43.md:334:+| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
+docs/perf/codex-reviews/fcf8b43.md:335:+| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
+docs/perf/codex-reviews/fcf8b43.md:336:+| **D2** ≤60Hz/view, working-set bound | S2, S3, S8 — emit-rate cap, planner dedup |
+docs/perf/codex-reviews/fcf8b43.md:337:+| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
+docs/perf/codex-reviews/fcf8b43.md:338:+| **D4** one writer per fact | S1, S5 — refcount only mutated on actor thread; reentrancy under same single-writer rule |
+docs/perf/codex-reviews/fcf8b43.md:339:+| **D5** snapshots bounded by what's open | S1 (refcount drives eviction) + S3 (full-state size scales with open views, not store) |
+docs/perf/codex-reviews/fcf8b43.md:472:+        --require-doctrines D0,D1,D2,D3,D4,D5 \
+docs/perf/codex-reviews/fcf8b43.md:568:+            fails.append("FFI grep yielded TODO/FIXME tokens; see §7.1")
+docs/perf/codex-reviews/fcf8b43.md:724:+4. Doctrine review (D0–D5) signed off in `doctrine-review.md`.
+docs/perf/codex-reviews/fcf8b43.md:744:+2. **§D** doctrine D0–D5 review checklist — each line item maps to
+docs/perf/codex-reviews/fcf8b43.md:844:+| Peak working-set RSS during storm | `<=` | 150 MiB | 200 MiB |
+docs/perf/codex-reviews/fcf8b43.md:887:+## §D. Doctrine D0–D5 review checklist
+docs/perf/codex-reviews/fcf8b43.md:892:+> **Note.** The task brief mentioned "D0–D5". The canonical list in
+docs/perf/codex-reviews/fcf8b43.md:893:+> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
+docs/perf/codex-reviews/fcf8b43.md:896:+> not redundantly re-prove — items beyond D0–D5 are covered by
+docs/perf/codex-reviews/fcf8b43.md:899:+### D0. Kernel never grows app nouns
+docs/perf/codex-reviews/fcf8b43.md:901:+- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
+docs/perf/codex-reviews/fcf8b43.md:908:+  `docs/perf/m10.5/doctrine-review.md` § D0.
+docs/perf/codex-reviews/fcf8b43.md:910:+### D1. Best-effort rendering — render now, refine in place
+docs/perf/codex-reviews/fcf8b43.md:922:+- 📝 **Sign-off:** doctrine-review.md § D1.
+docs/perf/codex-reviews/fcf8b43.md:924:+### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
+docs/perf/codex-reviews/fcf8b43.md:930:+- ✅ **Stress proof (working set):** S8 asserts planner state
+docs/perf/codex-reviews/fcf8b43.md:934:+- 📝 **Sign-off:** doctrine-review.md § D2.
+docs/perf/codex-reviews/fcf8b43.md:936:+### D3. Errors never cross FFI
+docs/perf/codex-reviews/fcf8b43.md:938:+- ⚠️ **Current state:** debt-inventory §3 D3 audit concludes
+docs/perf/codex-reviews/fcf8b43.md:947:+- 📝 **Sign-off:** doctrine-review.md § D3, with the explicit
+docs/perf/codex-reviews/fcf8b43.md:948:+  note that this milestone *closes* the D3-incomplete state
+docs/perf/codex-reviews/fcf8b43.md:951:+### D4. Single writer per fact — caches derive
+docs/perf/codex-reviews/fcf8b43.md:953:+- ✅ **Proof:** debt-inventory §3 D4 audit — single-threaded actor
+docs/perf/codex-reviews/fcf8b43.md:966:+- 📝 **Sign-off:** doctrine-review.md § D4.
+docs/perf/codex-reviews/fcf8b43.md:968:+### D5. Snapshots bounded by what's open
+docs/perf/codex-reviews/fcf8b43.md:978:+- 📝 **Sign-off:** doctrine-review.md § D5.
+docs/perf/codex-reviews/fcf8b43.md:982:+## §D.1 Doctrine sign-off artifact
+docs/perf/codex-reviews/fcf8b43.md:988:+# M10.5 Doctrine Review
+docs/perf/codex-reviews/fcf8b43.md:990:+| Doctrine | Status | Evidence | Reviewer | Date |
+docs/perf/codex-reviews/fcf8b43.md:992:+| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:993:+| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:994:+| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:995:+| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:996:+| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:997:+| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:1687:+working set explodes; relay workers can't send fast enough.
+docs/perf/codex-reviews/fcf8b43.md:1698:+1. Peak working-set memory during storm ≤ **150 MB** (planner is the
+docs/perf/codex-reviews/fcf8b43.md:1800:+You are reviewing a session's worth of merges on master in the nostr-multi-platform repo. NMP is a Rust multiplatform framework for Nostr apps building toward v1 per docs/plan.md. Doctrine D0–D5 (docs/product-spec.md §1.5):
+docs/perf/codex-reviews/fcf8b43.md:1801:+- D0 kernel never grows app nouns
+docs/perf/codex-reviews/fcf8b43.md:1802:+- D1 best-effort rendering with placeholders
+docs/perf/codex-reviews/fcf8b43.md:1803:+- D2 reactivity contract (composite reverse index, ≤60 Hz/view, working-set bound)
+docs/perf/codex-reviews/fcf8b43.md:1804:+- D3 errors never cross FFI (become toast state fields)
+docs/perf/codex-reviews/fcf8b43.md:1805:+- D4 one writer per fact
+docs/perf/codex-reviews/fcf8b43.md:1806:+- D5 capabilities report, never decide
+docs/perf/codex-reviews/fcf8b43.md:1828:+- 0 critical debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!)
+docs/perf/codex-reviews/fcf8b43.md:1830:+- All 5 cardinal doctrines (D0–D5) compliant
+docs/perf/codex-reviews/fcf8b43.md:1839:+Doctrine compliance (exit gates for M10.5):
+docs/perf/codex-reviews/fcf8b43.md:1840:+✅ D0 (kernel never grows app nouns): kernel is domain-agnostic
+docs/perf/codex-reviews/fcf8b43.md:1841:+✅ D1 (best-effort rendering): ProfileCard.placeholder renders immediately
+docs/perf/codex-reviews/fcf8b43.md:1842:+✅ D2 (reactivity contract): all updates flow through composite reverse index
+docs/perf/codex-reviews/fcf8b43.md:1843:+✅ D3 (errors never cross FFI): errors as advisory JSON data, not FFI codes
+docs/perf/codex-reviews/fcf8b43.md:1844:+✅ D4 (one writer per fact): kernel actor is single-threaded
+docs/perf/codex-reviews/fcf8b43.md:1845:+✅ D5 (capabilities report): iOS bridge is pure relay, no policy decisions
+docs/perf/codex-reviews/fcf8b43.md:1871:+- Zero open TODO/FIXME/unimplemented in FFI/actor/relay/kernel/iOS bridge
+docs/perf/codex-reviews/fcf8b43.md:1910:++| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+docs/perf/codex-reviews/fcf8b43.md:2067:++**Issue:** Casts a `usize` (which can be >u32 on 64-bit systems) to `u32` after explicit saturation at `u32::MAX`. This is intentional: if there are more than 2^32 profile claims (which will not occur in practice given working-set bounds), cap the refcount at u32::MAX. No silent overflow.
+docs/perf/codex-reviews/fcf8b43.md:2093:++**Issue:** Casting collection `.count()` and arithmetic results to metric types. No overflow risk because counts are bounded by working set size constraints (max 5,000 stored events per ADR-0001; max visible 500 per FFI clamp).
+docs/perf/codex-reviews/fcf8b43.md:2168:++## 3. Doctrine Violations
+docs/perf/codex-reviews/fcf8b43.md:2170:++### D0 Audit: Kernel Never Grows App Nouns
+docs/perf/codex-reviews/fcf8b43.md:2184:++### D1 Audit: Best-Effort Rendering with Placeholders
+docs/perf/codex-reviews/fcf8b43.md:2197:++### D2 Audit: Reactivity Contract (Composite Reverse Index)
+docs/perf/codex-reviews/fcf8b43.md:2211:++### D3 Audit: Errors Never Cross FFI
+docs/perf/codex-reviews/fcf8b43.md:2225:++### D4 Audit: One Writer Per Fact
+docs/perf/codex-reviews/fcf8b43.md:2239:++### D5 Audit: Capabilities Report, Never Decide
+docs/perf/codex-reviews/fcf8b43.md:2261:++| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
+docs/perf/codex-reviews/fcf8b43.md:2262:++| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
+docs/perf/codex-reviews/fcf8b43.md:2292:++- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
+docs/perf/codex-reviews/fcf8b43.md:2304:++| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/fcf8b43.md:2320:++**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/codex-reviews/fcf8b43.md:2361:++> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
+docs/perf/codex-reviews/fcf8b43.md:2386:++**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/fcf8b43.md:2397:++  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
+docs/perf/codex-reviews/fcf8b43.md:2404:++- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/fcf8b43.md:2421:++- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
+docs/perf/codex-reviews/fcf8b43.md:2577:+1. Doctrine compliance — any rule weakening?
+docs/perf/codex-reviews/fcf8b43.md:2578:+2. TODO / FIXME / unimplemented / "for later" snuck in
+docs/perf/codex-reviews/fcf8b43.md:2614:+/bin/zsh -lc 'rg -n "TODO|FIXME|XXX|unimplemented!|todo!|for later|later|Optional|optional|recommended|Recommended|No action required|defer|deferral|future|worktree remove --force|dropped" docs/perf/m10.5/debt-inventory.md docs/perf/orchestration-log.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/fcf8b43.md:2616:+docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+docs/perf/codex-reviews/fcf8b43.md:2627:+docs/perf/m10.5/debt-inventory.md:406:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/fcf8b43.md:2631:+docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
+docs/perf/codex-reviews/fcf8b43.md:2633:+docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
+docs/perf/codex-reviews/fcf8b43.md:2635:+docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/fcf8b43.md:2637:+docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/fcf8b43.md:2652:+   363	| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
+docs/perf/codex-reviews/fcf8b43.md:2653:+   364	| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
+docs/perf/codex-reviews/fcf8b43.md:2683:+   394	- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
+docs/perf/codex-reviews/fcf8b43.md:2695:+   406	| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/fcf8b43.md:2711:+   422	**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/codex-reviews/fcf8b43.md:2733:+   345	**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/fcf8b43.md:2744:+   356	  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
+docs/perf/codex-reviews/fcf8b43.md:2751:+   363	- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/fcf8b43.md:2768:+   380	- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
+docs/perf/codex-reviews/fcf8b43.md:2916:+docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/fcf8b43.md:2925:+/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/plan.md | rg -n "deferred|deferral|NIP-XX|optional|for later|wire it up later|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/fcf8b43.md:2927:+/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/perf/m10.5/debt-inventory.md | rg -n "optional|recommended|Recommended|READY|production-ready|No Action Required|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/fcf8b43.md:2929:+18:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+docs/perf/codex-reviews/fcf8b43.md:2937:+412:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/fcf8b43.md:2945:+40:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/fcf8b43.md:2946:+58:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/fcf8b43.md:3024:+- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
+docs/perf/codex-reviews/fcf8b43.md:3031:+No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
+docs/perf/codex-reviews/fcf8b43.md:3041:+- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
+docs/perf/codex-reviews/fcf8b43.md:3048:+No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
+docs/perf/codex-reviews/fcf8b43.md:3066:+| 14 integer casts (count→metric types) (F6–F14) | Justified | All bounded by design constraints (metrics counters, working-set bounds); no overflow risk. Casts are intentional and safe. |
+docs/perf/codex-reviews/fcf8b43.md:3068: | ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
+docs/perf/codex-reviews/fcf8b43.md:3069: | Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
+docs/perf/codex-reviews/fcf8b43.md:3103:-- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
+docs/perf/codex-reviews/fcf8b43.md:3112: | Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/fcf8b43.md:3130:-**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/codex-reviews/fcf8b43.md:3135:+The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/codex-reviews/fcf8b43.md:3148:+- Doctrine review signed in writing in `docs/perf/m10.5/doctrine-review.md`
+docs/perf/codex-reviews/fcf8b43.md:3161:-- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/fcf8b43.md:3162:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Hard zero — no deferral escape, no "tracking issue" carve-out. Every pre-existing one is resolved in M10.5. If something genuinely cannot be done in M10.5 because it belongs to a later milestone (e.g. NIP-65 outbox work), then it is not a TODO/FIXME in the scoped files — it lives as a milestone task in `docs/plan.md`, not as a code marker.
+docs/perf/codex-reviews/fcf8b43.md:3176:Review for: (1) doctrine compliance, (2) any TODO/FIXME/unimplemented sneaking in, (3) test coverage where tests should exist, (4) file-size compliance (any new file > 300 LOC?), (5) docs internal consistency (M10.5 gate, M11 pod-NIP language, etc), (6) hidden shortcuts. Be terse. If fine, say so. If concern, name file:line + fix.
+docs/perf/codex-reviews/fcf8b43.md:3193:9fead0e design(m3): LMDB schema + EventStore trait + GC policy
+docs/perf/codex-reviews/fcf8b43.md:3254:docs/design/ffi-hardening.md:217:| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
+docs/perf/codex-reviews/fcf8b43.md:3255:docs/design/ffi-hardening.md:243:D3-incomplete in the user-visible sense** (no toast surfaces in
+docs/perf/codex-reviews/fcf8b43.md:3261:docs/design/ffi-hardening.md:271:| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
+docs/perf/codex-reviews/fcf8b43.md:3273:docs/plan.md:356:  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
+docs/perf/codex-reviews/fcf8b43.md:3304:docs/design/ffi-hardening/gates.md:255:| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3305:docs/design/ffi-hardening/gates.md:256:| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3306:docs/design/ffi-hardening/gates.md:257:| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3307:docs/design/ffi-hardening/gates.md:258:| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3308:docs/design/ffi-hardening/gates.md:259:| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3309:docs/design/ffi-hardening/gates.md:260:| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3312:docs/perf/m10.5/debt-inventory.md:359:| 14 integer casts (count→metric types) (F6–F14) | Justified | All bounded by design constraints (metrics counters, working-set bounds); no overflow risk. Casts are intentional and safe. |
+docs/perf/codex-reviews/fcf8b43.md:3324:/bin/zsh -lc 'rg -n "TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit|optional|Optional|recommended|Recommended|defer|deferred|deferral|NIP-XX|production-ready|Exit Criteria|READY|acceptable|fast enough" docs/design/ffi-hardening.md docs/design/ffi-hardening/*.md docs/perf/codex-reviews/2026-05-18-session-1.md docs/perf/m10.5/debt-inventory.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/fcf8b43.md:3392:docs/design/ffi-hardening/gates.md:255:| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3393:docs/design/ffi-hardening/gates.md:256:| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3397:docs/design/ffi-hardening.md:138:| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
+docs/perf/codex-reviews/fcf8b43.md:3399:docs/design/ffi-hardening.md:143:| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
+docs/perf/codex-reviews/fcf8b43.md:3402:docs/design/ffi-hardening.md:217:| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
+docs/perf/codex-reviews/fcf8b43.md:3403:docs/design/ffi-hardening.md:268:| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
+docs/perf/codex-reviews/fcf8b43.md:3404:docs/design/ffi-hardening.md:269:| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
+docs/perf/codex-reviews/fcf8b43.md:3411:docs/design/ffi-hardening/scenarios.md:261:working set explodes; relay workers can't send fast enough.
+docs/perf/codex-reviews/fcf8b43.md:3415:docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+docs/perf/codex-reviews/fcf8b43.md:3417:docs/perf/m10.5/debt-inventory.md:383:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/fcf8b43.md:3418:docs/perf/m10.5/debt-inventory.md:396:The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/codex-reviews/fcf8b43.md:3423:docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
+docs/perf/codex-reviews/fcf8b43.md:3424:docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
+docs/perf/codex-reviews/fcf8b43.md:3426:docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/fcf8b43.md:3427:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Hard zero — no deferral escape, no "tracking issue" carve-out. Every pre-existing one is resolved in M10.5. If something genuinely cannot be done in M10.5 because it belongs to a later milestone (e.g. NIP-65 outbox work), then it is not a TODO/FIXME in the scoped files — it lives as a milestone task in `docs/plan.md`, not as a code marker.
+docs/perf/codex-reviews/fcf8b43.md:3429:docs/design/ffi-hardening/ci.md:182:            fails.append("FFI grep yielded TODO/FIXME tokens; see §7.1")
+docs/perf/codex-reviews/fcf8b43.md:3436:docs/design/ffi-hardening.md:225:grep -rEn '(TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit)' \
+docs/perf/codex-reviews/fcf8b43.md:3441:docs/perf/codex-reviews/2026-05-18-session-1.md:43:- 0 critical debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!)
+docs/perf/codex-reviews/fcf8b43.md:3445:docs/perf/codex-reviews/2026-05-18-session-1.md:86:- Zero open TODO/FIXME/unimplemented in FFI/actor/relay/kernel/iOS bridge
+docs/perf/codex-reviews/fcf8b43.md:3447:docs/perf/codex-reviews/2026-05-18-session-1.md:125:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+docs/perf/codex-reviews/fcf8b43.md:3453:docs/perf/codex-reviews/2026-05-18-session-1.md:519:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/fcf8b43.md:3456:docs/perf/codex-reviews/2026-05-18-session-1.md:535:+**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/codex-reviews/fcf8b43.md:3461:docs/perf/codex-reviews/2026-05-18-session-1.md:601:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/fcf8b43.md:3462:docs/perf/codex-reviews/2026-05-18-session-1.md:619:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/fcf8b43.md:3465:docs/perf/codex-reviews/2026-05-18-session-1.md:793:2. TODO / FIXME / unimplemented / "for later" snuck in
+docs/perf/codex-reviews/fcf8b43.md:3466:docs/perf/codex-reviews/2026-05-18-session-1.md:829:/bin/zsh -lc 'rg -n "TODO|FIXME|XXX|unimplemented!|todo!|for later|later|Optional|optional|recommended|Recommended|No action required|defer|deferral|future|worktree remove --force|dropped" docs/perf/m10.5/debt-inventory.md docs/perf/orchestration-log.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/fcf8b43.md:3467:docs/perf/codex-reviews/2026-05-18-session-1.md:831:docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+docs/perf/codex-reviews/fcf8b43.md:3473:docs/perf/codex-reviews/2026-05-18-session-1.md:842:docs/perf/m10.5/debt-inventory.md:406:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/fcf8b43.md:3477:docs/perf/codex-reviews/2026-05-18-session-1.md:846:docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
+docs/perf/codex-reviews/fcf8b43.md:3478:docs/perf/codex-reviews/2026-05-18-session-1.md:848:docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
+docs/perf/codex-reviews/fcf8b43.md:3480:docs/perf/codex-reviews/2026-05-18-session-1.md:850:docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/fcf8b43.md:3481:docs/perf/codex-reviews/2026-05-18-session-1.md:852:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/fcf8b43.md:3488:docs/perf/codex-reviews/2026-05-18-session-1.md:910:   406	| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/fcf8b43.md:3491:docs/perf/codex-reviews/2026-05-18-session-1.md:926:   422	**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/codex-reviews/fcf8b43.md:3494:docs/perf/codex-reviews/2026-05-18-session-1.md:948:   345	**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/fcf8b43.md:3495:docs/perf/codex-reviews/2026-05-18-session-1.md:966:   363	- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/fcf8b43.md:3500:docs/perf/codex-reviews/2026-05-18-session-1.md:1131:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/fcf8b43.md:3507:docs/perf/codex-reviews/2026-05-18-session-1.md:1140:/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/plan.md | rg -n "deferred|deferral|NIP-XX|optional|for later|wire it up later|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/fcf8b43.md:3508:docs/perf/codex-reviews/2026-05-18-session-1.md:1142:/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/perf/m10.5/debt-inventory.md | rg -n "optional|recommended|Recommended|READY|production-ready|No Action Required|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/fcf8b43.md:3509:docs/perf/codex-reviews/2026-05-18-session-1.md:1144:18:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+docs/perf/codex-reviews/fcf8b43.md:3515:docs/perf/codex-reviews/2026-05-18-session-1.md:1152:412:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/fcf8b43.md:3521:docs/perf/codex-reviews/2026-05-18-session-1.md:1160:40:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/fcf8b43.md:3522:docs/perf/codex-reviews/2026-05-18-session-1.md:1161:58:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/fcf8b43.md:3527:docs/perf/codex-reviews/2026-05-18-session-1.md:1239:- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
+docs/perf/codex-reviews/fcf8b43.md:3529:docs/perf/codex-reviews/2026-05-18-session-1.md:1246:No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
+docs/perf/codex-reviews/fcf8b43.md:3532:docs/perf/codex-reviews/2026-05-18-session-1.md:1256:- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
+docs/perf/codex-reviews/fcf8b43.md:3534:docs/perf/codex-reviews/2026-05-18-session-1.md:1263:No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
+docs/perf/codex-reviews/fcf8b43.md:3573:   107	| Peak working-set RSS during storm | `<=` | 150 MiB | 200 MiB |
+docs/perf/codex-reviews/fcf8b43.md:3578:   251	# M10.5 Doctrine Review
+docs/perf/codex-reviews/fcf8b43.md:3580:   253	| Doctrine | Status | Evidence | Reviewer | Date |
+docs/perf/codex-reviews/fcf8b43.md:3582:   255	| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3583:   256	| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3584:   257	| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3585:   258	| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3586:   259	| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3587:   260	| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3613:    66	| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
+docs/perf/codex-reviews/fcf8b43.md:3627:   217	| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
+docs/perf/codex-reviews/fcf8b43.md:3635:   225	grep -rEn '(TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit)' \
+docs/perf/codex-reviews/fcf8b43.md:3645:   235	### 7.2 D3 structural gap (named, not hidden)
+docs/perf/codex-reviews/fcf8b43.md:3651:   241	caller and without setting any state field. This is **D3-compliant in
+docs/perf/codex-reviews/fcf8b43.md:3653:   243	D3-incomplete in the user-visible sense** (no toast surfaces in
+docs/perf/codex-reviews/fcf8b43.md:3656:   246	The debt-inventory's D3 audit (lines 317–334) concludes the same:
+docs/perf/codex-reviews/fcf8b43.md:3779:   182	            fails.append("FFI grep yielded TODO/FIXME tokens; see §7.1")
+docs/perf/codex-reviews/fcf8b43.md:3818:I found one clear doctrine mismatch candidate: the new hardening docs appear to redefine D5 as “snapshots bounded by what’s open,” while the session brief and existing rubric define D5 as “capabilities report, never decide.” I’m verifying that against the repo’s canonical docs before calling it.
+docs/perf/codex-reviews/fcf8b43.md:3822:   150	## §D. Doctrine D0–D5 review checklist
+docs/perf/codex-reviews/fcf8b43.md:3827:   155	> **Note.** The task brief mentioned "D0–D5". The canonical list in
+docs/perf/codex-reviews/fcf8b43.md:3828:   156	> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
+docs/perf/codex-reviews/fcf8b43.md:3831:   159	> not redundantly re-prove — items beyond D0–D5 are covered by
+docs/perf/codex-reviews/fcf8b43.md:3834:   162	### D0. Kernel never grows app nouns
+docs/perf/codex-reviews/fcf8b43.md:3836:   164	- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
+docs/perf/codex-reviews/fcf8b43.md:3843:   171	  `docs/perf/m10.5/doctrine-review.md` § D0.
+docs/perf/codex-reviews/fcf8b43.md:3845:   173	### D1. Best-effort rendering — render now, refine in place
+docs/perf/codex-reviews/fcf8b43.md:3857:   185	- 📝 **Sign-off:** doctrine-review.md § D1.
+docs/perf/codex-reviews/fcf8b43.md:3859:   187	### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
+docs/perf/codex-reviews/fcf8b43.md:3865:   193	- ✅ **Stress proof (working set):** S8 asserts planner state
+docs/perf/codex-reviews/fcf8b43.md:3869:   197	- 📝 **Sign-off:** doctrine-review.md § D2.
+docs/perf/codex-reviews/fcf8b43.md:3871:   199	### D3. Errors never cross FFI
+docs/perf/codex-reviews/fcf8b43.md:3873:   201	- ⚠️ **Current state:** debt-inventory §3 D3 audit concludes
+docs/perf/codex-reviews/fcf8b43.md:3882:   210	- 📝 **Sign-off:** doctrine-review.md § D3, with the explicit
+docs/perf/codex-reviews/fcf8b43.md:3883:   211	  note that this milestone *closes* the D3-incomplete state
+docs/perf/codex-reviews/fcf8b43.md:3886:   214	### D4. Single writer per fact — caches derive
+docs/perf/codex-reviews/fcf8b43.md:3888:   216	- ✅ **Proof:** debt-inventory §3 D4 audit — single-threaded actor
+docs/perf/codex-reviews/fcf8b43.md:3901:   229	- 📝 **Sign-off:** doctrine-review.md § D4.
+docs/perf/codex-reviews/fcf8b43.md:3903:   231	### D5. Snapshots bounded by what's open
+docs/perf/codex-reviews/fcf8b43.md:3910:/bin/zsh -lc "rg -n \"D0|D1|D2|D3|D4|D5|capabilities report|never decide|snapshots bounded|bounded by what's open|Capabilities\" docs/product-spec* docs/aim.md docs/plan.md docs/design/ffi-hardening.md docs/design/ffi-hardening/gates.md docs/perf/m10.5/debt-inventory.md" in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/fcf8b43.md:3912:docs/plan.md:9:> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
+docs/perf/codex-reviews/fcf8b43.md:3913:docs/plan.md:20:- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
+docs/perf/codex-reviews/fcf8b43.md:3914:docs/plan.md:65:4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
+docs/perf/codex-reviews/fcf8b43.md:3915:docs/plan.md:66:5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
+docs/perf/codex-reviews/fcf8b43.md:3916:docs/plan.md:100:- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
+docs/perf/codex-reviews/fcf8b43.md:3917:docs/plan.md:271:**Scope.** Per spec doctrine D4 (single writer per fact) extended to account scope:
+docs/perf/codex-reviews/fcf8b43.md:3918:docs/plan.md:335:- Profile picture update through compose → kind:0 republish with new Blossom URL → in-place refinement across all open Profile / Timeline payloads (per doctrine D1).
+docs/perf/codex-reviews/fcf8b43.md:3919:docs/plan.md:356:  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
+docs/perf/codex-reviews/fcf8b43.md:3920:docs/plan.md:380:- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
+docs/perf/codex-reviews/fcf8b43.md:3921:docs/design/ffi-hardening.md:19:   (`docs/product-spec/overview-and-dx.md` §1.5 D0–D5), and every ownership
+docs/perf/codex-reviews/fcf8b43.md:3922:docs/design/ffi-hardening.md:66:| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
+docs/perf/codex-reviews/fcf8b43.md:3923:docs/design/ffi-hardening.md:135:| S1 | Mount/unmount churn | actor recv + refcount | D5 (snapshot bounded), bible #5 |
+docs/perf/codex-reviews/fcf8b43.md:3924:docs/design/ffi-hardening.md:137:| S3 | Snapshot pressure | listener serialization | bible #9 (≤60 Hz), D5 |
+docs/perf/codex-reviews/fcf8b43.md:3925:docs/design/ffi-hardening.md:138:| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
+docs/perf/codex-reviews/fcf8b43.md:3926:docs/design/ffi-hardening.md:141:| S7 | Error-shape exhaustion | every invalid input path | D3 (no errors cross FFI) |
+docs/perf/codex-reviews/fcf8b43.md:3927:docs/design/ffi-hardening.md:142:| S8 | Subscription planner DOS | OpenView/CloseView storm | D2 (≤60 Hz/view), D5 |
+docs/perf/codex-reviews/fcf8b43.md:3928:docs/design/ffi-hardening.md:143:| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
+docs/perf/codex-reviews/fcf8b43.md:3929:docs/design/ffi-hardening.md:197:├── doctrine-review.md       # D0–D5 sign-off (M10.5 exit-gate artifact)
+docs/perf/codex-reviews/fcf8b43.md:3930:docs/design/ffi-hardening.md:217:| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
+docs/perf/codex-reviews/fcf8b43.md:3931:docs/design/ffi-hardening.md:235:### 7.2 D3 structural gap (named, not hidden)
+docs/perf/codex-reviews/fcf8b43.md:3932:docs/design/ffi-hardening.md:241:caller and without setting any state field. This is **D3-compliant in
+docs/perf/codex-reviews/fcf8b43.md:3933:docs/design/ffi-hardening.md:243:D3-incomplete in the user-visible sense** (no toast surfaces in
+docs/perf/codex-reviews/fcf8b43.md:3934:docs/design/ffi-hardening.md:246:The debt-inventory's D3 audit (lines 317–334) concludes the same:
+docs/perf/codex-reviews/fcf8b43.md:3935:docs/design/ffi-hardening.md:263:Full D0–D5 line-item-to-scenario mapping in
+docs/perf/codex-reviews/fcf8b43.md:3936:docs/design/ffi-hardening.md:268:| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
+docs/perf/codex-reviews/fcf8b43.md:3937:docs/design/ffi-hardening.md:269:| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
+docs/perf/codex-reviews/fcf8b43.md:3938:docs/design/ffi-hardening.md:270:| **D2** ≤60Hz/view, working-set bound | S2, S3, S8 — emit-rate cap, planner dedup |
+docs/perf/codex-reviews/fcf8b43.md:3939:docs/design/ffi-hardening.md:271:| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
+docs/perf/codex-reviews/fcf8b43.md:3940:docs/design/ffi-hardening.md:272:| **D4** one writer per fact | S1, S5 — refcount only mutated on actor thread; reentrancy under same single-writer rule |
+docs/perf/codex-reviews/fcf8b43.md:3941:docs/design/ffi-hardening.md:273:| **D5** snapshots bounded by what's open | S1 (refcount drives eviction) + S3 (full-state size scales with open views, not store) |
+docs/perf/codex-reviews/fcf8b43.md:3944:docs/perf/m10.5/debt-inventory.md:272:### D0 Audit: Kernel Never Grows App Nouns
+docs/perf/codex-reviews/fcf8b43.md:3945:docs/perf/m10.5/debt-inventory.md:286:### D1 Audit: Best-Effort Rendering with Placeholders
+docs/perf/codex-reviews/fcf8b43.md:3946:docs/perf/m10.5/debt-inventory.md:299:### D2 Audit: Reactivity Contract (Composite Reverse Index)
+docs/perf/codex-reviews/fcf8b43.md:3947:docs/perf/m10.5/debt-inventory.md:313:### D3 Audit: Errors Never Cross FFI
+docs/perf/codex-reviews/fcf8b43.md:3948:docs/perf/m10.5/debt-inventory.md:327:### D4 Audit: One Writer Per Fact
+docs/perf/codex-reviews/fcf8b43.md:3949:docs/perf/m10.5/debt-inventory.md:341:### D5 Audit: Capabilities Report, Never Decide
+docs/perf/codex-reviews/fcf8b43.md:3950:docs/perf/m10.5/debt-inventory.md:361:| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
+docs/perf/codex-reviews/fcf8b43.md:3951:docs/perf/m10.5/debt-inventory.md:362:| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
+docs/perf/codex-reviews/fcf8b43.md:3952:docs/perf/m10.5/debt-inventory.md:396:The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/codex-reviews/fcf8b43.md:3953:docs/design/ffi-hardening/gates.md:7:2. **§D** doctrine D0–D5 review checklist — each line item maps to
+docs/perf/codex-reviews/fcf8b43.md:3954:docs/design/ffi-hardening/gates.md:150:## §D. Doctrine D0–D5 review checklist
+docs/perf/codex-reviews/fcf8b43.md:3955:docs/design/ffi-hardening/gates.md:155:> **Note.** The task brief mentioned "D0–D5". The canonical list in
+docs/perf/codex-reviews/fcf8b43.md:3956:docs/design/ffi-hardening/gates.md:156:> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
+docs/perf/codex-reviews/fcf8b43.md:3957:docs/design/ffi-hardening/gates.md:159:> not redundantly re-prove — items beyond D0–D5 are covered by
+docs/perf/codex-reviews/fcf8b43.md:3958:docs/design/ffi-hardening/gates.md:162:### D0. Kernel never grows app nouns
+docs/perf/codex-reviews/fcf8b43.md:3959:docs/design/ffi-hardening/gates.md:164:- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
+docs/perf/codex-reviews/fcf8b43.md:3960:docs/design/ffi-hardening/gates.md:171:  `docs/perf/m10.5/doctrine-review.md` § D0.
+docs/perf/codex-reviews/fcf8b43.md:3961:docs/design/ffi-hardening/gates.md:173:### D1. Best-effort rendering — render now, refine in place
+docs/perf/codex-reviews/fcf8b43.md:3962:docs/design/ffi-hardening/gates.md:185:- 📝 **Sign-off:** doctrine-review.md § D1.
+docs/perf/codex-reviews/fcf8b43.md:3963:docs/design/ffi-hardening/gates.md:187:### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
+docs/perf/codex-reviews/fcf8b43.md:3964:docs/design/ffi-hardening/gates.md:197:- 📝 **Sign-off:** doctrine-review.md § D2.
+docs/perf/codex-reviews/fcf8b43.md:3965:docs/design/ffi-hardening/gates.md:199:### D3. Errors never cross FFI
+docs/perf/codex-reviews/fcf8b43.md:3966:docs/design/ffi-hardening/gates.md:201:- ⚠️ **Current state:** debt-inventory §3 D3 audit concludes
+docs/perf/codex-reviews/fcf8b43.md:3967:docs/design/ffi-hardening/gates.md:210:- 📝 **Sign-off:** doctrine-review.md § D3, with the explicit
+docs/perf/codex-reviews/fcf8b43.md:3968:docs/design/ffi-hardening/gates.md:211:  note that this milestone *closes* the D3-incomplete state
+docs/perf/codex-reviews/fcf8b43.md:3969:docs/design/ffi-hardening/gates.md:214:### D4. Single writer per fact — caches derive
+docs/perf/codex-reviews/fcf8b43.md:3970:docs/design/ffi-hardening/gates.md:216:- ✅ **Proof:** debt-inventory §3 D4 audit — single-threaded actor
+docs/perf/codex-reviews/fcf8b43.md:3971:docs/design/ffi-hardening/gates.md:229:- 📝 **Sign-off:** doctrine-review.md § D4.
+docs/perf/codex-reviews/fcf8b43.md:3972:docs/design/ffi-hardening/gates.md:231:### D5. Snapshots bounded by what's open
+docs/perf/codex-reviews/fcf8b43.md:3973:docs/design/ffi-hardening/gates.md:241:- 📝 **Sign-off:** doctrine-review.md § D5.
+docs/perf/codex-reviews/fcf8b43.md:3974:docs/design/ffi-hardening/gates.md:255:| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3975:docs/design/ffi-hardening/gates.md:256:| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3976:docs/design/ffi-hardening/gates.md:257:| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3977:docs/design/ffi-hardening/gates.md:258:| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3978:docs/design/ffi-hardening/gates.md:259:| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3979:docs/design/ffi-hardening/gates.md:260:| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
+docs/perf/codex-reviews/fcf8b43.md:3980:docs/product-spec/appendices.md:11:**`AppState` is bounded by what's open.** It does not contain the event store, the gossip cache, the working set, or anything proportional to the local cache size. It contains:
+docs/perf/codex-reviews/fcf8b43.md:3981:docs/product-spec/cli-toolchain-phasing.md:181:- **Best-effort rendering.** Doctrine D1: render what's available, refine in place; never withhold cached data; never block on fetches.
+docs/perf/codex-reviews/fcf8b43.md:3982:docs/product-spec/overview-and-dx.md:31:### D0. Kernel + extension modules — no app nouns in `nmp-core`
+docs/perf/codex-reviews/fcf8b43.md:3983:docs/product-spec/overview-and-dx.md:41:### D1. Best-effort rendering — render now, refine in place
+docs/perf/codex-reviews/fcf8b43.md:3984:docs/product-spec/overview-and-dx.md:54:### D2. Negentropy first, REQ second
+docs/perf/codex-reviews/fcf8b43.md:3985:docs/product-spec/overview-and-dx.md:60:### D3. Outbox routing is automatic; manual relay selection is the opt-out
+docs/perf/codex-reviews/fcf8b43.md:3986:docs/product-spec/overview-and-dx.md:73:### D4. Single writer per fact; caches derive
+docs/perf/codex-reviews/fcf8b43.md:3987:docs/product-spec/overview-and-dx.md:77:### D5. Snapshots bounded by what's open
+docs/perf/codex-reviews/fcf8b43.md:3990:docs/product-spec/subsystems.md:74:Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
+docs/perf/codex-reviews/fcf8b43.md:3991:docs/product-spec/subsystems.md:181:**Best-effort field contract (per doctrine D1).** Every display-bearing field in every view payload is **non-optional** and has a defined placeholder when the underlying data is missing:
+docs/perf/codex-reviews/fcf8b43.md:3992:docs/product-spec/subsystems.md:242:Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.
+docs/perf/codex-reviews/fcf8b43.md:4005:     9	> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
+docs/perf/codex-reviews/fcf8b43.md:4054:    31	### D0. Kernel + extension modules — no app nouns in `nmp-core`
+docs/perf/codex-reviews/fcf8b43.md:4064:    41	### D1. Best-effort rendering — render now, refine in place
+docs/perf/codex-reviews/fcf8b43.md:4077:    54	### D2. Negentropy first, REQ second
+docs/perf/codex-reviews/fcf8b43.md:4083:    60	### D3. Outbox routing is automatic; manual relay selection is the opt-out
+docs/perf/codex-reviews/fcf8b43.md:4096:    73	### D4. Single writer per fact; caches derive
+docs/perf/codex-reviews/fcf8b43.md:4098:    75	The "single source of truth" doctrine does not mean one cache — there are five layers (durable event store, in-memory working set, view payloads, gossip cache, platform reactive shadow). It means **one writer per fact**, and every downstream cache derives from the writer mechanically. Cache invalidation is not a concept in the public API. Recomputation happens in the actor; the platform receives new derived state.
+docs/perf/codex-reviews/fcf8b43.md:4100:    77	### D5. Snapshots bounded by what's open
+docs/perf/codex-reviews/fcf8b43.md:4113:   327	### D4 Audit: One Writer Per Fact
+docs/perf/codex-reviews/fcf8b43.md:4127:   341	### D5 Audit: Capabilities Report, Never Decide
+docs/perf/codex-reviews/fcf8b43.md:4137:   396	The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/codex-reviews/fcf8b43.md:4145:   404	- Doctrine review signed in writing in `docs/perf/m10.5/doctrine-review.md`
+docs/perf/codex-reviews/fcf8b43.md:4153:   231	### D5. Snapshots bounded by what's open
+docs/perf/codex-reviews/fcf8b43.md:4163:   241	- 📝 **Sign-off:** doctrine-review.md § D5.
+docs/perf/codex-reviews/fcf8b43.md:4263:    86	        --require-doctrines D0,D1,D2,D3,D4,D5 \
+docs/perf/codex-reviews/fcf8b43.md:4340:   361	| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
+docs/perf/codex-reviews/fcf8b43.md:4341:   362	| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
+docs/perf/codex-reviews/fcf8b43.md:4362:   383	| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/fcf8b43.md:4375:   396	The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/codex-reviews/fcf8b43.md:4383:   404	- Doctrine review signed in writing in `docs/perf/m10.5/doctrine-review.md`
+docs/perf/codex-reviews/fcf8b43.md:4746:- [docs/design/ffi-hardening/gates.md:231](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:231) redefines D5 as “snapshots bounded by what’s open,” but [docs/plan.md:9](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:9) and [debt-inventory.md:341](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:341) define D5 as “capabilities report, never decide.” Fix the doctrine mapping before sign-off; add capability-bridge evidence instead of snapshot evidence, or explicitly reconcile the canonical doctrine docs.
+docs/perf/codex-reviews/fcf8b43.md:4752:- [docs/design/ffi-hardening/ci.md:334](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:334) allows M10.5 close with S10 PASS-with-deferral, while [gates.md:256](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:256) uses S10 as D1 sign-off evidence. Fix: either make S10 required for M10.5 or remove it from doctrine evidence and replace with a non-conditional proof.
+docs/perf/codex-reviews/fcf8b43.md:4759:No executable `TODO/FIXME/unimplemented` debt was added in source; hits are docs/audit text and the archived Codex transcript.
+docs/perf/codex-reviews/fcf8b43.md:4765:- [docs/design/ffi-hardening/gates.md:231](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:231) redefines D5 as “snapshots bounded by what’s open,” but [docs/plan.md:9](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:9) and [debt-inventory.md:341](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:341) define D5 as “capabilities report, never decide.” Fix the doctrine mapping before sign-off; add capability-bridge evidence instead of snapshot evidence, or explicitly reconcile the canonical doctrine docs.
+docs/perf/codex-reviews/fcf8b43.md:4771:- [docs/design/ffi-hardening/ci.md:334](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:334) allows M10.5 close with S10 PASS-with-deferral, while [gates.md:256](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:256) uses S10 as D1 sign-off evidence. Fix: either make S10 required for M10.5 or remove it from doctrine evidence and replace with a non-conditional proof.
+docs/perf/codex-reviews/fcf8b43.md:4778:No executable `TODO/FIXME/unimplemented` debt was added in source; hits are docs/audit text and the archived Codex transcript.
+docs/design/view-catalog/stubs-validation-next.md:56:- **Computed-from-native fields.** All formatting, all derivations live in Rust per doctrine D5.
+docs/design/view-catalog/conversation-and-cross-cutting.md:95:- Decryption failure → don't add to view; record in `DebugDiagnostics`. Doctrine: never expose ciphertext as a message.
+docs/design/view-catalog/profile-timeline-thread-reactions.md:103:- **Bare pubkey path.** If no kind:0 exists in the store yet, `open()` returns a placeholder-filled payload and the reverse-index dependency triggers `on_event_inserted` when one arrives. Doctrine D1 prohibits returning `None` here.
+docs/design/view-catalog/profile-timeline-thread-reactions.md:234:- **Author kind:0 not yet loaded.** Per doctrine D1, render the item with placeholders. The `by_author` reindex on `on_projection_changed` updates the item in place. **The post is rendered immediately.**
+docs/perf/reactivity-bench/1779046753-run-002.json:655:        "working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100",
+docs/perf/reactivity-bench/1779046753-run-002.json:662:    "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.",
+docs/design/reactivity/scheduling-and-data-model.md:83:│   Tier 2: Rust working set + projections                 │
+docs/design/reactivity/scheduling-and-data-model.md:89:                       │ EventStore reads
+docs/design/reactivity/scheduling-and-data-model.md:102:- **Tier 2 is bounded** (working-set policy, ADR-0003). The reverse index keys on attributes, not bodies, so it can cover unbounded Tier-1 events.
+docs/design/reactivity/scheduling-and-data-model.md:104:- **Reads flow up; updates flow down.** Component reads happen entirely in Tier 3 — no FFI on the read path. Updates from relays land in Tier 1, propagate to Tier 2 working set + projections, then to Tier 3 via `ViewBatch`.
+docs/design/reactivity/scheduling-and-data-model.md:109:The `EventStore` holds a **bounded hot working set** in memory; cold events live in the durable storage backend. The reverse index covers both.
+docs/design/reactivity/scheduling-and-data-model.md:121:The working-set memory budget (≤ 100 MB at 100 active views, 10k hot events) is what reactivity-bench gates against. Total cached events on disk is unbounded.
+docs/decisions/0001-composite-dependency-keys.md:34:- Index registration size grows by the product of axis sizes for a view. A timeline with 1k authors × 3 kinds inserts 3k composite entries (vs ~1k under the v0 model). Acceptable; far smaller than the working-set memory budget.
+docs/decisions/0008-twitter-clone-on-ios-as-the-slice-target.md:66:The original ADR-0006 slice. Desktop iced binary; in-memory `EventStore` with kind:0 supersession + composite reverse index `(kind, author)`; minimal `Profile` view kind; manually-written `useProfile(pubkey)` wrapper for iced; one WebSocket via `nostr-sdk` to primal.
+docs/decisions/0008-twitter-clone-on-ios-as-the-slice-target.md:87:- Storage backend abstraction: `Box<dyn EventStore>` swap from in-memory to LMDB. LMDB schema design (key encoding, secondary indexes, kind:5 tombstones, watermarks placeholder for 1b).
+docs/decisions/0008-twitter-clone-on-ios-as-the-slice-target.md:94:**Exit gate.** Cold launch with primed LMDB renders the seed-driven timeline in ≤ 1.5s, showing fresh content from hundreds of authors followed by the seeds; tap an author → profile screen → back works; kind:0 arriving mid-scroll updates all author rows in place per doctrine D1; updating one seed's kind:3 mid-session re-resolves the timeline's author set without manual intervention; reactivity-bench `--standard` continues to pass at the larger author-set size; firehose-bench `live` for `sustained_firehose` (running at the real seed-author scale, not modeled) lands within budgets.
+docs/design/reactivity/view-deltas-and-projections.md:20:pub fn open(spec: TimelineSpec, store: &EventStore) -> (State, Dependencies, TimelineView) {
+docs/design/reactivity/view-deltas-and-projections.md:28:pub fn on_event_inserted(state: &mut State, event: &Event, store: &EventStore) -> Option<TimelineDelta> {
+docs/design/reactivity/view-deltas-and-projections.md:38:pub fn on_event_replaced(state: &mut State, old_id: &EventId, new_event: &Event, store: &EventStore) -> Option<TimelineDelta> { ... }
+docs/design/reactivity/view-deltas-and-projections.md:53:    fn on_event_inserted(&mut self, event: &Event, store: &EventStore) -> Option<ViewDelta> {
+docs/design/podcast/inventory.md:65:| `Utilities/ErrorPresentation.swift` | 58 | swift | `ios/NmpPodcast/Bridge/ErrorPresentation.swift` | UI-only — `AppError`/`ErrorHandler` map `toast: Option<String>` from `AppState` (doctrine D3). No business logic. |
+docs/design/podcast/podcast-core.md:240:17 `ViewModule`s. Each in `podcast-core/src/views/<name>.rs`. All share `View*` payload conventions: pre-formatted strings (per doctrine D1), no business logic in Swift, composite-keyed `ViewDependencies` (per ADR-0001).
+docs/perf/reactivity-bench/2026-05-17-run-001.md:14:The reverse-index lookup and per-view recompute paths have 100×–1000× headroom against their gates. The current dependency model produces excessive false wakes. The delta-volume gate was set as an absolute and should have been per-view. The memory gate assumed all events resident and should have been working-set only.
+docs/perf/reactivity-bench/2026-05-17-run-001.md:66:The 100 MB gate at 1M events assumed all events resident in memory. This is the anti-pattern the spec already calls out: the storage backend (LMDB / SQLite / IndexedDB) holds events; the actor keeps a bounded working set hot.
+docs/perf/reactivity-bench/2026-05-17-run-001.md:68:**Refined gate: working-set memory ≤ 100 MB**, measured at 100 active views and ≤ 10k hot events. Total cached events on disk is unbounded (or capped by the storage backend's quota).
+docs/perf/reactivity-bench/2026-05-17-run-001.md:135:### EventStore working-set discipline
+docs/perf/reactivity-bench/2026-05-17-run-001.md:137:Add to `reactivity.md` §3 and `product-spec.md` §7.1: the EventStore holds a **bounded hot working set** in memory; cold events live in the storage backend. The reverse index indexes both. Lookups against cold events return their ids and attributes immediately; loading event bodies for delta construction happens lazily and synchronously via the backend.
+docs/perf/reactivity-bench/2026-05-17-run-001.md:144:This makes the "1M events at 130 MB" finding a non-finding: total cached events is unbounded; working-set is bounded.
+docs/perf/reactivity-bench/2026-05-17-run-001.md:157:| Memory | ≤ 100 MB at 100k events | **≤ 100 MB working set at 100 views, 10k hot events** |
+docs/perf/reactivity-bench/2026-05-17-run-001.md:167:- **Storage-tier interaction with reverse-index updates.** When a cold event is loaded for delta construction, we don't need to add it to the working set unless it'll be re-referenced. Define the policy explicitly in the EventStore design.
+docs/perf/reactivity-bench/2026-05-17-run-001.md:175:- ADR-0003: working-set memory budget (this run).
+docs/decisions/0006-vertical-slice-first.md:13:The classic failure mode at this stage is **horizontal expansion** — building "the EventStore" comprehensively, then "the planner" comprehensively, then "the views" comprehensively, then finally stitching them together at the end, only to discover that the FFI surface or the relay adapter or the storage backend doesn't actually compose the way the model assumed.
+docs/decisions/0006-vertical-slice-first.md:43:│  - on_event_inserted dispatched via composite reverse index  │
+docs/decisions/0006-vertical-slice-first.md:48:│  EventStore (minimal)                                        │
+docs/decisions/0006-vertical-slice-first.md:51:│  - composite reverse index keyed by (kind, author)           │
+docs/decisions/0006-vertical-slice-first.md:94:- A real WebSocket → real EventStore → real DeltaBuffer → real component update is measurable end-to-end.
+docs/decisions/0006-vertical-slice-first.md:112:- **Subsequent expansion has a working substrate to build on.** Adding LMDB is a `Box<dyn EventStore>` swap. Adding multi-relay is a planner change. Adding negentropy is a planner change. Adding iOS is a UniFFI wrap of the existing actor. None require redesigning the architecture.
+docs/perf/codex-reviews/9fead0e.md:15:You are reviewing merge 9fead0e (M3 LMDB schema + EventStore trait + GC policy design) on master in nostr-multi-platform. Doctrine D0-D5. File size: 300 LOC soft, 500 hard.
+docs/perf/codex-reviews/9fead0e.md:17:M3 goal (per docs/plan.md §M3): swap in-memory EventStore for LMDB; implement full insert invariants (replaceable kinds, kind:5 delete, NIP-40 expiration, dedup w/ provenance merge); claim-based GC; watermark table populated in M4.
+docs/perf/codex-reviews/9fead0e.md:30:9fead0e design(m3): LMDB schema + EventStore trait + GC policy
+docs/perf/codex-reviews/9fead0e.md:37:Decision: adopt nostr-lmdb as the on-disk byte store, wrap behind a
+docs/perf/codex-reviews/9fead0e.md:38:NMP-owned EventStore trait, add NMP sub-databases for the rows
+docs/perf/codex-reviews/9fead0e.md:39:nostr-lmdb does not model (claim-pinning, watermarks, projection
+docs/perf/codex-reviews/9fead0e.md:52:+# Design: LMDB schema + EventStore trait + GC policy (M3)
+docs/perf/codex-reviews/9fead0e.md:57:+> **Prerequisites:** `docs/product-spec/subsystems.md` §7.1 (insert invariants), `docs/decisions/0003-working-set-memory.md` (GC policy intent), `docs/decisions/0009-app-extension-kernel-boundary.md` (DomainModule storage), `docs/design/kernel-substrate.md` §2 (DomainModule trait).
+docs/perf/codex-reviews/9fead0e.md:62:+## 1. Decision: wrap `nostr-lmdb` behind our own `EventStore` trait
+docs/perf/codex-reviews/9fead0e.md:64:+**Adopt `nostr-lmdb` as the on-disk byte store. Wrap it behind the NMP `EventStore` trait. Add NMP-owned LMDB sub-databases for the rows `nostr-lmdb` does not model.**
+docs/perf/codex-reviews/9fead0e.md:66:+The competing options were (1) use `nostr-lmdb` directly via its concrete `NostrLMDB` type (or via `nostr_database::NostrEventsDatabase`), (2) wrap behind our own trait, or (3) hand-roll an LMDB layer.
+docs/perf/codex-reviews/9fead0e.md:68:+**`nostr-lmdb` gives us** (per `docs.rs/nostr-lmdb`): `save_event(&Event)`, `event_by_id(&EventId)`, `check_id(&EventId) -> DatabaseEventStatus`, `query(Filter) -> Events`, `count(Filter)`, `delete(Filter)`, `wipe()`, `negentropy_items(Filter) -> Vec<(EventId, Timestamp)>`. It owns the page allocator, the LMDB environment, primary by-id store, indexes derived from `Filter`, replaceable / parameterized-replaceable supersession, and NIP-09 delete handling. It is the only mature Rust LMDB store for Nostr events with proven NIP-77 integration; reinventing it is a year of work that we will not recoup.
+docs/perf/codex-reviews/9fead0e.md:70:+**What `nostr-lmdb` does *not* model** (the gap that justifies a wrapper):
+docs/perf/codex-reviews/9fead0e.md:72:+| Concern | Why `nostr-lmdb` doesn't cover it | Where NMP needs it |
+docs/perf/codex-reviews/9fead0e.md:84:+**Therefore.** `EventStore` is a NMP-owned trait, with one production impl `LmdbEventStore` that holds (a) a `NostrLMDB` for the canonical event store and Nostr-shaped queries, and (b) NMP-owned secondary LMDB sub-databases under the same `lmdb::Environment` for the gap rows. The in-memory backend (`MemEventStore`) remains, both for tests and as the web-pre-M15 fallback. See [`lmdb/trait.md`](lmdb/trait.md) for the exact trait shape and the relayed-vs-owned method split.
+docs/perf/codex-reviews/9fead0e.md:88:+- *Use `NostrLMDB` directly, no wrapper.* Loses every gap row above. Forces the kernel actor to know about LMDB transactions and a non-NMP concrete type, breaking the `Box<dyn EventStore>` substitutability M3 requires.
+docs/perf/codex-reviews/9fead0e.md:89:+- *Roll our own.* Reinvents NIP-09 / replaceable handling that `nostr-lmdb` already gets right. ~2,000 LOC of avoidable code with a worse bug surface than upstream.
+docs/perf/codex-reviews/9fead0e.md:90:+- *SQLite-backed `nostr-sdk` store.* Larger memory footprint at our 10k-event hot working set; iOS-disk-WAL fsync cost not justified for this access pattern. Held in reserve for the web port (M15) if IndexedDB OPFS proves unworkable.
+docs/perf/codex-reviews/9fead0e.md:97:+  trait.rs              — `EventStore` (see lmdb/trait.md)
+docs/perf/codex-reviews/9fead0e.md:100:+    mod.rs              — `LmdbEventStore` orchestrator
+docs/perf/codex-reviews/9fead0e.md:114:+## 3. EventStore trait
+docs/perf/codex-reviews/9fead0e.md:116:+See [`lmdb/trait.md`](lmdb/trait.md) for the exact `pub trait EventStore` signature with all required methods, return types, and the `StoreError` enum. Summary:
+docs/perf/codex-reviews/9fead0e.md:126:+**Error semantics.** All methods return `Result<T, StoreError>`. Per doctrine D3, store errors do **not** cross FFI — the actor maps every variant to either (a) a typed `Effect` (e.g. `StoreCorrupt`, surfaces via diagnostics + toast), (b) a `tracing::warn!` log + degraded state, or (c) a panic at startup if the LMDB environment refuses to open. The trait itself uses `Result` since it is internal to the actor process.
+docs/perf/codex-reviews/9fead0e.md:132:+- Primary `events`: `event_id[32]` → `Event` (CBOR via `nostr` crate's serialization). Owned by `nostr-lmdb`.
+docs/perf/codex-reviews/9fead0e.md:160:+Key: `filter_hash[32] || relay_url_bytes` (no length prefix needed — relay URL is the variable suffix; lookup uses exact key). Populated by M4 (NIP-77) and consulted by M2's planner (cache-coverage check before issuing backfill REQ). Survives restarts; loaded into the actor on startup as a `HashMap<(filter_hash, relay_url), WatermarkRow>` for hot lookups, with all writes going through `EventStore` for durability.
+docs/perf/codex-reviews/9fead0e.md:177:+## 7. GC working-set policy
+docs/perf/codex-reviews/9fead0e.md:193:+ADR-0003's numbers are preserved as the M3 exit gate (§11 below): ≤ 100 MB working-set at 100 active views / 10k hot events / 1M cached on disk.
+docs/perf/codex-reviews/9fead0e.md:211:+Rejected: stuffing provenance into the `Event` row. That requires re-serializing the full `Event` on every relay redelivery (high write amplification — popular events arrive 5–20× from the relay fan-out) and forks the `nostr-lmdb` row format, which we explicitly want to keep upstream-compatible. The sidecar is appended cheaply with a single CBOR re-encode of the (typically small) `sources` vector.
+docs/perf/codex-reviews/9fead0e.md:259:+| Working-set memory at 100 active views / 10k hot / 1M on disk | ≤ 100 MB resident | Instruments Allocations + `reactivity-bench` working-set scenario |
+docs/perf/codex-reviews/9fead0e.md:269:+1. **`nostr-lmdb` LMDB environment sharing.** Can we open the same `lmdb::Environment` for both `NostrLMDB`'s sub-databases and our own NMP sub-databases (provenance, watermarks, claims, domain rows)? If yes, we get atomic cross-sub-db transactions for free (a single `RwTxn` covers event + provenance + secondary indexes). If `nostr-lmdb` insists on opening its own `Environment`, we lose that and the insert path needs a two-phase write with crash-recovery logic. Investigate before implementation — may require an upstream PR exposing `Environment` access.
+docs/perf/codex-reviews/9fead0e.md:274:+6. **`ModuleRegistry::register_domain` API stability.** Adding `DomainFactories` to `ModuleDescriptor` is a non-breaking additive change to the public substrate API (existing callers using only the generic `register_domain::<M>()` continue to compile), but it commits us to keeping `DomainModule::SCHEMA_VERSION` and `DomainModule::migrations` as compile-time-resolvable items rather than object-safe methods. Confirm this with the substrate maintainer before M3 lands — if `DomainModule` is expected to support runtime composition (e.g., plugin loading), we need option (c): the actor passes the live `&[Box<dyn DomainModule>]` to `EventStore::open` instead. Recommended default: stay with `fn`-pointer factories; revisit if a plugin-loading use case appears.
+docs/perf/codex-reviews/9fead0e.md:280:+- Replaceable supersession (current scattered logic to be centralised in `EventStore::insert`): `kernel/ingest.rs:166-185` (profile replace by `(pubkey, kind)`), `ingest.rs:218-233` (NIP-65 list replace by `(pubkey, 10002)`).
+docs/perf/codex-reviews/9fead0e.md:281:+- Profile claim refcounting (current in-memory analogue of `EventStore::claim/release`): `kernel/mod.rs:315` (`profile_claims: HashMap<String, BTreeSet<String>>`), `kernel/requests.rs:202` (`claim_profile`), `requests.rs:239` (`release_profile`).
+docs/perf/codex-reviews/9fead0e.md:285:+The M3 implementation deletes none of the existing files outright — the kernel's `events: HashMap` and `profiles: HashMap` are replaced by calls to `Box<dyn EventStore>` held inside the `Kernel` struct, and the per-field tests (`kernel/tests.rs`) shift to the new trait via `MemEventStore`. No public FFI surface changes.
+docs/perf/codex-reviews/9fead0e.md:292:+# LMDB sub-design: GC working-set policy
+docs/perf/codex-reviews/9fead0e.md:294:+> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Formalises the hot-resident / cold-on-disk split required by ADR-0003 (`docs/decisions/0003-working-set-memory.md`).
+docs/perf/codex-reviews/9fead0e.md:306:+                  computed from the composite reverse-index per ADR-0001
+docs/perf/codex-reviews/9fead0e.md:421:+2. The composite reverse-index resolves the dependency set to a (small, bounded) set of currently-known event ids — the *view cover*.
+docs/perf/codex-reviews/9fead0e.md:434:+The relevant figure for the M3 exit gate is **working-set RSS at the configuration described in ADR-0003 §Decision**: 100 active views, 10k hot events, 1M cached on disk, ≤ 100 MB.
+docs/perf/codex-reviews/9fead0e.md:444:+| LMDB page cache (kernel-owned, *not* counted toward RSS budget) | 0 | OS-paged, evicted under pressure; counts against system memory but not app working set |
+docs/perf/codex-reviews/9fead0e.md:495:+> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Fixes the byte layout of every sub-database the NMP store opens. Primary event storage is owned by `nostr-lmdb`; everything below is NMP-owned and lives in NMP-managed sub-databases under the same `lmdb::Environment` (per open question 1 in the master doc).
+docs/perf/codex-reviews/9fead0e.md:503:+| (multiple) | `nostr-lmdb` | upstream | upstream | event primary, internal filter indexes, kind:5 suppression |
+docs/perf/codex-reviews/9fead0e.md:518:+Sub-databases are opened lazily on first access and cached on the `LmdbEventStore`.
+docs/perf/codex-reviews/9fead0e.md:546:+The value is the `event_id`; the primary event itself lives in the `nostr-lmdb` events sub-db. On supersession, the old event-id is fetched from this row, both primary and old `idx_*` rows are deleted, and the value is overwritten with the new id.
+docs/perf/codex-reviews/9fead0e.md:651:+> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Maps every insert invariant in `docs/product-spec/subsystems.md` §7.1 to a concrete test in `crates/nmp-testing/tests/`. Each test exists for both `MemEventStore` (always) and `LmdbEventStore` (under `#[cfg(feature = "lmdb-backend")]`).
+docs/perf/codex-reviews/9fead0e.md:658:+    pub store: Box<dyn EventStore>,
+docs/perf/codex-reviews/9fead0e.md:664:+    pub fn mem() -> Self { /* MemEventStore */ }
+docs/perf/codex-reviews/9fead0e.md:665:+    pub fn lmdb() -> Self { /* LmdbEventStore in tmp dir */ }
+docs/perf/codex-reviews/9fead0e.md:704:+Plus a static-assertion-style test ensuring no other public function on `EventStore` writes to the primary store (compile-time check by inspecting trait method list via a build script — deferred to v1.x; v1 covers via review).
+docs/perf/codex-reviews/9fead0e.md:878:+# LMDB sub-design: `EventStore` trait
+docs/perf/codex-reviews/9fead0e.md:884:+`crates/nmp-core/src/store/events.rs` (filename note: `trait` is a Rust keyword, so the file is named `events.rs` and exposes `pub trait EventStore`). Re-exported from `nmp_core::store::EventStore`. The actor (`crates/nmp-core/src/actor.rs`) holds the store as `store: Box<dyn EventStore>`; backends are constructed by the factory in `store/mod.rs::open_event_store(&AppConfig) -> Result<Box<dyn EventStore>, StoreError>`.
+docs/perf/codex-reviews/9fead0e.md:1021:+pub trait EventStore: Send + Sync {
+docs/perf/codex-reviews/9fead0e.md:1159:+## 5. Error semantics (doctrine D3)
+docs/perf/codex-reviews/9fead0e.md:1165:+- `Encoding` → `tracing::error!` with the offending key/namespace; the action that triggered it fails with a `toast: Some("internal storage error; please restart")` per D3.
+docs/perf/codex-reviews/9fead0e.md:1176:+pub struct MemEventStore { /* HashMap-backed; preserves the current kernel state */ }
+docs/perf/codex-reviews/9fead0e.md:1179:+pub struct LmdbEventStore { /* wraps nostr_lmdb::NostrLMDB + NMP sub-dbs */ }
+docs/perf/codex-reviews/9fead0e.md:1181:+pub fn open_event_store(cfg: &AppConfig) -> Result<Box<dyn EventStore>, StoreError> {
+docs/perf/codex-reviews/9fead0e.md:1183:+        StorageBackend::Memory => Ok(Box::new(MemEventStore::new())),
+docs/perf/codex-reviews/9fead0e.md:1184:+        StorageBackend::Lmdb { ref path } => Ok(Box::new(LmdbEventStore::open(path)?)),
+docs/perf/codex-reviews/9fead0e.md:1189:+`MemEventStore` implements every method using `HashMap` / `BTreeMap`. The same test suite runs against both backends with `#[cfg(feature = "lmdb-backend")]` gating only the LMDB-specific edge tests (corruption recovery, oversized values).
+docs/perf/codex-reviews/9fead0e.md:1242:+On `LmdbEventStore::open()`, the store reads all `watermarks` rows and builds an in-memory `HashMap<WatermarkKey, WatermarkRow>` for hot lookups. Every `write_watermark` updates both the in-memory map and the LMDB row in a single `RwTxn`. Restart re-derives the map; we don't need a separate cache file.
+docs/perf/codex-reviews/9fead0e.md:1388:Review: (1) doctrine compliance (in particular D0 + D2 — composite reverse index + working-set bound must remain enforced through the trait); (2) any TODO/FIXME/unimplemented; (3) file-size compliance; (4) consistency of EventStore trait surface across the 6 sub-docs; (5) the choice of nostr-lmdb as on-disk byte store vs roll-our-own — is the gap analysis honest?; (6) the GC eviction algorithm + budget; (7) the migration plumbing's testability; (8) any hidden shortcuts. Be terse; name file:line + fix for any concern.
+docs/design/podcast/capabilities.md:4:> Substrate reference: [`../kernel-substrate.md`](../kernel-substrate.md) §5; doctrine: D5 (capabilities report, never decide).
+docs/design/podcast/capabilities.md:85:### Bounded-state proof (D5)
+docs/design/reactivity/validation-harness.md:11:- **Mutation of `EventStore` from within a view.** Views observe; they don't write. Only the actor's top-level handlers and actions write.
+docs/design/reactivity/validation-harness.md:39:- Spawns a configurable `EventStore` (in-memory, LMDB, or SQLite backend).
+docs/product-spec/cli-toolchain-phasing.md:174:- **EventStore.** The reactive single source of truth for all Nostr events. Owned by the actor; not exposed at FFI.
+docs/product-spec/cli-toolchain-phasing.md:177:- **View.** A pre-built derived projection of `EventStore` contents. Opened by `OpenView` action; payload arrives via `AppState.views` / `ViewBatch`.
+docs/product-spec/cli-toolchain-phasing.md:181:- **Best-effort rendering.** Doctrine D1: render what's available, refine in place; never withhold cached data; never block on fetches.
+docs/decisions/0003-working-set-memory.md:1:# ADR 0003: Memory budget is for working set, not total cached events
+docs/decisions/0003-working-set-memory.md:11:The actor should keep a **bounded working set** of hot events in memory; cold events live on disk. The reverse index can cover both — it keys on attributes, not event bodies.
+docs/decisions/0003-working-set-memory.md:15:The memory budget targets **working-set memory at typical active load**, not total cached events.
+docs/decisions/0003-working-set-memory.md:34:- The 1M-events-resident scenario is no longer a failure — it's an unintended test of an unintended configuration. Re-run with bounded working set.
+docs/decisions/0003-working-set-memory.md:46:Re-run reactivity-bench with bounded working set; require ≤ 100 MB at 100 views / 10k hot events / 1M cached events on disk.
+docs/perf/codex-reviews/0dfb975.md:15:You are reviewing merge 0dfb97581315aa04bd66341a98a32215de5e14d6 (the M11 podcast-app rebuild design) on master in nostr-multi-platform. Doctrine D0-D5 (D0 kernel never grows app nouns, D1 best-effort rendering, D2 reactivity ≤60Hz/view, D3 no errors across FFI, D4 one writer per fact, D5 capabilities report don't decide). File size: 300 LOC soft, 500 hard.
+docs/perf/codex-reviews/0dfb975.md:244:+> Substrate reference: [`../kernel-substrate.md`](../kernel-substrate.md) §5; doctrine: D5 (capabilities report, never decide).
+docs/perf/codex-reviews/0dfb975.md:325:+### Bounded-state proof (D5)
+docs/perf/codex-reviews/0dfb975.md:675:+- The placeholder block is fenced by `// MARK: NMP-WIRE` (start) and either `// MARK: NMP-WIRE — wired` (after wiring) or `// MARK: NMP-WIRE — TODO` (still pending).
+docs/perf/codex-reviews/0dfb975.md:677:+- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/` is part of the M11 exit gate (must be zero).
+docs/perf/codex-reviews/0dfb975.md:686:+// MARK: NMP-WIRE — TODO
+docs/perf/codex-reviews/0dfb975.md:795:+- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` is the work-remaining counter for Step 7.
+docs/perf/codex-reviews/0dfb975.md:895:+Every test file ≤ 500 LOC. The cross-cutting kill-relaunch test is the most load-bearing — it asserts D2 (reactivity) and D4 (single writer per fact) hold under app termination at every state-transition boundary.
+docs/perf/codex-reviews/0dfb975.md:916:+- The doctrine review at `docs/perf/m11/doctrine-review.md` signs off D0–D5 against the M11 surface (template: `docs/perf/m10.5/doctrine-review.md`).
+docs/perf/codex-reviews/0dfb975.md:993:+| `Utilities/ErrorPresentation.swift` | 58 | swift | `ios/NmpPodcast/Bridge/ErrorPresentation.swift` | UI-only — `AppError`/`ErrorHandler` map `toast: Option<String>` from `AppState` (doctrine D3). No business logic. |
+docs/perf/codex-reviews/0dfb975.md:1182:+- "No business logic in native." (Doctrine D0 + AGENTS.md guardrails.)
+docs/perf/codex-reviews/0dfb975.md:1441:+17 `ViewModule`s. Each in `podcast-core/src/views/<name>.rs`. All share `View*` payload conventions: pre-formatted strings (per doctrine D1), no business logic in Swift, composite-keyed `ViewDependencies` (per ADR-0001).
+docs/perf/codex-reviews/0dfb975.md:1767:+All actions return parsed records. None of them write to the domain store directly — `podcast-core::SubscribePodcast`/`RefreshFeed` orchestrate the writes, preserving the doctrine: **one writer per fact** (D4).
+docs/perf/codex-reviews/0dfb975.md:2087:+- Failure-mode test: capability emits `Error { reason: "rate_limit" }` mid-stream → `ChatTurn` commits with `is_error: true` and the `toast` field is set per doctrine D3.
+docs/perf/codex-reviews/0dfb975.md:2753:+3. Replace `// MARK: NMP-WIRE — TODO` blocks with `// MARK: NMP-WIRE — wired` consuming the generated `@<View>` property wrapper.
+docs/perf/codex-reviews/0dfb975.md:2830:+- Acceptance: subscribe to a real RSS feed (Tim Ferriss) → library shows it → tap → see ≥ 5 episodes parsed; UI updates as `FetchFeed` action completes (no spinner gate; placeholder rows during fetch per doctrine D1).
+docs/perf/codex-reviews/0dfb975.md:2943:+`grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` ratchets to zero across the 7 lanes.
+docs/perf/codex-reviews/0dfb975.md:2954:+- [ ] All `// MARK: NMP-WIRE — TODO` in the lane removed
+docs/perf/codex-reviews/0dfb975.md:2973:1. D0 enforcement — does anything in the design leak podcast nouns into nmp-core? Are the new capabilities (Audio, Background, Embedding, KV) general-shape or accidentally podcast-shaped?
+docs/perf/codex-reviews/0dfb975.md:2974:2. D5 — do the capability traits report only, or do any of them embed policy?
+docs/perf/codex-reviews/0dfb975.md:2979:7. Any TODO/FIXME/unimplemented sneaking in?
+docs/perf/codex-reviews/0dfb975.md:2986:/bin/zsh -lc 'rg -n "nostr-multi-platform|M11|podcast-app|D0|kernel never" /Users/pablofernandez/.codex/memories/MEMORY.md' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/0dfb975.md:3037:docs/plan.md:20:- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
+docs/perf/codex-reviews/0dfb975.md:3041:docs/plan.md:66:5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
+docs/perf/codex-reviews/0dfb975.md:3055:docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/0dfb975.md:3169:docs/decisions/0008-twitter-clone-on-ios-as-the-slice-target.md:66:The original ADR-0006 slice. Desktop iced binary; in-memory `EventStore` with kind:0 supersession + composite reverse index `(kind, author)`; minimal `Profile` view kind; manually-written `useProfile(pubkey)` wrapper for iced; one WebSocket via `nostr-sdk` to primal.
+docs/perf/codex-reviews/0dfb975.md:3178:/bin/zsh -lc 'rg -n "NO podcast nouns|grep -RE|AudioPlayback|AppleIntelligence|KeyValueStore|Authoritative settings|ImportOpml|QueueEntry|disabled by default|ImageCache|ClearImageCache|App/PodcastApp|Swift files staying|Swift files moving|18 view files|47 Swift files|8,793|24 files|Generated property wrappers|@propertyWrapper|UseFoo|useFoo|rig-core|gpt-4o-mini|SpeechAnalyzer|iOS 26|FoundationModels|sqlite-vec|load_extension|Prompt|byte-identical|Reference Swift app|commit at task time|Source:|mcp__xcode|macOS-15|iOS 26.5|Line|TODO|FIXME|unimplemented" docs/design/podcast-app-rebuild.md docs/design/podcast/*.md' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/0dfb975.md:3252:docs/design/podcast/copy.md:81:- The placeholder block is fenced by `// MARK: NMP-WIRE` (start) and either `// MARK: NMP-WIRE — wired` (after wiring) or `// MARK: NMP-WIRE — TODO` (still pending).
+docs/perf/codex-reviews/0dfb975.md:3254:docs/design/podcast/copy.md:83:- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/` is part of the M11 exit gate (must be zero).
+docs/perf/codex-reviews/0dfb975.md:3255:docs/design/podcast/copy.md:92:// MARK: NMP-WIRE — TODO
+docs/perf/codex-reviews/0dfb975.md:3258:docs/design/podcast/copy.md:201:- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` is the work-remaining counter for Step 7.
+docs/perf/codex-reviews/0dfb975.md:3261:docs/design/podcast/wiring.md:11:3. Replace `// MARK: NMP-WIRE — TODO` blocks with `// MARK: NMP-WIRE — wired` consuming the generated `@<View>` property wrapper.
+docs/perf/codex-reviews/0dfb975.md:3276:docs/design/podcast/wiring.md:201:`grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` ratchets to zero across the 7 lanes.
+docs/perf/codex-reviews/0dfb975.md:3277:docs/design/podcast/wiring.md:212:- [ ] All `// MARK: NMP-WIRE — TODO` in the lane removed
+docs/perf/codex-reviews/0dfb975.md:3493:A few issues are now concrete rather than stylistic: the design has at least one hard LOC conflict with the copy-verbatim requirement, and the screenshot gate currently depends on an agent-only UI introspection path. I’m pulling the exact lines for those plus the doctrine/D4 ownership concerns.
+docs/perf/codex-reviews/0dfb975.md:3720:    85	### Bounded-state proof (D5)
+docs/perf/codex-reviews/0dfb975.md:3938:   201	- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` is the work-remaining counter for Step 7.
+docs/perf/codex-reviews/0dfb975.md:3978:3. [capabilities.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/capabilities.md:79): `AudioPlaybackBridge` decides repeated `Load` is a no-op based on `currentEpisode`. That is playback policy in native, and it conflicts with the same doc’s D5 claim at [capabilities.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/capabilities.md:87) plus the lesson at [lessons.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/lessons.md:58). Fix: Rust decides whether to send `Load`; the bridge only executes requests and reports state.
+docs/perf/codex-reviews/0dfb975.md:3980:4. [podcast-core.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/podcast-core.md:130), [podcast-core.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/podcast-core.md:231), and [podcast-rag.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/podcast-rag.md:132): `podcast-core` stores `embedding_id`, says it is re-exported from `podcast-rag`, while `podcast-rag` depends on `podcast-core` and backfills the core record. That creates either a crate cycle or a D4 one-writer violation. Fix: define `EmbeddingId` in `podcast-core` or a shared types crate, and have core own any writes to core records after `podcast-rag` returns `Indexed`.
+docs/perf/codex-reviews/0dfb975.md:3997:3. [capabilities.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/capabilities.md:79): `AudioPlaybackBridge` decides repeated `Load` is a no-op based on `currentEpisode`. That is playback policy in native, and it conflicts with the same doc’s D5 claim at [capabilities.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/capabilities.md:87) plus the lesson at [lessons.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/lessons.md:58). Fix: Rust decides whether to send `Load`; the bridge only executes requests and reports state.
+docs/perf/codex-reviews/0dfb975.md:3999:4. [podcast-core.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/podcast-core.md:130), [podcast-core.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/podcast-core.md:231), and [podcast-rag.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/podcast-rag.md:132): `podcast-core` stores `embedding_id`, says it is re-exported from `podcast-rag`, while `podcast-rag` depends on `podcast-core` and backfills the core record. That creates either a crate cycle or a D4 one-writer violation. Fix: define `EmbeddingId` in `podcast-core` or a shared types crate, and have core own any writes to core records after `podcast-rag` returns `Indexed`.
+docs/design/podcast/podcast-llm.md:272:- Failure-mode test: capability emits `Error { reason: "rate_limit" }` mid-stream → `ChatTurn` commits with `is_error: true` and the `toast` field is set per doctrine D3.
+docs/perf/codex-reviews/03d7a23.md:15:You are reviewing merge 03d7a23710136279b730ec0227e49575e3658695 on master. NMP nostr-multi-platform. Doctrine D0-D5, file size 300 LOC soft/500 hard. This merge adds SAFETY comments to ffi.rs unsafe blocks and a clarifying comment in relay_worker.rs. Verify:
+docs/perf/codex-reviews/03d7a23.md:18:3. No new TODO/FIXME introduced.
+docs/perf/codex-reviews/03d7a23.md:464:/bin/zsh -lc 'rg -n "TODO|FIXME" crates/nmp-core/src/ffi.rs crates/nmp-core/src/relay_worker.rs' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a41054fd04781ce06
+docs/perf/codex-reviews/03d7a23.md:467:The committed diff is comments only, and the TODO/FIXME scan of the two touched files is clean. I’m checking the workspace commands now so the review covers the requested build surface too.
+docs/perf/codex-reviews/03d7a23.md:1139:/bin/zsh -lc 'git show --no-ext-diff --format= --no-renames 03d7a23710136279b730ec0227e49575e3658695 | rg -n "TODO|FIXME"' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a41054fd04781ce06
+docs/perf/codex-reviews/03d7a23.md:1300:- No new TODO/FIXME.
+docs/perf/codex-reviews/03d7a23.md:1313:- No new TODO/FIXME.
+docs/design/podcast/podcast-feeds.md:122:All actions return parsed records. None of them write to the domain store directly — `podcast-core::SubscribePodcast`/`RefreshFeed` orchestrate the writes, preserving the doctrine: **one writer per fact** (D4).
+docs/perf/reactivity-bench/1779049354-run-002.md:44:- working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100
+docs/perf/reactivity-bench/1779049354-run-002.md:49:- Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.
+docs/perf/reactivity-bench/1779046753-run-002.md:44:- working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100
+docs/perf/reactivity-bench/1779046753-run-002.md:49:- Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.
+docs/design/podcast/copy.md:81:- The placeholder block is fenced by `// MARK: NMP-WIRE` (start) and either `// MARK: NMP-WIRE — wired` (after wiring) or `// MARK: NMP-WIRE — TODO` (still pending).
+docs/design/podcast/copy.md:83:- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/` is part of the M11 exit gate (must be zero).
+docs/design/podcast/copy.md:92:// MARK: NMP-WIRE — TODO
+docs/design/podcast/copy.md:201:- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` is the work-remaining counter for Step 7.
+docs/design/reactivity/loop-and-reverse-index.md:9:> **Status:** rev 1 — incorporating findings from reactivity-bench run 001 (2026-05-17). See `docs/perf/reactivity-bench/2026-05-17-run-001.md` for the measurement report. Decisions: ADR-0001 (composite keys), ADR-0002 (per-view delta budget), ADR-0003 (working-set memory), ADR-0004 (allocation measurement).
+docs/design/reactivity/loop-and-reverse-index.md:38:                │  EventStore::insert(event)               │
+docs/design/reactivity/loop-and-reverse-index.md:67:- **`EventStore`** owns the actual events and the reverse index. Inserting goes through it.
+docs/design/reactivity/loop-and-reverse-index.md:133:**Why composite-first:** reactivity-bench run 001 measured 98% false-wakeup rate in quiet_idle and 49% in following_timeline_scroll under the v0 design (which unioned independent axis buckets). Conjunctive composite keys eliminate the false wakes. The cost is registration-size growth (kinds × authors cartesian product), bounded by working-set memory budget.
+docs/perf/reactivity-bench/1779050935-run-002.json:655:        "working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100",
+docs/perf/reactivity-bench/1779050935-run-002.json:662:    "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.",
+docs/product-spec/subsystems.md:7:### 7.1 EventStore
+docs/product-spec/subsystems.md:74:Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
+docs/product-spec/subsystems.md:181:**Best-effort field contract (per doctrine D1).** Every display-bearing field in every view payload is **non-optional** and has a defined placeholder when the underlying data is missing:
+docs/product-spec/subsystems.md:242:Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.
+docs/product-spec/subsystems.md:247:View opens → Live REQ handler starts → Planner consults coverage → Sync engine reconciles gaps → EventStore inserts → ViewBatch emits
+docs/product-spec/subsystems.md:419:- Memory footprint of the actor's working set.
+docs/perf/reactivity-bench/1779045194-run-002.md:44:- working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100
+docs/perf/reactivity-bench/1779045194-run-002.md:49:- Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.
+docs/perf/reactivity-bench/1779051783-run-002.md:44:- working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100
+docs/perf/reactivity-bench/1779051783-run-002.md:49:- Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.
+docs/perf/codex-reviews/2026-05-18-session-1.md:15:You are reviewing a session's worth of merges on master in the nostr-multi-platform repo. NMP is a Rust multiplatform framework for Nostr apps building toward v1 per docs/plan.md. Doctrine D0–D5 (docs/product-spec.md §1.5):
+docs/perf/codex-reviews/2026-05-18-session-1.md:16:- D0 kernel never grows app nouns
+docs/perf/codex-reviews/2026-05-18-session-1.md:17:- D1 best-effort rendering with placeholders
+docs/perf/codex-reviews/2026-05-18-session-1.md:18:- D2 reactivity contract (composite reverse index, ≤60 Hz/view, working-set bound)
+docs/perf/codex-reviews/2026-05-18-session-1.md:19:- D3 errors never cross FFI (become toast state fields)
+docs/perf/codex-reviews/2026-05-18-session-1.md:20:- D4 one writer per fact
+docs/perf/codex-reviews/2026-05-18-session-1.md:21:- D5 capabilities report, never decide
+docs/perf/codex-reviews/2026-05-18-session-1.md:43:- 0 critical debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!)
+docs/perf/codex-reviews/2026-05-18-session-1.md:45:- All 5 cardinal doctrines (D0–D5) compliant
+docs/perf/codex-reviews/2026-05-18-session-1.md:54:Doctrine compliance (exit gates for M10.5):
+docs/perf/codex-reviews/2026-05-18-session-1.md:55:✅ D0 (kernel never grows app nouns): kernel is domain-agnostic
+docs/perf/codex-reviews/2026-05-18-session-1.md:56:✅ D1 (best-effort rendering): ProfileCard.placeholder renders immediately
+docs/perf/codex-reviews/2026-05-18-session-1.md:57:✅ D2 (reactivity contract): all updates flow through composite reverse index
+docs/perf/codex-reviews/2026-05-18-session-1.md:58:✅ D3 (errors never cross FFI): errors as advisory JSON data, not FFI codes
+docs/perf/codex-reviews/2026-05-18-session-1.md:59:✅ D4 (one writer per fact): kernel actor is single-threaded
+docs/perf/codex-reviews/2026-05-18-session-1.md:60:✅ D5 (capabilities report): iOS bridge is pure relay, no policy decisions
+docs/perf/codex-reviews/2026-05-18-session-1.md:86:- Zero open TODO/FIXME/unimplemented in FFI/actor/relay/kernel/iOS bridge
+docs/perf/codex-reviews/2026-05-18-session-1.md:125:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+docs/perf/codex-reviews/2026-05-18-session-1.md:282:+**Issue:** Casts a `usize` (which can be >u32 on 64-bit systems) to `u32` after explicit saturation at `u32::MAX`. This is intentional: if there are more than 2^32 profile claims (which will not occur in practice given working-set bounds), cap the refcount at u32::MAX. No silent overflow.
+docs/perf/codex-reviews/2026-05-18-session-1.md:308:+**Issue:** Casting collection `.count()` and arithmetic results to metric types. No overflow risk because counts are bounded by working set size constraints (max 5,000 stored events per ADR-0001; max visible 500 per FFI clamp).
+docs/perf/codex-reviews/2026-05-18-session-1.md:383:+## 3. Doctrine Violations
+docs/perf/codex-reviews/2026-05-18-session-1.md:385:+### D0 Audit: Kernel Never Grows App Nouns
+docs/perf/codex-reviews/2026-05-18-session-1.md:399:+### D1 Audit: Best-Effort Rendering with Placeholders
+docs/perf/codex-reviews/2026-05-18-session-1.md:412:+### D2 Audit: Reactivity Contract (Composite Reverse Index)
+docs/perf/codex-reviews/2026-05-18-session-1.md:426:+### D3 Audit: Errors Never Cross FFI
+docs/perf/codex-reviews/2026-05-18-session-1.md:440:+### D4 Audit: One Writer Per Fact
+docs/perf/codex-reviews/2026-05-18-session-1.md:454:+### D5 Audit: Capabilities Report, Never Decide
+docs/perf/codex-reviews/2026-05-18-session-1.md:476:+| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
+docs/perf/codex-reviews/2026-05-18-session-1.md:477:+| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
+docs/perf/codex-reviews/2026-05-18-session-1.md:507:+- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
+docs/perf/codex-reviews/2026-05-18-session-1.md:519:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/2026-05-18-session-1.md:535:+**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/codex-reviews/2026-05-18-session-1.md:576:+> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
+docs/perf/codex-reviews/2026-05-18-session-1.md:601:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/2026-05-18-session-1.md:612:+  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
+docs/perf/codex-reviews/2026-05-18-session-1.md:619:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/2026-05-18-session-1.md:636:+- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
+docs/perf/codex-reviews/2026-05-18-session-1.md:792:1. Doctrine compliance — any rule weakening?
+docs/perf/codex-reviews/2026-05-18-session-1.md:793:2. TODO / FIXME / unimplemented / "for later" snuck in
+docs/perf/codex-reviews/2026-05-18-session-1.md:829:/bin/zsh -lc 'rg -n "TODO|FIXME|XXX|unimplemented!|todo!|for later|later|Optional|optional|recommended|Recommended|No action required|defer|deferral|future|worktree remove --force|dropped" docs/perf/m10.5/debt-inventory.md docs/perf/orchestration-log.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/2026-05-18-session-1.md:831:docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+docs/perf/codex-reviews/2026-05-18-session-1.md:842:docs/perf/m10.5/debt-inventory.md:406:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/2026-05-18-session-1.md:846:docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
+docs/perf/codex-reviews/2026-05-18-session-1.md:848:docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
+docs/perf/codex-reviews/2026-05-18-session-1.md:850:docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/2026-05-18-session-1.md:852:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/2026-05-18-session-1.md:867:   363	| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
+docs/perf/codex-reviews/2026-05-18-session-1.md:868:   364	| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
+docs/perf/codex-reviews/2026-05-18-session-1.md:898:   394	- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
+docs/perf/codex-reviews/2026-05-18-session-1.md:910:   406	| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/2026-05-18-session-1.md:926:   422	**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
+docs/perf/codex-reviews/2026-05-18-session-1.md:948:   345	**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/2026-05-18-session-1.md:959:   356	  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
+docs/perf/codex-reviews/2026-05-18-session-1.md:966:   363	- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/2026-05-18-session-1.md:983:   380	- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
+docs/perf/codex-reviews/2026-05-18-session-1.md:1131:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/2026-05-18-session-1.md:1140:/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/plan.md | rg -n "deferred|deferral|NIP-XX|optional|for later|wire it up later|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/2026-05-18-session-1.md:1142:/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/perf/m10.5/debt-inventory.md | rg -n "optional|recommended|Recommended|READY|production-ready|No Action Required|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
+docs/perf/codex-reviews/2026-05-18-session-1.md:1144:18:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+docs/perf/codex-reviews/2026-05-18-session-1.md:1152:412:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/codex-reviews/2026-05-18-session-1.md:1160:40:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/perf/codex-reviews/2026-05-18-session-1.md:1161:58:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/perf/codex-reviews/2026-05-18-session-1.md:1239:- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
+docs/perf/codex-reviews/2026-05-18-session-1.md:1246:No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
+docs/perf/codex-reviews/2026-05-18-session-1.md:1256:- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
+docs/perf/codex-reviews/2026-05-18-session-1.md:1263:No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
+docs/design/subscription-compilation/outbox.md:215:- The local store insert happens *before* the publish step (optimistic insert), with rollback on `PartiallyFailed` if `required_success_count` is not met. This matches the "atomic with reversibility" reading of doctrine D4 (single writer per fact).
+docs/perf/reactivity-bench/1779049354-run-002.json:655:        "working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100",
+docs/perf/reactivity-bench/1779049354-run-002.json:662:    "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.",
+docs/design/subscription-compilation/intro.md:15:- **Mailbox cache exists but no consumer.** `crates/nmp-core/src/kernel/ingest.rs:209-233` (`ingest_relay_list`) already parses kind:10002 into `self.author_relay_lists: HashMap<String, AuthorRelayList>` (declared at `crates/nmp-core/src/kernel/mod.rs:269-275` and reserved at `mod.rs:313`). The cache is written; **nothing reads it for routing**. This is the bug doctrine D5 ("capabilities report, never decide") inverted: we have the data, we ignore it.
+docs/design/podcast/exit-gate.md:85:Every test file ≤ 500 LOC. The cross-cutting kill-relaunch test is the most load-bearing — it asserts D2 (reactivity) and D4 (single writer per fact) hold under app termination at every state-transition boundary.
+docs/design/podcast/exit-gate.md:106:- The doctrine review at `docs/perf/m11/doctrine-review.md` signs off D0–D5 against the M11 surface (template: `docs/perf/m10.5/doctrine-review.md`).
+docs/product-spec/appendices.md:11:**`AppState` is bounded by what's open.** It does not contain the event store, the gossip cache, the working set, or anything proportional to the local cache size. It contains:
+docs/product-spec/appendices.md:17:The event store, gossip cache, sync watermarks, working set, and signer state all live in the actor and **never cross FFI**.
+docs/design/kernel-substrate.md:173:- Pre-formatted display fields per doctrine D1.
+docs/design/kernel-substrate.md:429:Integration tests across modules use the real EventStore and a `MockRelay`.
+docs/design/kernel-substrate.md:483:1. Phase 1a.1 (kernel substrate prototype) ships with one fixture module (`fixture-todo-core`) demonstrating each of the five trait families. Codegen produces a working `nmp-app-fixture` crate. Desktop iced app renders a TODO list, no business logic in Swift / iced.
+docs/design/lmdb/watermarks.md:47:On `LmdbEventStore::open()`, the store reads all `watermarks` rows and builds an in-memory `HashMap<WatermarkKey, WatermarkRow>` for hot lookups. Every `write_watermark` updates both the in-memory map and the LMDB row in a single `RwTxn`. Restart re-derives the map; we don't need a separate cache file.
+docs/design/lmdb/gc.md:1:# LMDB sub-design: GC working-set policy
+docs/design/lmdb/gc.md:3:> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Formalises the hot-resident / cold-on-disk split required by ADR-0003 (`docs/decisions/0003-working-set-memory.md`).
+docs/design/lmdb/gc.md:15:                  computed from the composite reverse-index per ADR-0001
+docs/design/lmdb/gc.md:130:2. The composite reverse-index resolves the dependency set to a (small, bounded) set of currently-known event ids — the *view cover*.
+docs/design/lmdb/gc.md:143:The relevant figure for the M3 exit gate is **working-set RSS at the configuration described in ADR-0003 §Decision**: 100 active views, 10k hot events, 1M cached on disk, ≤ 100 MB.
+docs/design/lmdb/gc.md:153:| LMDB page cache (kernel-owned, *not* counted toward RSS budget) | 0 | OS-paged, evicted under pressure; counts against system memory but not app working set |
+docs/design/ffi-hardening.md:19:   (`docs/product-spec/overview-and-dx.md` §1.5 D0–D5), and every ownership
+docs/design/ffi-hardening.md:66:| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
+docs/design/ffi-hardening.md:135:| S1 | Mount/unmount churn | actor recv + refcount | D5 (snapshot bounded), bible #5 |
+docs/design/ffi-hardening.md:137:| S3 | Snapshot pressure | listener serialization | bible #9 (≤60 Hz), D5 |
+docs/design/ffi-hardening.md:138:| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
+docs/design/ffi-hardening.md:141:| S7 | Error-shape exhaustion | every invalid input path | D3 (no errors cross FFI) |
+docs/design/ffi-hardening.md:142:| S8 | Subscription planner DOS | OpenView/CloseView storm | D2 (≤60 Hz/view), D5 |
+docs/design/ffi-hardening.md:143:| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
+docs/design/ffi-hardening.md:197:├── doctrine-review.md       # D0–D5 sign-off (M10.5 exit-gate artifact)
+docs/design/ffi-hardening.md:217:| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
+docs/design/ffi-hardening.md:225:grep -rEn '(TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit)' \
+docs/design/ffi-hardening.md:235:### 7.2 D3 structural gap (named, not hidden)
+docs/design/ffi-hardening.md:241:caller and without setting any state field. This is **D3-compliant in
+docs/design/ffi-hardening.md:243:D3-incomplete in the user-visible sense** (no toast surfaces in
+docs/design/ffi-hardening.md:246:The debt-inventory's D3 audit (lines 317–334) concludes the same:
+docs/design/ffi-hardening.md:261:## 8. Doctrine review checklist
+docs/design/ffi-hardening.md:263:Full D0–D5 line-item-to-scenario mapping in
+docs/design/ffi-hardening.md:266:| Doctrine | Proven by |
+docs/design/ffi-hardening.md:268:| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
+docs/design/ffi-hardening.md:269:| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
+docs/design/ffi-hardening.md:270:| **D2** ≤60Hz/view, working-set bound | S2, S3, S8 — emit-rate cap, planner dedup |
+docs/design/ffi-hardening.md:271:| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
+docs/design/ffi-hardening.md:272:| **D4** one writer per fact | S1, S5 — refcount only mutated on actor thread; reentrancy under same single-writer rule |
+docs/design/ffi-hardening.md:273:| **D5** snapshots bounded by what's open | S1 (refcount drives eviction) + S3 (full-state size scales with open views, not store) |
+docs/design/lmdb/keys.md:3:> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Fixes the byte layout of every sub-database the NMP store opens. Primary event storage is owned by `nostr-lmdb`; everything below is NMP-owned and lives in NMP-managed sub-databases under the same `lmdb::Environment` (per open question 1 in the master doc).
+docs/design/lmdb/keys.md:11:| (multiple) | `nostr-lmdb` | upstream | upstream | event primary, internal filter indexes, kind:5 suppression |
+docs/design/lmdb/keys.md:26:Sub-databases are opened lazily on first access and cached on the `LmdbEventStore`.
+docs/design/lmdb/keys.md:54:The value is the `event_id`; the primary event itself lives in the `nostr-lmdb` events sub-db. On supersession, the old event-id is fetched from this row, both primary and old `idx_*` rows are deleted, and the value is overwritten with the new id.
+docs/design/lmdb/tests.md:3:> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Maps every insert invariant in `docs/product-spec/subsystems.md` §7.1 to a concrete test in `crates/nmp-testing/tests/`. Each test exists for both `MemEventStore` (always) and `LmdbEventStore` (under `#[cfg(feature = "lmdb-backend")]`).
+docs/design/lmdb/tests.md:10:    pub store: Box<dyn EventStore>,
+docs/design/lmdb/tests.md:16:    pub fn mem() -> Self { /* MemEventStore */ }
+docs/design/lmdb/tests.md:17:    pub fn lmdb() -> Self { /* LmdbEventStore in tmp dir */ }
+docs/design/lmdb/tests.md:56:Plus a static-assertion-style test ensuring no other public function on `EventStore` writes to the primary store (compile-time check by inspecting trait method list via a build script — deferred to v1.x; v1 covers via review).
+docs/design/podcast/lessons.md:106:- "No business logic in native." (Doctrine D0 + AGENTS.md guardrails.)
+docs/design/firehose-bench.md:7:> **Prerequisites:** `product-spec.md` (especially §7.1 EventStore, §7.2 planner, §7.3 outbox, §7.8 sync engine, §7.16 metrics); `reactivity.md`; ADRs 0001–0005.
+docs/design/lmdb/trait.md:1:# LMDB sub-design: `EventStore` trait
+docs/design/lmdb/trait.md:7:`crates/nmp-core/src/store/events.rs` (filename note: `trait` is a Rust keyword, so the file is named `events.rs` and exposes `pub trait EventStore`). Re-exported from `nmp_core::store::EventStore`. The actor (`crates/nmp-core/src/actor.rs`) holds the store as `store: Box<dyn EventStore>`; backends are constructed by the factory in `store/mod.rs::open_event_store(&AppConfig) -> Result<Box<dyn EventStore>, StoreError>`.
+docs/design/lmdb/trait.md:144:pub trait EventStore: Send + Sync {
+docs/design/lmdb/trait.md:282:## 5. Error semantics (doctrine D3)
+docs/design/lmdb/trait.md:288:- `Encoding` → `tracing::error!` with the offending key/namespace; the action that triggered it fails with a `toast: Some("internal storage error; please restart")` per D3.
+docs/design/lmdb/trait.md:299:pub struct MemEventStore { /* HashMap-backed; preserves the current kernel state */ }
+docs/design/lmdb/trait.md:302:pub struct LmdbEventStore { /* wraps nostr_lmdb::NostrLMDB + NMP sub-dbs */ }
+docs/design/lmdb/trait.md:304:pub fn open_event_store(cfg: &AppConfig) -> Result<Box<dyn EventStore>, StoreError> {
+docs/design/lmdb/trait.md:306:        StorageBackend::Memory => Ok(Box::new(MemEventStore::new())),
+docs/design/lmdb/trait.md:307:        StorageBackend::Lmdb { ref path } => Ok(Box::new(LmdbEventStore::open(path)?)),
+docs/design/lmdb/trait.md:312:`MemEventStore` implements every method using `HashMap` / `BTreeMap`. The same test suite runs against both backends with `#[cfg(feature = "lmdb-backend")]` gating only the LMDB-specific edge tests (corruption recovery, oversized values).
+docs/design/ffi-hardening/ci.md:86:        --require-doctrines D0,D1,D2,D3,D4,D5 \
+docs/design/ffi-hardening/ci.md:182:            fails.append("FFI grep yielded TODO/FIXME tokens; see §7.1")
+docs/design/ffi-hardening/ci.md:338:4. Doctrine review (D0–D5) signed off in `doctrine-review.md`.
+docs/design/ffi-hardening/scenarios.md:261:working set explodes; relay workers can't send fast enough.
+docs/design/ffi-hardening/scenarios.md:272:1. Peak working-set memory during storm ≤ **150 MB** (planner is the
+docs/product-spec/api-surface.md:157:Doctrine:
+docs/design/ffi-hardening/gates.md:7:2. **§D** doctrine D0–D5 review checklist — each line item maps to
+docs/design/ffi-hardening/gates.md:107:| Peak working-set RSS during storm | `<=` | 150 MiB | 200 MiB |
+docs/design/ffi-hardening/gates.md:150:## §D. Doctrine D0–D5 review checklist
+docs/design/ffi-hardening/gates.md:155:> **Note.** The task brief mentioned "D0–D5". The canonical list in
+docs/design/ffi-hardening/gates.md:156:> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
+docs/design/ffi-hardening/gates.md:159:> not redundantly re-prove — items beyond D0–D5 are covered by
+docs/design/ffi-hardening/gates.md:162:### D0. Kernel never grows app nouns
+docs/design/ffi-hardening/gates.md:164:- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
+docs/design/ffi-hardening/gates.md:171:  `docs/perf/m10.5/doctrine-review.md` § D0.
+docs/design/ffi-hardening/gates.md:173:### D1. Best-effort rendering — render now, refine in place
+docs/design/ffi-hardening/gates.md:185:- 📝 **Sign-off:** doctrine-review.md § D1.
+docs/design/ffi-hardening/gates.md:187:### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
+docs/design/ffi-hardening/gates.md:193:- ✅ **Stress proof (working set):** S8 asserts planner state
+docs/design/ffi-hardening/gates.md:197:- 📝 **Sign-off:** doctrine-review.md § D2.
+docs/design/ffi-hardening/gates.md:199:### D3. Errors never cross FFI
+docs/design/ffi-hardening/gates.md:201:- ⚠️ **Current state:** debt-inventory §3 D3 audit concludes
+docs/design/ffi-hardening/gates.md:210:- 📝 **Sign-off:** doctrine-review.md § D3, with the explicit
+docs/design/ffi-hardening/gates.md:211:  note that this milestone *closes* the D3-incomplete state
+docs/design/ffi-hardening/gates.md:214:### D4. Single writer per fact — caches derive
+docs/design/ffi-hardening/gates.md:216:- ✅ **Proof:** debt-inventory §3 D4 audit — single-threaded actor
+docs/design/ffi-hardening/gates.md:229:- 📝 **Sign-off:** doctrine-review.md § D4.
+docs/design/ffi-hardening/gates.md:231:### D5. Snapshots bounded by what's open
+docs/design/ffi-hardening/gates.md:241:- 📝 **Sign-off:** doctrine-review.md § D5.
+docs/design/ffi-hardening/gates.md:245:## §D.1 Doctrine sign-off artifact
+docs/design/ffi-hardening/gates.md:251:# M10.5 Doctrine Review
+docs/design/ffi-hardening/gates.md:253:| Doctrine | Status | Evidence | Reviewer | Date |
+docs/design/ffi-hardening/gates.md:255:| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
+docs/design/ffi-hardening/gates.md:256:| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
+docs/design/ffi-hardening/gates.md:257:| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
+docs/design/ffi-hardening/gates.md:258:| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
+docs/design/ffi-hardening/gates.md:259:| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
+docs/design/ffi-hardening/gates.md:260:| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
+docs/design/lmdb-schema.md:1:# Design: LMDB schema + EventStore trait + GC policy (M3)
+docs/design/lmdb-schema.md:6:> **Prerequisites:** `docs/product-spec/subsystems.md` §7.1 (insert invariants), `docs/decisions/0003-working-set-memory.md` (GC policy intent), `docs/decisions/0009-app-extension-kernel-boundary.md` (DomainModule storage), `docs/design/kernel-substrate.md` §2 (DomainModule trait).
+docs/design/lmdb-schema.md:11:## 1. Decision: wrap `nostr-lmdb` behind our own `EventStore` trait
+docs/design/lmdb-schema.md:13:**Adopt `nostr-lmdb` as the on-disk byte store. Wrap it behind the NMP `EventStore` trait. Add NMP-owned LMDB sub-databases for the rows `nostr-lmdb` does not model.**
+docs/design/lmdb-schema.md:15:The competing options were (1) use `nostr-lmdb` directly via its concrete `NostrLMDB` type (or via `nostr_database::NostrEventsDatabase`), (2) wrap behind our own trait, or (3) hand-roll an LMDB layer.
+docs/design/lmdb-schema.md:17:**`nostr-lmdb` gives us** (per `docs.rs/nostr-lmdb`): `save_event(&Event)`, `event_by_id(&EventId)`, `check_id(&EventId) -> DatabaseEventStatus`, `query(Filter) -> Events`, `count(Filter)`, `delete(Filter)`, `wipe()`, `negentropy_items(Filter) -> Vec<(EventId, Timestamp)>`. It owns the page allocator, the LMDB environment, primary by-id store, indexes derived from `Filter`, replaceable / parameterized-replaceable supersession, and NIP-09 delete handling. It is the only mature Rust LMDB store for Nostr events with proven NIP-77 integration; reinventing it is a year of work that we will not recoup.
+docs/design/lmdb-schema.md:19:**What `nostr-lmdb` does *not* model** (the gap that justifies a wrapper):
+docs/design/lmdb-schema.md:21:| Concern | Why `nostr-lmdb` doesn't cover it | Where NMP needs it |
+docs/design/lmdb-schema.md:33:**Therefore.** `EventStore` is a NMP-owned trait, with one production impl `LmdbEventStore` that holds (a) a `NostrLMDB` for the canonical event store and Nostr-shaped queries, and (b) NMP-owned secondary LMDB sub-databases under the same `lmdb::Environment` for the gap rows. The in-memory backend (`MemEventStore`) remains, both for tests and as the web-pre-M15 fallback. See [`lmdb/trait.md`](lmdb/trait.md) for the exact trait shape and the relayed-vs-owned method split.
+docs/design/lmdb-schema.md:37:- *Use `NostrLMDB` directly, no wrapper.* Loses every gap row above. Forces the kernel actor to know about LMDB transactions and a non-NMP concrete type, breaking the `Box<dyn EventStore>` substitutability M3 requires.
+docs/design/lmdb-schema.md:38:- *Roll our own.* Reinvents NIP-09 / replaceable handling that `nostr-lmdb` already gets right. ~2,000 LOC of avoidable code with a worse bug surface than upstream.
+docs/design/lmdb-schema.md:39:- *SQLite-backed `nostr-sdk` store.* Larger memory footprint at our 10k-event hot working set; iOS-disk-WAL fsync cost not justified for this access pattern. Held in reserve for the web port (M15) if IndexedDB OPFS proves unworkable.
+docs/design/lmdb-schema.md:46:  trait.rs              — `EventStore` (see lmdb/trait.md)
+docs/design/lmdb-schema.md:49:    mod.rs              — `LmdbEventStore` orchestrator
+docs/design/lmdb-schema.md:63:## 3. EventStore trait
+docs/design/lmdb-schema.md:65:See [`lmdb/trait.md`](lmdb/trait.md) for the exact `pub trait EventStore` signature with all required methods, return types, and the `StoreError` enum. Summary:
+docs/design/lmdb-schema.md:75:**Error semantics.** All methods return `Result<T, StoreError>`. Per doctrine D3, store errors do **not** cross FFI — the actor maps every variant to either (a) a typed `Effect` (e.g. `StoreCorrupt`, surfaces via diagnostics + toast), (b) a `tracing::warn!` log + degraded state, or (c) a panic at startup if the LMDB environment refuses to open. The trait itself uses `Result` since it is internal to the actor process.
+docs/design/lmdb-schema.md:81:- Primary `events`: `event_id[32]` → `Event` (CBOR via `nostr` crate's serialization). Owned by `nostr-lmdb`.
+docs/design/lmdb-schema.md:109:Key: `filter_hash[32] || relay_url_bytes` (no length prefix needed — relay URL is the variable suffix; lookup uses exact key). Populated by M4 (NIP-77) and consulted by M2's planner (cache-coverage check before issuing backfill REQ). Survives restarts; loaded into the actor on startup as a `HashMap<(filter_hash, relay_url), WatermarkRow>` for hot lookups, with all writes going through `EventStore` for durability.
+docs/design/lmdb-schema.md:126:## 7. GC working-set policy
+docs/design/lmdb-schema.md:142:ADR-0003's numbers are preserved as the M3 exit gate (§11 below): ≤ 100 MB working-set at 100 active views / 10k hot events / 1M cached on disk.
+docs/design/lmdb-schema.md:160:Rejected: stuffing provenance into the `Event` row. That requires re-serializing the full `Event` on every relay redelivery (high write amplification — popular events arrive 5–20× from the relay fan-out) and forks the `nostr-lmdb` row format, which we explicitly want to keep upstream-compatible. The sidecar is appended cheaply with a single CBOR re-encode of the (typically small) `sources` vector.
+docs/design/lmdb-schema.md:208:| Working-set memory at 100 active views / 10k hot / 1M on disk | ≤ 100 MB resident | Instruments Allocations + `reactivity-bench` working-set scenario |
+docs/design/lmdb-schema.md:218:1. **`nostr-lmdb` LMDB environment sharing.** Can we open the same `lmdb::Environment` for both `NostrLMDB`'s sub-databases and our own NMP sub-databases (provenance, watermarks, claims, domain rows)? If yes, we get atomic cross-sub-db transactions for free (a single `RwTxn` covers event + provenance + secondary indexes). If `nostr-lmdb` insists on opening its own `Environment`, we lose that and the insert path needs a two-phase write with crash-recovery logic. Investigate before implementation — may require an upstream PR exposing `Environment` access.
+docs/design/lmdb-schema.md:223:6. **`ModuleRegistry::register_domain` API stability.** Adding `DomainFactories` to `ModuleDescriptor` is a non-breaking additive change to the public substrate API (existing callers using only the generic `register_domain::<M>()` continue to compile), but it commits us to keeping `DomainModule::SCHEMA_VERSION` and `DomainModule::migrations` as compile-time-resolvable items rather than object-safe methods. Confirm this with the substrate maintainer before M3 lands — if `DomainModule` is expected to support runtime composition (e.g., plugin loading), we need option (c): the actor passes the live `&[Box<dyn DomainModule>]` to `EventStore::open` instead. Recommended default: stay with `fn`-pointer factories; revisit if a plugin-loading use case appears.
+docs/design/lmdb-schema.md:229:- Replaceable supersession (current scattered logic to be centralised in `EventStore::insert`): `kernel/ingest.rs:166-185` (profile replace by `(pubkey, kind)`), `ingest.rs:218-233` (NIP-65 list replace by `(pubkey, 10002)`).
+docs/design/lmdb-schema.md:230:- Profile claim refcounting (current in-memory analogue of `EventStore::claim/release`): `kernel/mod.rs:315` (`profile_claims: HashMap<String, BTreeSet<String>>`), `kernel/requests.rs:202` (`claim_profile`), `requests.rs:239` (`release_profile`).
+docs/design/lmdb-schema.md:234:The M3 implementation deletes none of the existing files outright — the kernel's `events: HashMap` and `profiles: HashMap` are replaced by calls to `Box<dyn EventStore>` held inside the `Kernel` struct, and the per-field tests (`kernel/tests.rs`) shift to the new trait via `MemEventStore`. No public FFI surface changes.
+docs/product-spec/overview-and-dx.md:31:### D0. Kernel + extension modules — no app nouns in `nmp-core`
+docs/product-spec/overview-and-dx.md:41:### D1. Best-effort rendering — render now, refine in place
+docs/product-spec/overview-and-dx.md:54:### D2. Negentropy first, REQ second
+docs/product-spec/overview-and-dx.md:60:### D3. Outbox routing is automatic; manual relay selection is the opt-out
+docs/product-spec/overview-and-dx.md:73:### D4. Single writer per fact; caches derive
+docs/product-spec/overview-and-dx.md:75:The "single source of truth" doctrine does not mean one cache — there are five layers (durable event store, in-memory working set, view payloads, gossip cache, platform reactive shadow). It means **one writer per fact**, and every downstream cache derives from the writer mechanically. Cache invalidation is not a concept in the public API. Recomputation happens in the actor; the platform receives new derived state.
+docs/product-spec/overview-and-dx.md:77:### D5. Snapshots bounded by what's open
+docs/design/podcast/wiring.md:11:3. Replace `// MARK: NMP-WIRE — TODO` blocks with `// MARK: NMP-WIRE — wired` consuming the generated `@<View>` property wrapper.
+docs/design/podcast/wiring.md:88:- Acceptance: subscribe to a real RSS feed (Tim Ferriss) → library shows it → tap → see ≥ 5 episodes parsed; UI updates as `FetchFeed` action completes (no spinner gate; placeholder rows during fetch per doctrine D1).
+docs/design/podcast/wiring.md:201:`grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` ratchets to zero across the 7 lanes.
+docs/design/podcast/wiring.md:212:- [ ] All `// MARK: NMP-WIRE — TODO` in the lane removed
+
+codex
+The first pass found the relevant doctrine anchors and the M3 docs. I’m also checking prior project memory for any known review pattern or prior M3 context, then I’ll reduce this to concrete findings.
+exec
+/bin/zsh -lc 'rg -n "nostr-multi-platform|9fead0e|M3|lmdb|EventStore|Doctrine D0|D2|working-set|working set" /Users/pablofernandez/.codex/memories/MEMORY.md' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+236:- com.podcastr.next.Podcastr, devicectl, 00008150-001E118E3CD2401C, Pablo's iPhone, xcodebuild -showdestinations, /tmp/podcast-rmp-add-toolbar-device-build, App installed, Launched application
+250:- the working device path here was `xcodebuild -showdestinations`, then `xcodebuild build` for device id `00008150-001E118E3CD2401C`, then `xcrun devicectl device install app`, then `xcrun devicectl device process launch --terminate-existing com.podcastr.next.Podcastr` [Task 2]
+2304:- `xcodegen generate` is required here after adding new Swift files, and the working device path was `xcodebuild -project WeightTracker.xcodeproj -scheme WeightTracker -destination 'platform=iOS,id=00008150-001E118E3CD2401C' build` followed by `xcrun devicectl device install app` and `xcrun devicectl device process launch` [Task 4][Task 5]
+2468:- the working iPhone path was `cargo build --target aarch64-apple-ios --release -p tenex-core`, then `xcodebuild -workspace TenexMVP.xcworkspace -scheme TenexMVP -configuration Debug -destination 'platform=iOS,id=00008150-001E118E3CD2401C' -derivedDataPath .codex-derived-ios-device -clonedSourcePackagesDirPath .codex-derived-ios-device/SourcePackages build`, followed by `xcrun devicectl device install app` and `xcrun devicectl device process launch --terminate-existing com.tenex.mvp` [Task 1]
+2637:- the working iPhone deployment sequence was `xcodebuild ... -destination 'id=00008150-001E118E3CD2401C' -derivedDataPath /tmp/WinTheDayDerived build`, then `xcrun devicectl device install app`, then `xcrun devicectl device process launch`; `No provider was found` warnings from `devicectl` were noisy but non-fatal here [Task 3]
+2943:- rust migration, tenex-daemon, daemon.ts, M1, M2, M3, M8, M9, quality gates, publish_outbox, worker_protocol, can I use it yet, test:rust:publish-interop
+
+exec
+/bin/zsh -lc "nl -ba docs/product-spec/overview-and-dx.md | sed -n '25,85p'" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc 'wc -l docs/design/lmdb-schema.md docs/design/lmdb/gc.md docs/design/lmdb/keys.md docs/design/lmdb/tests.md docs/design/lmdb/trait.md docs/design/lmdb/watermarks.md' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     234 docs/design/lmdb-schema.md
+     195 docs/design/lmdb/gc.md
+     150 docs/design/lmdb/keys.md
+     223 docs/design/lmdb/tests.md
+     312 docs/design/lmdb/trait.md
+     191 docs/design/lmdb/watermarks.md
+    1305 total
+
+ succeeded in 0ms:
+    25	---
+    26	
+    27	## 1.5 Cardinal doctrines
+    28	
+    29	Six named principles that subsume the rest of this spec. Every API decision answers to at least one of these; conflicts between them resolve in the order listed.
+    30	
+    31	### D0. Kernel + extension modules — no app nouns in `nmp-core`
+    32	
+    33	Per ADR-0009, NMP is a Nostr-native app kernel plus extension modules. The kernel provides substrate; protocol modules and app modules contribute typed variants via `ViewModule`, `ActionModule`, `DomainModule`, `CapabilityModule`, and `IdentityModule`. If implementing a real app requires adding domain nouns to `nmp-core`, the kernel boundary is wrong and must change.
+    34	
+    35	This rules out:
+    36	
+    37	- `nmp-core` becoming a junk drawer of every consumer's domain concepts.
+    38	- App-specific business logic in Swift, Kotlin, or TypeScript shells.
+    39	- Closed FFI enums that prevent modules from contributing typed views, actions, updates, capabilities, or identity scopes.
+    40	
+    41	### D1. Best-effort rendering — render now, refine in place
+    42	
+    43	Apps built with this framework **never withhold cached data and never block on fetches**. Every view payload field carries a value, not a "loading" status. Missing display names default to a shortened npub; missing pictures default to a deterministic identicon URI; missing timestamps default to "now". When a more authoritative value (e.g., the author's kind:0) arrives later, the view payload updates in place and the affected cell re-renders. The UI never sees a spinner gating already-renderable content.
+    44	
+    45	The doctrine is enforced by the view payload **types**: display fields are non-`Option`, placeholders are part of the type contract, and freshness is exposed (when relevant) as an optional badge hint, not a render gate. There is no `if has_profile { render } else { spinner }` pattern available in the API — the framework does not provide one.
+    46	
+    47	This rules out, by construction, the most common Nostr-client failure modes:
+    48	
+    49	- Hiding a post because the author's profile hasn't loaded yet.
+    50	- Replacing cached profile metadata with a spinner because "we might have something newer."
+    51	- Refusing to render threads because the root event isn't in cache.
+    52	- Profile-picture flicker between cached and placeholder.
+    53	
+    54	### D2. Negentropy first, REQ second
+    55	
+    56	NIP-77 negentropy reconciliation is the default backfill mechanism. Every `(filter, relay)` pair the app touches is treated as a tracked sync target with a watermark. Live REQ remains the tailing path, but historical gaps consult coverage first and prefer sync over REQ scans when relays support it.
+    57	
+    58	This is not a product feature you opt into later; it is a subscription policy built on explicit coverage metadata. See §7.8.
+    59	
+    60	### D3. Outbox routing is automatic; manual relay selection is the opt-out
+    61	
+    62	Per NIP-65, reads and writes are routed to the relevant relays by framework policy without normal app code specifying them. Subscriptions with `authors` filters route to those authors' write relays; publishes go to the author's write relays plus tagged recipients' inbox relays; discovery falls back to a configurable indexer set.
+    63	
+    64	The safe public path does not ask the developer to pick relays per operation. Explicit override and diagnostic/test paths exist, but they are named, observable, and excluded from the default app-building flow.
+    65	
+    66	This rules out, by construction:
+    67	
+    68	- Posts to relays the author hasn't declared as write relays.
+    69	- DMs leaked to public relays.
+    70	- Silent reads against a default relay set that miss an author's actual relays; unknown relay lists surface as coverage/diagnostic state and use a bounded fallback policy.
+    71	- Hand-rolled fan-out logic in app code.
+    72	
+    73	### D4. Single writer per fact; caches derive
+    74	
+    75	The "single source of truth" doctrine does not mean one cache — there are five layers (durable event store, in-memory working set, view payloads, gossip cache, platform reactive shadow). It means **one writer per fact**, and every downstream cache derives from the writer mechanically. Cache invalidation is not a concept in the public API. Recomputation happens in the actor; the platform receives new derived state.
+    76	
+    77	### D5. Snapshots bounded by what's open
+    78	
+    79	What crosses FFI is the projection through currently-open views, not the underlying event store. `AppState` carries small screen-shaped data plus a map of `ViewId → ViewPayload` for views currently in use. Closing a view evicts its payload from the snapshot. The event store itself never crosses FFI. See §6.2 and the FFI architecture appendix (§A1).
+    80	
+    81	---
+    82	
+    83	## 2. Audience and use cases
+    84	
+    85	**Primary audience.** Application developers building Nostr clients for production distribution on iOS, Android, desktop, and web — including LLM-driven and inexperienced developers who lack the protocol literacy to navigate Nostr's footguns unaided.
+
+exec
+/bin/zsh -lc "nl -ba docs/product-spec/subsystems.md | sed -n '120,275p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+   120	    pub contacts_view_id: ViewId,
+   121	    pub mailboxes_view_id: ViewId,
+   122	    pub mutes_view_id: ViewId,
+   123	    pub status: AccountStatus,
+   124	}
+   125	```
+   126	
+   127	Signers are managed entirely in `nmp-core`. The initial product signer catalog is:
+   128	
+   129	- Local key (raw nsec, stored encrypted via `KeyringCapability`)
+   130	- NIP-49 (password-encrypted private key)
+   131	- NIP-46 bunker / Nostr Connect
+   132	- NIP-07 (web only)
+   133	- External — Android Amber (NIP-55) bridged via `ExternalSignerCapability`
+   134	
+   135	The signer abstraction inside `nmp-core` is a Rust trait with `sign(unsigned_event) -> Future<signed_event>`. Adding a signer kind is an internal task; external developers do not implement signers.
+   136	
+   137	### 7.5 Actions catalog
+   138	
+   139	Actions live in `nmp-actions`. Each action is a Rust async fn taking an action context (`event_store`, `signer`, `publisher`, `active_account`) and producing zero or more signed events. The actor runs actions on its tokio runtime; results route through `InternalEvent` back to the actor for atomic state update.
+   140	
+   141	Action authoring contract for the framework's own contributors (not exposed at FFI):
+   142	
+   143	```rust
+   144	#[async_trait]
+   145	pub trait Action: Send + Sync + 'static {
+   146	    type Output: Send + 'static;
+   147	    async fn run(self, cx: &ActionCx) -> Result<Self::Output>;
+   148	}
+   149	```
+   150	
+   151	Built-in actions (long-term product catalog): the AppAction variants listed in §6.3 each map to one Action implementation. v1 ships only the generic kernel actions named in `docs/plan.md`. Custom actions are first-class via a sister crate pattern (apps add their own actions crate that depends on `nmp-actions`).
+   152	
+   153	Atomicity invariant: an action's local event-store commit, side-effect intent, and ledger transition happen as actor messages with one parent action id. The action future runs on the tokio runtime, but all state mutation happens in `handle_message`. There is no public API that lets a developer publish, upload, sign, or issue an NWC request without a renderable action-ledger row.
+   154	
+   155	The ledger is general, not relay-only. It can represent local optimistic commit, signer prompt, per-relay publish attempt, HTTP upload, NWC request, retry, repair, partial failure, timeout, and final status. Relay publishes additionally track attempted/acked/failed/timed-out by relay plus required success count.
+   156	
+   157	### 7.6 Views
+   158	
+   159	`nmp-views` defines `ViewSpec` and all built-in `ViewPayload` variants:
+   160	
+   161	| View | Inputs | Payload |
+   162	|---|---|---|
+   163	| Profile | `pubkey` | latest kind-0 parsed; pre-formatted display name; verified domain |
+   164	| Contacts | `pubkey` | parsed kind-3 follow list, with per-followee metadata |
+   165	| Mailboxes | `pubkey` | parsed kind-10002 |
+   166	| Mutes | `pubkey` | parsed kind-10000 |
+   167	| Blossom servers | `pubkey` | parsed kind-10063 |
+   168	| Timeline | `filter` (kind, authors, hashtags, time window) | sorted slice with pagination cursor |
+   169	| Thread | `root_event_id` | tree with per-node metadata |
+   170	| Replies | `event_coord` | flat list with per-reply metadata |
+   171	| Reactions | `event_coord` | grouped count by emoji + per-pubkey list |
+   172	| Conversation list | `account_pubkey` | sorted DM threads with unread counts and latest message preview |
+   173	| Conversation | `peer_pubkey` | paginated decrypted messages |
+   174	| Zap history | `account_pubkey` | bidirectional list |
+   175	| Wallet balance | `wallet_id` | balance + pending transactions |
+   176	| WoT rank | `pubkey` | trust score + reasoning |
+   177	| Search | `query`, `kinds`, `time_window` | result list |
+   178	
+   179	Each payload type carries **pre-formatted** display strings (timestamps in user locale, npub-shortened forms, sat amounts). Per bible doctrine: no platform-side formatting.
+   180	
+   181	**Best-effort field contract (per doctrine D1).** Every display-bearing field in every view payload is **non-optional** and has a defined placeholder when the underlying data is missing:
+   182	
+   183	| Field | Placeholder when missing |
+   184	|---|---|
+   185	| Display name | Shortened npub: `npub1abc…xyz` |
+   186	| Picture URL | Deterministic identicon URI derived from pubkey |
+   187	| NIP-05 verified domain | empty string (UI conditionally renders a checkmark only when non-empty) |
+   188	| Timestamp string | "just now" |
+   189	| Reaction count | 0 |
+   190	| Zap total | 0 sats |
+   191	| Content body (if missing) | empty string (the item still renders; only the body region is blank) |
+   192	
+   193	When the underlying data arrives — kind:0 for an author, kind-9735 zap receipts for a note, the actual decrypted body for a DM — the view payload updates in place, the platform's reactive primitive detects the change, and only the affected cell re-renders. No spinner is ever shown for already-rendered cells.
+   194	
+   195	**Stale freshness is exposed, not gated.** Each enriched-from-cache field may optionally carry a sibling field `xxx_freshness: FreshnessHint` (recent, hours_old, days_old, never_verified). UI may choose to render a small badge. The framework never withholds the underlying value based on freshness.
+   196	
+   197	**Concrete example: lean timeline payload.**
+   198	
+   199	```rust
+   200	#[derive(Clone, uniffi::Record)]
+   201	pub struct TimelineView {
+   202	    pub cursor: Cursor,
+   203	    pub items: Vec<TimelineItem>,
+   204	    pub has_more: bool,
+   205	}
+   206	
+   207	#[derive(Clone, uniffi::Record)]
+   208	pub struct TimelineItem {
+   209	    pub id: String,                   // event id hex
+   210	    pub author_pubkey: String,
+   211	    pub author_display: String,       // never empty; npub-shortened if no kind:0
+   212	    pub author_picture: String,       // never empty; identicon URI if no kind:0
+   213	    pub author_nip05_domain: String,  // empty if not verified
+   214	    pub content_preview: String,      // pre-truncated for list display
+   215	    pub created_at_display: String,   // pre-formatted, locale-aware
+   216	    pub reaction_summary: ReactionSummary,
+   217	    pub zap_sats_total: u64,
+   218	    pub reply_count: u32,
+   219	    pub repost_of: Option<EventCoord>,
+   220	    pub quote_of: Option<EventCoord>,
+   221	}
+   222	```
+   223	
+   224	`TimelineItem` is a flat summary. The full event content, raw tags, signature, and provenance live in the event store inside Rust and do not cross FFI. This matches the precedent set by the bible's reference implementation (Pika): chat list is summaries; current chat loads full content on demand.
+   225	
+   226	View warmth: a view stays cached for 30 seconds after its last claim is dropped (configurable). Re-opening within the window costs zero relay traffic and zero re-sync.
+   227	
+   228	Post-v1 content rendering contract: protocol-aware content parsing lives in Rust, not in platform shells. The content layer emits serializable nodes for text, links, NIP-19/NIP-21 entities, hashtags, media hints, mentions, quotes, and truncation boundaries. Platform shells render those nodes and may style them, but they do not parse Nostr content or decide URL/media safety policy.
+   229	
+   230	### 7.7 Web of Trust
+   231	
+   232	`nmp-wot` ships as an optional subsystem (gated by `AppConfig.wot_enabled`). On enable:
+   233	
+   234	- Loads the active account's follow graph to a configurable depth (default 2).
+   235	- Computes per-pubkey trust scores (default algorithm: simple in-degree weighted by depth; pluggable via a trait).
+   236	- Exposes a global filter: when on, every view applies the score threshold before emitting; pubkeys below the threshold are tagged but rendered with a "low trust" UI hint (the renderer chooses; the payload exposes the score).
+   237	
+   238	Computation is incremental; updates to follow lists update scores without recomputing from scratch.
+   239	
+   240	### 7.8 Sync engine (live REQ plus NIP-77 backfill)
+   241	
+   242	Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.
+   243	
+   244	**Position in the stack.**
+   245	
+   246	```
+   247	View opens → Live REQ handler starts → Planner consults coverage → Sync engine reconciles gaps → EventStore inserts → ViewBatch emits
+   248	                                ↓ (fallback)
+   249	                                bounded fetch / REQ scan
+   250	```
+   251	
+   252	**Watermarks as a first-class type.** The engine reads and writes the `watermarks` table introduced in §7.1. A watermark answers two questions:
+   253	
+   254	- Has this `(filter, relay)` pair ever been synced?
+   255	- If so, up to what timestamp?
+   256	
+   257	Answers to those questions inform every backfill, every fallback-loader decision, and every "is this cache miss authoritative?" check.
+   258	
+   259	**Three triggers, all built-in.**
+   260	
+   261	1. **App foreground.** On `AppAction::Foreground`, the engine schedules an incremental sync for the active user's home filter (kind:1, kind:6, kind:7 matching followed authors) against their write relays. Runs in the tokio runtime; emits `SyncState` updates as it progresses; no UI blocking.
+   262	2. **View open.** When a view opens whose filter has a gap (per watermark/coverage), the engine reconciles the gap concurrently with the live REQ tail. Progress is visible in `SyncState`; the view payload streams in as events land.
+   263	3. **Relay reconnect.** On reconnect, the planner re-establishes live REQs and schedules a coverage-aware gap fill. The gap between disconnect and reconnect is filled by sync when possible, not by re-fetching from scratch.
+   264	
+   265	**Manual sync as an action.** `AppAction::RunSync { spec }` lets apps trigger arbitrary reconciliations (e.g., "sync this user's last 30 days of articles"). Same engine, different trigger.
+   266	
+   267	```rust
+   268	pub struct SyncSpec {
+   269	    pub filter: Filter,
+   270	    pub relay: String,
+   271	    pub time_window: Option<(u64, u64)>,
+   272	    pub direction: SyncDirection,           // Pull, Push, Bidirectional
+   273	    pub on_completion: SyncCompletionAction,
+   274	}
+   275	```
+
+exec
+/bin/zsh -lc "nl -ba docs/decisions/0003-working-set-memory.md | sed -n '1,220p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# ADR 0003: Memory budget is for working set, not total cached events
+     2	
+     3	**Date:** 2026-05-17
+     4	**Status:** accepted
+     5	**Supersedes:** `reactivity.md` rev 0 §10.3 memory budget
+     6	
+     7	## Context
+     8	
+     9	The initial gate read "≤ 100 MB at 100k events / 100 views." The reactivity-bench harness (run 001) reported 130.8 MB at 1M events, failing the gate. But this is misleading: holding 1M events resident in memory is the anti-pattern the spec already calls out for the durable storage backend (LMDB / SQLite / IndexedDB / nostrdb).
+    10	
+    11	The actor should keep a **bounded working set** of hot events in memory; cold events live on disk. The reverse index can cover both — it keys on attributes, not event bodies.
+    12	
+    13	## Decision
+    14	
+    15	The memory budget targets **working-set memory at typical active load**, not total cached events.
+    16	
+    17	| Metric | Budget |
+    18	|---|---|
+    19	| Working-set memory at 100 active views, 10k hot events | ≤ 100 MB |
+    20	| Total cached events on disk | unbounded (or capped by backend quota) |
+    21	
+    22	Working-set policy:
+    23	
+    24	- **Hot:** events referenced by any open view's claim set, plus a configurable recency window (default: most recent 5,000 events globally).
+    25	- **Cold:** everything else, on disk only.
+    26	- **Eviction:** LRU among hot events not currently claimed.
+    27	
+    28	The reverse index indexes both hot and cold events. Lookup returns view ids immediately; event bodies for delta construction load lazily and synchronously via the storage backend.
+    29	
+    30	Projection caches (`author_display`, `reaction_summary`, etc.) are LRU-bounded by referenced-view count; not every pubkey ever seen stays in the projection cache.
+    31	
+    32	## Consequences
+    33	
+    34	- The 1M-events-resident scenario is no longer a failure — it's an unintended test of an unintended configuration. Re-run with bounded working set.
+    35	- Cold-event delta construction has a one-time disk hit; this is acceptable for replaceable events (kind:0 re-load on profile fan-out) but worth measuring.
+    36	- Eviction policy needs explicit design; LRU is the default but priority-ordered (e.g., never evict claimed events) is the real invariant.
+    37	
+    38	## Alternatives considered
+    39	
+    40	- **Keep absolute gate, raise number.** Rejected — doesn't address the underlying anti-pattern.
+    41	- **Cap total cached events.** Rejected — the storage backend already handles this; the framework should not duplicate.
+    42	- **All-in-memory cache.** Rejected — doesn't scale and is contrary to the storage abstraction.
+    43	
+    44	## Validation
+    45	
+    46	Re-run reactivity-bench with bounded working set; require ≤ 100 MB at 100 views / 10k hot events / 1M cached events on disk.
+
+exec
+/bin/zsh -lc "nl -ba docs/product-spec/subsystems.md | sed -n '1,120p'" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc "nl -ba docs/plan.md | sed -n '1,175p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# Product Spec: Subsystems
+     2	
+     3	[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)
+     4	
+     5	## 7. Subsystem specifications
+     6	
+     7	### 7.1 EventStore
+     8	
+     9	Single instance per `FfiApp`, owned by the actor. Public to the framework (not to native).
+    10	
+    11	Behaviors guaranteed at insert time:
+    12	
+    13	| Concern | Behavior |
+    14	|---|---|
+    15	| Insert API | Every event enters through one actor-owned insert path returning `InsertOutcome`; no caller mutates indexes or storage directly. |
+    16	| Signature/delegation validity | Verified before any tombstone, provenance, replaceable-index, or durable-storage mutation. |
+    17	| Duplicate id | Merge relay provenance set; keep earliest `received_at`; do not overwrite. |
+    18	| Replaceable kinds (0, 3, 10000-19999) | Compare `(pubkey, kind)` against existing; keep newest `created_at`; tie-break by lexicographically smallest `id`. |
+    19	| Parameterized replaceable (30000-39999) | Compare `(pubkey, kind, d-tag)`; same supersession rule. |
+    20	| Kind 5 (delete) | After verification, scan referenced `e` and `a` tags and remove matching events authored by the deleter. Persisted as tombstone so later re-insertion is suppressed. Tombstone timestamp is the maximum delete timestamp observed for that target. |
+    21	| NIP-40 expiration | Schedule a tokio timer to remove the event at the expiration timestamp; on actor restart, scan and re-schedule. |
+    22	| NIP-26 delegation | Validate delegation tag at insert; reject malformed. |
+    23	| Ephemeral events | Delivered to live consumers but not durably stored. |
+    24	| Provenance | Every event records typed sidecar provenance: relay URL, first seen, last seen, source, and deterministic primary relay. |
+    25	| Query matching | Storage backends may return candidates; every result is re-run through the canonical matcher before it affects state or views. |
+    26	
+    27	Storage backend is configurable via `AppConfig.storage_backend` (LMDB or SQLite-style native backend, IndexedDB/OPFS strategy for web, final choice resolved before v1). The store wraps the Rust Nostr SDK protocol types, but NMP owns the application-kernel storage traits because the app kernel needs typed provenance, action ledger rows, relay metadata, domain records, and bounded-view indexes in addition to raw events.
+    28	
+    29	GC: a claim-based collector tracks `view_id → Vec<event_id>` references. View close drops claims. A periodic `prune()` removes events with zero claims that are also absent from declared "pinned" sets (sessions' contact-list events, sessions' relay-list events).
+    30	
+    31	**Sync watermarks.** The store maintains a per-`(filter_signature, relay_url)` table:
+    32	
+    33	```
+    34	watermarks {
+    35	  filter_sig: Hash,            // canonicalized filter
+    36	  relay_url: String,
+    37	  synced_up_to: u64,           // unix seconds; "we have everything matching this filter on this relay up to T"
+    38	  last_sync_method: SyncMethod, // Negentropy | ReqScan | Manual
+    39	  bytes_saved_vs_req: u64,     // cumulative, for diagnostics
+    40	  updated_at: u64,
+    41	}
+    42	```
+    43	
+    44	Watermarks are durable. On startup they are loaded into the actor; they survive app restarts. The planner (§7.2) consults them before issuing any backfill, and the sync engine (§7.8) updates them after every reconciliation.
+    45	
+    46	A cache-miss query against a fully-synced `(filter, relay)` pair is **authoritative**: the answer is "this event does not exist on that relay." A cache-miss against an unsynced pair triggers either a sync (if NIP-77 supported) or a fallback fetch.
+    47	
+    48	Fallback loading is split by need:
+    49	
+    50	- Pointer/address misses: cache-first lookup for event id or replaceable address, batched and deduped across waiting views, then relay hints, then configured fallback sources.
+    51	- Tag-value and timeline-window misses: bounded historical window loads that record what range is still unknown.
+    52	- Authoritative absence: only a complete coverage record/watermark can turn a miss into "not found." A non-empty cache result is not proof that a query is complete.
+    53	
+    54	The default loader queries open relays and configured sources. Users can add custom sources (CDN cache, local mirror, etc.) through app-kernel extension points, but loaded events still enter through the same verified insert path.
+    55	
+    56	### 7.2 Subscription planner
+    57	
+    58	Owns the mapping from `ViewSpec` → `Vec<Filter>` → `Vec<RelayUrl>` → on-the-wire REQ.
+    59	
+    60	Behaviors:
+    61	
+    62	- **Live tail first.** Live subscriptions register their local handler and start REQ tailing immediately. Historical backfill runs beside it, not before it.
+    63	- **Coverage-aware backfill.** Before issuing historical traffic, the planner consults cache coverage/watermarks (§7.1). Complete coverage serves from cache; partial coverage schedules a gap fill; unknown coverage triggers bounded fetch/REQ or NIP-77 if supported. A non-empty cache result is never treated as complete by itself.
+    64	- **Logical vs. wire subscriptions.** A logical subscription belongs to a view/action/monitor. A wire subscription belongs to a relay. Many logical consumers may share one wire REQ, and each consumer still receives only events matching its canonical filter.
+    65	- **Coalescing.** Filters that are equal or safely subsumable into a single broader filter share one REQ per relay. The planner maintains a formal merge lattice for `limit`, `since`, `until`, multi-filter arrays, and tag operators.
+    66	- **Loader integration.** Pointer/address/tag/timeline misses go through the pointer loader registry with cache-first batching, dedupe, relay hints, cancellation on view close, and explicit missing-window state.
+    67	- **Auto-close.** REQs without consumers are CLOSE'd. One-shot filters (those with no live subscribers, only an `until` upper bound) are CLOSE'd on EOSE.
+    68	- **Buffering.** Inbound events are batched to ≤ 60Hz per view (configurable). Batches turn into one `ViewBatch` per tick.
+    69	- **Backpressure.** If platform-side rendering falls behind, the planner drops `ViewBatch` updates in favor of a single `FullState` catch-up. View payload semantics make this lossless.
+    70	- **Reconnect.** On relay reconnect, the planner restores live REQs and schedules a coverage-aware gap fill. View payloads do not reset.
+    71	
+    72	### 7.3 Outbox routing
+    73	
+    74	Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
+    75	
+    76	**Resolution algorithm.**
+    77	
+    78	| Operation | Relay set |
+    79	|---|---|
+    80	| Subscription with `authors` filter | Union of each pubkey's write relays (kind-10002), deduplicated. Pubkeys without known mailboxes trigger an opportunistic kind-10002 fetch from indexer relays. |
+    81	| Subscription with `p` tag filter or notifications | Union of each tagged pubkey's inbox relays. |
+    82	| Subscription with neither | Active session's read relays. |
+    83	| Publish of any signed event | Author's write relays. |
+    84	| Publish with `p` tags (DMs, mentions, reactions) | Author's write relays **plus** each tagged pubkey's inbox relays. |
+    85	| DM (NIP-17 gift-wrapped) | **Only** resolved recipient inbox relays. Never the author's write relays. Never the active session's "default" relays. Missing recipient inbox relays fail closed. |
+    86	| Discovery (kind-10002 fetch for unknown pubkeys) | Configurable indexer relay set (default: a curated list of high-coverage relays). |
+    87	
+    88	**Why this prevents specific failure modes.**
+    89	
+    90	- "Publish leaked to wrong relays" → ruled out by the safe API. The developer cannot supply a relay list to `SendNote`. Explicit overrides are named, one-shot, and debug-flagged in logs.
+    91	- "DM accidentally public" → ruled out by the safe API. The DM publish path consults only resolved inbox relays; there is no fallback-to-all-relays path for gift wraps.
+    92	- "Reads missing an author's actual relays" → bounded and surfaced. If the author's kind-10002 is reachable it is opportunistically fetched on first contact; if not, coverage and diagnostic state expose the miss risk and configured fallback policy.
+    93	- "Hand-rolled fan-out logic" → no API surface for it.
+    94	
+    95	**Per-pubkey relay-list lifecycle.**
+    96	
+    97	- First contact with an unknown pubkey → enqueue kind-10002 fetch from indexer relays.
+    98	- Fresher kind-10002 arrives → invalidate dependent subscriptions, recompute relay sets, re-issue REQs as needed.
+    99	- Kind-10002 missing for a pubkey after N seconds → fall back to indexer set for reads only; do not publish to indexers.
+   100	
+   101	The gossip cache is the `nostr-gossip` crate; backend selection (in-memory vs SQLite) follows the storage backend choice. Watermarks (§7.1) intersect with outbox: a sync watermark is keyed by `(filter, relay)` and naturally tracks per-author per-relay coverage.
+   102	
+   103	### 7.4 Sessions
+   104	
+   105	`SessionState` holds:
+   106	
+   107	```rust
+   108	pub struct SessionState {
+   109	    pub accounts: Vec<Account>,
+   110	    pub active: Option<String>,             // pubkey
+   111	    pub status: SessionStatus,              // Loading / Syncing / Online / Offline
+   112	    pub last_activity_ms: u64,
+   113	}
+   114	
+   115	pub struct Account {
+   116	    pub pubkey: String,
+   117	    pub display: AccountDisplay,            // pre-formatted name + npub
+   118	    pub signer_kind: SignerKind,
+   119	    pub profile_view_id: ViewId,            // points into ViewSnapshots
+   120	    pub contacts_view_id: ViewId,
+
+ succeeded in 0ms:
+     1	# Build & Validation Plan
+     2	
+     3	> Companion to `docs/product-spec.md` (what we ship) and the design docs in `docs/design/` (how each subsystem works). This document defines **the single ladder of milestones**, each one a runnable product that proves a specific architectural claim with real (not modeled) evidence.
+     4	
+     5	> **Four arcs:** Kernel substrate + Nostr social stack (M0–M10) → FFI hardening + iOS empirical proof (M10.5) → kernel-boundary proof with a non-social-domain app (M11, the **`../podcast` rebuild on NMP**) → wallet/WoT + cross-platform + release (M12–M17).
+     6	
+     7	> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.
+     8	
+     9	> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
+    10	
+    11	---
+    12	
+    13	## 0. Where we are right now
+    14	
+    15	Honest accounting before forecasting forward.
+    16	
+    17	### Implemented and running
+    18	
+    19	- **Kernel substrate** in `crates/nmp-core` (~3,800 LOC): actor on a dedicated OS thread, mailbox-driven (ADR feedback adopted — relay reads happen in tokio reader tasks, the actor blocks on its own channel with deadline timeouts), substrate trait families (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`) in `nmp-core/src/substrate/`, ingest pipeline (`kernel/ingest.rs`), claim/release refcounting for profile interest (commit `23ae829`), composite reverse-index dependency tracking.
+    20	- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
+    21	- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
+    22	- **Codegen tool** in `crates/nmp-codegen` (~423 LOC): reads `nmp.toml`, produces a per-app crate, has determinism tests.
+    23	- **Benches** in `crates/nmp-testing`: `reactivity-bench` (composite-key reverse index + coalescer + working set; run 002 passed all ADR-0001..0004 gates) and `firehose-bench` (replay + capture + live modes; replay scenarios pass the modeled budget contract).
+    24	- **Perf reports** in `docs/perf/` documenting reactivity-bench run 002, firehose-bench replay runs, and three iOS measurement reports (relay lifecycle, profile/thread subscriptions, the primal slice baseline).
+    25	- **Architecture decisions** locked in 10 ADRs (`docs/decisions/0001`–`0010`).
+    26	
+    27	### Designed but not implemented
+    28	
+    29	- LMDB / IndexedDB persistent storage (in-memory only today).
+    30	- NIP-65 outbox routing (hardcoded content + indexer relays today).
+    31	- NIP-77 negentropy sync.
+    32	- NIP-42 relay auth.
+    33	- Multi-account / multi-session model and account switching.
+    34	- Signer trait + local-key signer + NIP-46 bunker signer.
+    35	- Action ledger + write path (compose / react / repost / quote).
+    36	- NIP-17 messaging and the NSE companion crate.
+    37	- Blossom uploads / downloads with resumable progress.
+    38	- Wallet stack (NWC, NIP-57 zaps, Cashu, nutzaps).
+    39	- Web-of-Trust subsystem.
+    40	- UniFFI bindings (current iOS bridge is raw C FFI).
+    41	- Android shell, Desktop shell, Web shell.
+    42	- The `nmp` CLI scaffolding tool.
+    43	- A non-Nostr-shaped product (podcast app) demonstrating the kernel boundary in production.
+    44	
+    45	### Gaps in the prior plan that this rewrite addresses
+    46	
+    47	- The prior plan was phase-numbered (Phase 1, 2, …) without explicit *demoable products* per phase.
+    48	- NIP-42 wasn't covered.
+    49	- Subscription compilation (the load-bearing NDK/Applesauce lesson) wasn't elevated as its own milestone.
+    50	- Blossom and media-capability lifecycle (long-running, resumable, background) were one bullet under Phase 6.
+    51	- No milestone proved the kernel boundary for a fundamentally non-social product.
+    52	- The plan didn't reflect that M0 and M1 are largely done.
+    53	- **No dedicated FFI hardening + iOS empirical proof gate before the kernel-boundary proof.** The prior M11 implicitly assumed the FFI surface was ready; this rewrite makes it a separate milestone (M10.5).
+    54	- **M11 was generic.** This rewrite ties it concretely to `/Users/pablofernandez/src/podcast` (the fully-functional Swift app) as the rebuild target, with copy-first UI fidelity and an explicit view-by-view module mapping.
+    55	
+    56	The plan below is a single ladder of eighteen milestones (M0–M17, with M10.5 inserted as the FFI gate), each producing a runnable artifact, ordered so that each milestone strictly adds capabilities to the prior demoable product.
+    57	
+    58	---
+    59	
+    60	## 1. Principles of execution
+    61	
+    62	1. **Each milestone is a runnable product.** Not a feature branch; a thing you can build, launch on real hardware, and demo. Unit tests verify correctness; the milestone product validates the architecture.
+    63	2. **Real measured evidence over modeled budgets.** Modeled passes in `firehose-bench` replay establish the budget contract. Real passes in `firehose-bench live` against the iOS / Android / Desktop / Web app are the actual gate.
+    64	3. **Capability layering is strict.** Each milestone adds exactly one new architectural ingredient on top of the previous demo. No "we'll wire it up later" — wiring is the milestone.
+    65	4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
+    66	5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
+    67	6. **No phase ends silently.** Each milestone exit produces: regression tests added to `nmp-testing`, a perf report in `docs/perf/m<N>/`, an ADR if a design decision was revised, and a runnable artifact tagged in git.
+    68	
+    69	---
+    70	
+    71	## 2. The milestone ladder
+    72	
+    73	Each milestone has: **demo product**, **scope (what gets built)**, **subsystem deliverables**, **exit gate (measurable)**, and **runnable artifact**. Estimates are for one experienced developer focused on the work; they are not commitments.
+    74	
+    75	### M0 — Kernel substrate + non-Nostr fixture *(DONE)*
+    76	
+    77	**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
+    78	
+    79	**Scope.** Five extension trait families. Composite reverse index. Delta buffer with coalescing. Claim-based GC. Codegen producing a working per-app crate from a fixture module.
+    80	
+    81	**Subsystem deliverables.** `nmp-core::substrate`, `nmp-codegen`, `fixture-todo-core`, `nmp-testing` harness skeletons.
+    82	
+    83	**Exit gate.** ✅ Fixture compiles and runs; codegen determinism test passes; substrate registry test passes (`crates/nmp-core/tests/substrate_registry.rs`).
+    84	
+    85	**Runnable artifact.** `cargo test --workspace`; the fixture module loads in any host.
+    86	
+    87	---
+    88	
+    89	### M1 — Read-only Twitter slice on iOS *(LARGELY DONE; final hardening in flight)*
+    90	
+    91	**Demo product:** `ios/NmpStress` — SwiftUI app pulling live from primal, rendering seed-driven timeline, profile cards, threads, diagnostics screen.
+    92	
+    93	**Scope.** Per ADR-0006 + ADR-0008 + ADR-0009: kind:0 Profile path end-to-end against a real relay, on iOS, through real FFI. Seed-driven discovery (union of follow lists from pablof7z + fiatjaf + jb55). Refcounted claim/release pattern per ADR-0005 (profile interest commit `23ae829`). Diagnostics surface per ADR-0007.
+    94	
+    95	**Subsystem deliverables.**
+    96	
+    97	- ✅ Kernel actor with mailbox-driven relay ingestion (commit `9e9ce04`).
+    98	- ✅ Real WebSocket connections via `tungstenite` + `rustls`.
+    99	- ✅ Profile / Timeline / Thread view kinds wired through the kernel.
+   100	- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
+   101	- ✅ iOS bridge (`KernelBridge.swift`, `KernelModel.swift`, content views).
+   102	- ✅ Diagnostics screen showing relay state, logical interests, wire subs (ADR-0007).
+   103	- 🟡 Firehose-bench `live` scenarios `cold_start` + `profile_thrashing` running against the iOS app's kernel with **measured numbers** documented as the M1 baseline. (Initial reports exist in `docs/perf/ios-demo/` but should be promoted to `docs/perf/m1/` and gated.)
+   104	
+   105	**Exit gate.**
+   106	
+   107	- Avatar / name / picture / NIP-05 fields update in place when kind:0 arrives mid-scroll without any spinner gate.
+   108	- Mount/unmount of 100 avatar components rapidly produces correct refcount lifecycle (no leaks, claim drops on grace period).
+   109	- Primal connection survives a 30-second disconnect via reconnect with no observable data loss in a retried scroll.
+   110	- Firehose-bench `live cold_start` against primal: time to first profile rendered ≤ 800 ms p99, time to filled timeline (200 items) ≤ 5 s p99 on developer hardware.
+   111	- Firehose-bench `live profile_thrashing` (50/sec mount/unmount over 10 min) against primal: zero subscription leaks; `OpenView`/`CloseView` dispatch rate ≤ 60% of mount rate (grace-period absorption working).
+   112	- All reactivity-bench `--standard` gates continue to pass against the real kernel code path, not just the synthetic model.
+   113	
+   114	**Runnable artifact.** `just run-ios` launches the app on iPhone simulator pulled from real primal. `docs/perf/m1/baseline.md` published with measured numbers.
+   115	
+   116	---
+   117	
+   118	### M2 — Subscription compilation + outbox routing
+   119	
+   120	**Demo product:** Same iOS app as M1, but timeline subscriptions are routed per-author to those authors' write relays per NIP-65, not to the hardcoded primal/purplepag.es pair. Diagnostics screen visibly shows the per-relay fan-out and which authors each relay covers.
+   121	
+   122	**Scope.** The planner becomes a **subscription compilation stage** per the NDK/Applesauce lessons doc. Logical interests get compiled into per-relay plans; recompilation happens when relay metadata arrives late. NIP-65 routing is the default for both reads and writes. Provenance / NIP-65 / relay hints / user-configured relays are four distinct facts that inform each other but never collapse.
+   123	
+   124	**Subsystem deliverables.**
+   125	
+   126	- `nmp-nip65` protocol module: Mailboxes view module (parsed kind:10002); outbox routing as a planner subsystem; recompilation triggers (kind:10002 arrival, view open, relay reconnect).
+   127	- Planner refactored from "hardcoded relay set" to "compiler of logical interests → per-relay plans." See `docs/design/ndk-applesauce-lessons.md` §7 (subscription compilation lessons).
+   128	- Per-pubkey relay-list cache (durable, even before LMDB lands — keep it in-memory until M3, but the data model is correct).
+   129	- Indexer fallback when a pubkey's kind:10002 is unknown: opportunistic discovery from a configurable indexer relay set.
+   130	- Reverse-relay-coverage view for diagnostics: "this relay is serving N authors of our timeline."
+   131	
+   132	**Exit gate.**
+   133	
+   134	- Bug-extinction test #3 (publish to wrong relays): no public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning.
+   135	- Subscription compilation correctness: for a timeline of 1000 authors, the compiled plan opens REQs only against the union of those authors' write relays (de-duplicated). Test asserts on the wire frame count.
+   136	- Late-arriving kind:10002 triggers recompilation: an author whose mailbox was unknown gets re-routed once their kind:10002 lands, without the platform observing protocol churn.
+   137	- Distinct-source visibility: the diagnostics screen shows the four relay-fact lanes (NIP-65 / hint / provenance / user-configured) separately.
+   138	
+   139	**Runnable artifact.** iOS app with measurably different relay-fan-out behavior; demo screenshot in `docs/perf/m2/outbox-routing.md` showing per-relay coverage.
+   140	
+   141	---
+   142	
+   143	### M3 — Persistence (LMDB) + full insert invariants
+   144	
+   145	**Demo product:** iOS app cold-starts in ≤ 1.5 s with the previous session's events already on screen.
+   146	
+   147	**Scope.** Swap in-memory `EventStore` for LMDB via `Box<dyn EventStore>`. Implement the full insert invariants from `product-spec.md` §7.1: parameterized replaceable events (kind 30000–39999 by `(pubkey, kind, d-tag)`), kind:5 delete handling with tombstone persistence, NIP-40 expiration scheduling, dedup with provenance merge, claim-based GC running.
+   148	
+   149	**Subsystem deliverables.**
+   150	
+   151	- LMDB schema design doc (`docs/design/lmdb-schema.md`) — key encoding, secondary indexes, tombstones, watermarks table (populated in M4), backup/export format.
+   152	- `EventStore` trait abstracted; LMDB backend; in-memory backend kept for tests.
+   153	- Migration plumbing (ties into `DomainModule::migrations()`).
+   154	- GC working set policy per ADR-0003: hot ≤ 10k events resident + claim-pinned set; cold on disk.
+   155	
+   156	**Exit gate.**
+   157	
+   158	- Cold-start with primed LMDB: time-to-first-painted-timeline ≤ 1.5 s on iPhone 12.
+   159	- Working-set memory under sustained scroll: ≤ 100 MB at 100 active views / 10k hot events / 1 M cached on disk.
+   160	- Replaceable correctness across restart: a kind:0 written, app killed, app reopened — the latest version is served, not stale.
+   161	- Kind:5 self-delete persists; foreign kind:5 ignored.
+   162	
+   163	**Runnable artifact.** iOS app surviving termination + relaunch with state preserved. Report in `docs/perf/m3/persistence.md`.
+   164	
+   165	---
+   166	
+   167	### M4 — NIP-77 negentropy sync engine
+   168	
+   169	**Demo product:** Profile screen for a new author cold-syncs via NIP-77 against primal, visibly faster and with measured bytes savings vs REQ scan.
+   170	
+   171	**Scope.** Per `product-spec.md` §7.8 and ADR (sync as engine, not feature):
+   172	
+   173	**Subsystem deliverables.**
+   174	
+   175	- `nmp-nip77` protocol module: negentropy reconciliation client (use `nostr-sdk`'s implementation or `negentropy` crate directly).
+
+exec
+/bin/zsh -lc "nl -ba docs/design/lmdb-schema.md | sed -n '1,245p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# Design: LMDB schema + EventStore trait + GC policy (M3)
+     2	
+     3	> **Audience:** kernel implementers landing M3 (persistence).
+     4	> **Status:** rev 0 — proposed; opens ADR slot for any open-question resolution.
+     5	> **Companion docs:** [`lmdb/trait.md`](lmdb/trait.md), [`lmdb/keys.md`](lmdb/keys.md), [`lmdb/gc.md`](lmdb/gc.md), [`lmdb/watermarks.md`](lmdb/watermarks.md), [`lmdb/tests.md`](lmdb/tests.md).
+     6	> **Prerequisites:** `docs/product-spec/subsystems.md` §7.1 (insert invariants), `docs/decisions/0003-working-set-memory.md` (GC policy intent), `docs/decisions/0009-app-extension-kernel-boundary.md` (DomainModule storage), `docs/design/kernel-substrate.md` §2 (DomainModule trait).
+     7	> **Plan reference:** [`docs/plan.md`](../plan.md) §M3.
+     8	
+     9	---
+    10	
+    11	## 1. Decision: wrap `nostr-lmdb` behind our own `EventStore` trait
+    12	
+    13	**Adopt `nostr-lmdb` as the on-disk byte store. Wrap it behind the NMP `EventStore` trait. Add NMP-owned LMDB sub-databases for the rows `nostr-lmdb` does not model.**
+    14	
+    15	The competing options were (1) use `nostr-lmdb` directly via its concrete `NostrLMDB` type (or via `nostr_database::NostrEventsDatabase`), (2) wrap behind our own trait, or (3) hand-roll an LMDB layer.
+    16	
+    17	**`nostr-lmdb` gives us** (per `docs.rs/nostr-lmdb`): `save_event(&Event)`, `event_by_id(&EventId)`, `check_id(&EventId) -> DatabaseEventStatus`, `query(Filter) -> Events`, `count(Filter)`, `delete(Filter)`, `wipe()`, `negentropy_items(Filter) -> Vec<(EventId, Timestamp)>`. It owns the page allocator, the LMDB environment, primary by-id store, indexes derived from `Filter`, replaceable / parameterized-replaceable supersession, and NIP-09 delete handling. It is the only mature Rust LMDB store for Nostr events with proven NIP-77 integration; reinventing it is a year of work that we will not recoup.
+    18	
+    19	**What `nostr-lmdb` does *not* model** (the gap that justifies a wrapper):
+    20	
+    21	| Concern | Why `nostr-lmdb` doesn't cover it | Where NMP needs it |
+    22	|---|---|---|
+    23	| Per-relay provenance (which relays delivered each event; first seen / last seen) | Out of scope; the crate models events, not their wire history | `subsystems.md` §7.1 "Duplicate id → merge relay provenance set"; ADR-0007 diagnostics; outbox routing scoring in M2+ |
+    24	| Sync watermarks `(filter_hash, relay) → synced_up_to` | Out of scope; the crate does not know about logical filters or relay identity | `subsystems.md` §7.1 + §7.8; M4 NIP-77 engine needs them to be authoritative |
+    25	| Claim register / release for view-driven GC | Out of scope; the crate has no concept of an "open view" | ADR-0003; current in-memory analogue in `kernel/mod.rs:315` `profile_claims: HashMap<String, BTreeSet<String>>` |
+    26	| Working-set hot/cold split with eviction LRU | LMDB is OS-paged; the crate trusts the kernel page cache | ADR-0003 numeric gate (≤ 100 MB at 100 views / 10k hot) |
+    27	| Kernel-side secondary indexes for query shapes our planner uses (e.g. `(p-tag, timestamp)` desc scan, `(e-tag, timestamp)` desc scan, `(expires_at, event_id)` for NIP-40 wakeups) | The `Filter` API recomputes per call; not optimal for our planner's repeat shapes | Planner cache-coverage queries (§7.2); NIP-40 expiration scheduling (§7.1) |
+    28	| Tombstone-as-row that survives independent of the deleted event | The crate suppresses re-insert via its own delete index; we want it exposed for export / restoring across re-syncs | `subsystems.md` §7.1 kind:5 row "persisted as tombstone so later re-insertion is suppressed" |
+    29	| `DomainModule` rows (drafts, settings, action ledger, projection caches) | Entirely out of scope; the crate stores Nostr `Event` only | ADR-0009 + `kernel-substrate.md` §2 — kernel hosts non-Nostr typed rows |
+    30	| Migrations versioned per namespace | Out of scope | `kernel-substrate.md` §2: `DomainModule::migrations() -> Vec<DomainMigration>` |
+    31	| `nmp dump` deterministic export | Out of scope | M3 exit criteria; M11 cross-app proof |
+    32	
+    33	**Therefore.** `EventStore` is a NMP-owned trait, with one production impl `LmdbEventStore` that holds (a) a `NostrLMDB` for the canonical event store and Nostr-shaped queries, and (b) NMP-owned secondary LMDB sub-databases under the same `lmdb::Environment` for the gap rows. The in-memory backend (`MemEventStore`) remains, both for tests and as the web-pre-M15 fallback. See [`lmdb/trait.md`](lmdb/trait.md) for the exact trait shape and the relayed-vs-owned method split.
+    34	
+    35	**Rejected alternatives.**
+    36	
+    37	- *Use `NostrLMDB` directly, no wrapper.* Loses every gap row above. Forces the kernel actor to know about LMDB transactions and a non-NMP concrete type, breaking the `Box<dyn EventStore>` substitutability M3 requires.
+    38	- *Roll our own.* Reinvents NIP-09 / replaceable handling that `nostr-lmdb` already gets right. ~2,000 LOC of avoidable code with a worse bug surface than upstream.
+    39	- *SQLite-backed `nostr-sdk` store.* Larger memory footprint at our 10k-event hot working set; iOS-disk-WAL fsync cost not justified for this access pattern. Held in reserve for the web port (M15) if IndexedDB OPFS proves unworkable.
+    40	
+    41	## 2. Subsystem ownership map
+    42	
+    43	```
+    44	crates/nmp-core/src/store/
+    45	  mod.rs                — trait re-exports + factory
+    46	  trait.rs              — `EventStore` (see lmdb/trait.md)
+    47	  mem.rs                — in-memory backend (preserved from kernel/mod.rs current state)
+    48	  lmdb/
+    49	    mod.rs              — `LmdbEventStore` orchestrator
+    50	    env.rs              — `lmdb::Environment` + sub-db handles + open()
+    51	    events.rs           — wraps `nostr_lmdb::NostrLMDB`; primary-by-id, replaceable rules, kind:5 handling
+    52	    secondary.rs        — NMP-owned secondary indexes (see lmdb/keys.md §3)
+    53	    provenance.rs       — provenance sub-db (see lmdb/watermarks.md §2)
+    54	    watermarks.rs       — watermark sub-db (see lmdb/watermarks.md §1)
+    55	    claims.rs           — claim register / release + hot-set hints (see lmdb/gc.md §2)
+    56	    gc.rs               — `gc_step()` algorithm (see lmdb/gc.md §3)
+    57	    domain.rs           — per-DomainModule sub-db namespacing + migration runner
+    58	    dump.rs             — `nmp dump` (see §9 below)
+    59	```
+    60	
+    61	Each file is bounded ≤ 300 LOC per AGENTS.md. The trait module is read by the actor; backend modules are read only by the orchestrator.
+    62	
+    63	## 3. EventStore trait
+    64	
+    65	See [`lmdb/trait.md`](lmdb/trait.md) for the exact `pub trait EventStore` signature with all required methods, return types, and the `StoreError` enum. Summary:
+    66	
+    67	- **Reads:** `get_by_id`, `scan_by_author_kind`, `scan_by_kind_dtag`, `scan_by_etag`, `scan_by_ptag`, `scan_by_kind_time`, `scan_expiring_before`. All `scan_*` methods return a streaming `EventIter` so the planner pages without materialising. Cache-coverage queries take a `WatermarkKey` and answer authoritatively.
+    68	- **Writes:** `insert(event, RelayUrl)` returns `InsertOutcome` matching §7.1's table. `delete_by_filter` for foreign-relay cleanups. `tombstones_for` for replay.
+    69	- **Watermarks / sync:** `read_watermark`, `write_watermark`, `list_watermarks_for_relay`.
+    70	- **GC:** `claim(ClaimerId, &[EventId])`, `release(ClaimerId)`, `hot_set_hint(&[EventId])`, `gc_step(GcBudget) -> GcReport`.
+    71	- **Domain rows:** `domain_open(namespace) -> DomainHandle` returns a typed handle; `DomainHandle::put/get/scan_index` is the per-DomainModule API.
+    72	- **Migration:** `run_migrations(&[DomainMigration])` runs at startup, transactional per migration.
+    73	- **Export:** `dump(out: &mut dyn Write, format: DumpFormat) -> Result<DumpStats>`.
+    74	
+    75	**Error semantics.** All methods return `Result<T, StoreError>`. Per doctrine D3, store errors do **not** cross FFI — the actor maps every variant to either (a) a typed `Effect` (e.g. `StoreCorrupt`, surfaces via diagnostics + toast), (b) a `tracing::warn!` log + degraded state, or (c) a panic at startup if the LMDB environment refuses to open. The trait itself uses `Result` since it is internal to the actor process.
+    76	
+    77	## 4. Key encoding
+    78	
+    79	Full byte layout for primary + every secondary in [`lmdb/keys.md`](lmdb/keys.md). At a glance:
+    80	
+    81	- Primary `events`: `event_id[32]` → `Event` (CBOR via `nostr` crate's serialization). Owned by `nostr-lmdb`.
+    82	- Secondary `idx_author_kind`: `pubkey[32] || kind_be[4] || created_at_be[8] || event_id[32]` → empty. NMP-owned.
+    83	- Secondary `idx_kind_dtag`: `kind_be[4] || dtag_len_be[2] || dtag_bytes || pubkey[32]` → `event_id[32]`. NMP-owned. Parameterized replaceable address lookup.
+    84	- Secondary `idx_etag_time`, `idx_ptag_time`: `tag_value[32] || created_at_desc_be[8] || event_id[32]` → empty. NMP-owned. `created_at_desc = u64::MAX - created_at` so a forward LMDB scan is newest-first.
+    85	- Secondary `idx_kind_time`: `kind_be[4] || created_at_desc_be[8] || event_id[32]` → empty.
+    86	- Secondary `idx_expires`: `expires_at_be[8] || event_id[32]` → empty. Scanned by the NIP-40 reaper.
+    87	- `tombstones`: `target_id[32]` → `TombstoneRow { kind5_event_id, deleter_pubkey, deleted_at, sources: Vec<RelayUrl> }` (CBOR).
+    88	
+    89	`created_at_be` is big-endian so byte order matches numeric order; `created_at_desc_be = u64::MAX - created_at` then big-endian for newest-first scans without `MDB_LAST + MDB_PREV`.
+    90	
+    91	All secondaries are maintained inside the same `RwTxn` as the primary write — atomicity is achieved by LMDB transactionality, not by post-hoc reconciliation.
+    92	
+    93	## 5. Watermark table
+    94	
+    95	See [`lmdb/watermarks.md`](lmdb/watermarks.md) for full layout. Row shape (CBOR):
+    96	
+    97	```rust
+    98	struct WatermarkRow {
+    99	  filter_hash: [u8; 32],     // BLAKE3 of canonicalised filter (see watermarks.md §3)
+   100	  relay_url: String,
+   101	  synced_up_to: u64,         // unix seconds
+   102	  last_sync_method: SyncMethod, // Negentropy | ReqScan | Manual
+   103	  last_negentropy_state: Option<Vec<u8>>, // engine-opaque resume blob
+   104	  bytes_saved_vs_req: u64,
+   105	  updated_at: u64,
+   106	}
+   107	```
+   108	
+   109	Key: `filter_hash[32] || relay_url_bytes` (no length prefix needed — relay URL is the variable suffix; lookup uses exact key). Populated by M4 (NIP-77) and consulted by M2's planner (cache-coverage check before issuing backfill REQ). Survives restarts; loaded into the actor on startup as a `HashMap<(filter_hash, relay_url), WatermarkRow>` for hot lookups, with all writes going through `EventStore` for durability.
+   110	
+   111	## 6. Migration plumbing
+   112	
+   113	See [`lmdb/watermarks.md`](lmdb/watermarks.md) §4. A `DomainModule` (per `kernel-substrate.md` §2) declares `const NAMESPACE: &'static str` and `const SCHEMA_VERSION: u32` plus `fn migrations() -> Vec<DomainMigration>`. The store assigns one LMDB sub-database per `(namespace, "data")`, plus one per `(namespace, index_name)` for each declared index. A `_meta` sub-database tracks `(namespace, current_version)`.
+   114	
+   115	The current `ModuleRegistry` (`crates/nmp-core/src/substrate/mod.rs:41`) discards the concrete `M: DomainModule` type after `register_domain::<M>()` returns — only the `ModuleDescriptor` is retained. The store cannot get from a namespace string back to `M::SCHEMA_VERSION` or `M::migrations()` at runtime. M3 adds a `DomainFactories { schema_version: fn() -> u32, migrations: fn() -> Vec<DomainMigration>, indexes: fn() -> Vec<DomainIndex> }` struct attached per descriptor, populated by capturing the `M::*` consts and fns in `fn`-pointer closures at register time. This matches the existing `key_fn: fn(&[u8]) -> Option<Vec<u8>>` pattern in `DomainIndex` (`crates/nmp-core/src/substrate/domain.rs:18`) — no `Box<dyn DomainModule>` and no new trait object-safety constraints on `DomainModule`. The change is additive to the substrate module surface. See [`lmdb/watermarks.md`](lmdb/watermarks.md) §4.1 for the registry-side code shape.
+   116	
+   117	On startup:
+   118	
+   119	1. For every registered `DomainModule`, read its row from `_meta`.
+   120	2. If absent, treat current as 0 and run all migrations from 0 to `SCHEMA_VERSION` in one `RwTxn` per step.
+   121	3. If present and less than `SCHEMA_VERSION`, run the missing steps.
+   122	4. If greater, refuse to start (downgrade not supported); surface as `Effect::DomainSchemaTooNew { namespace }`.
+   123	
+   124	Each `DomainMigration::apply` receives a `MigrationTx` with put/get/delete + index rebuild helpers. Rollback semantics: each migration step is its own LMDB write transaction; failure aborts the step cleanly. If migration N succeeds and N+1 fails, the store stays at version N — the actor refuses to start the affected module and the rest of the kernel runs in degraded mode (the module's actions return `ActionRejection::ModuleUnavailable`).
+   125	
+   126	## 7. GC working-set policy
+   127	
+   128	See [`lmdb/gc.md`](lmdb/gc.md) for the eviction algorithm. Formal statement (matches ADR-0003):
+   129	
+   130	```
+   131	hot_resident = {e | e is in claim_pinned}
+   132	             ∪ {e | e is in open_view_cover}
+   133	             ∪ {e | e is among the ≤10k most-recently-touched events}
+   134	
+   135	cold = stored_events \ hot_resident
+   136	```
+   137	
+   138	`hot_resident` lives in a `lru::LruCache<EventId, Arc<Event>>` capped at the configured hot ceiling (default 10,000) plus an unbounded pinned overlay holding events with non-zero claim count. `cold` lives only on disk; lookup pays one LMDB `get` (memory-mapped — typically already in OS page cache for recently-evicted items).
+   139	
+   140	**Eviction algorithm.** On any insert that pushes the LRU over its ceiling, the oldest non-pinned entry is dropped. `gc_step()` is called periodically by the actor (default every 60 s and on memory pressure callbacks from `MemoryWarningCapability`): it (a) reaps NIP-40 expired events using `idx_expires`, (b) trims the LRU to `target_hot_size`, (c) deletes tombstones older than `tombstone_retention` (default 90 days) whose target event is absent from the store, (d) returns a `GcReport` for diagnostics.
+   141	
+   142	ADR-0003's numbers are preserved as the M3 exit gate (§11 below): ≤ 100 MB working-set at 100 active views / 10k hot events / 1M cached on disk.
+   143	
+   144	## 8. Replaceable + tombstone semantics
+   145	
+   146	The `insert()` path implements exactly the §7.1 invariants:
+   147	
+   148	- **Replaceable (kinds 0, 3, 10000–19999).** Look up the existing event for `(pubkey, kind)` in `idx_author_kind` (most recent suffix). If incoming `created_at` is newer, replace; if equal, keep lexicographically smallest `id`; else drop. Replacement deletes the old primary row and all secondary entries in the same `RwTxn`.
+   149	- **Parameterized replaceable (30000–39999).** Same algorithm keyed on `(pubkey, kind, d-tag)` via `idx_kind_dtag` (which holds `event_id` as value so we don't need a separate `idx_author_kind_dtag`; the dtag prefix is unique per author by Nostr semantics — see [`lmdb/keys.md`](lmdb/keys.md) §3.2 for the per-author scoping note).
+   150	- **Kind:5 self-delete.** Verify signature, scan referenced `e` and `a` tags, for each target `e_id` that is authored by the deleter or whose `a` address matches `(deleter_pubkey, kind, d-tag)`: delete the primary + all secondaries + write the tombstone row. Tombstone timestamp = `max(existing.deleted_at, kind5.created_at)`. Re-insert of the deleted event id is suppressed at insert time by a `tombstones.contains(event_id)` check.
+   151	- **Foreign kind:5.** A kind:5 referencing events not authored by the kind:5's `pubkey` is ignored (per spec) — the event is *still stored* as a kind:5 (so other clients can render it / dedup it), but it has no side effect on the targets. The tombstone row is **not** written.
+   152	- **NIP-40 expiration.** On insert, parse `expiration` tag; if present, write `idx_expires`. On `gc_step()`, scan `idx_expires` for keys with `expires_at_be ≤ now`, delete them like kind:5 (full primary + secondaries + tombstone marker noting `kind: Expired`).
+   153	
+   154	The tombstone schema is in [`lmdb/keys.md`](lmdb/keys.md) §4.
+   155	
+   156	## 9. Provenance: per-row sidecar sub-database
+   157	
+   158	**Decision: separate `provenance` sub-database keyed by `event_id[32]`.** Value is CBOR `ProvenanceRow { sources: Vec<ProvenanceEntry> }` where `ProvenanceEntry = { relay_url, first_seen_ms, last_seen_ms, primary: bool }`.
+   159	
+   160	Rejected: stuffing provenance into the `Event` row. That requires re-serializing the full `Event` on every relay redelivery (high write amplification — popular events arrive 5–20× from the relay fan-out) and forks the `nostr-lmdb` row format, which we explicitly want to keep upstream-compatible. The sidecar is appended cheaply with a single CBOR re-encode of the (typically small) `sources` vector.
+   161	
+   162	On duplicate-id insert (§7.1 row 2), `insert()` does not touch the primary; it only updates the provenance sidecar (`last_seen_ms` bump on the matching `ProvenanceEntry`, or append). The "primary relay" — for outbox-routing scoring (M2) and ADR-0007 diagnostics — is deterministically the first relay observed (`sources[0]` after sort by `first_seen_ms`).
+   163	
+   164	The export format (§ next) includes the provenance row alongside each event so a `nmp dump` round-trip restores it.
+   165	
+   166	## 10. Backup / export format
+   167	
+   168	`nmp dump` writes line-delimited JSON to stdout (or a file). Each line is a single tagged record:
+   169	
+   170	```json
+   171	{"type":"event","event": {...nostr event...},"provenance":[{"relay_url":"wss://relay.primal.net","first_seen_ms":1747000000000,"last_seen_ms":1747001234567,"primary":true}]}
+   172	{"type":"tombstone","target_id":"abc...","kind5_event_id":"def...","deleter_pubkey":"...","deleted_at":1747000000,"sources":["wss://..."]}
+   173	{"type":"watermark","filter_hash":"hex32","relay_url":"wss://...","synced_up_to":1747000000,"last_sync_method":"Negentropy","bytes_saved_vs_req":12345,"updated_at":1747000123}
+   174	{"type":"domain","namespace":"twitter.drafts","schema_version":1,"key_hex":"...","value_b64":"..."}
+   175	```
+   176	
+   177	JSONL is the chosen format because (a) it streams (no holding the full dump in memory; cold-events page in as scanned), (b) it diffs cleanly (one record per line), (c) any line is independently parsable for partial recovery, (d) `jq` works out of the box. Binary CBOR is faster but loses ad-hoc inspectability — JSONL is the right tradeoff for an export format.
+   178	
+   179	`nmp restore` is symmetric: read JSONL, replay through `insert()` for events (so all secondaries are rebuilt from scratch — provenance is restored separately by writing the sidecar row directly after each event), `write_watermark` for watermarks, `DomainHandle::put` for domain rows. Restore is idempotent: replaying the same dump twice produces the same store.
+   180	
+   181	## 11. Test plan
+   182	
+   183	See [`lmdb/tests.md`](lmdb/tests.md) for the full mapping of every spec §7.1 invariant to a concrete test file under `crates/nmp-testing/tests/`. Highlights:
+   184	
+   185	| Invariant (§7.1) | Test file |
+   186	|---|---|
+   187	| Insert API single path | `crates/nmp-testing/tests/store_insert_path.rs` |
+   188	| Signature verification before persist | `crates/nmp-testing/tests/store_invalid_sig.rs` |
+   189	| Duplicate id → merge provenance, keep earliest received_at | `crates/nmp-testing/tests/store_provenance_merge.rs` |
+   190	| Replaceable supersession | `crates/nmp-testing/tests/store_replaceable.rs` |
+   191	| Parameterized replaceable supersession | `crates/nmp-testing/tests/store_param_replaceable.rs` |
+   192	| Kind:5 self-delete persists as tombstone | `crates/nmp-testing/tests/store_kind5_tombstone.rs` |
+   193	| Foreign kind:5 ignored | `crates/nmp-testing/tests/store_kind5_foreign.rs` |
+   194	| NIP-40 expiration scheduled + reaped | `crates/nmp-testing/tests/store_nip40_expiration.rs` |
+   195	| Watermarks survive restart, authoritative cache-miss | `crates/nmp-testing/tests/store_watermarks.rs` |
+   196	| Claim register / release; GC drops un-claimed cold | `crates/nmp-testing/tests/store_gc_claims.rs` |
+   197	| `nmp dump` round-trip is byte-identical for second dump | `crates/nmp-testing/tests/store_dump_roundtrip.rs` |
+   198	| Migration v0→v1 success; rollback on N+1 failure | `crates/nmp-testing/tests/store_domain_migration.rs` |
+   199	| Domain isolation: module A cannot read module B's sub-db | `crates/nmp-testing/tests/store_domain_isolation.rs` |
+   200	| Working-set ≤ 100 MB at 100 views / 10k hot / 1M cached | `crates/nmp-testing/bin/reactivity-bench` (extended scenario) |
+   201	
+   202	## 12. Performance budget
+   203	
+   204	| Gate | Budget | Measurement |
+   205	|---|---|---|
+   206	| Cold-start time-to-first-painted-timeline on iPhone 12 (primed LMDB, last session's events on disk) | ≤ 1.5 s p99 | `firehose-bench live cold_start --device iphone12` |
+   207	| Cold-start time-to-first-painted-timeline on simulator | ≤ 800 ms p99 (looser than device because no thermal envelope) | same harness, simulator scenario |
+   208	| Working-set memory at 100 active views / 10k hot / 1M on disk | ≤ 100 MB resident | Instruments Allocations + `reactivity-bench` working-set scenario |
+   209	| Single `insert()` for an unseen kind:1 with 4 secondaries | ≤ 250 µs p99 on iPhone 12 | criterion bench in `crates/nmp-testing/benches/store_insert.rs` |
+   210	| `scan_by_author_kind` returning 200 newest events | ≤ 5 ms p99 | criterion bench in `crates/nmp-testing/benches/store_scan.rs` |
+   211	| `gc_step()` work-batch ceiling (single call) | ≤ 50 ms total wall time | bounded by `GcBudget { max_events, max_duration_ms }` |
+   212	| `nmp dump` of 1M events | sustained ≥ 50k events/sec on M-series Mac | wall-clock measurement in dump-roundtrip test |
+   213	
+   214	Each gate is measurable; any miss revises the design via an ADR before M3 is declared complete (per `plan.md` §1.6 "no silent endings").
+   215	
+   216	## 13. Open questions for ADR after review
+   217	
+   218	1. **`nostr-lmdb` LMDB environment sharing.** Can we open the same `lmdb::Environment` for both `NostrLMDB`'s sub-databases and our own NMP sub-databases (provenance, watermarks, claims, domain rows)? If yes, we get atomic cross-sub-db transactions for free (a single `RwTxn` covers event + provenance + secondary indexes). If `nostr-lmdb` insists on opening its own `Environment`, we lose that and the insert path needs a two-phase write with crash-recovery logic. Investigate before implementation — may require an upstream PR exposing `Environment` access.
+   219	2. **Watermark `filter_hash` canonicalisation.** Two `Filter`s that are semantically identical but field-ordered differently must hash the same. The canonicalisation rule (likely: sort all tag-value arrays, sort kinds, sort authors, lexicographic field order before BLAKE3) needs to be specified once and shared with the planner so cache-coverage lookups hit. Candidate: a single `fn canonical_filter_hash(&Filter) -> [u8; 32]` in `nmp-core::store::watermarks`.
+   220	3. **Projection cache durability.** Currently in-memory in the existing kernel (`kernel/mod.rs:293` `profiles: HashMap`). Do we persist projection caches as a `DomainModule` or rebuild from events at cold-start? Rebuild is simpler and avoids cache-staleness bugs but adds startup cost; persistence is faster but requires invalidation logic on kind:0 replacement. Recommended default: rebuild on cold-start, measure, decide whether to add the persistence layer in M3.x or M4.
+   221	4. **Domain-module per-record encoding.** CBOR via `serde_cbor` vs serde-json vs bincode. CBOR is upstream-compatible (matches `nostr` crate); bincode is faster but stratifies the format. Default: CBOR for cross-language readability; revisit if benchmarks show >5% insert-time cost.
+   222	5. **iOS keychain-stored encryption-at-rest key for LMDB.** Out of scope for M3 (mentioned for M6 keychain work) but the schema must not assume cleartext-on-disk forever; reserve a `meta` row for `encryption_version: u32` so a future migration can wrap pages.
+   223	6. **`ModuleRegistry::register_domain` API stability.** Adding `DomainFactories` to `ModuleDescriptor` is a non-breaking additive change to the public substrate API (existing callers using only the generic `register_domain::<M>()` continue to compile), but it commits us to keeping `DomainModule::SCHEMA_VERSION` and `DomainModule::migrations` as compile-time-resolvable items rather than object-safe methods. Confirm this with the substrate maintainer before M3 lands — if `DomainModule` is expected to support runtime composition (e.g., plugin loading), we need option (c): the actor passes the live `&[Box<dyn DomainModule>]` to `EventStore::open` instead. Recommended default: stay with `fn`-pointer factories; revisit if a plugin-loading use case appears.
+   224	
+   225	## 14. Citations to current code
+   226	
+   227	- In-memory event store: `crates/nmp-core/src/kernel/mod.rs:294` (`events: HashMap<String, StoredEvent>`), `kernel/mod.rs:46` (`StoredEvent` struct).
+   228	- Insert path under wrap: `crates/nmp-core/src/kernel/ingest.rs:166` (`ingest_profile`), `ingest.rs:235` (`ingest_timeline_event`), `ingest.rs:209` (`ingest_relay_list`).
+   229	- Replaceable supersession (current scattered logic to be centralised in `EventStore::insert`): `kernel/ingest.rs:166-185` (profile replace by `(pubkey, kind)`), `ingest.rs:218-233` (NIP-65 list replace by `(pubkey, 10002)`).
+   230	- Profile claim refcounting (current in-memory analogue of `EventStore::claim/release`): `kernel/mod.rs:315` (`profile_claims: HashMap<String, BTreeSet<String>>`), `kernel/requests.rs:202` (`claim_profile`), `requests.rs:239` (`release_profile`).
+   231	- Substrate `DomainModule` trait the store backs: `crates/nmp-core/src/substrate/domain.rs:1` (current shape, lines 1–49).
+   232	- Module registry the store consumes at startup: `crates/nmp-core/src/substrate/mod.rs:41` (`ModuleRegistry::register_domain`).
+   233	
+   234	The M3 implementation deletes none of the existing files outright — the kernel's `events: HashMap` and `profiles: HashMap` are replaced by calls to `Box<dyn EventStore>` held inside the `Kernel` struct, and the per-field tests (`kernel/tests.rs`) shift to the new trait via `MemEventStore`. No public FFI surface changes.
+
+exec
+/bin/zsh -lc "nl -ba docs/aim.md | sed -n '210,245p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+   210	│   └── chat-web/
+   211	├── justfile
+   212	└── flake.nix
+   213	```
+   214	
+   215	The core crate compiles as `cdylib + staticlib + rlib`. Desktop and CLI consumers link the rlib directly (no FFI). iOS links the staticlib via xcframework. Android links the cdylib via cargo-ndk. Web compiles to wasm32-unknown-unknown via the wasm crate. **One source of truth, four delivery paths.**
+   216	
+   217	---
+   218	
+   219	## 6. Doctrine — the rules the API must make hard to violate
+   220	
+   221	These rules are the framework's identity. They derive from the RMP bible and from the protocol-correctness lessons of the libraries we are synthesizing:
+   222	
+   223	1. **One event store per application.** Singleton enforced at the FFI boundary.
+   224	2. **All reads through the store.** No "fetch from relay, return to caller" API exists. Relay results land in the store; callers subscribe to the store.
+   225	3. **All writes through actions.** No "build event, sign, publish" sequence the developer assembles manually.
+   226	4. **Replaceable-event invariants enforced on insert.** Stale kind-0/3/10002/parameterized-replaceable events are impossible to retain.
+   227	5. **Outbox routing automatic.** Manual relay selection is the opt-out, not the default.
+   228	6. **Subscriptions auto-group, auto-close, auto-dedup, auto-buffer.** The developer never writes grouping/dedup/cleanup code.
+   229	7. **Sessions are state, switching is an action.** No imperative "log out, then log in, then reload" dance.
+   230	8. **No errors cross FFI.** All operational failure surfaces as state fields.
+   231	9. **No business logic in native code.** Enforced by docs, examples, and an architectural lint where feasible.
+   232	10. **Provenance preserved.** Every event in the store remembers which relays delivered it; private events cannot be accidentally republished to public relays.
+   233	11. **Capabilities, not callbacks.** Native↔Rust interactions go through bounded, idempotent capability bridges modeled exactly on the RMP bible's pattern.
+   234	12. **Snapshots by default, granular updates as optimization.** Start with `AppUpdate::FullState`; add granular `AppUpdate::*` variants only where profiling demands.
+   235	
+   236	---
+   237	
+   238	## 7. Open design questions (must resolve before substantive coding)
+   239	
+   240	1. **State granularity across FFI.** Full-state snapshots are clean but expensive for large stores. Where do we draw the line, and what granular update variants are needed (e.g. `EventAdded`, `ViewChanged { view_id }`, `SessionSwitched`)?
+   241	2. **Where do views live?** (a) Materialized in `AppState`, (b) lazy with `ViewHandle` opaque references the UI subscribes to, (c) computed in platform code. Bible rules out (c). Pick between (a) and (b) — leaning (b) for efficiency, but it complicates the FFI surface.
+   242	3. **Reactive cross-FFI subscription protocol.** UniFFI gives callback interfaces, not native reactive streams. Swift wants `@Observable`, Kotlin wants `Flow`, JS wants Observables/Promises. Define a single `Subscription` opaque handle + reconciler-style callback that adapts cleanly per platform.
+   243	4. **NIP-46 bunker as a capability bridge.** Long-lived, stateful, involves user approval on another device. Needs careful design as an RMP-style capability bridge.
+   244	5. **Background notification decryption.** iOS Notification Service Extensions and Android background workers must call into the Rust core for NIP-17 decryption without spinning up the full actor. Likely a smaller "decrypt-only" surface area in a sibling crate.
+   245	6. **Frozen offline action queue.** Actions dispatched while offline must persist and replay on reconnect, with correct ordering and timestamping. Where does the queue live — in the actor, in SQLite, in a separate durable channel?
+
+codex
+A few likely consistency issues are emerging: key-layout drift between the master doc and `keys.md`, trait methods named in summaries but absent from the actual trait, and a possible D2/working-set leak around the “open view cover” being outside the trait contract. I’m checking those against the exact sub-doc lines now.
+exec
+/bin/zsh -lc "nl -ba docs/design/lmdb/trait.md | sed -n '1,340p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# LMDB sub-design: `EventStore` trait
+     2	
+     3	> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). This file fixes the trait surface; the master doc fixes the decision.
+     4	
+     5	## 1. Crate placement
+     6	
+     7	`crates/nmp-core/src/store/events.rs` (filename note: `trait` is a Rust keyword, so the file is named `events.rs` and exposes `pub trait EventStore`). Re-exported from `nmp_core::store::EventStore`. The actor (`crates/nmp-core/src/actor.rs`) holds the store as `store: Box<dyn EventStore>`; backends are constructed by the factory in `store/mod.rs::open_event_store(&AppConfig) -> Result<Box<dyn EventStore>, StoreError>`.
+     8	
+     9	## 2. Supporting types
+    10	
+    11	```rust
+    12	use std::sync::Arc;
+    13	
+    14	pub type EventId = [u8; 32];
+    15	pub type PubKey = [u8; 32];
+    16	pub type RelayUrl = String;
+    17	
+    18	#[derive(Clone, Debug)]
+    19	pub struct StoredEvent {
+    20	    pub raw: Arc<nostr::Event>,         // upstream nostr crate type
+    21	    pub received_at_ms: u64,            // wall-clock first arrival across all relays
+    22	}
+    23	
+    24	#[derive(Clone, Debug)]
+    25	pub struct ProvenanceEntry {
+    26	    pub relay_url: RelayUrl,
+    27	    pub first_seen_ms: u64,
+    28	    pub last_seen_ms: u64,
+    29	    pub primary: bool,                  // first observed relay (deterministic)
+    30	}
+    31	
+    32	#[derive(Clone, Debug)]
+    33	pub enum InsertOutcome {
+    34	    /// Fresh insert; secondary indexes written.
+    35	    Inserted { id: EventId, sources_after: u32 },
+    36	    /// Duplicate id; provenance updated, primary untouched.
+    37	    Duplicate { id: EventId, sources_after: u32 },
+    38	    /// Replaceable supersession: this event replaced an older one.
+    39	    Replaced { new_id: EventId, replaced_id: EventId },
+    40	    /// Replaceable supersession: incoming was older, dropped.
+    41	    Superseded { id: EventId, current_id: EventId },
+    42	    /// Suppressed because target is tombstoned.
+    43	    Tombstoned { id: EventId, target_kind5_id: EventId },
+    44	    /// Signature / delegation / structural validity failed.
+    45	    Rejected { id: EventId, reason: RejectReason },
+    46	    /// Ephemeral kind: delivered to live consumers, not stored.
+    47	    Ephemeral { id: EventId },
+    48	}
+    49	
+    50	#[derive(Clone, Debug)]
+    51	pub enum RejectReason {
+    52	    BadSignature,
+    53	    BadDelegation(String),
+    54	    Malformed(String),
+    55	    ExpiredOnArrival,                   // NIP-40 expiration already in the past
+    56	}
+    57	
+    58	#[derive(Clone, Debug)]
+    59	pub struct TombstoneRow {
+    60	    pub target_id: EventId,
+    61	    pub kind5_event_id: Option<EventId>, // None for NIP-40 expiry tombstones
+    62	    pub deleter_pubkey: Option<PubKey>,
+    63	    pub deleted_at: u64,                 // unix seconds
+    64	    pub sources: Vec<RelayUrl>,
+    65	    pub origin: TombstoneOrigin,
+    66	}
+    67	
+    68	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
+    69	pub enum TombstoneOrigin { Kind5, NIP40Expiry, AdminPurge }
+    70	
+    71	#[derive(Clone, Debug)]
+    72	pub struct WatermarkKey {
+    73	    pub filter_hash: [u8; 32],
+    74	    pub relay_url: RelayUrl,
+    75	}
+    76	
+    77	#[derive(Clone, Debug)]
+    78	pub struct WatermarkRow {
+    79	    pub key: WatermarkKey,
+    80	    pub synced_up_to: u64,               // unix seconds
+    81	    pub last_sync_method: SyncMethod,
+    82	    pub last_negentropy_state: Option<Vec<u8>>,
+    83	    pub bytes_saved_vs_req: u64,
+    84	    pub updated_at: u64,
+    85	}
+    86	
+    87	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
+    88	pub enum SyncMethod { Negentropy, ReqScan, Manual }
+    89	
+    90	#[derive(Clone, Copy, Debug)]
+    91	pub struct ClaimerId(pub u64);           // opaque view-handle id from the actor
+    92	
+    93	#[derive(Clone, Copy, Debug)]
+    94	pub struct GcBudget {
+    95	    pub max_events_per_step: usize,
+    96	    pub max_duration_ms: u32,
+    97	}
+    98	
+    99	#[derive(Clone, Debug, Default)]
+   100	pub struct GcReport {
+   101	    pub expired_reaped: usize,
+   102	    pub lru_evicted: usize,
+   103	    pub tombstones_purged: usize,
+   104	    pub duration_ms: u32,
+   105	}
+   106	
+   107	#[derive(Clone, Copy, Debug)]
+   108	pub enum DumpFormat { Jsonl, Cbor }
+   109	
+   110	#[derive(Clone, Debug, Default)]
+   111	pub struct DumpStats {
+   112	    pub events: u64,
+   113	    pub tombstones: u64,
+   114	    pub watermarks: u64,
+   115	    pub domain_rows: u64,
+   116	    pub bytes_written: u64,
+   117	}
+   118	
+   119	#[derive(Debug, thiserror::Error)]
+   120	pub enum StoreError {
+   121	    #[error("backend i/o: {0}")] Io(String),
+   122	    #[error("backend corruption: {0}")] Corrupt(String),
+   123	    #[error("encoding: {0}")] Encoding(String),
+   124	    #[error("schema too new: {namespace} on-disk={on_disk} expected={expected}")]
+   125	    SchemaTooNew { namespace: String, on_disk: u32, expected: u32 },
+   126	    #[error("schema migration failed: {namespace} v{from}->v{to}: {reason}")]
+   127	    MigrationFailed { namespace: String, from: u32, to: u32, reason: String },
+   128	    #[error("unknown namespace: {0}")] UnknownNamespace(String),
+   129	}
+   130	```
+   131	
+   132	The store iterates lazily for scans:
+   133	
+   134	```rust
+   135	pub trait EventIter: Iterator<Item = Result<StoredEvent, StoreError>> + Send {}
+   136	impl<T: Iterator<Item = Result<StoredEvent, StoreError>> + Send> EventIter for T {}
+   137	```
+   138	
+   139	`StoredEvent::raw` is `Arc<nostr::Event>` so the hot LRU can hold reference-counted copies without cloning the event body on each `get_by_id`.
+   140	
+   141	## 3. The trait
+   142	
+   143	```rust
+   144	pub trait EventStore: Send + Sync {
+   145	    // ─────── Reads ───────
+   146	
+   147	    /// Primary lookup. Returns Ok(None) if absent; tombstones do not count as "present".
+   148	    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError>;
+   149	
+   150	    /// `idx_author_kind` scan, newest-first. `kinds` empty = any kind.
+   151	    fn scan_by_author_kind<'a>(
+   152	        &'a self,
+   153	        author: &PubKey,
+   154	        kinds: &[u32],
+   155	        since: Option<u64>,
+   156	        until: Option<u64>,
+   157	        limit: usize,
+   158	    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
+   159	
+   160	    /// `idx_kind_dtag` lookup. Returns the current authoritative parameterized
+   161	    /// replaceable for `(pubkey, kind, d_tag)`, or Ok(None).
+   162	    fn get_param_replaceable(
+   163	        &self,
+   164	        pubkey: &PubKey,
+   165	        kind: u32,
+   166	        d_tag: &[u8],
+   167	    ) -> Result<Option<StoredEvent>, StoreError>;
+   168	
+   169	    /// `idx_etag_time` scan, newest-first. Used by reaction / repost / thread views.
+   170	    fn scan_by_etag<'a>(
+   171	        &'a self,
+   172	        target: &EventId,
+   173	        kinds: &[u32],
+   174	        limit: usize,
+   175	    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
+   176	
+   177	    /// `idx_ptag_time` scan, newest-first. Used by notifications / mention views.
+   178	    fn scan_by_ptag<'a>(
+   179	        &'a self,
+   180	        target: &PubKey,
+   181	        kinds: &[u32],
+   182	        limit: usize,
+   183	    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
+   184	
+   185	    /// `idx_kind_time` scan, newest-first. Used by timeline backfills.
+   186	    /// `kinds` empty = any kind (parity with `scan_by_author_kind`).
+   187	    fn scan_by_kind_time<'a>(
+   188	        &'a self,
+   189	        kinds: &[u32],
+   190	        since: Option<u64>,
+   191	        until: Option<u64>,
+   192	        limit: usize,
+   193	    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
+   194	
+   195	    /// `idx_expires` scan, ascending — used by the NIP-40 reaper.
+   196	    fn scan_expiring_before<'a>(
+   197	        &'a self,
+   198	        unix_seconds: u64,
+   199	        limit: usize,
+   200	    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
+   201	
+   202	    /// Tombstones referencing a target id (typically one row).
+   203	    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError>;
+   204	
+   205	    /// Iterate all tombstones (used by `nmp dump`).
+   206	    fn list_tombstones<'a>(&'a self)
+   207	        -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>;
+   208	
+   209	    /// Provenance sidecar for an event.
+   210	    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError>;
+   211	
+   212	    // ─────── Writes ───────
+   213	
+   214	    /// The single insert path. `source` is the relay that delivered this copy.
+   215	    /// Verifies signature/delegation, applies §7.1 invariants, updates secondaries
+   216	    /// + provenance + tombstones atomically. Returns InsertOutcome per §7.1.
+   217	    fn insert(&self, event: nostr::Event, source: &RelayUrl, received_at_ms: u64)
+   218	        -> Result<InsertOutcome, StoreError>;
+   219	
+   220	    /// Delete by a NMP-internal filter — for admin / GC / kind:5 application.
+   221	    /// Returns the number of primary rows removed.
+   222	    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError>;
+   223	
+   224	    // ─────── Watermarks ───────
+   225	
+   226	    fn read_watermark(&self, key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError>;
+   227	    fn write_watermark(&self, row: WatermarkRow) -> Result<(), StoreError>;
+   228	    fn list_watermarks_for_relay<'a>(
+   229	        &'a self,
+   230	        relay_url: &str,
+   231	    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>;
+   232	
+   233	    // ─────── Hot-set / claims (GC) ───────
+   234	
+   235	    /// Register a claim: caller pins `ids` against eviction until `release`.
+   236	    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError>;
+   237	    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError>;
+   238	
+   239	    /// Soft hint: keep these in hot LRU on a best-effort basis.
+   240	    fn hot_set_hint(&self, ids: &[EventId]) -> Result<(), StoreError>;
+   241	
+   242	    /// One bounded GC pass — reap expired, trim LRU, purge old tombstones.
+   243	    fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError>;
+   244	
+   245	    // ─────── Domain rows (per-DomainModule typed namespace) ───────
+   246	
+   247	    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle<'_>, StoreError>;
+   248	    fn run_migrations(&self, namespace: &'static str, target_version: u32,
+   249	                      migrations: &[crate::substrate::DomainMigration])
+   250	        -> Result<(), StoreError>;
+   251	
+   252	    // ─────── Export ───────
+   253	
+   254	    fn dump(&self, out: &mut dyn std::io::Write, format: DumpFormat)
+   255	        -> Result<DumpStats, StoreError>;
+   256	}
+   257	```
+   258	
+   259	`DeleteFilter` mirrors the limited subset of admin operations the kernel needs (by-relay-only events, by-author, by-id-list, by-kind range); it is **not** a pass-through to `nostr::Filter` — we intentionally do not expose arbitrary remote filters as a delete vector.
+   260	
+   261	## 4. `DomainHandle`
+   262	
+   263	```rust
+   264	pub struct DomainHandle<'env> {
+   265	    pub(crate) namespace: &'static str,
+   266	    pub(crate) inner: DomainHandleInner<'env>,  // backend-specific
+   267	}
+   268	
+   269	impl<'env> DomainHandle<'env> {
+   270	    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError>;
+   271	    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;
+   272	    pub fn delete(&self, key: &[u8]) -> Result<bool, StoreError>;
+   273	    pub fn scan_prefix<'a>(&'a self, prefix: &[u8])
+   274	        -> Result<Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>, StoreError>;
+   275	    pub fn scan_index<'a>(&'a self, index: &'static str, key_prefix: &[u8])
+   276	        -> Result<Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>, StoreError>;
+   277	}
+   278	```
+   279	
+   280	A handle is module-scoped; the kernel does not give a `DraftsModule` handle to `SettingsModule` (per `kernel-substrate.md` §8 "Domain stores are isolated"). The handle is `'env`-bounded so it cannot outlive the LMDB environment.
+   281	
+   282	## 5. Error semantics (doctrine D3)
+   283	
+   284	The trait returns `Result<T, StoreError>`. The actor's wrapper functions map them as:
+   285	
+   286	- `Io / Corrupt` at startup → panic (we cannot run without a store; surfaces to platform shell as a process restart).
+   287	- `Io / Corrupt` mid-run → `Effect::StoreDegraded { details }` published on the diagnostics bridge (ADR-0007); the affected operation returns the closest-fit graceful default (empty iterator, drop-write); the next gc_step retries.
+   288	- `Encoding` → `tracing::error!` with the offending key/namespace; the action that triggered it fails with a `toast: Some("internal storage error; please restart")` per D3.
+   289	- `SchemaTooNew` at startup → publish `Effect::DomainSchemaTooNew { namespace }`, the affected module starts in degraded mode (its actions reject with `ActionRejection::ModuleUnavailable`), rest of the kernel runs.
+   290	- `MigrationFailed` → same as above, plus a one-time toast on first action attempt.
+   291	- `UnknownNamespace` → programming error; assert in debug, log + drop in release.
+   292	
+   293	No `StoreError` ever crosses FFI. The `AppUpdate` carries only successful state + optional `toast: Option<String>`.
+   294	
+   295	## 6. Two backends in v1
+   296	
+   297	```rust
+   298	// In-memory backend, kept for tests + web-pre-M15.
+   299	pub struct MemEventStore { /* HashMap-backed; preserves the current kernel state */ }
+   300	
+   301	// Production backend on iOS / Android / Desktop.
+   302	pub struct LmdbEventStore { /* wraps nostr_lmdb::NostrLMDB + NMP sub-dbs */ }
+   303	
+   304	pub fn open_event_store(cfg: &AppConfig) -> Result<Box<dyn EventStore>, StoreError> {
+   305	    match cfg.storage_backend {
+   306	        StorageBackend::Memory => Ok(Box::new(MemEventStore::new())),
+   307	        StorageBackend::Lmdb { ref path } => Ok(Box::new(LmdbEventStore::open(path)?)),
+   308	    }
+   309	}
+   310	```
+   311	
+   312	`MemEventStore` implements every method using `HashMap` / `BTreeMap`. The same test suite runs against both backends with `#[cfg(feature = "lmdb-backend")]` gating only the LMDB-specific edge tests (corruption recovery, oversized values).
+
+exec
+/bin/zsh -lc "nl -ba docs/design/lmdb/keys.md | sed -n '1,180p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# LMDB sub-design: key encoding
+     2	
+     3	> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Fixes the byte layout of every sub-database the NMP store opens. Primary event storage is owned by `nostr-lmdb`; everything below is NMP-owned and lives in NMP-managed sub-databases under the same `lmdb::Environment` (per open question 1 in the master doc).
+     4	
+     5	## 1. LMDB environment layout
+     6	
+     7	One `lmdb::Environment` per app data directory. Sub-databases:
+     8	
+     9	| Sub-db | Owner | Key shape | Value | Notes |
+    10	|---|---|---|---|---|
+    11	| (multiple) | `nostr-lmdb` | upstream | upstream | event primary, internal filter indexes, kind:5 suppression |
+    12	| `idx_author_kind` | NMP | `pubkey[32] ‖ kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` | empty | newest-first scans for `(author, kinds[])` |
+    13	| `idx_kind_dtag` | NMP | `kind_be[4] ‖ pubkey[32] ‖ dtag_len_be[2] ‖ dtag_bytes` | `event_id[32]` | parameterized replaceable address lookup |
+    14	| `idx_etag_time` | NMP | `target_event_id[32] ‖ created_at_desc_be[8] ‖ event_id[32]` | `kind_be[4]` | reaction/reply/thread view scans |
+    15	| `idx_ptag_time` | NMP | `target_pubkey[32] ‖ created_at_desc_be[8] ‖ event_id[32]` | `kind_be[4]` | mentions / notifications |
+    16	| `idx_kind_time` | NMP | `kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` | empty | global-by-kind backfills |
+    17	| `idx_expires` | NMP | `expires_at_be[8] ‖ event_id[32]` | empty | NIP-40 reaper |
+    18	| `tombstones` | NMP | `target_event_id[32]` | CBOR `TombstoneRow` | persists past delete |
+    19	| `provenance` | NMP | `event_id[32]` | CBOR `ProvenanceRow` | per-relay sidecar (master doc §9) |
+    20	| `watermarks` | NMP | `filter_hash[32] ‖ relay_url_bytes` | CBOR `WatermarkRow` | M4 NIP-77 sync state |
+    21	| `claims_meta` | NMP | `claimer_id_be[8]` | CBOR `Vec<EventId>` | pinned set per ClaimerId; rebuilt on restart from open views |
+    22	| `domain_<ns>_data` | NMP, per `DomainModule` | module-defined | module-defined | one sub-db per registered namespace |
+    23	| `domain_<ns>_idx_<name>` | NMP, per `DomainModule` index | `index_key ‖ primary_key` | empty | secondary indexes per `DomainIndex` |
+    24	| `_meta` | NMP | string namespace | `{ schema_version: u32, opened_with_nmp_version: String }` | migration tracking |
+    25	
+    26	Sub-databases are opened lazily on first access and cached on the `LmdbEventStore`.
+    27	
+    28	## 2. Endian + ordering conventions
+    29	
+    30	- All integers in keys are **big-endian** so LMDB's byte-wise comparator matches numeric order.
+    31	- `created_at_desc_be = (u64::MAX - created_at).to_be_bytes()` so a forward scan returns newest-first without `MDB_PREV` gymnastics.
+    32	- All pubkeys / event ids are fixed-width 32 bytes; the `nostr` crate's `EventId` and `PublicKey` give us byte arrays directly.
+    33	
+    34	## 3. Secondary index details
+    35	
+    36	### 3.1 `idx_author_kind`
+    37	
+    38	Key: `pubkey[32] ‖ kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` → empty value.
+    39	
+    40	Scan recipes:
+    41	
+    42	- *Newest N events by author* — `range(pubkey ‖ 0u32_be ‖ ..)` (kind=0 lower bound) up to `pubkey ‖ u32::MAX_be ‖ ..`, take N.
+    43	- *Newest N events by `(author, kind=1)`* — `range(pubkey ‖ 1u32_be ‖ ..)` up to `pubkey ‖ 1u32_be ‖ u64::MAX_be`, take N.
+    44	- *All kind:0 for author* — `range(pubkey ‖ 0u32_be ‖ ..)`, take 1 (because the replaceable index ensures only one).
+    45	
+    46	Replaceable supersession (§7.1): on insert of a new kind in [0, 3, 10000–19999], find existing row via this index with `(pubkey, kind)` prefix, compare `created_at`, if incoming wins delete old + write new. Both deletes happen in the same `RwTxn` as the new write so there is no half-state visible to readers.
+    47	
+    48	### 3.2 `idx_kind_dtag` (parameterized replaceable)
+    49	
+    50	Key: `kind_be[4] ‖ pubkey[32] ‖ dtag_len_be[2] ‖ dtag_bytes` → `event_id[32]`.
+    51	
+    52	The d-tag bytes go last so two events with the same `(kind, pubkey)` but different `d` tags don't collide; the explicit length prefix avoids `d="foo"` vs `d="foob"` aliasing under prefix scans. Lookup is exact-key: `get_param_replaceable(pubkey, kind, d_tag)` builds the key and reads.
+    53	
+    54	The value is the `event_id`; the primary event itself lives in the `nostr-lmdb` events sub-db. On supersession, the old event-id is fetched from this row, both primary and old `idx_*` rows are deleted, and the value is overwritten with the new id.
+    55	
+    56	### 3.3 `idx_etag_time` and `idx_ptag_time`
+    57	
+    58	Key: `target[32] ‖ created_at_desc_be[8] ‖ event_id[32]` → `kind_be[4]`.
+    59	
+    60	The value holds the kind so a reactions view can filter `(kinds == 7)` during scan without a primary-row fetch per candidate. Bookmark / repost / thread views similarly avoid the `get_by_id` round trip until they need the body.
+    61	
+    62	On insert, the kernel walks the event's `tags`: every `e` tag value goes into `idx_etag_time` and every `p` tag value goes into `idx_ptag_time`. Tag values must be 32-byte hex (validated at insert time); non-conformant tags are silently skipped from indexing (they are still stored in the event body).
+    63	
+    64	### 3.4 `idx_kind_time`
+    65	
+    66	Key: `kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` → empty.
+    67	
+    68	Used by *global-by-kind* backfills (e.g. "recent kind:0 across all authors" during diagnostics). Heavy index — populated for **all** kinds by default but the implementation may skip kinds in a configurable deny-list to keep write amplification down (default deny-list: kind:1 if config flag `index_kind1_globally=false`, which it is by default; M2's planner does not need a global kind:1 scan).
+    69	
+    70	### 3.5 `idx_expires`
+    71	
+    72	Key: `expires_at_be[8] ‖ event_id[32]` → empty.
+    73	
+    74	Populated **only** for events that have an `expiration` tag at insert (NIP-40). `gc_step()` opens a read cursor at `expires_at = 0`, walks forward up to the configured budget, and reaps any keys whose `expires_at ≤ now_unix_seconds()`. Each reaped event triggers a tombstone-of-origin `NIP40Expiry` write so re-insertions (from a re-sync) don't resurrect it.
+    75	
+    76	## 4. Tombstones
+    77	
+    78	Key: `target_event_id[32]` → CBOR `TombstoneRow`:
+    79	
+    80	```rust
+    81	#[derive(Serialize, Deserialize)]
+    82	struct TombstoneRow {
+    83	    target_id: [u8; 32],
+    84	    origin: TombstoneOrigin,             // Kind5 | NIP40Expiry | AdminPurge
+    85	    kind5_event_id: Option<[u8; 32]>,    // None for non-Kind5 origins
+    86	    deleter_pubkey: Option<[u8; 32]>,    // None for NIP40Expiry / AdminPurge
+    87	    deleted_at: u64,                     // max observed across kind:5 redeliveries
+    88	    sources: Vec<String>,                // relay urls that delivered the kind:5
+    89	}
+    90	```
+    91	
+    92	Insert pre-check: before any new event hits the primary store, `tombstones.contains_key(event.id)` is consulted. A hit yields `InsertOutcome::Tombstoned { target_kind5_id }` and the event is dropped. This is the "later re-insertion is suppressed" behavior of §7.1.
+    93	
+    94	Foreign kind:5 (where the kind:5 author did not author all targets) is **stored** as an ordinary event (so other clients can render the delete intent) but **does not** write a `TombstoneRow` for any of its targets — per §7.1 "foreign kind:5 ignored". The kind:5 event itself goes through the normal insert path including secondaries.
+    95	
+    96	## 5. Watermarks
+    97	
+    98	Key: `filter_hash[32] ‖ relay_url_bytes` — variable-length, exact-key lookups only. `filter_hash` is BLAKE3 of the canonical filter encoding (see `lmdb/watermarks.md` §3 for the canonicalisation algorithm).
+    99	
+   100	Value: CBOR `WatermarkRow` (same shape as the trait type in [`trait.md`](trait.md) §2).
+   101	
+   102	## 6. Provenance
+   103	
+   104	Key: `event_id[32]` → CBOR `ProvenanceRow { sources: Vec<ProvenanceEntry> }`. On duplicate insert: read, mutate (append or bump `last_seen_ms`), write back. Bounded growth — the kernel caps `sources.len()` at 32 (the 33rd unique relay overwrites the oldest non-primary entry); for nearly all events this is non-binding. The `primary: bool` flag is deterministic: `sources[0]` after sorting by `(first_seen_ms, relay_url)`.
+   105	
+   106	## 7. Domain rows (per `DomainModule`)
+   107	
+   108	For each `DomainModule` with namespace `"foo.bar"`:
+   109	
+   110	- `domain_foo.bar_data` — primary data sub-db. Module owns key + value encoding.
+   111	- `domain_foo.bar_idx_<index>` — one sub-db per `DomainIndex` (per `crates/nmp-core/src/substrate/domain.rs:16`). Key = `index_key_fn(data_value) ‖ primary_key`; value = empty. The index is rewritten on every put (delete-old, write-new).
+   112	
+   113	The actor exposes them only via `DomainHandle` (see [`trait.md`](trait.md) §4); modules never see the sub-db handles directly. Module isolation per `kernel-substrate.md` §8 is preserved: the handle factory checks the caller's registered namespace.
+   114	
+   115	## 8. `_meta` sub-database
+   116	
+   117	Key: namespace string (e.g. `"twitter.drafts"`, `"_kernel"`). Value: CBOR `{ schema_version: u32, opened_with_nmp_version: String, last_migration_at_ms: u64 }`. Read at startup by the migration runner; written after every successful migration step.
+   118	
+   119	The reserved `_kernel` namespace tracks the LMDB store's own schema version (currently 1). A bumped `_kernel` version triggers store-wide migrations (e.g. re-encoding all `ProvenanceRow` values when the format changes).
codex
I’ll verify against the checked-out repo, not the pasted stat, because the fixes should be in the `ffi-hardening` docs and related perf docs rather than the review-transcript files shown here.
exec
/bin/zsh -lc 'git status -sb && git rev-parse --short HEAD && git branch --show-current' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
## master...origin/master
 M docs/design/podcast/copy.md
?? .claire/
?? .claude/
?? docs/perf/codex-reviews/fb139ab.md
9eace68
master

exec
/bin/zsh -lc 'rg -n "fb139ab|fcf8b43|D5|stress-gate-fast|stress-close-gate|freed-pointer|S10|ffi-hardening|14 symbols|13 symbols|CapabilityModule|session-1" /Users/pablofernandez/.codex/memories/MEMORY.md' in /Users/pablofernandez/Work/nostr-multi-platform
 exited 1 in 0ms:
codex
The worktree is dirty, but only with unrelated local/untracked paths from the current run and a podcast doc edit. I’ll keep this read-only and compare `fb139ab` plus the current files that should contain the fixes.
exec
/bin/zsh -lc 'git log --oneline --decorate -n 20' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
9eace68 (HEAD -> master, origin/master) plan(scope): defer DMs (M9) + Wallet (M12); add M11.5 Highlighter; framework-magic contract
fb139ab (worktree-agent-acdef1454b384c4b2) docs(m10.5): fix codex feedback (D5 evidence, CI gate split, freed-ptr probe, file sizes, symbol count)
20b2e2d perf(codex): reviews of 031fc07 (M2) + 9fead0e (M3); heartbeat #2 log
b4b6afa docs(m11): fix codex feedback (file sizes, harness, D5, D4, rig.rs gate, OPML, counts)
75d18ff (worktree-agent-a1da29da2c5b36de9) perf(codex): review of fcf8b43
043f8b7 perf(codex): review of 03d7a23
03d7a23 (worktree-agent-ad49d31a2b138ee89) m10.5(ffi): add SAFETY comments + clarify defensive pattern
2f16a8c perf(orchestration): wave-1 complete; codex reviews logged
0dfb975 design(m11): ../podcast rebuild plan on NMP
fcf8b43 docs: apply codex review fixes from session-1
9fead0e design(m3): LMDB schema + EventStore trait + GC policy
031fc07 design(m2): subscription compilation + outbox routing spec
f1e374b perf(m1): clean-build verification baseline
d660735 audit(m10.5): FFI + iOS bridge debt inventory
31fa612 perf(orchestration): advisor adjustments (push protocol, triage, gates)
18e4953 perf(orchestration): start log; wave 1 dispatched
55dd5f2 docs(plan): insert M10.5 FFI hardening gate + concretize M11 podcast rebuild
e9cbafa docs(plan): consolidate into single milestone-driven plan with product checks
2cd423a ios: wire ProfileInterestAvatar to kernel claim/release system
3210bb7 actor/ffi: expose ClaimProfile and ReleaseProfile commands

exec
/bin/zsh -lc 'git show --stat --oneline --decorate --no-renames fb139ab' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
fb139ab (worktree-agent-acdef1454b384c4b2) docs(m10.5): fix codex feedback (D5 evidence, CI gate split, freed-ptr probe, file sizes, symbol count)
 docs/design/ffi-hardening.md                    |  42 +--
 docs/design/ffi-hardening/ci-tiers.md           | 166 ++++++++++
 docs/design/ffi-hardening/ci.md                 | 190 ++----------
 docs/design/ffi-hardening/gates.md              |  55 +++-
 docs/design/ffi-hardening/harness-runners.md    | 155 ++++++++++
 docs/design/ffi-hardening/harness.md            | 177 ++---------
 docs/design/ffi-hardening/scenarios-detail.md   | 180 +++++++++++
 docs/design/ffi-hardening/scenarios.md          | 172 +----------
 docs/perf/codex-reviews/2026-05-18-session-1.md |   2 +
 docs/perf/m10.5/debt-inventory-findings.md      | 218 +++++++++++++
 docs/perf/m10.5/debt-inventory.md               | 386 ++++--------------------
 11 files changed, 888 insertions(+), 855 deletions(-)

exec
/bin/zsh -lc 'wc -l docs/design/ffi-hardening.md docs/design/ffi-hardening/*.md docs/perf/m10.5/debt-inventory.md docs/perf/codex-reviews/2026-05-18-session-1.md' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     298 docs/design/ffi-hardening.md
     166 docs/design/ffi-hardening/ci-tiers.md
     209 docs/design/ffi-hardening/ci.md
     290 docs/design/ffi-hardening/gates.md
     155 docs/design/ffi-hardening/harness-runners.md
     299 docs/design/ffi-hardening/harness.md
     180 docs/design/ffi-hardening/scenarios-detail.md
     187 docs/design/ffi-hardening/scenarios.md
     125 docs/perf/m10.5/debt-inventory.md
    1265 docs/perf/codex-reviews/2026-05-18-session-1.md
    3174 total

exec
/bin/zsh -lc 'rg -n "D5|CapabilityModule|snapshot|stress-gate-fast|stress-close-gate|pre-merge|milestone-close|freed-pointer|freed pointer|S10|S3|S4|D1|14 symbols|13 symbols|session-1|archiv" docs/design/ffi-hardening.md docs/design/ffi-hardening/*.md docs/perf/m10.5/debt-inventory.md docs/perf/codex-reviews/2026-05-18-session-1.md docs/perf/orchestration-log.md' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
docs/perf/m10.5/debt-inventory.md:58:Doctrine audits (D0–D5) in
docs/perf/m10.5/debt-inventory.md:70:| ProfileCard.placeholder (D1) | Design compliance | Correct D1 implementation; refinement in place. |
docs/perf/m10.5/debt-inventory.md:113:(D0–D5) are upheld.
docs/design/ffi-hardening/ci.md:5:2. **§C** CI integration — pre-merge / nightly / on-demand tiers.
docs/design/ffi-hardening/ci.md:30:        snapshot-pressure --duration 30s --fail-on-gate
docs/design/ffi-hardening/ci.md:84:stress-gate-fast:
docs/design/ffi-hardening/ci.md:86:        --scenarios S1,S2,S3,S5,S7,S8
docs/design/ffi-hardening/ci.md:89:stress-close-gate:
docs/design/ffi-hardening/ci.md:91:        --require-doctrines D0,D1,D2,D3,D4,D5 \
docs/design/ffi-hardening/ci.md:121:├── S3/ ...
docs/design/ffi-hardening/ci.md:122:├── ... (one dir per scenario through S10)
docs/design/ffi-hardening/ci.md:165:**Fast mode (`stress-gate-fast` — pre-merge):** only fast simulator
docs/design/ffi-hardening/ci.md:168:**Close mode (`stress-close-gate` — milestone close):** full battery
docs/design/ffi-hardening/ci.md:177:    # Fast mode: ["S1","S2","S3","S5","S7","S8"]
docs/design/ffi-hardening/ci.md:178:    # Close mode: ["S1","S2","S3","S5","S6","S7","S8","S9"]
docs/design/ffi-hardening/ci.md:179:    # S4 is XCUITest-only; S10 deferred until M3+M4 complete
docs/design/ffi-hardening/ci.md:180:    for scenario in (scenarios or ["S1","S2","S3","S5","S6","S7","S8","S9"]):
docs/design/ffi-hardening/ci.md:207:CI tier workflows (pre-merge, nightly, on-demand, trace replay,
docs/design/ffi-hardening/scenarios-detail.md:1:# FFI hardening — scenarios S6–S10
docs/design/ffi-hardening/scenarios-detail.md:58:Note: freed-pointer (use-after-free) probes are **not** included.
docs/design/ffi-hardening/scenarios-detail.md:146:## S10. Long suspend simulation — 60-second background
docs/design/ffi-hardening/scenarios-detail.md:150:only if M3+M4 are complete by then.** If not, S10 graduates to
docs/design/ffi-hardening/scenarios-detail.md:151:M11.5 and the M10.5 gate excludes it. S10 is **not** used as
docs/design/ffi-hardening/scenarios-detail.md:152:doctrine sign-off evidence for M10.5 (see gates.md §D1 note).
docs/design/ffi-hardening/scenarios-detail.md:165:3. Verify state reconciles to the same snapshot a never-suspended
docs/design/ffi-hardening/scenarios-detail.md:176:4. Final state snapshot identical to the non-suspended control run.
docs/design/ffi-hardening/scenarios-detail.md:180:**Numeric gate.** See gates.md §G-S10.
docs/design/ffi-hardening/harness.md:59:│   ├── snapshot_pressure.rs # S3
docs/design/ffi-hardening/harness.md:65:│   └── long_suspend.rs      # S10 (conditional on M3+M4)
docs/design/ffi-hardening/harness.md:69:(S4 reconciler back-pressure is iOS-main-thread-only; lives in
docs/design/ffi-hardening/harness.md:80:  snapshot-pressure    S3
docs/design/ffi-hardening/harness.md:86:  long-suspend         S10 (skipped unless --experimental-suspend)
docs/design/ffi-hardening/harness.md:176:only (not in `nmp-core`). Used by S1, S2, S3, S6, S8 to detect heap
docs/design/ffi-hardening/harness.md:225:├── S3SnapshotPressure.swift
docs/design/ffi-hardening/harness.md:226:├── S4ReconcilerBackpressure.swift  # iOS-only
docs/design/ffi-hardening/harness.md:232:└── S10LongSuspend.swift            # conditional on M3+M4
docs/design/ffi-hardening/harness.md:284:- `XCTHitchMetric` — main-thread hitches (S2, S3, S4).
docs/design/ffi-hardening/harness.md:286:- `XCTMemoryMetric` — RSS sample (S1, S3, S8).
docs/design/ffi-hardening/harness.md:287:- `XCTCPUMetric` — CPU usage (S2, S3).
docs/design/ffi-hardening/harness-runners.md:99:not pre-merge; treated as advisory unless multiple agents in one
docs/design/ffi-hardening.md:19:   (`docs/product-spec/overview-and-dx.md` §1.5 D0–D5), and every ownership
docs/design/ffi-hardening.md:114:| S1 | Mount/unmount churn | actor recv + refcount | D5 (snapshot bounded), bible #5 |
docs/design/ffi-hardening.md:116:| S3 | Snapshot pressure | listener serialization | bible #9 (≤60 Hz), D5 |
docs/design/ffi-hardening.md:117:| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
docs/design/ffi-hardening.md:121:| S8 | Subscription planner DOS | OpenView/CloseView storm | D2 (≤60 Hz/view), D5 |
docs/design/ffi-hardening.md:123:| S10 | Long suspend simulation | background → foreground | bible #1 (monotonic rev) |
docs/design/ffi-hardening.md:161:just stress          # local simulator, fast scenarios (S1, S2, S3, S5, S7, S8)
docs/design/ffi-hardening.md:176:├── doctrine-review.md       # D0–D5 sign-off (M10.5 exit-gate artifact)
docs/design/ffi-hardening.md:242:Full D0–D5 line-item-to-scenario mapping in
docs/design/ffi-hardening.md:248:| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
docs/design/ffi-hardening.md:249:| **D2** ≤60Hz/view, working-set bound | S2, S3, S8 — emit-rate cap, planner dedup |
docs/design/ffi-hardening.md:252:| **D5** snapshots bounded by what's open | S1 (refcount drives eviction) + S3 (full-state size scales with open views, not store) |
docs/design/ffi-hardening.md:260:| **Pre-merge** | every PR | S1 (short), S2, S3 (10k events), S5, S7, S8 | GH Actions `macos-14` runner, iPhone 16 Pro sim |
docs/design/ffi-hardening.md:261:| **Nightly** | daily | All ten, S1 full 10-min, S4 250 ms stalls × 60, S9 100 flap, S10 60s suspend | Mac mini self-hosted runner, iPhone 12 device |
docs/design/ffi-hardening.md:273:   nightly-only (no pre-merge gating), or do we record + replay agent
docs/design/ffi-hardening.md:290:5. **Handle registry for freed-pointer safety (M14).** Calling `nmp_app_*`
docs/design/ffi-hardening.md:292:   to make it recoverable. S7 excludes freed-pointer probes for this reason.
docs/perf/orchestration-log.md:9:| 2026-05-18 01:55 | 1 | Wave 1 completed in ~30 min. Landed: T7 build-verifier f1e374b (all gates green, sim screenshot captured); T6 debt-auditor d660735 (0 TODO/FIXME, 0 unimplemented in FFI surface, 4 must-fix safety-comment items); T2 m2-designer 031fc07 (subscription compilation spec, 8 files); T3 m3-designer 9fead0e (LMDB schema spec, 6 files); T5 m11-designer 0dfb975 (M11 podcast design, 13 files); T4 m105-designer's ffi-hardening files (5) absorbed into orchestrator commit fcf8b43. Three codex post-merge reviews ran: review-1 on the wave-1 cumulative diff flagged 5 issues (debt-inventory exit-ready language, NIP-XX placeholder, ADR-deferral escape, file sizes); review-2 on fcf8b43 flagged 6 issues (D5 mismatch, pre-merge CI gate, freed-pointer UB, S10 conditional, file sizes again, symbol count); review-3 on 0dfb975 flagged 7 issues (Swift file sizes 898/642, MCP-in-CI, AudioPlayback policy, EmbeddingId cycle, rig.rs weakened, OPML pixel-parity, view count). All flagged issues are being addressed via dedicated fix-it tasks T8 (codex-fixer-1) and T11 (codex-fixer-2). T1 (m1-hardener) unblocked + dispatched. T9 (ffi-safety-commenter) dispatched for the 4 must-fix items. Post-merge codex review protocol established and entered memory. |
docs/design/ffi-hardening/ci-tiers.md:4:`just stress-gate-fast`, `just stress-close-gate`) and the gate
docs/design/ffi-hardening/ci-tiers.md:15:**Scenarios.** S1 (short — 60 s), S2 (30 s), S3 (30 s), S5 (30 s),
docs/design/ffi-hardening/ci-tiers.md:16:S7 (full matrix), S8 (60 s). **Not S4** (iOS-main-thread, slow XCUITest
docs/design/ffi-hardening/ci-tiers.md:18:**Not S9** (10 min) — nightly. **Not S10** (conditional on M3+M4).
docs/design/ffi-hardening/ci-tiers.md:20:**Workflow.** `.github/workflows/stress-pre-merge.yml`:
docs/design/ffi-hardening/ci-tiers.md:23:name: FFI stress (pre-merge)
docs/design/ffi-hardening/ci-tiers.md:40:      - run: just stress-gate-fast
docs/design/ffi-hardening/ci-tiers.md:44:          name: stress-pre-merge-${{ github.run_id }}
docs/design/ffi-hardening/ci-tiers.md:48:**Gating.** The `just stress-gate-fast` step exit code is the PR gate.
docs/design/ffi-hardening/ci-tiers.md:49:Fast gate checks S1, S2, S3, S5, S7, S8 only — no doctrine review,
docs/design/ffi-hardening/ci-tiers.md:50:no debt-inventory check (both are milestone-close artifacts only).
docs/design/ffi-hardening/ci-tiers.md:58:S3 (10 emits × 100 k events), S4 (60 s × 12 stalls), S5 (30 s),
docs/design/ffi-hardening/ci-tiers.md:60:100/min), S10 (60 s suspend — *only if M3+M4 are complete; the
docs/design/ffi-hardening/ci-tiers.md:81:      - run: just stress-gate-fast || echo "::warning::nightly gate failed"
docs/design/ffi-hardening/ci-tiers.md:90:but do not block merge. They block the M10.5 milestone-close
docs/design/ffi-hardening/ci-tiers.md:127:- **iPhone 12 hardware-only scenarios** (S9 device variant, S4 device
docs/design/ffi-hardening/ci-tiers.md:131:- **S10 if M3+M4 are not complete:** scenario reports as `skipped:
docs/design/ffi-hardening/ci-tiers.md:132:  prereq` with a note in `metrics.json`; gate script omits S10
docs/design/ffi-hardening/ci-tiers.md:133:  from nightly gate check until M3+M4 land. S10 is not used as
docs/design/ffi-hardening/ci-tiers.md:134:  doctrine sign-off evidence for M10.5 (see gates.md §D1 note).
docs/design/ffi-hardening/ci-tiers.md:143:| Nightly | 90 days | Per-run; archived to S3 quarterly |
docs/design/ffi-hardening/ci-tiers.md:154:1. `just stress-close-gate` exits 0 — runs the full battery
docs/design/ffi-hardening/ci-tiers.md:155:   (S1–S9, excluding S10 while M3+M4 are pending) plus doctrine
docs/design/ffi-hardening/ci-tiers.md:159:4. Doctrine review (D0–D5) signed off in `doctrine-review.md`.
docs/design/ffi-hardening/scenarios.md:84:## S3. Snapshot pressure — `AppUpdate::FullState` with 100k events
docs/design/ffi-hardening/scenarios.md:86:**The bug shape.** Marshal cost for a full-state snapshot grows
docs/design/ffi-hardening/scenarios.md:89:spends every frame applying snapshots.
docs/design/ffi-hardening/scenarios.md:92:(`docs/perf/firehose-bench/traces/m10.5-snapshot.cap`, captured
docs/design/ffi-hardening/scenarios.md:119:## S4. Reconciler back-pressure — main thread stalled 250 ms
docs/design/ffi-hardening/scenarios.md:185:Scenarios S6–S10 (capability storms, error exhaustion, planner DOS,
docs/design/ffi-hardening/gates.md:7:2. **§D** doctrine D0–D5 review checklist — each line item maps to
docs/design/ffi-hardening/gates.md:46:### G-S3. Snapshot pressure (100 k events, 10 emits)
docs/design/ffi-hardening/gates.md:58:### G-S4. Reconciler back-pressure (12 stalls × 250 ms, 60 s)
docs/design/ffi-hardening/gates.md:101:| Symbols × invalid-input variants exercised | `>=` | 70 (14 symbols × 5 variants avg) | 70 |
docs/design/ffi-hardening/gates.md:126:### G-S10. Long suspend (conditional on M3+M4)
docs/design/ffi-hardening/gates.md:134:| Post-resume state snapshot equals control | `==` | byte-equal (relevant payloads) | byte-equal |
docs/design/ffi-hardening/gates.md:150:## §D. Doctrine D0–D5 review checklist
docs/design/ffi-hardening/gates.md:155:> **Note.** The task brief mentioned "D0–D5". The canonical list in
docs/design/ffi-hardening/gates.md:156:> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
docs/design/ffi-hardening/gates.md:159:> not redundantly re-prove — items beyond D0–D5 are covered by
docs/design/ffi-hardening/gates.md:173:### D1. Best-effort rendering — render now, refine in place
docs/design/ffi-hardening/gates.md:175:- ✅ **Stress proof:** S3 (snapshot pressure) — every emit must
docs/design/ffi-hardening/gates.md:182:- ✅ **Stress proof (back-pressure path):** S4 (reconciler
docs/design/ffi-hardening/gates.md:187:  immediately from `debt-inventory.md` §3 D1 audit; no spinner, no
docs/design/ffi-hardening/gates.md:189:- 📝 **Sign-off:** doctrine-review.md § D1.
docs/design/ffi-hardening/gates.md:191:> **Note on S10.** S10 (long suspend) would provide additional D1
docs/design/ffi-hardening/gates.md:192:> evidence for the resume path, but S10 is conditional on M3+M4
docs/design/ffi-hardening/gates.md:194:> S10 is deferred to M11.5 or whenever M3+M4 land; it is not used as
docs/design/ffi-hardening/gates.md:195:> doctrine sign-off evidence here. D1 is signed off on S3 + S4 + code
docs/design/ffi-hardening/gates.md:200:- ✅ **Stress proof:** S2 (dispatch flood), S3 (snapshot pressure),
docs/design/ffi-hardening/gates.md:242:### D5. Capabilities report, never decide
docs/design/ffi-hardening/gates.md:247:- ✅ **Code proof:** `CapabilityModule` trait (`substrate/capability.rs`)
docs/design/ffi-hardening/gates.md:264:- 📝 **Sign-off:** doctrine-review.md § D5.
docs/design/ffi-hardening/gates.md:279:| D1 | PASS | S3 + S4 metrics.json + S3/screenshots (placeholder-then-refine path) | <name> | <date> |
docs/design/ffi-hardening/gates.md:280:| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
docs/design/ffi-hardening/gates.md:283:| D5 | PASS | debt-inventory §3 D5 + S6 metrics.json + capability.rs code review | <name> | <date> |
docs/perf/codex-reviews/2026-05-18-session-1.md:17:You are reviewing a session's worth of merges on master in the nostr-multi-platform repo. NMP is a Rust multiplatform framework for Nostr apps building toward v1 per docs/plan.md. Doctrine D0–D5 (docs/product-spec.md §1.5):
docs/perf/codex-reviews/2026-05-18-session-1.md:19:- D1 best-effort rendering with placeholders
docs/perf/codex-reviews/2026-05-18-session-1.md:23:- D5 capabilities report, never decide
docs/perf/codex-reviews/2026-05-18-session-1.md:47:- All 5 cardinal doctrines (D0–D5) compliant
docs/perf/codex-reviews/2026-05-18-session-1.md:58:✅ D1 (best-effort rendering): ProfileCard.placeholder renders immediately
docs/perf/codex-reviews/2026-05-18-session-1.md:62:✅ D5 (capabilities report): iOS bridge is pure relay, no policy decisions
docs/perf/codex-reviews/2026-05-18-session-1.md:83:  snapshot pressure, reconciler back-pressure, reentrancy, capability
docs/perf/codex-reviews/2026-05-18-session-1.md:401:+### D1 Audit: Best-Effort Rendering with Placeholders
docs/perf/codex-reviews/2026-05-18-session-1.md:456:+### D5 Audit: Capabilities Report, Never Decide
docs/perf/codex-reviews/2026-05-18-session-1.md:464:+- The `CapabilityModule` trait (in `substrate/capability.rs`) defines how modules *report* capabilities, not how the bridge decides what to expose.
docs/perf/codex-reviews/2026-05-18-session-1.md:478:+| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
docs/perf/codex-reviews/2026-05-18-session-1.md:509:+- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
docs/perf/codex-reviews/2026-05-18-session-1.md:537:+**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/2026-05-18-session-1.md:578:+> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
docs/perf/codex-reviews/2026-05-18-session-1.md:638:+- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
docs/perf/codex-reviews/2026-05-18-session-1.md:803:I’ll verify the merged snapshot, then check the new docs against the stated doctrine, LOC ceiling, and shortcut language with exact line refs.
docs/perf/codex-reviews/2026-05-18-session-1.md:869:   363	| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
docs/perf/codex-reviews/2026-05-18-session-1.md:900:   394	- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
docs/perf/codex-reviews/2026-05-18-session-1.md:928:   422	**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/2026-05-18-session-1.md:985:   380	- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.

codex
The six targeted fixes mostly show up in the split `ffi-hardening` files. One thing I’m checking now is whether the D5 correction was only made in `gates.md` or whether stale D5 wording remains in the top-level index.
exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening/gates.md | sed -n '90,115p;126,205p;238,286p'" in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening/scenarios-detail.md | sed -n '40,75p;146,156p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
    40	**The bug shape.** A typed FFI error path produces an uncaught
    41	exception, a crash, or — per the §7.2 finding in the parent doc —
    42	*silent loss*: invalid input is dropped without any state field
    43	surfacing the problem.
    44	
    45	**Setup.** For every `nmp_app_*` symbol that takes a `*const c_char`
    46	input, exercise the full set of invalid inputs:
    47	
    48	| Symbol | Invalid inputs to test |
    49	|---|---|
    50	| `nmp_app_open_author` | NULL, "", " ", "not-hex", 63-char hex, 65-char hex, UTF-8 with non-hex chars |
    51	| `nmp_app_open_thread` | same shapes |
    52	| `nmp_app_open_firehose_tag` | NULL, "" (others valid; tag is unconstrained) |
    53	| `nmp_app_claim_profile` | NULL/empty/non-hex pubkey × {NULL, "", "valid"} consumer_id |
    54	| `nmp_app_release_profile` | same matrix; also: release without prior claim |
    55	| `nmp_app_close_author` / `_thread` | same |
    56	| any `_app` arg | NULL |
    57	
    58	Note: freed-pointer (use-after-free) probes are **not** included.
    59	Calling any `nmp_app_*` with a dangling pointer after `nmp_app_free`
    60	is undefined behavior on raw FFI — the harness cannot make a
    61	no-crash assertion for UB. A handle registry to convert UB into a
    62	recoverable error is deferred to M14 (UniFFI migration); tracked as
    63	an open question in `ffi-hardening.md` §10.
    64	
    65	**Threading.** Caller. Pure FFI exercise.
    66	
    67	**Assertions.**
    68	1. Zero crashes / SIGSEGV / SIGABRT across the full matrix.
    69	2. NULL app pointer calls: **silent no-op only** (early return in
    70	   `app_ref()`). No toast assertion for NULL app — there is no state
    71	   channel to deliver a toast when the app pointer is null.
    72	3. Every other silent-no-op validation failure (invalid pubkey,
    73	   empty string, non-hex input) produces a **toast field** in the
    74	   next emit (post §7.2 toast-bridge addition).
    75	4. Every typed error path's toast string is non-empty and
   146	## S10. Long suspend simulation — 60-second background
   147	
   148	**Status: conditional on M3 (event store) + M4 (sync watermarks).**
   149	This scenario is specified now and **scheduled to land in M10.5
   150	only if M3+M4 are complete by then.** If not, S10 graduates to
   151	M11.5 and the M10.5 gate excludes it. S10 is **not** used as
   152	doctrine sign-off evidence for M10.5 (see gates.md §D1 note).
   153	
   154	**The bug shape.** iOS suspends the app for 60 s (background). On
   155	resume, the kernel actor's main loop has paused; relay sockets
   156	have timed out; sync watermarks need to drive the catch-up. If the

 succeeded in 0ms:
    90	| Deadlocks (5 s per-cycle watchdog) | `==` | 0 | 0 |
    91	
    92	### G-S7. Error-shape exhaustion (full matrix)
    93	
    94	| Metric | Op | Sim threshold | iPhone 12 threshold |
    95	|---|---|---|---|
    96	| Crashes / signals across full input matrix | `==` | 0 | 0 |
    97	| Crashes on NULL app pointer calls | `==` | 0 | 0 |
    98	| Toast field populated on every silent-no-op path | `==` | 100 % of validation-fail paths | 100 % |
    99	| Toast strings non-empty + actionable (regex match) | `==` | 100 % | 100 % |
   100	| Instruments-Allocations delta over full matrix | `==` | 0 (no leak per error path) | 0 |
   101	| Symbols × invalid-input variants exercised | `>=` | 70 (14 symbols × 5 variants avg) | 70 |
   102	
   103	### G-S8. Subscription planner DOS (5 × 10 k storm, 30 s gap)
   104	
   105	| Metric | Op | Sim threshold | iPhone 12 threshold |
   106	|---|---|---|---|
   107	| Peak working-set RSS during storm | `<=` | 150 MiB | 200 MiB |
   108	| Wire-REQ frame count per 10 k OpenViews | `<=` | 2,500 (4× dedup floor) | 2,500 |
   109	| Planner state size after all closes | `<=` | baseline × 1.05 | baseline × 1.10 |
   110	| Actor stalls > 50 ms during storm | `==` | 0 | 0 |
   111	| Auto-close success rate (every release → close) | `==` | 100 % | 100 % |
   112	| Wire subscription leaks after 5 storms | `==` | 0 | 0 |
   113	
   114	### G-S9. Relay flap (100/min × 10 min = 1 000 flaps)
   115	
   126	### G-S10. Long suspend (conditional on M3+M4)
   127	
   128	| Metric | Op | Sim threshold | iPhone 12 threshold |
   129	|---|---|---|---|
   130	| Watermark persisted across suspend | `==` | true | true |
   131	| Catch-up REQ uses `since` filter | `==` | true | true |
   132	| Catch-up window wall-time | `<=` | 5 s | 8 s |
   133	| Catch-up bandwidth vs steady-state | `<=` | 3× | 3× |
   134	| Post-resume state snapshot equals control | `==` | byte-equal (relevant payloads) | byte-equal |
   135	
   136	---
   137	
   138	## §G.1 Aggregate gates (apply across all scenarios)
   139	
   140	| Metric | Op | Threshold | Source |
   141	|---|---|---|---|
   142	| Total Instruments-Leaks across the suite | `==` | 0 | every scenario with `--instruments` |
   143	| Total crashes / panics across the suite | `==` | 0 | every scenario |
   144	| M1–M10 firehose-bench rerun regression (p99 ms, any scenario) | `<=` | +5 % vs baseline | plan.md §M10.5 exit-gate row 2 |
   145	| Pre-merge runtime budget per PR | `<=` | 15 min wall-time on `macos-14` | ci.md §1 |
   146	| Nightly runtime budget on Mac mini self-hosted | `<=` | 90 min | ci.md §2 |
   147	
   148	---
   149	
   150	## §D. Doctrine D0–D5 review checklist
   151	
   152	Cardinal doctrines per `docs/product-spec/overview-and-dx.md` §1.5.
   153	Each line item names the artifact that proves it.
   154	
   155	> **Note.** The task brief mentioned "D0–D5". The canonical list in
   156	> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
   157	> file follows that list. (The aim.md §6 list of 12 items is a
   158	> larger doctrine set the FFI hardening pass deliberately does
   159	> not redundantly re-prove — items beyond D0–D5 are covered by
   160	> earlier milestones' own gates.)
   161	
   162	### D0. Kernel never grows app nouns
   163	
   164	- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
   165	  no app-domain types in `nmp-core` substrate.
   166	- ✅ **Stress proof:** S6 (capability lifecycle storms) does
   167	  1 000 start/stop/restart cycles; the kernel's capability set is
   168	  unchanged across all cycles (no dynamic registration of new
   169	  noun types).
   170	- 📝 **Sign-off:** auditor signature line in
   171	  `docs/perf/m10.5/doctrine-review.md` § D0.
   172	
   173	### D1. Best-effort rendering — render now, refine in place
   174	
   175	- ✅ **Stress proof:** S3 (snapshot pressure) — every emit must
   176	  satisfy the placeholder-then-refine contract: missing display
   177	  names → shortened-npub placeholders are present (no `None`); when
   178	  kind:0 arrives, the next emit updates in place. Assertion: for
   179	  every author with placeholder text in emit N, emit M > N where
   180	  the kind:0 arrived must contain the resolved text and an
   181	  unchanged `id`.
   182	- ✅ **Stress proof (back-pressure path):** S4 (reconciler
   183	  back-pressure) — on stall release, the timeline renders with
   184	  placeholders immediately, not spinner-blocked. Emit ordering is
   185	  monotonic; no frame is dropped.
   186	- ✅ **Code proof:** iOS `ProfileCard.placeholder(pubkey:)` renders
   187	  immediately from `debt-inventory.md` §3 D1 audit; no spinner, no
   188	  `nil` guard blocking render.
   189	- 📝 **Sign-off:** doctrine-review.md § D1.
   190	
   191	> **Note on S10.** S10 (long suspend) would provide additional D1
   192	> evidence for the resume path, but S10 is conditional on M3+M4
   193	> (persistence + watermarks), which are not complete at M10.5 close.
   194	> S10 is deferred to M11.5 or whenever M3+M4 land; it is not used as
   195	> doctrine sign-off evidence here. D1 is signed off on S3 + S4 + code
   196	> proof above.
   197	
   198	### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
   199	
   200	- ✅ **Stress proof:** S2 (dispatch flood), S3 (snapshot pressure),
   201	  S8 (planner DOS) all assert reconciler frequency stays bounded
   202	  (currently capped at 12 Hz; the doctrine says ≤ 60 Hz; the gate
   203	  is the 60 Hz ceiling, the configured cap is internal policy).
   204	- ✅ **Stress proof (working set):** S8 asserts planner state
   205	  returns to baseline ± 5 % after close storms.
   238	  `loom`-style instrumentation in the harness binary only, not
   239	  prod).
   240	- 📝 **Sign-off:** doctrine-review.md § D4.
   241	
   242	### D5. Capabilities report, never decide
   243	
   244	Canonical (plan.md:9): capabilities surface position events to the
   245	iOS layer; **no policy decisions are made at the bridge**.
   246	
   247	- ✅ **Code proof:** `CapabilityModule` trait (`substrate/capability.rs`)
   248	  defines typed `Request`/`Result` pairs — modules *report* capability
   249	  results back to the platform; they never decide what to do with them.
   250	  The `callback_interface_name()` entry point delivers results to the
   251	  iOS layer as data, not as control signals.
   252	- ✅ **Capability evidence (M10.5 surface):** The relay-role capability
   253	  (content + indexer) is the only active module today. `RelayStatus` is
   254	  emitted as an update field — the kernel reports relay position; iOS
   255	  renders it. No routing decisions are made at the bridge.
   256	- ✅ **M11 prep evidence:** Both `AudioPlaybackCapability` and
   257	  `EmbeddingCapability` (planned for M11) follow the same pattern: the
   258	  kernel emits position/ready events back to the platform; the platform
   259	  renders them. No capability module will acquire routing authority.
   260	- ✅ **Stress proof:** S6 (capability lifecycle storms) — 1,000
   261	  start/stop/restart cycles verify the relay capability lifecycle without
   262	  any module gaining decision authority; all `RelayControl` entries are
   263	  closed after every Stop.
   264	- 📝 **Sign-off:** doctrine-review.md § D5.
   265	
   266	---
   267	
   268	## §D.1 Doctrine sign-off artifact
   269	
   270	`docs/perf/m10.5/doctrine-review.md` is produced at the *end* of
   271	M10.5, not as part of this design. The structure is:
   272	
   273	```
   274	# M10.5 Doctrine Review
   275	
   276	| Doctrine | Status | Evidence | Reviewer | Date |
   277	|---|---|---|---|---|
   278	| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
   279	| D1 | PASS | S3 + S4 metrics.json + S3/screenshots (placeholder-then-refine path) | <name> | <date> |
   280	| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
   281	| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
   282	| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
   283	| D5 | PASS | debt-inventory §3 D5 + S6 metrics.json + capability.rs code review | <name> | <date> |
   284	
   285	## Notes
   286	<any caveats, deferrals, follow-ups>

exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening/ci.md | sed -n '78,96p;158,181p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
    78	# Aggregate all scenario JSON into one markdown summary
    79	stress-report:
    80	    python3 scripts/stress-aggregate.py docs/perf/m10.5/ \
    81	        > docs/perf/m10.5/m10.5-summary.md
    82	
    83	# Pre-merge gate: fast scenarios only; no doctrine review required
    84	stress-gate-fast:
    85	    python3 scripts/stress-gate.py docs/perf/m10.5/ \
    86	        --scenarios S1,S2,S3,S5,S7,S8
    87	
    88	# Milestone-close gate: full battery + doctrine review + debt clean
    89	stress-close-gate:
    90	    python3 scripts/stress-gate.py docs/perf/m10.5/ \
    91	        --require-doctrines D0,D1,D2,D3,D4,D5 \
    92	        --require-debt-inventory-clean \
    93	        --require-grep-clean
    94	```
    95	
    96	### R.2 Output bundle layout
   158	```
   159	
   160	### R.4 The gate script
   161	
   162	`scripts/stress-gate.py` is the source of truth. It supports two
   163	modes, controlled by CLI flags:
   164	
   165	**Fast mode (`stress-gate-fast` — pre-merge):** only fast simulator
   166	scenarios. No doctrine review. No debt-inventory check.
   167	
   168	**Close mode (`stress-close-gate` — milestone close):** full battery
   169	plus doctrine review, debt-inventory, and grep gate.
   170	
   171	Pseudocode:
   172	
   173	```python
   174	def gate(perf_dir, scenarios=None, require_doctrines=None,
   175	         require_debt_clean=False, require_grep_clean=False):
   176	    fails = []
   177	    # Fast mode: ["S1","S2","S3","S5","S7","S8"]
   178	    # Close mode: ["S1","S2","S3","S5","S6","S7","S8","S9"]
   179	    # S4 is XCUITest-only; S10 deferred until M3+M4 complete
   180	    for scenario in (scenarios or ["S1","S2","S3","S5","S6","S7","S8","S9"]):
   181	        m = load(f"{perf_dir}/{scenario}/metrics.json")

exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening/ci-tiers.md | sed -n '1,60p;120,160p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# FFI hardening — CI tiers and M10.5 close protocol (§9)
     2	
     3	See [`ci.md`](./ci.md) for the local run recipes (`just stress`,
     4	`just stress-gate-fast`, `just stress-close-gate`) and the gate
     5	script pseudocode.
     6	
     7	---
     8	
     9	## §C. CI integration
    10	
    11	### C.1 Pre-merge tier (every PR)
    12	
    13	**Runner.** GitHub Actions `macos-14` (Apple Silicon, ~10 min budget).
    14	
    15	**Scenarios.** S1 (short — 60 s), S2 (30 s), S3 (30 s), S5 (30 s),
    16	S7 (full matrix), S8 (60 s). **Not S4** (iOS-main-thread, slow XCUITest
    17	boot) — runs nightly instead. **Not S6** (5 min) — runs nightly.
    18	**Not S9** (10 min) — nightly. **Not S10** (conditional on M3+M4).
    19	
    20	**Workflow.** `.github/workflows/stress-pre-merge.yml`:
    21	
    22	```yaml
    23	name: FFI stress (pre-merge)
    24	on:
    25	  pull_request:
    26	    paths:
    27	      - 'crates/nmp-core/**'
    28	      - 'crates/nmp-testing/**'
    29	      - 'ios/NmpStress/**'
    30	
    31	jobs:
    32	  stress-fast:
    33	    runs-on: macos-14
    34	    timeout-minutes: 15
    35	    steps:
    36	      - uses: actions/checkout@v4
    37	      - uses: dtolnay/rust-toolchain@stable
    38	      - run: just stress
    39	      - run: just stress-report
    40	      - run: just stress-gate-fast
    41	      - uses: actions/upload-artifact@v4
    42	        if: always()
    43	        with:
    44	          name: stress-pre-merge-${{ github.run_id }}
    45	          path: docs/perf/m10.5/
    46	```
    47	
    48	**Gating.** The `just stress-gate-fast` step exit code is the PR gate.
    49	Fast gate checks S1, S2, S3, S5, S7, S8 only — no doctrine review,
    50	no debt-inventory check (both are milestone-close artifacts only).
    51	
    52	### C.2 Nightly tier
    53	
    54	**Runner.** Mac mini self-hosted runner with an iPhone 12 wired
    55	in. ~90 min budget.
    56	
    57	**Scenarios.** All ten at full duration: S1 (10 min), S2 (60 s),
    58	S3 (10 emits × 100 k events), S4 (60 s × 12 stalls), S5 (30 s),
    59	S6 (1 000 cycles), S7 (full matrix), S8 (5 storms), S9 (10 min ×
    60	100/min), S10 (60 s suspend — *only if M3+M4 are complete; the
   120	`docs/design/firehose-bench.md` §5).
   121	
   122	### C.5 What does not block CI
   123	
   124	- **Sonnet-agent runs** are advisory. Flake by design; failures are
   125	  triaged manually. Two-or-more agents hitting the same failure in
   126	  one nightly = upgraded to a tracked bug.
   127	- **iPhone 12 hardware-only scenarios** (S9 device variant, S4 device
   128	  variant) skip if the device is detached/offline; the missing
   129	  results are noted in the report and the gate script
   130	  treats "device-absent" as a deferred-not-failed state.
   131	- **S10 if M3+M4 are not complete:** scenario reports as `skipped:
   132	  prereq` with a note in `metrics.json`; gate script omits S10
   133	  from nightly gate check until M3+M4 land. S10 is not used as
   134	  doctrine sign-off evidence for M10.5 (see gates.md §D1 note).
   135	
   136	---
   137	
   138	## §C.6 CI artifact retention
   139	
   140	| Tier | Retention | Notes |
   141	|---|---|---|
   142	| Pre-merge | 14 days | Per-PR; bulk delete |
   143	| Nightly | 90 days | Per-run; archived to S3 quarterly |
   144	| On-demand (RC) | indefinite | Release-attached artifact |
   145	
   146	Instruments traces are large (50–500 MiB per scenario). Pre-merge
   147	runs omit `--instruments` to stay within 15 min; only nightly + RC
   148	capture traces.
   149	
   150	---
   151	
   152	## §C.7 The M10.5 close protocol
   153	
   154	1. `just stress-close-gate` exits 0 — runs the full battery
   155	   (S1–S9, excluding S10 while M3+M4 are pending) plus doctrine
   156	   review, debt-inventory, and grep checks.
   157	2. `docs/perf/m10.5/debt-inventory.md` must-fix list = empty.
   158	3. §7.1 grep gate = 0 hits.
   159	4. Doctrine review (D0–D5) signed off in `doctrine-review.md`.
   160	5. iPhone 12 baseline = published in `iphone12-baseline.md` with no

exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening.md | sed -n '108,126p;238,256p;286,294p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   108	
   109	Ten scenarios — full table with assertions and numeric thresholds is in
   110	[`ffi-hardening/scenarios.md`](./ffi-hardening/scenarios.md). Summary:
   111	
   112	| # | Scenario | Threading concern | Primary doctrine tested |
   113	|---|---|---|---|
   114	| S1 | Mount/unmount churn | actor recv + refcount | D5 (snapshot bounded), bible #5 |
   115	| S2 | Dispatch flood | mpsc backpressure | bible #3 (fire-and-forget) |
   116	| S3 | Snapshot pressure | listener serialization | bible #9 (≤60 Hz), D5 |
   117	| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
   118	| S5 | Reentrancy | callback re-enters FFI | bible #3, deadlock-free |
   119	| S6 | Capability lifecycle storms | start/stop/restart | bible #7 (idempotent) |
   120	| S7 | Error-shape exhaustion | every invalid input path | D3 (no errors cross FFI) |
   121	| S8 | Subscription planner DOS | OpenView/CloseView storm | D2 (≤60 Hz/view), D5 |
   122	| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
   123	| S10 | Long suspend simulation | background → foreground | bible #1 (monotonic rev) |
   124	
   125	## 4. Harness architecture
   126	
   238	returns; M10.5 ships the interim toast-field bridge.
   239	
   240	## 8. Doctrine review checklist
   241	
   242	Full D0–D5 line-item-to-scenario mapping in
   243	[`ffi-hardening/gates.md`](./ffi-hardening/gates.md) §8. Headline:
   244	
   245	| Doctrine | Proven by |
   246	|---|---|
   247	| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
   248	| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
   249	| **D2** ≤60Hz/view, working-set bound | S2, S3, S8 — emit-rate cap, planner dedup |
   250	| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
   251	| **D4** one writer per fact | S1, S5 — refcount only mutated on actor thread; reentrancy under same single-writer rule |
   252	| **D5** snapshots bounded by what's open | S1 (refcount drives eviction) + S3 (full-state size scales with open views, not store) |
   253	
   254	## 9. CI integration
   255	
   256	Three tiers, defined in [`ffi-hardening/ci.md`](./ffi-hardening/ci.md):
   286	   as a supported pattern, or actively discouraged? Recommend: document
   287	   as supported (Swift will inevitably do it via Combine pipelines);
   288	   the actor's mpsc Sender is `Send + Sync` and the test proves it
   289	   works.
   290	5. **Handle registry for freed-pointer safety (M14).** Calling `nmp_app_*`
   291	   after `nmp_app_free` is undefined behavior; no handle registry exists
   292	   to make it recoverable. S7 excludes freed-pointer probes for this reason.
   293	   M14 UniFFI migration replaces raw pointers with typed handles, eliminating
   294	   the UB class. Until then, caller must not call after free.

exec
/bin/zsh -lc "nl -ba docs/perf/codex-reviews/2026-05-18-session-1.md | sed -n '1,20p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	> **Archival.** This file is the raw `codex exec` review transcript from session 1. Generated artifact; not subject to the 300/500-LOC limit. Future review files follow the same convention under `docs/perf/codex-reviews/`.
     2	
     3	Reading additional input from stdin...
     4	2026-05-17T22:25:40.547257Z ERROR codex_core::session: failed to load skill /Users/pablofernandez/.agents/skills/voice-capture-sheet/SKILL.md: invalid YAML: mapping values are not allowed in this context at line 2 column 116
     5	OpenAI Codex v0.129.0 (research preview)
     6	--------
     7	workdir: /Users/pablofernandez/Work/nostr-multi-platform
     8	model: gpt-5.5
     9	provider: openai
    10	approval: never
    11	sandbox: workspace-write [workdir, /tmp, $TMPDIR, /Users/pablofernandez/.codex/memories]
    12	reasoning effort: xhigh
    13	reasoning summaries: none
    14	session id: 019e380b-88c6-7481-a75b-c6b7cc0c162f
    15	--------
    16	user
    17	You are reviewing a session's worth of merges on master in the nostr-multi-platform repo. NMP is a Rust multiplatform framework for Nostr apps building toward v1 per docs/plan.md. Doctrine D0–D5 (docs/product-spec.md §1.5):
    18	- D0 kernel never grows app nouns
    19	- D1 best-effort rendering with placeholders
    20	- D2 reactivity contract (composite reverse index, ≤60 Hz/view, working-set bound)

exec
/bin/zsh -lc 'rg -n "13 symbols|14 symbols|symbols|Symbols|nmp_app_" docs/design/ffi-hardening.md docs/design/ffi-hardening/*.md docs/perf/m10.5/debt-inventory.md docs/perf/m10.5/debt-inventory-findings.md' in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc 'rg -n "D5|CapabilityModule|capability|snapshot bounded|snapshots bounded|S10|D1|stress-gate-fast|stress-close-gate|freed-pointer|use-after-free|NULL app|14 symbols|13 symbols" docs/design/ffi-hardening.md docs/design/ffi-hardening/*.md docs/perf/m10.5/debt-inventory.md docs/perf/m10.5/debt-inventory-findings.md' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
docs/design/ffi-hardening/gates.md:101:| Symbols × invalid-input variants exercised | `>=` | 70 (14 symbols × 5 variants avg) | 70 |
docs/design/ffi-hardening/scenarios-detail.md:17:`nmp_app_start → wait 100ms → nmp_app_stop → wait 100ms → nmp_app_start → wait 100ms`.
docs/design/ffi-hardening/scenarios-detail.md:45:**Setup.** For every `nmp_app_*` symbol that takes a `*const c_char`
docs/design/ffi-hardening/scenarios-detail.md:50:| `nmp_app_open_author` | NULL, "", " ", "not-hex", 63-char hex, 65-char hex, UTF-8 with non-hex chars |
docs/design/ffi-hardening/scenarios-detail.md:51:| `nmp_app_open_thread` | same shapes |
docs/design/ffi-hardening/scenarios-detail.md:52:| `nmp_app_open_firehose_tag` | NULL, "" (others valid; tag is unconstrained) |
docs/design/ffi-hardening/scenarios-detail.md:53:| `nmp_app_claim_profile` | NULL/empty/non-hex pubkey × {NULL, "", "valid"} consumer_id |
docs/design/ffi-hardening/scenarios-detail.md:54:| `nmp_app_release_profile` | same matrix; also: release without prior claim |
docs/design/ffi-hardening/scenarios-detail.md:55:| `nmp_app_close_author` / `_thread` | same |
docs/design/ffi-hardening/scenarios-detail.md:59:Calling any `nmp_app_*` with a dangling pointer after `nmp_app_free`
docs/perf/m10.5/debt-inventory-findings.md:17:pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
docs/perf/m10.5/debt-inventory-findings.md:28:// safe: caller guarantees app is a valid pointer allocated by nmp_app_new()
docs/design/ffi-hardening.md:51:The current FFI surface is **14 exported C symbols** in
docs/design/ffi-hardening.md:59:| 1 | `nmp_app_new` | `void * nmp_app_new(void)` | Returns a `Box::into_raw(NmpApp)`. Caller owns. Spawns 2 OS threads (actor + listener) + N relay-worker threads on `start`. Pointer is `Send` because internal `Sender`/`Mutex` are `Send`; **callers must not share the same pointer across threads without external synchronization**. | n/a (returns) |
docs/design/ffi-hardening.md:60:| 2 | `nmp_app_free(*mut NmpApp)` | `void nmp_app_free(void *app)` | Reclaims the box, sends `Shutdown` to actor, joins actor + listener threads inside `Drop`. **Idempotent on null.** Caller must guarantee no other thread is mid-call into the same pointer when this is invoked. | Early-return if null (ffi.rs:74). |
docs/design/ffi-hardening.md:61:| 3 | `nmp_app_set_update_callback` | `void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback cb)` | Replaces the registered `(context, fn)` pair. The `*mut c_void` context is **stored as `usize`** (ffi.rs:13–16) and dereferenced inside the listener thread — caller owns the context lifetime and **must clear the callback to null before freeing the context**. Passing `cb=None` clears registration. | Returns silently if app null or mutex poisoned (ffi.rs:87–96). |
docs/design/ffi-hardening.md:62:| 4 | `nmp_app_start` | `void nmp_app_start(void *app, uint events_per_second, uint visible_limit, uint emit_hz)` | Fire-and-forget. `events_per_second` is currently ignored (`_events_per_second`); kept for ABI stability. `visible_limit` clamped to `[1, 500]`; 0 → default 80. `emit_hz` clamped to `[1, 12]`; 0 → default 4. Spawns relay workers on first call. | Silent no-op on null (ffi.rs:107–108). |
docs/design/ffi-hardening.md:63:| 5 | `nmp_app_configure` | same shape as `_start` | Same clamping. Re-tunes a running actor. | Silent no-op on null. |
docs/design/ffi-hardening.md:64:| 6 | `nmp_app_stop` | `void nmp_app_stop(void *app)` | Sends `Stop`; actor closes relay workers, keeps actor + listener threads alive. Caller may call `_start` again. | Silent no-op on null. |
docs/design/ffi-hardening.md:65:| 7 | `nmp_app_reset` | `void nmp_app_reset(void *app)` | Closes relays, replaces the in-actor `Kernel` instance, reopens relays if running. | Silent no-op on null. |
docs/design/ffi-hardening.md:66:| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
docs/design/ffi-hardening.md:67:| 9 | `nmp_app_open_thread(*mut, *const c_char)` | `void nmp_app_open_thread(void *app, const char *event_id)` | UTF-8, 64-char hex event id. `is_hex_id`-validated. Same silent-drop on bad input. | Silent no-op. |
docs/design/ffi-hardening.md:68:| 10 | `nmp_app_open_firehose_tag(*mut, *const c_char)` | `void nmp_app_open_firehose_tag(void *app, const char *tag)` | UTF-8 tag value. No hex validation. Empty → silent no-op. | Silent no-op. |
docs/design/ffi-hardening.md:69:| 11 | `nmp_app_claim_profile(*mut, *const c_char, *const c_char)` | `void nmp_app_claim_profile(void *app, const char *pubkey, const char *consumer_id)` | Hex-pubkey-validated. `consumer_id` is an opaque caller-chosen string (used by the kernel refcount table). Two strings, two lifetime contracts: both must be valid for the duration of the call only — the kernel `String`-copies them. | Silent no-op on any null or invalid. |
docs/design/ffi-hardening.md:70:| 12 | `nmp_app_release_profile(*mut, *const c_char, *const c_char)` | mirror of `_claim_profile` | Same. **Pair invariant:** every `claim_profile(pk, id)` must be matched by exactly one `release_profile(pk, id)`; an unmatched `release` is silently dropped by the kernel refcount logic. | Silent no-op. |
docs/design/ffi-hardening.md:71:| 13 | `nmp_app_close_author(*mut, *const c_char)` | `void nmp_app_close_author(void *app, const char *pubkey)` | Closes the named author view. Different from `release_profile` — closes the *view*, not a refcounted profile claim. | Silent no-op. |
docs/design/ffi-hardening.md:72:| 14 | `nmp_app_close_thread(*mut, *const c_char)` | `void nmp_app_close_thread(void *app, const char *event_id)` | Closes the named thread view. | Silent no-op. |
docs/design/ffi-hardening.md:88:  `nmp_app_set_update_callback`. Per ffi.rs:13–16 it is round-tripped via
docs/design/ffi-hardening.md:92:- The callback **must not call back into any `nmp_app_*` function on the
docs/design/ffi-hardening.md:131:   surface directly via the same `nmp_app_*` C symbols Swift uses,
docs/design/ffi-hardening.md:218:bad input — `nmp_app_open_author(app, NULL)`, an empty string, or a
docs/design/ffi-hardening.md:290:5. **Handle registry for freed-pointer safety (M14).** Calling `nmp_app_*`
docs/design/ffi-hardening.md:291:   after `nmp_app_free` is undefined behavior; no handle registry exists
docs/design/ffi-hardening/harness.md:4:FFI symbols directly (fastest iteration). The XCUITest target exercises
docs/design/ffi-hardening/harness.md:15:caller thread ──────► │ nmp_app_*() FFI entry    │
docs/design/ffi-hardening/harness.md:115:    let app = unsafe { nmp_core_ffi::nmp_app_new() };
docs/design/ffi-hardening/harness.md:118:        nmp_core_ffi::nmp_app_set_update_callback(app, ctx, Some(sink_cb));
docs/design/ffi-hardening/harness.md:119:        nmp_core_ffi::nmp_app_start(app, 0, 80, 4);
docs/design/ffi-hardening/harness.md:133:            nmp_core_ffi::nmp_app_claim_profile(app, pk_c.as_ptr(), cn_c.as_ptr());
docs/design/ffi-hardening/harness.md:137:            nmp_core_ffi::nmp_app_release_profile(app, pk_c.as_ptr(), cn_c.as_ptr());
docs/design/ffi-hardening/harness.md:162:        nmp_core_ffi::nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
docs/design/ffi-hardening/harness.md:163:        nmp_core_ffi::nmp_app_free(app);
docs/design/ffi-hardening/harness.md:191:`nmp-core-ffi-decls` crate that re-exports the `extern "C"` symbols
docs/design/ffi-hardening/harness.md:197:    pub fn nmp_app_new() -> *mut std::ffi::c_void;
docs/design/ffi-hardening/harness.md:198:    pub fn nmp_app_free(app: *mut std::ffi::c_void);
docs/design/ffi-hardening/harness.md:199:    pub fn nmp_app_set_update_callback(
docs/design/ffi-hardening/scenarios.md:9:- "dispatch" = a single Swift→Rust call through any `nmp_app_*` symbol.
docs/design/ffi-hardening/scenarios.md:95:`nmp_app_configure` (which currently triggers `emit_now`) ten times

 succeeded in 0ms:
docs/design/ffi-hardening/ci.md:84:stress-gate-fast:
docs/design/ffi-hardening/ci.md:89:stress-close-gate:
docs/design/ffi-hardening/ci.md:91:        --require-doctrines D0,D1,D2,D3,D4,D5 \
docs/design/ffi-hardening/ci.md:122:├── ... (one dir per scenario through S10)
docs/design/ffi-hardening/ci.md:165:**Fast mode (`stress-gate-fast` — pre-merge):** only fast simulator
docs/design/ffi-hardening/ci.md:168:**Close mode (`stress-close-gate` — milestone close):** full battery
docs/design/ffi-hardening/ci.md:179:    # S4 is XCUITest-only; S10 deferred until M3+M4 complete
docs/perf/m10.5/debt-inventory-findings.md:167:## Doctrine Audits D0–D5
docs/perf/m10.5/debt-inventory-findings.md:178:### D1: Best-Effort Rendering with Placeholders — COMPLIANT
docs/perf/m10.5/debt-inventory-findings.md:213:### D5: Capabilities Report, Never Decide — COMPLIANT
docs/perf/m10.5/debt-inventory-findings.md:215:- iOS FFI bridge exposes capability requests as simple commands with no decision logic.
docs/perf/m10.5/debt-inventory-findings.md:218:- `CapabilityModule` trait defines how modules *report*, not how bridge decides.
docs/perf/m10.5/debt-inventory.md:30:| substrate/capability.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
docs/perf/m10.5/debt-inventory.md:58:Doctrine audits (D0–D5) in
docs/perf/m10.5/debt-inventory.md:70:| ProfileCard.placeholder (D1) | Design compliance | Correct D1 implementation; refinement in place. |
docs/perf/m10.5/debt-inventory.md:113:(D0–D5) are upheld.
docs/design/ffi-hardening/ci-tiers.md:4:`just stress-gate-fast`, `just stress-close-gate`) and the gate
docs/design/ffi-hardening/ci-tiers.md:18:**Not S9** (10 min) — nightly. **Not S10** (conditional on M3+M4).
docs/design/ffi-hardening/ci-tiers.md:40:      - run: just stress-gate-fast
docs/design/ffi-hardening/ci-tiers.md:48:**Gating.** The `just stress-gate-fast` step exit code is the PR gate.
docs/design/ffi-hardening/ci-tiers.md:60:100/min), S10 (60 s suspend — *only if M3+M4 are complete; the
docs/design/ffi-hardening/ci-tiers.md:81:      - run: just stress-gate-fast || echo "::warning::nightly gate failed"
docs/design/ffi-hardening/ci-tiers.md:131:- **S10 if M3+M4 are not complete:** scenario reports as `skipped:
docs/design/ffi-hardening/ci-tiers.md:132:  prereq` with a note in `metrics.json`; gate script omits S10
docs/design/ffi-hardening/ci-tiers.md:133:  from nightly gate check until M3+M4 land. S10 is not used as
docs/design/ffi-hardening/ci-tiers.md:134:  doctrine sign-off evidence for M10.5 (see gates.md §D1 note).
docs/design/ffi-hardening/ci-tiers.md:154:1. `just stress-close-gate` exits 0 — runs the full battery
docs/design/ffi-hardening/ci-tiers.md:155:   (S1–S9, excluding S10 while M3+M4 are pending) plus doctrine
docs/design/ffi-hardening/ci-tiers.md:159:4. Doctrine review (D0–D5) signed off in `doctrine-review.md`.
docs/design/ffi-hardening/gates.md:7:2. **§D** doctrine D0–D5 review checklist — each line item maps to
docs/design/ffi-hardening/gates.md:97:| Crashes on NULL app pointer calls | `==` | 0 | 0 |
docs/design/ffi-hardening/gates.md:101:| Symbols × invalid-input variants exercised | `>=` | 70 (14 symbols × 5 variants avg) | 70 |
docs/design/ffi-hardening/gates.md:126:### G-S10. Long suspend (conditional on M3+M4)
docs/design/ffi-hardening/gates.md:150:## §D. Doctrine D0–D5 review checklist
docs/design/ffi-hardening/gates.md:155:> **Note.** The task brief mentioned "D0–D5". The canonical list in
docs/design/ffi-hardening/gates.md:156:> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
docs/design/ffi-hardening/gates.md:159:> not redundantly re-prove — items beyond D0–D5 are covered by
docs/design/ffi-hardening/gates.md:166:- ✅ **Stress proof:** S6 (capability lifecycle storms) does
docs/design/ffi-hardening/gates.md:167:  1 000 start/stop/restart cycles; the kernel's capability set is
docs/design/ffi-hardening/gates.md:173:### D1. Best-effort rendering — render now, refine in place
docs/design/ffi-hardening/gates.md:187:  immediately from `debt-inventory.md` §3 D1 audit; no spinner, no
docs/design/ffi-hardening/gates.md:189:- 📝 **Sign-off:** doctrine-review.md § D1.
docs/design/ffi-hardening/gates.md:191:> **Note on S10.** S10 (long suspend) would provide additional D1
docs/design/ffi-hardening/gates.md:192:> evidence for the resume path, but S10 is conditional on M3+M4
docs/design/ffi-hardening/gates.md:194:> S10 is deferred to M11.5 or whenever M3+M4 land; it is not used as
docs/design/ffi-hardening/gates.md:195:> doctrine sign-off evidence here. D1 is signed off on S3 + S4 + code
docs/design/ffi-hardening/gates.md:242:### D5. Capabilities report, never decide
docs/design/ffi-hardening/gates.md:247:- ✅ **Code proof:** `CapabilityModule` trait (`substrate/capability.rs`)
docs/design/ffi-hardening/gates.md:248:  defines typed `Request`/`Result` pairs — modules *report* capability
docs/design/ffi-hardening/gates.md:252:- ✅ **Capability evidence (M10.5 surface):** The relay-role capability
docs/design/ffi-hardening/gates.md:259:  renders them. No capability module will acquire routing authority.
docs/design/ffi-hardening/gates.md:260:- ✅ **Stress proof:** S6 (capability lifecycle storms) — 1,000
docs/design/ffi-hardening/gates.md:261:  start/stop/restart cycles verify the relay capability lifecycle without
docs/design/ffi-hardening/gates.md:264:- 📝 **Sign-off:** doctrine-review.md § D5.
docs/design/ffi-hardening/gates.md:279:| D1 | PASS | S3 + S4 metrics.json + S3/screenshots (placeholder-then-refine path) | <name> | <date> |
docs/design/ffi-hardening/gates.md:283:| D5 | PASS | debt-inventory §3 D5 + S6 metrics.json + capability.rs code review | <name> | <date> |
docs/design/ffi-hardening/scenarios-detail.md:1:# FFI hardening — scenarios S6–S10
docs/design/ffi-hardening/scenarios-detail.md:14:**Setup.** For each capability handle (today: the relay role pair
docs/design/ffi-hardening/scenarios-detail.md:58:Note: freed-pointer (use-after-free) probes are **not** included.
docs/design/ffi-hardening/scenarios-detail.md:69:2. NULL app pointer calls: **silent no-op only** (early return in
docs/design/ffi-hardening/scenarios-detail.md:70:   `app_ref()`). No toast assertion for NULL app — there is no state
docs/design/ffi-hardening/scenarios-detail.md:146:## S10. Long suspend simulation — 60-second background
docs/design/ffi-hardening/scenarios-detail.md:150:only if M3+M4 are complete by then.** If not, S10 graduates to
docs/design/ffi-hardening/scenarios-detail.md:151:M11.5 and the M10.5 gate excludes it. S10 is **not** used as
docs/design/ffi-hardening/scenarios-detail.md:152:doctrine sign-off evidence for M10.5 (see gates.md §D1 note).
docs/design/ffi-hardening/scenarios-detail.md:180:**Numeric gate.** See gates.md §G-S10.
docs/design/ffi-hardening.md:19:   (`docs/product-spec/overview-and-dx.md` §1.5 D0–D5), and every ownership
docs/design/ffi-hardening.md:114:| S1 | Mount/unmount churn | actor recv + refcount | D5 (snapshot bounded), bible #5 |
docs/design/ffi-hardening.md:116:| S3 | Snapshot pressure | listener serialization | bible #9 (≤60 Hz), D5 |
docs/design/ffi-hardening.md:117:| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
docs/design/ffi-hardening.md:121:| S8 | Subscription planner DOS | OpenView/CloseView storm | D2 (≤60 Hz/view), D5 |
docs/design/ffi-hardening.md:123:| S10 | Long suspend simulation | background → foreground | bible #1 (monotonic rev) |
docs/design/ffi-hardening.md:176:├── doctrine-review.md       # D0–D5 sign-off (M10.5 exit-gate artifact)
docs/design/ffi-hardening.md:242:Full D0–D5 line-item-to-scenario mapping in
docs/design/ffi-hardening.md:247:| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
docs/design/ffi-hardening.md:248:| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
docs/design/ffi-hardening.md:252:| **D5** snapshots bounded by what's open | S1 (refcount drives eviction) + S3 (full-state size scales with open views, not store) |
docs/design/ffi-hardening.md:261:| **Nightly** | daily | All ten, S1 full 10-min, S4 250 ms stalls × 60, S9 100 flap, S10 60s suspend | Mac mini self-hosted runner, iPhone 12 device |
docs/design/ffi-hardening.md:290:5. **Handle registry for freed-pointer safety (M14).** Calling `nmp_app_*`
docs/design/ffi-hardening.md:292:   to make it recoverable. S7 excludes freed-pointer probes for this reason.
docs/design/ffi-hardening/harness.md:65:│   └── long_suspend.rs      # S10 (conditional on M3+M4)
docs/design/ffi-hardening/harness.md:86:  long-suspend         S10 (skipped unless --experimental-suspend)
docs/design/ffi-hardening/harness.md:232:└── S10LongSuspend.swift            # conditional on M3+M4
docs/design/ffi-hardening/scenarios.md:185:Scenarios S6–S10 (capability storms, error exhaustion, planner DOS,

exec
/bin/zsh -lc 'git diff --check fb139ab''^ fb139ab -- docs/design/ffi-hardening.md docs/design/ffi-hardening docs/perf/m10.5/debt-inventory.md docs/perf/m10.5/debt-inventory-findings.md docs/perf/codex-reviews/2026-05-18-session-1.md' in /Users/pablofernandez/Work/nostr-multi-platform
 exited 2 in 0ms:
docs/perf/m10.5/debt-inventory-findings.md:12: trailing whitespace.
+**File:** `crates/nmp-core/src/ffi.rs:73–79`  
docs/perf/m10.5/debt-inventory-findings.md:13: trailing whitespace.
+**Severity:** Medium (correct code, documentation gap)  
docs/perf/m10.5/debt-inventory-findings.md:36: trailing whitespace.
+**File:** `crates/nmp-core/src/ffi.rs:271–277`  
docs/perf/m10.5/debt-inventory-findings.md:57: trailing whitespace.
+**File:** `crates/nmp-core/src/ffi.rs:279–290`  
docs/perf/m10.5/debt-inventory-findings.md:77: trailing whitespace.
+**File:** `crates/nmp-core/src/relay_worker.rs:240–245`  
docs/perf/m10.5/debt-inventory-findings.md:113: trailing whitespace.
+**Files:** `kernel/update.rs:43–56` (5 casts), `kernel/nostr.rs:85` (1 cast).  
docs/perf/m10.5/debt-inventory-findings.md:125: trailing whitespace.
+**Files:** `kernel/ingest.rs:13,20`, `kernel/requests.rs:572`.  
docs/perf/m10.5/debt-inventory.md:124: trailing whitespace.
+items above are tracked.  

context compacted
codex
No, not cleanly.

All 6 targeted fixes are present in the intended detailed docs: D5 in [gates.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:242), CI split in [ci.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:83), freed-pointer exclusion in [scenarios-detail.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/scenarios-detail.md:58), S10 excluded from D1 evidence in [gates.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:191), LOC/header fixed, and 14 symbols in [ffi-hardening.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening.md:51).

New/residual issues:
- [ffi-hardening.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening.md:248) still says D1 evidence is `S3 + S10`, contradicting the fix.
- [ffi-hardening.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening.md:252) still defines D5 as snapshot-bounded, not capabilities-report-never-decide.
- `git diff --check fb139ab^ fb139ab` fails: trailing whitespace in [debt-inventory-findings.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory-findings.md:12) and [debt-inventory.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:124).
tokens used
277,663
No, not cleanly.

All 6 targeted fixes are present in the intended detailed docs: D5 in [gates.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:242), CI split in [ci.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:83), freed-pointer exclusion in [scenarios-detail.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/scenarios-detail.md:58), S10 excluded from D1 evidence in [gates.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:191), LOC/header fixed, and 14 symbols in [ffi-hardening.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening.md:51).

New/residual issues:
- [ffi-hardening.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening.md:248) still says D1 evidence is `S3 + S10`, contradicting the fix.
- [ffi-hardening.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening.md:252) still defines D5 as snapshot-bounded, not capabilities-report-never-decide.
- `git diff --check fb139ab^ fb139ab` fails: trailing whitespace in [debt-inventory-findings.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory-findings.md:12) and [debt-inventory.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:124).
