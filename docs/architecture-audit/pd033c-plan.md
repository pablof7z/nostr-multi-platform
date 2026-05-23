# PD-033-C — Dual Subscription System D4 Violation: Architectural Fix Plan

**Status:** Decision plan (pre-implementation).
**Scope:** `crates/nmp-core/src/kernel/` + `crates/nmp-core/src/subs/`.
**Decision (already taken in BACKLOG.md V-04):** **Option A** — designate
`InterestRegistry` canonical, migrate every `req()` / `req_for_relay()` caller
to it, delete the hand-rolled path.

This document fills in the *how*: where the violation actually lives, how to
stage the migration, what is at risk, and what "done" looks like.

---

## 1. The two systems — and where the D4 violation actually lives

### 1.1 What each system does

**System #1 — M1 hand-rolled `req()` / `req_for_relay()`**
File: `crates/nmp-core/src/kernel/requests/mod.rs`.
- `Kernel::req(role, sub_id, summary, filter) -> Vec<OutboundMessage>` (line 123)
  fans out one frame per configured bootstrap URL for that `RelayRole`.
- `Kernel::req_for_relay(role, relay_url, sub_id, summary, filter) -> OutboundMessage`
  (line 158) builds a single REQ frame addressed to one URL **and** writes the
  `WireSub` row into `self.wire.subs` (lines 176–190).
- Callers are kernel-internal request builders: `startup.rs`,
  `requests/profile.rs`, `requests/thread.rs`, `discovery.rs`.
- The actor drains these via `Kernel::pending_view_requests()`
  (`kernel/requests/mod.rs:46`) which is hand-built from a list of "is this
  view pending?" flags (`author_view.request_pending`, etc.).

**System #2 — `InterestRegistry` (M2/M8 lifecycle)**
Files: `crates/nmp-core/src/subs/registry.rs`,
`crates/nmp-core/src/subs/lifecycle.rs`,
`crates/nmp-core/src/subs/wire.rs`,
`crates/nmp-core/src/subs/handlers.rs`,
`crates/nmp-core/src/subs/recompile.rs`.
- `InterestRegistry::ensure_sub(identity, interest)` /
  `set_sub(identity, interest)` / `drop_owner(&identity)` / `push(interest)` /
  `withdraw(id)` (`subs/registry.rs:67–145`).
- Keyed by the `(SubOwnerKey, SubKey, SubScope)` triple from
  `docs/design/nostrdb-notedeck-lessons.md` §3.2 with owner-refcount dedup.
- Driven by `SubscriptionLifecycle::drain_tick(&mailboxes)`
  (`subs/handlers.rs`, surface), which calls `recompile_and_diff(&mailboxes)`
  (`subs/recompile.rs`) → produces a `Vec<WireFrame>` against the last plan.
- The actor converts those frames at
  `crates/nmp-core/src/actor/outbound.rs:43` via
  `wire_frames_to_outbound()`, which **also** writes `self.wire.subs` (via
  `Kernel::register_planner_wire_frames`, `kernel/requests/mod.rs:259-311`).
- Live callers today (production paths):
  - `crates/nmp-core/src/kernel_action.rs:97-98` — `OpenUri` reducer registers
    interests via `ensure_sub` (the only "by-the-book" caller).
  - `crates/nmp-core/src/actor/dispatch.rs:951` — `ActorCommand::PushInterest`
    → `registry_mut().push(interest)`.
  - `crates/nmp-core/src/actor/dispatch.rs:960` — `ActorCommand::WithdrawInterest`
    → `registry_mut().withdraw(&id)`.
  - `crates/nmp-core/src/kernel/discovery.rs:118-120` and `:148-150` —
    `oneshot.request(registry, …)`.

### 1.2 The D4 violation, named precisely

The doubly-written fact is **`Kernel.wire.subs: HashMap<(CanonicalRelayUrl, String), WireSub>`** —
the kernel's authoritative bookkeeping for "what wire-subs are live."

Two functions insert into that map:

- **Writer #1 (M1):** `Kernel::req_for_relay` at
  `crates/nmp-core/src/kernel/requests/mod.rs:176-190` — `self.wire.subs.insert(...)`.
- **Writer #2 (M2):** `Kernel::register_planner_wire_frames` at
  `crates/nmp-core/src/kernel/requests/mod.rs:284-298` — `self.wire.subs.insert(...)`.

That violates D4: one fact, two writers, two state machines pushing into the
same map under different sub-id namespaces, different `state` initial values
(M1 stamps `"opening"` *or* `"auth_paused"` based on `relay_auth_paused(role)`
at line 175; M2 hardcodes `"opening"` at line 291), and different lifecycle
discipline (M1 evicts via `close_subscriptions_with_prefixes` matching string
prefixes; M2 evicts via `WireFrame::Close` against the canonical key).

The kernel itself documents the coexistence at
`crates/nmp-core/src/kernel/mod.rs:382-391` and the M8 plan freezes it as
intentional ("the two paths coexist until M11 begins migrating … onto
`LogicalInterest`," `docs/plan/m8-subscription-lifecycle.md:95-96`).

### 1.3 Smoking gun

`crates/nmp-core/src/kernel/discovery.rs::drain_unknown_oneshots` registers in
**both systems in the same arm:**

```text
discovery.rs:118-129   (events oneshot)
  let token = self.oneshot.request(registry, …);     // System #2 write
  out.extend(self.req(RelayRole::Content, &sub_id,  // System #1 write
                       …, json!({ "ids": batch, … })));

discovery.rs:148-159   (profiles oneshot)
  let token = self.oneshot.request(registry, …);     // System #2 write
  out.extend(self.req(RelayRole::Indexer, …));        // System #1 write
```

The file-level comment at `discovery.rs:8-15` admits this explicitly: "the
oneshot registry registration is the forward-looking half, the `req()`
emission is what fetches today." That is the textbook description of a D4
violation: two systems share the same fact, neither is authoritative.

---

## 2. Which system is canonical, and why

### 2.1 Verdict — Option A: `InterestRegistry` is canonical

The architectural answer is **Option A** without ambiguity, and it is the
choice already documented in `docs/BACKLOG.md` V-04 line 81 and PD-033-C
line 210. Adopting it formally here:

- **`InterestRegistry` is the single writer of "what is subscribed."**
- **`Kernel::register_planner_wire_frames` is the single writer of
  `self.wire.subs`.**
- `req()`, `req_for_relay()`, `pending_view_requests()`, `defer_outbound`,
  `partition_auth_paused`, `close_subscriptions_with_prefixes`, the
  `request_pending` flags, the `profile_requests` / `thread_view` /
  `author_view` / `diagnostic_firehose` per-view state machines and all
  `ViewInterest.refcount` fields are deleted.

### 2.2 Why Option A wins on the merits

1. **It is what the design documents already say.** `m8-subscription-lifecycle.md`
   §3 names `InterestRegistry` "the single writer of the active-interest set
   (D4)." `subs/registry.rs:1-23` repeats it. The kernel itself calls the
   coexistence "until M11 migrates view modules onto `LogicalInterest`"
   (`kernel/mod.rs:387`). M1 was always a transition shim.
2. **It is the only system that expresses owner-refcount dedup natively.**
   `InterestRegistry::ensure_sub` + `drop_owner` already implement the
   `(SubOwnerKey, SubKey, SubScope)`-keyed refcount pattern that M1
   reinvents poorly across `ViewInterest.refcount`, `profile_requests.requested`,
   `thread_view.requested_ids`, `thread_view.requested_reply_targets`,
   `diagnostic_firehose.interest.refcount`. Migrating to it deletes ~5
   parallel refcount implementations.
3. **It is already coupled to the planner.** Routing decisions (NIP-65
   outbox, indexer fallback, `relay_pin`, `p_tag_routing`) live in
   `planner/` and are exercised by `recompile_and_diff`. Today M1 callers
   replicate routing inline (`author_indexer_relays`, `author_write_relays`,
   `partition_ids_by_author_write_relays`, `recipient_read_relays`,
   `bootstrap_discovery_relays`) — direct violations of D3 (one routing
   table). Migrating to `InterestRegistry` collapses these into one.
4. **It already won once.** T140 retired the original M1 follow-feed
   (`seed-timeline-*`) by migrating it onto `LogicalInterest`. The test
   `t140_m1_retirement_tests.rs` proves the migration pattern works
   end-to-end. No new doctrine; just repeat the proven recipe for the
   remaining call sites.
5. **Option B (designate `req()` canonical) is incompatible with the rest of
   the substrate.** `KernelAction::OpenUri` (`kernel_action.rs:42`) already
   registers via `ensure_sub`. View modules and the negentropy `coverage_hook`
   work against `CompiledPlan`. Keeping M1 means rolling back the substrate
   thesis and rewriting two ADRs (`0012-relay-pinned-interest-and-third-routing-lane.md`
   and `m8-subscription-lifecycle.md`). It is strictly more work than Option A
   for a strictly worse outcome.
6. **Option C (rewrite both) is forbidden by AGENTS.md.** "Zero-tolerance on
   hacks, debt, and fragmentation … one canonical representation and one code
   path." We have a canonical representation. The question is migration, not
   re-invention.

### 2.3 What we are *not* relitigating

This plan does not redesign `LogicalInterest`, `InterestShape`, or the
planner. If a specific routing pattern turns out to be inexpressible (see
§4.3) the implementation agent will surface it as a separate PR that extends
the planner; this plan only enumerates the migration path against the
planner-as-it-exists.

---

## 3. Migration plan

### 3.1 Call-site inventory (production paths only)

Every line below is a current `req()` / `req_for_relay()` call site that must
move to `InterestRegistry::ensure_sub` (or `set_sub` when the filter shape
mutates over the slot's lifetime). Tests are listed separately in §3.3.

| # | File:line | Today | Target `LogicalInterest` |
|---|-----------|-------|--------------------------|
| 1 | `kernel/requests/startup.rs:32` | `req(Indexer, "profile-target", …, kind:0 author=self limit:1)` | `InterestShape::profile_for(self_pk)` narrowed to `kinds={0}, limit=1`; scope = `Account(self_pk)`; lifecycle = `OneShot`. |
| 2 | `kernel/requests/startup.rs:38` | `req(Indexer, "target-relays", kind:10002 author=self limit:1)` | shape `kinds={10002}, authors={self_pk}, limit=1`; scope `Account(self_pk)`; lifecycle `OneShot`. |
| 3 | `kernel/requests/startup.rs:44` | `req(Indexer, "self-dm-relays", kind:10050 author=self limit:1)` | shape `kinds={10050}, authors={self_pk}, limit=1`; scope `Account(self_pk)`; lifecycle `OneShot`. |
| 4 | `kernel/requests/startup.rs:50` | `req(Indexer, "self-contacts", kind:3 author=self limit:1)` | shape `kinds={3}, authors={self_pk}, limit=1`; scope `Account(self_pk)`; lifecycle `OneShot`. |
| 5 | `kernel/requests/startup.rs:64` | `req(Content, "self-zap-receipts", kind:9735 #p=self limit:50)` | shape `kinds={9735}, tags={p: {self_pk}}, limit=50, p_tag_routing=Nip17DmRelays`-equivalent **OR** `PTagRouting::Nip65ReadRelays`; scope `Account(self_pk)`; lifecycle `Tailing`. |
| 6 | `kernel/requests/profile.rs:252` (`firehose_requests`) | `req_for_relay(Content, recipient_read_relay, "diag-firehose-{seq}-{tag}", kind:1 #t=tag limit:500)` | shape `kinds={1}, tags={t: {tag}}, limit=500, p_tag_routing=Nip65ReadRelays`; scope `ActiveAccount`; lifecycle `Tailing`. Owner key = `("diag-firehose", tag)`. |
| 7 | `kernel/requests/profile.rs:305` (`pending_profile_claim_requests`, batched) | `req_for_relay(Indexer, per_relay_url, "profile-batch-…", kind:0 authors=[…] limit=n)` | One `LogicalInterest` per claimed author: shape `kinds={0}, authors={pk}, limit=1`; scope `Account(pk)`; lifecycle `OneShot`. Planner merges by `Rule 2` author-set union → one batched REQ per write relay (M3 outbox direction). |
| 8 | `kernel/requests/profile.rs:331` (`profile_claim_request`, single) | as above, single | same shape; `ensure_sub` with owner = `("profile-claim", consumer_id)`. |
| 9 | `kernel/requests/profile.rs:370` (`author_requests`, kind:10002) | `req_for_relay(Indexer, bootstrap, "author-relays-…", kind:10002 author=pk limit:1)` | shape `kinds={10002}, authors={pk}, limit=1`; scope `Account(pk)`; lifecycle `OneShot`; **routing is bootstrap-discovery** — planner extension may be needed (see §4.3). |
|10 | `kernel/requests/profile.rs:377` (`author_requests`, kind:0) | `req_for_relay(Indexer, bootstrap, "author-profile-…", kind:0 author=pk limit:1)` | shape `kinds={0}, authors={pk}, limit=1`; scope `Account(pk)`; lifecycle `OneShot`. |
|11 | `kernel/requests/profile.rs:387` (`author_requests`, kinds:1,6) | `req_for_relay(Content, write_relay, "author-notes-…", kinds:[1,6] author=pk limit:100)` | shape `kinds={1,6}, authors={pk}, limit=100`; scope `Account(pk)`; lifecycle `Tailing`. Owner key = `("author-view", pk)`. |
|12 | `kernel/requests/thread.rs:178` (thread ids hydration) | `req_for_relay(Content, partition_relay, "thread-ids-…", ids=[…] limit=20)` | shape `event_ids={…}, limit=20`; scope `Global`; lifecycle `OneShot`. One `LogicalInterest` per id; planner merges by `Rule X` event-id union, partitions by `partition_ids_by_author_write_relays`. May require planner extension (see §4.3). |
|13 | `kernel/requests/thread.rs:214` (recursive replies) | `req_for_relay(Content, partition_relay, "thread-replies-…", kinds:[1,6] #e=[…] limit=200)` | shape `kinds={1,6}, tags={e: {…}}, limit=200`; scope `Global`; lifecycle `Tailing` (replies arrive over time). |
|14 | `kernel/discovery.rs:124` (events oneshot) | `req(Content, oneshot-disc-{token}, ids=[…] limit=n)` | already a `LogicalInterest` via `oneshot.request(registry, …)` at line 118 — **delete the `self.req(…)` call**; planner will emit the matching WireFrame on the next drain. |
|15 | `kernel/discovery.rs:154` (profiles oneshot) | `req(Indexer, oneshot-disc-{token}, kinds:[0,3,10002] authors=[…] limit=n*3)` | same — **delete `self.req(…)`** at line 154; the `OneShot` lifecycle on the registered interest does the wire emit. |

Net count: **15 production call sites** + 6 view-state structures
(`ViewInterest`, `profile_requests`, `thread_view`, `author_view`,
`diagnostic_firehose`, `deferred_outbound`) to retire. Estimate: **~600–900
lines deleted**, **~200–300 lines added** (mostly per-call-site
`LogicalInterest` construction + `ensure_sub`/`drop_owner` wiring).

### 3.2 View-state coupling — the hidden cost

The M1 callers are NOT just "call `req()`". They each carry parallel
refcount + pending/requested state that the registry already implements:

- `ViewInterest { key, refcount }` (`author_view`, `diagnostic_firehose`) ⇒
  replaced by `InterestRegistry`'s owner refcount.
- `profile_requests.requested: HashSet<Pubkey>` + `.pending: HashSet<Pubkey>` ⇒
  replaced by registry slot presence and `OneShot` lifecycle.
- `thread_view.requested_ids` / `.pending_ids` / `.requested_reply_targets` /
  `.pending_reply_targets` / `ids_inflight` / `replies_inflight` ⇒ replaced
  by `ensure_sub` idempotency + `OneShot` lifecycle.
- `profile_claims: HashMap<Pubkey, BTreeSet<ConsumerId>>` (the
  `MAX_CLAIMS_PER_PUBKEY` cap at `kernel/requests/profile.rs:127`) ⇒
  replaced by per-consumer-owner attachment under one `(profile-claim, pk)`
  slot; the cap becomes a cap on slot owners.
- `deferred_outbound: VecDeque<OutboundMessage>` +
  `partition_auth_paused` ⇒ replaced by `SubscriptionLifecycle::auth_gate`
  (which already has a pending-REQ buffer; see `subs/auth_gate.rs`).

The migration succeeds only if these are deleted with their M1 producers. A
half-migration that leaves the refcount state behind re-introduces D4 at a
different layer.

### 3.3 Test inventory

Tests that must be updated (asserting on M1 sub-id prefixes or `req()` /
`req_for_relay()` directly):

- `crates/nmp-core/src/kernel/auth_tests.rs:55` — `kernel.req(…)` direct call.
- `crates/nmp-core/src/kernel/auth_tests.rs:61, 253, 315, 393, 454` —
  `kernel.partition_auth_paused(…)`.
- `crates/nmp-core/src/kernel/closed_classifier_tests.rs:229, 277` —
  `kernel.req_for_relay(…)`.
- `crates/nmp-core/src/kernel/eose_ok_notice_ingest_tests.rs:43` (doc-string
  only).
- `crates/nmp-core/src/kernel/retention_tests.rs:401, 437, 465, 472, 479,
  527, 534` — `kernel.req_for_relay(…)`.
- `crates/nmp-core/src/kernel/replay_tests.rs:67, 70, 90` — replay calls
  `kernel.req_for_relay`; the production code path
  `crates/nmp-core/src/kernel/replay.rs:102` does too.
- `crates/nmp-core/src/kernel/outbox_tests.rs:599` —
  `kernel.defer_outbound(…)`.
- `crates/nmp-core/src/kernel/auth_fail_closed_tests.rs:29, 80` —
  `partition_auth_paused(…)`.
- `crates/nmp-core/src/kernel/requests/startup.rs::tests` (lines 100–266) —
  asserts on `self-dm-relays`, `profile-target`, `target-relays`,
  `self-contacts`, `self-zap-receipts` sub-ids in REQ frames. Either preserve
  sub-id naming via `SubKey::builder` payloads at registration sites OR
  rewrite the assertions against `(authors, kinds, tags)` filter-shape
  matchers. **Recommendation: rewrite the assertions** — sub-id is wire-private
  and shouldn't appear in semantic tests.
- `crates/nmp-core/src/kernel/t140_m1_retirement_tests.rs` — already proves
  the retirement pattern; extend its `seed-timeline-*` ban to all retired
  prefixes (`author-profile-`, `author-notes-`, `author-relays-`,
  `thread-ids-`, `thread-replies-`, `profile-claim-`, `profile-batch-`,
  `diag-firehose-`, `self-dm-relays`, `self-contacts`, `self-zap-receipts`,
  `oneshot-disc-`).

Tests asserting on observable behaviour (an event is ingested, an EOSE
transitions a sub to `live`, an `unknown_id` is resolved) should be
**unchanged** — that's the regression net the migration relies on.

### 3.4 The `wire.subs` consolidation

The single concrete code change that turns "two writers" into "one writer" is
deleting `self.wire.subs.insert(…)` from `Kernel::req_for_relay`
(`kernel/requests/mod.rs:176-190`) and routing every wire-sub insertion
through `Kernel::register_planner_wire_frames`
(`kernel/requests/mod.rs:259-311`). After Stage 6 (§5), `req_for_relay` and
`req` cease to exist; `register_planner_wire_frames` is the sole writer.

---

## 4. Risks

### 4.1 Behavioural deltas the migration must preserve

- **AUTH-paused initial state.** M1 `req_for_relay:175-183` stamps
  `state: "auth_paused"` when `relay_auth_paused(role)` is true at REQ-emission
  time. M2 `register_planner_wire_frames:283` hardcodes `state: "opening"`.
  The migration must teach `register_planner_wire_frames` (or its caller) to
  consult `relay_auth_paused` per-frame, OR move auth-pause stamping into the
  `AuthGate` already held by `SubscriptionLifecycle`
  (`subs/auth_gate.rs`). **This is a pre-existing latent gap unmasked by the
  migration, not a regression introduced by it** — flag it for the
  implementation agent.
- **`defer_outbound` queue replacement.** The cap of 64 deferred frames
  (`kernel/requests/mod.rs:204-206`) and its drain order via
  `pending_view_requests` must move to `AuthGate`'s pending buffer (`subs/
  auth_gate.rs`). If `AuthGate` does not already enforce a cap, the
  implementation must add it (D8: no unbounded queues).
- **Sub-id naming churn.** Diagnostic surfaces (relay log, S3 harness, iOS
  diagnostic panel) display sub-ids. If `SubKey::builder` produces an opaque
  hash-suffixed sub-id, those displays will change. Acceptable churn, but
  worth a heads-up to the iOS team — pin the new prefixes in the diagnostics
  test if there is one.
- **Profile-claim drop counter `claim_drops_total`.** The
  `MAX_CLAIMS_PER_PUBKEY` overflow path (`profile.rs:127-132`) bumps a
  diagnostic counter surfaced as `Metrics::claim_drops_total`. After
  migration the equivalent overflow is "too many owners on one slot"; the
  registry doesn't have this cap. Either (a) add an owner-cap parameter to
  `InterestRegistry::ensure_sub`, or (b) keep the cap check in the caller
  before `ensure_sub`. **Decision deferred to implementation** — both are
  one-line wraps; (b) preserves the registry's purity and is preferred.

### 4.2 Tests that cover the current behaviour (regression net)

- `t140_m1_retirement_tests.rs` — proves the seed-timeline retirement
  pattern and is the template for asserting other M1 prefixes never re-emit.
- `eose_ok_notice_ingest_tests.rs` — covers the EOSE handler against
  canonical-URL keying; the migration preserves this by routing all inserts
  through `register_planner_wire_frames`.
- `outbox_tests.rs` — D3 outbox routing; sensitive to any change in which
  relay serves which author. The planner already implements outbox routing
  for `LogicalInterest`s, so equivalence should hold.
- `auth_tests.rs`, `auth_fail_closed_tests.rs` — the auth gate; sensitive to
  the `defer_outbound` / `AuthGate` consolidation called out in §4.1.
- `replay_tests.rs` + `replay.rs:102` — replay calls `kernel.req_for_relay`
  directly. After migration, replay must call `register_planner_wire_frames`
  (or a small test-only helper that wraps it). Production `replay.rs` is the
  load-bearing change here, not just tests.
- `t142_drain_lifecycle_tick_tests.rs` — already exercises the
  `drain_lifecycle_tick` path end-to-end.
- `retention_tests.rs` — covers store retention; tests that build wire-subs
  manually via `req_for_relay` must switch to constructing them via
  `LogicalInterest` registration.

### 4.3 Planner expressiveness gaps to verify before declaring done

The implementation agent must verify the planner can express each of the
following (or document the extension needed as its own PR):

- **Per-author write-relay partitioning over an `event_ids` shape.**
  `thread.rs::partition_ids_by_author_write_relays` (lines 174, 210) routes
  per-id by the *event-id's author's* NIP-65 write relays. The planner today
  routes per-author for `authors`-keyed shapes; an `event_ids`-keyed shape's
  routing path needs review.
- **Bootstrap fan-out to ALL configured indexer URLs.** `startup.rs`'s
  `req(RelayRole::Indexer, …)` fans out one frame per configured
  bootstrap URL (`bootstrap_urls_for_role`). The planner does this for
  author-scoped interests via NIP-65, but bootstrap REQs are
  *pre-NIP-65* (the kind:10002 fetch itself is one of them, so NIP-65 is
  unknown). Confirm the planner has a "cold-start: fan to bootstrap
  discovery seeds" path, or extend it.
- **`p_tag_routing` for `#p`-targeted REQs.** The zap-receipts REQ (kind:9735
  `#p=self`) targets *the recipient's read relays*. The planner already
  has `PTagRouting` (`crates/nmp-core/src/planner/interest.rs:175`) — confirm
  this is the right variant.
- **`relay_pin` for `firehose_requests`.** The firehose targets
  `recipient_read_relays(active_account)`. Confirm `PTagRouting::Nip65ReadRelays`
  or equivalent routes there; otherwise use `relay_pin` per resolved URL.

If any of these requires a planner extension, scope it as a **separate PR**
inserted before the migration stage that depends on it. Do NOT widen this
plan's scope into planner internals.

### 4.4 Out-of-tree risk

PR #56 (action layer) and ADR-0027 (unified action module trait) demonstrate
that staged structural migrations in this codebase can stall when batched
too large. The 7-stage layout in §5 is deliberately fine-grained so each PR
is reviewable, CI-gateable, and revertible.

---

## 5. PR structure — staged, not monolithic

**One PR is wrong.** The blast radius is too large (15 call sites + 6 state
structures + ~30 tests across 12 files) and the V-04 entry has been pending
since 2026-05-20 specifically because no contributor could safely land it as
a single change. Stage it as follows; each stage is its own PR, each merges
green to master before the next begins.

### Stage 0 — `wire.subs` insert consolidation (PURE REFACTOR)

**Goal:** make `self.wire.subs.insert` syntactically appear in exactly one
location, *before any behaviour changes.*

- Extract a `Kernel::insert_wire_sub(role, relay_url, sub_id, summary,
  initial_state)` helper that performs the canonicalization + insert (the
  body currently shared between `req_for_relay:166-190` and
  `register_planner_wire_frames:271-298`).
- Replace both insert sites with calls to the helper.
- CI gate: `grep -c 'wire.subs.insert' crates/nmp-core/src/kernel/ == 1` (in
  `insert_wire_sub` body).

**Why first:** turns "two writers" into "two callers of one writer" before
the harder migration starts. If anything breaks, the diff is tiny.

### Stage 1 — Discovery oneshots (smallest blast radius)

**Goal:** delete the double-write in `drain_unknown_oneshots`.

- `kernel/discovery.rs:124, 154`: delete `self.req(…)`. The
  `oneshot.request(registry, …)` already registers the interest; the
  planner's next `drain_tick` emits the WireFrame.
- Verify in `actor/mod.rs:1273` that `drain_lifecycle_tick` is called every
  actor tick (it is).
- Tests: extend `t140_m1_retirement_tests` with an assertion that no
  `oneshot-disc-*` sub-id is emitted via M1 paths.

**Why second:** smallest production impact; already half-migrated (registry
side exists); failure mode is "discovery doesn't resolve unknown ids,"
which is loud and CI-detectable via
`crates/nmp-core/src/kernel/discovery_tests.rs`.

### Stage 2 — Bootstrap REQs (`startup.rs`)

**Goal:** migrate the 5 self-bootstrap REQs (#1–#5 in §3.1).

- Replace the 5 `req(…)` calls in `active_account_bootstrap_requests` with
  5 `lifecycle_mut().registry_mut().ensure_sub(identity, interest)` calls
  bracketed by `enqueue_trigger(CompileTrigger::InvalidateCompile{…})`.
- Delete `active_account_bootstrap_requests`'s caller in
  `startup_requests`; replace with a single trigger enqueue.
- May require planner extension for bootstrap-cold-start fan-out (see §4.3);
  if so, land that planner PR first.
- Update `startup.rs::tests` to assert against `iter_active()` /
  resolved WireFrames, not against `req()` outputs.

**Why third:** 5 well-tested REQs with no view-state coupling; clean
self-contained PR.

### Stage 3 — Profile claims (`profile.rs::claim/release/firehose/pending`)

**Goal:** migrate #6–#8 from §3.1 and retire the `profile_requests` /
`profile_claims` state machines.

- Replace `claim_profile` body with `ensure_sub(("profile-claim",
  consumer_id), LogicalInterest{ shape: profile_for(pk), scope: Account(pk),
  lifecycle: OneShot, … })`.
- Replace `release_profile` body with `drop_owner(…)`.
- Replace `firehose_requests` with a single `ensure_sub` on
  `diagnostic_firehose.interest` change.
- Delete `profile_requests: ProfileRequestState`, `profile_claims`,
  `pending_profile_claim_requests`, `profile_claim_request`,
  `claim_drops_total` (or keep the counter, see §4.1).
- Decide on `MAX_CLAIMS_PER_PUBKEY` cap — keep the pre-check in the caller
  (recommendation in §4.1).
- Update `T114b` retention test to assert via registry owner count.

### Stage 4 — Author view (`profile.rs::open_author/close_author`)

**Goal:** migrate #9–#11 from §3.1 and retire `author_view: AuthorViewState`.

- `open_author(pk)` → 3 `ensure_sub` calls (kind:10002, kind:0, kinds:1,6)
  with owner = `("author-view", pk)`. The 3rd is `Tailing`; the first 2 are
  `OneShot`.
- `close_author(pk)` → 3 `drop_owner` calls.
- Delete `ViewInterest { refcount }` from `author_view`; refcount is
  intrinsic to the registry via owner attachment count.
- Delete `request_pending` flag and `author_requests()` builder.
- Delete `close_subscriptions_with_prefixes(&["author-…"])` calls; the
  planner emits `WireFrame::Close` when the owner leaves.

### Stage 5 — Thread view (`thread.rs`)

**Goal:** migrate #12–#13 from §3.1 and retire `thread_view: ThreadViewState`.

- `open_thread(id)` → register thread-root + recursive-replies interests.
- `enqueue_thread_id` / `enqueue_thread_reply_target` become `ensure_sub`
  with `event_ids` shape; the planner handles partitioning.
- `close_thread(id)` → drop owners.
- Delete `thread_view.pending_ids`, `.requested_ids`, `.pending_reply_targets`,
  `.requested_reply_targets`, `.ids_inflight`, `.replies_inflight`.
- May require planner extension for per-event-id outbox routing (see §4.3).

### Stage 6 — Demolition

**Goal:** delete the M1 surface so it cannot grow back.

- Delete `Kernel::req`, `Kernel::req_for_relay`, `Kernel::defer_outbound`,
  `Kernel::partition_auth_paused`, `Kernel::pending_view_requests`,
  `Kernel::deferred_outbound`, `Kernel::close_subscriptions_with_prefixes`
  if no callers remain.
- Delete `defer_outbound_silent` from `kernel/requests/auth_gate.rs`.
- Confirm `subs/auth_gate.rs` enforces a bounded pending buffer (D8); add
  the cap if missing.
- Delete the M8/M2-migration-plan doc comments at the top of
  `requests/profile.rs:1-21` and `requests/thread.rs:1-22`.
- Delete the comment block at `kernel/mod.rs:382-391` documenting the
  coexistence.
- Update `kernel_action.rs`'s doc-string at line 1220 to drop the "M1
  hand-rolled" qualifier.
- Update `m8-subscription-lifecycle.md:93-96` to record retirement.

### Stage 7 (optional) — `ActorCommand::PushInterest` / `WithdrawInterest`
collapse into `dispatch_action`

If the dispatch_action seam (PR #56 era) is the canonical action surface,
these two raw-registry ActorCommands should be removed in favour of an
`ActionModule` for "open/close arbitrary URI." Out of scope for V-04 fix
but worth noting as a follow-up.

---

## 6. Acceptance criteria — CI-gateable, not prose

A PR series implementing this plan is done when **all** of the following hold:

1. `! grep -rn "fn req\b\|fn req_for_relay\b\|fn defer_outbound\b\|fn
   partition_auth_paused\b\|fn close_subscriptions_with_prefixes\b\|fn
   pending_view_requests\b" crates/nmp-core/src/kernel/` returns no matches.
2. `grep -rn "wire\.subs\.insert" crates/nmp-core/src/kernel/ | wc -l`
   equals 1 (inside `Kernel::register_planner_wire_frames` after the
   helper-extraction collapse, or inside the extracted
   `Kernel::insert_wire_sub` helper called only by
   `register_planner_wire_frames`).
3. `grep -rn "ViewInterest\b\|profile_requests\b\|thread_view\b\|author_view\b\|diagnostic_firehose\b" crates/nmp-core/src/kernel/`
   returns only deletion markers or `dead_code`-attributed fields scheduled
   for removal in the next PR (ideally: zero matches in production fields).
4. The comment block at `crates/nmp-core/src/kernel/mod.rs:382-391` is
   deleted.
5. `! grep -rn "M2 migration plan\|two paths coexist\|M1 hand-rolled" docs/ crates/`
   except in a single "V-04 closed" log entry in `docs/BACKLOG.md`.
6. All existing tests pass green: `cargo test -p nmp-core`. Targeted suites:
   - `t140_m1_retirement_tests`
   - `eose_ok_notice_ingest_tests`
   - `outbox_tests`
   - `auth_tests`, `auth_fail_closed_tests`, `auth_url_threading_tests`
   - `discovery_tests`, `kernel/discovery_tests`
   - `replay_tests`
   - `retention_tests`
   - `t142_drain_lifecycle_tick_tests`
   - `local_publish_intent_tests`
   - `profile_claim_tests`
   - `nip17_dm_inbox_routing_tests`
   - `coverage_hook_tests`
7. A new `t140`-style retirement test asserts that NO wire-sub is emitted
   under any of the retired sub-id prefixes: `author-`, `thread-`,
   `profile-claim-`, `profile-batch-`, `profile-target`, `target-relays`,
   `self-dm-relays`, `self-contacts`, `self-zap-receipts`, `diag-firehose-`,
   `oneshot-disc-` (the migration replaces them with `SubKey`-derived sub-ids).
8. Doctrine-lint: `crates/nmp-core/src/subs/registry.rs:42-45` retains the
   "Single-writer registry of active logical interests" doc; no new
   `pub fn` is added to a non-`subs::` module that mutates the active
   wire-sub set. (Manual review item — no automated check.)
9. PR description for the final stage links the V-04 entry in `BACKLOG.md`
   and moves it from the open list to the closed (Appendix) list.

### What a follow-up Opus reviewer must check

- That no parallel "subscription" state has reappeared inside `Kernel`,
  `actor/`, or any app crate (`apps/chirp/`, etc.). The registry is the only
  game in town.
- That every retired view-state struct's removal is permanent: not
  re-introduced under a new name (e.g. `RequestPending`, `InflightThreadIds`).
- That the new `t140`-style retirement test exists, runs in CI, and covers
  every retired prefix.
- That `Kernel::register_planner_wire_frames` is the sole inserter into
  `wire.subs` (point 2 above).
- That the M8 plan document `docs/plan/m8-subscription-lifecycle.md` records
  the retirement (point 5 above) — otherwise future agents will believe the
  systems still coexist and re-add to M1.

---

## 7. Doctrine reminders for the implementation agent

- **D0** — `nmp-core` must not grow app nouns. `LogicalInterest`/`SubKey` are
  substrate; no protocol-specific (nip17_, nip29_, marmot_) field names.
- **D4** — one writer per fact. The whole point of the exercise.
- **D6** — no panics across FFI. The migration must preserve total-function
  contracts on every kernel reducer touched.
- **D8** — no polling, no unbounded queues. The `AuthGate`'s pending buffer
  must have a cap equivalent to the retired `defer_outbound` cap of 64
  frames (`kernel/requests/mod.rs:204-206`).
- **No "for now" hacks.** If a planner extension is required (§4.3), land it
  as its own PR with its own test; do not stub it inside the migration.
- **Worktree + PR-per-stage discipline** per `AGENTS.md` — one stage per
  worktree, one PR per worktree, no force-pushes to master.
