# Opus Direction Review #18 — 2026-05-24

Reviewer: Opus. Lens: code-grounded honesty against the four mandate questions, with
explicit avoidance of anything already tracked in `docs/BACKLOG.md` or surfaced by
reviews #13 (post-V-04 / post-Notes) and #14 (product honesty).

Tree read at HEAD `28cf348d` (master). Files inspected: `aim.md`, `plan.md`, `BACKLOG.md`,
`WIP.md`, `AGENTS.md`, `crates/nmp-core/src/ffi/{mod,snapshot}.rs`,
`crates/nmp-core/src/kernel/{update,perf_tests,publish_cmd}.rs`,
`crates/nmp-core/src/publish/action.rs`, `crates/nmp-core/src/actor/commands/{zap,wallet}.rs`,
`apps/notes/ios/Notes/{Bridge,Models,Views}/*.swift`,
`apps/chirp/nmp-app-chirp/src/ffi/register.rs`,
`ios/Chirp/Chirp/Bridge/{KernelBridge.swift,Generated/KernelTypes.generated.swift}`,
`crates/nmp-nip29/src/projection/group_chat.rs`, `docs/dispatch-actions.md`, `crates/` directory listing.

---

## What NMP should support that it doesn't

### 1. Whole families of high-ROI Nostr NIPs are absent and untracked

**Evidence (code-grounded).** `ls crates/` shows `nmp-nip{01,02,17,29,42,57,59,65}` only.
There is no `nmp-nip23` (long-form articles), no `nmp-nip51` (lists: bookmarks, mute lists,
pinned notes, communities), no `nmp-nip94`/`nmp-nip96` (file metadata + media servers).
`docs/aim.md` §4.6 explicitly names "mute list view" as session state and §4.11 names
Blossom as the media client — neither has any tracker entry. `docs/BACKLOG.md` §5 lists
Blossom and a single line about "more video/long-form work post-v1" but the rest is silent.

`crates/nmp-content-fixtures/src/dto.rs:186-213` defines a `Nip51List` DTO for tests but
no production projection exists. kind:30023 appears in `crates/nmp-core/src/tags.rs:8`
and in `kernel_action.rs:213` only as a constant — there is no decoder, no projection, no
action module. The Chirp `SettingsHubView.swift` Roadmap text (V-14 finding 7) says
"Lists / bookmarks — Save notes" is on the roadmap, but no `BACKLOG.md` item tracks it.

**Why it matters.** A "build any Nostr client" framework that cannot render a profile's
long-form article tab, save a bookmark, or display a mute list is not framework-grade. The
post-v1 list misses these and Blossom is the only media tracker — which is the protocol
that is **least adopted** in the actual Nostr graph today. NIP-94 (`imeta`/file metadata)
ships in every modern client because relay-hosted-media URLs need it for HEIC vs JPEG,
dimensions, MIME, and SHA-256.

**Severity.** Mute lists are v1-A safety-relevant (a user has no way to hide harassment
that an app on top of NMP would normally suppress); NIP-23 / NIP-51 / NIP-94 are post-v1
but should be **on a roadmap row**, not absent. **Recommended action:** add a `BACKLOG.md`
§5 row for each, with a one-line scope and the prerequisite (most need only an
`ActionModule` + a `KernelEventObserver` projection — a 1–2 day pattern Chirp already
proves). Mute list specifically: promote to v1-A blocker if the Chirp `BlockListView` is
not deleted from the iOS shell first (it is currently absent — I grep'd and found nothing
under `ios/Chirp/Chirp/Features/`, which confirms the gap).

### 2. The `dispatch_action` catalog has no multi-step async chain contract

**Evidence.** `docs/dispatch-actions.md:171-193` documents `nmp.nip57.zap` as a single
action. `crates/nmp-core/src/actor/commands/zap.rs:202-219`:

```rust
let _ = command_tx.send(ActorCommand::WalletPayInvoice {
    bolt11: bolt11.clone(),
    amount_msats: Some(amount_msats),
    correlation_id: None,                     // <-- bridge breaks here
});
// ...
if let Some(cid) = correlation_id {
    let _ = command_tx
        .send(ActorCommand::RecordActionSuccess { correlation_id: cid });
}
```

The original zap dispatch closes `RecordActionSuccess` the moment the LNURL provider
returns a valid bolt11. The wallet pay then runs under a separate, **anonymous**
correlation_id; its outcome is delivered through the wallet's own correlation channel
(`crates/nmp-core/src/actor/commands/wallet.rs:485-512`). The kind:9735 receipt that
proves the zap was actually paid is observed by `ZapsAggregateProjection`, which has
no link back to the dispatcher.

`crates/nmp-core/src/kernel/publish_cmd.rs:233-236` — `action_lifecycle_projection`
returns `{in_flight, recent_terminal}` per correlation_id. It does not collapse two
different correlation_ids into one chain. So a host that dispatched `nmp.nip57.zap`
sees `Success` ~200 ms after the LNURL provider responds, regardless of whether
NWC actually pays or whether the receipt ever arrives.

**Why it matters.** This is the framework's own answer to "how do you build a correct
zap UX?" The answer the code currently gives is "you can't — the framework reports
Success too early and silently abandons the rest." A user-facing zap that fails at the
wallet hop shows a successful spinner-close in the UI and no receipt.

**Severity.** Medium. UX-correctness issue, not a substrate violation. F-04 explicitly
calls for "Zap E2E round-trip verification" — but the verification has no contract to
verify against because the action shape was never designed for the multi-step chain.

**Recommended action.** Document a `nmp.nip57.zap` *chain* contract in
`docs/dispatch-actions.md`: either (a) the original correlation_id stays open until
the kind:9735 receipt arrives and `ZapsAggregateProjection` records success, or
(b) introduce `Stage::Bolt11Received`, `Stage::WalletPaid`, `Stage::ReceiptObserved`
on the existing `action_stages` substrate so the host sees the full chain at one id.
Option (b) generalises — every NIP-57 action and every future multi-step dispatch
(NIP-46 sign request → relay broadcast → receipt) gets the same pattern.

### 3. There is no "decrypt-only" surface for iOS Notification Service Extension

**Evidence.** `aim.md` §7 open design question #5: "iOS Notification Service Extensions
and Android background workers must call into the Rust core for NIP-17 decryption
without spinning up the full actor. Likely a smaller 'decrypt-only' surface area in a
sibling crate." That question has been open since the start. The crate doesn't exist.
`crates/nmp-nip59/` has the gift-wrap codec — but exposing it requires linking the
whole `nmp-core` static lib (which spawns the actor, sets up storage paths, etc.) into
a 6 MB NSE binary that Apple caps at 24 MB total and 60 s wall-clock.

`grep -rn "NotificationServiceExtension\|UNNotification" ios/` returns zero matches.
The Chirp app ships NIP-17 DMs but the user does not receive a notification when one
arrives unless the app is foregrounded — which defeats the protocol's whole point.

**Severity.** v1-A blocker if Chirp is meant to ship as a real DM-capable app on iOS
(the only platform whose NSE limits make this a hard architectural concern).
v1-B post-NSE work for Android.

**Recommended action.** Add a `crates/nmp-nip59-decrypt-only/` crate that exposes one
function `unwrap_gift_wrap(envelope_json, local_nsec) -> rumor_json` and links a
2 MB-ish static lib (no actor, no storage, no relay code). Add a `BACKLOG.md` row;
this is the only way iOS DMs are competitive with Signal/iMessage notification UX.

### 4. There is no `LogicalInterest::FollowSetKind1` (or equivalent) in substrate

**Evidence.** Already cited in BACKLOG `V-37 (c)`. But the framing is wrong there — V-37
treats it as a Notes-rewrite prerequisite. The actual scope is bigger: every "show me
notes from people I follow" app on top of NMP needs this, and today the only way to get
it is to pull in `nmp-nip02::FollowListProjection` *plus* the Chirp registration code
that wires it. Read `apps/chirp/nmp-app-chirp/src/ffi/register.rs:370-403` — the
follow-list wiring is a 30-line incantation that a third developer would never assemble
from documentation. The substrate offers no shortcut for the most common Nostr-client
read pattern.

**Severity.** v1-B framework readiness. The framework's selling line is "one-shot a
working Nostr application" (aim.md §1); this is the single read pattern that one-shot
needs, and the affordance for it does not exist.

**Recommended action.** Treat V-37(c) as its own item: design a substrate-level
`LogicalInterest::SocialTimeline { viewer: Pubkey, kinds: Vec<u16> }` that pulls in the
follow-set automatically and outbox-routes through the planner. Drop V-37(c) and add
this as a positive feature with its own row.

### 5. There is no offline action queue

**Evidence.** `aim.md` §7 open design question #6 — "Actions dispatched while offline
must persist and replay on reconnect, with correct ordering and timestamping. Where does
the queue live — in the actor, in SQLite, in a separate durable channel?" Open since
inception. `crates/nmp-core/src/publish/engine.rs` (827 LOC, BACKLOG V-12) is an
in-process retry engine; it does not survive a process restart. A user composes a note
on the subway, hits Send, the actor accepts it, the device is offline, the user
backgrounds the app — on next launch the note is gone. There is no row in `BACKLOG.md`
for this and no row in `aim.md` §7's "resolved" column.

**Severity.** v1-A blocker for any social product (Chirp ships kind:1 as the primary
feature). Either implement, or remove from `aim.md` §7 with a written rationale
("offline composition is not in scope for v1").

---

## What NMP shouldn't do that it does

### 6. The snapshot built-in projection cluster is unbounded — D5 is silently violated

**Evidence (code-grounded).** `crates/nmp-core/src/kernel/update.rs:267-440` —
`snapshot_projections_with_publish_cluster` *unconditionally* inserts on every tick,
regardless of whether any host view is open:

```
publish_queue, publish_outbox, outbox_summary, relay_edit_rows,
relay_role_options, settings_hub, accounts, active_account,
profile, timeline, author_view, thread_view, inserted, updated, removed,
relay_diagnostics, mention_profiles
```

Plus, conditionally on tick activity: `action_results`, `action_stages`,
`action_lifecycle`. Plus all host-registered projections (Chirp registers
`nmp.nip57.zaps`, `nmp.follow_list`, `nmp.nip17.dm_inbox`, `nmp.nip29.group_chat`,
`nmp.nip29.discovered_groups`, plus `wallet` + `bunker_handshake`).

D5 in `plan.md:43` reads "snapshots bounded by open views." The built-in cluster is
**not bounded by open views**: `timeline` runs `visible_items()` every tick;
`relay_diagnostics` rolls every relay + every wire sub; `mention_profiles` walks every
visible item plus `author_view` plus `thread_view` building a derived map; and so on.
Even with zero open views the cluster carries 17+ keys.

The perf gate at `crates/nmp-core/src/kernel/perf_tests.rs:128` runs against
`Kernel::new()` (zero registered host projections) and a 1k-event firehose — Opus #13
finding #3 already flagged this. But the deeper issue is that the **built-in cluster
is the thing that grows with feature work**, and D5 does not actually constrain it.
Every new host projection (V-37 added `outbox_summary`, V-28 the avatar / display
fields on `TimelineItem`, V-22/V-25/V-27 the per-row display strings on group chat /
DM / modular timeline) widens the always-emitted snapshot.

**Why it's net-negative.** D5's phrasing — "snapshots bounded by open views" —
suggests a structural guarantee the code does not provide. A reader of `plan.md`
believes the snapshot is bounded; the actual contract is "we hope it stays small
enough." That gap means perf regressions are caught by *running iOS Chirp*, not by
doctrine. When the contract is "use a perf-gate test" the rule should say so.

**Recommended action.** Either (a) rewrite D5 to "snapshots are bounded by a static
cluster + open-view dependent payloads; the static cluster is gated by `snapshot_perf_firehose_gate`",
or (b) move the genuinely view-dependent payloads (`author_view`, `thread_view`,
`timeline`, `inserted`, `updated`, `removed`) out of the unconditional insert and
into a "only if a view subscribed" branch. Option (b) is the doctrine-honest version
and the right one.

### 7. The Notes app proves the framework's guarantee can be defeated in 96 LOC of Swift

**Evidence.** Already cited at length in Opus #13 (PD-033-A re-opening). The piece I
want to add: this is **not just an issue with PD-033-A** — it is structural evidence
that the framework's `aim.md` §1 promise ("make it nearly impossible to build a broken
Nostr application") is **false at the FFI seam**. The framework guards the kernel; it
does not guard what a host can do with the raw event tap, `JSONSerialization`, and a
`raw_event_observer` registration. The Notes proof is not that "the spike was sloppy"
— it is that the framework's whole protective claim is at the wrong layer.

`crates/nmp-core/src/ffi/raw_event_tap.rs` exposes `nmp_app_register_raw_event_observer`
with no doc warning that bypassing the framework here voids every D1/D3/D8 guarantee.
`apps/notes/ios/Notes/Bridge/NotesBridge.swift:73-76` registers it without ceremony.
Apple App Store review will not catch this; doctrine-lint cannot catch it (it runs in
Rust, not Swift). The framework guarantees end where Swift `unsafe pointer` calls
begin.

**Why it's net-negative.** Either fix the layer (deprecate `register_raw_event_observer`
as a host-facing entry point, restrict it to in-tree app crates) or rewrite the
`aim.md` §1 line as "make it nearly impossible to **accidentally** build a broken Nostr
application, given the developer follows the action/projection seam." The current
phrasing oversells what the framework structurally enforces.

**Recommended action.** Add a §1 caveat to `aim.md`, then write a contributor doc
("How to *not* defeat the framework") that names the four raw-tap escape hatches
(`register_raw_event_observer`, `inject_pre_verified_events`, `inject_signed_event_json`,
the host-supplied `NmpSnapshotProjector` callback) and says where each one is
appropriate. The current docs assume the developer will not reach for them.

### 8. F-05 codegen claims it's "v1 quality" but the actual coverage is ~20%

**Evidence (code-grounded).**
- `ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift` — 258 LOC, 8 generated
  `public struct`s (KernelMetrics, RelayStatus, LogicalInterestStatus,
  WireSubscriptionStatus, AccountSummary, RelayEditRow, RelayRoleOption, TimelineItem).
- `ios/Chirp/Chirp/Bridge/KernelBridge.swift` — 1,895 LOC, ~40 handwritten `Decodable`
  structs (KernelUpdate, MentionProfileWire, SettingsHubSummary, GroupChatMessage,
  GroupChatSnapshot, DiscoveredGroup, DiscoveredGroupsSnapshot, ZapCount,
  ZapsAggregateSnapshot, DmRelayListSnapshot, DmMessage, DmConversation, FollowEntry,
  FollowListSnapshot, DmInboxSnapshot, RelayDiagnosticsWireSub, RelayDiagnosticsRow,
  RelayDiagnosticsInterest, RelayDiagnosticsSnapshot, BunkerHandshake, Nip46Onboarding,
  ThreadView, PublishQueueEntry, LastActionResult, ActionStageEntry,
  ActionLifecycleEntry, ActionLifecycleSnapshot, PublishOutboxItem, PublishOutboxRelay,
  OutboxSummary, WalletStatusData, ProfileCard, ProfileDispatchSpec, ProfileAction,
  AuthorProfileSnapshot + the enums ActionStage, ActionLifecycleStage).

`BACKLOG.md F-05` says Stage 3 is "blocked on emitter extensions" — tagged-enum
support, per-field overrides, `legacy_default`. These are non-trivial codegen work
items. Stage 1 (7 flat records) is done; Stage 3 partial added `TimelineItem` (now 8).
**Generated coverage: 8 / ~48 = ~17 %.** The remaining 40 are exactly the types that
would benefit most (the snapshot payload, the multi-state enums, the projection
clusters that change shape often).

**Why it's net-negative.** Listing F-05 as a "v1 quality" exit criterion when the
remaining work is blocked on architectural emitter design means F-05 will either (a)
slip and gate v1, or (b) ship with 17% coverage and the v1 announcement contains a
misleading claim. Either outcome is bad.

**Recommended action.** Either split F-05 into "F-05a: Stage 1+2 (DONE)" + "F-05b:
tagged-enum emitter (post-v1)", or drop the v1-quality framing and call F-05 "exploratory
codegen pilot — not v1-gating." The current "v1 QUALITY" tag in `BACKLOG.md` line 1070
is honest only if Stage 3 is *intended* to remain partial.

---

## The most painful DX friction point

**Building a second NMP app requires assembling the registration boilerplate from
scratch by reading Chirp's source.**

**Evidence.** Compare a developer trying to build a Notes-style app today vs. what
they actually face:

- The catalog (`docs/dispatch-actions.md`, 337 LOC) lists *what to call* but not *what
  to register first*. The "Action registration (Rust side)" section at the bottom
  (lines 323-337) shows `nmp_nipNN::register_actions(app)` calls but does not name the
  `register_snapshot_projection`, `register_event_observer`, `register_dm_runtime`,
  `register_zap_receipts_runtime`, or `set_coverage_hook` calls a working app needs.
- The reference implementation lives in `apps/chirp/nmp-app-chirp/src/ffi/register.rs`
  — 403 LOC of `unsafe { &mut *app }` borrows and SAFETY-comment incantations. The
  ordering matters (action registration before `&NmpApp` borrow; observer
  registration before `nmp_app_start`); ordering violations fail silently with a
  null handle.
- The "blessed minimal path" is not documented. A developer reads
  `apps/notes/nmp-app-notes/src/lib.rs` (98 LOC, half comments) and finds it
  registers *nothing* — because the Notes spike opted out of the framework's seams
  (Opus #13 finding #1). So the smallest existing app is also the wrong example.

**The fix.** Two complementary things:

1. **`nmp-app-template` crate.** A `cargo new`-able starter that exposes `nmp_app_template_register(*mut NmpApp) -> *mut TemplateHandle` and includes the canonical wiring (action registry, default projections for kind:1 + profiles, coverage hook). A developer copies this crate, renames it, edits the projections they need. This replaces the "read 403 LOC of Chirp and pray" workflow with "copy the template."

2. **`nmp init <appname>` scaffolder.** Already named in `aim.md` §4.14. Does not exist
   (`crates/nmp-cli` exists per BACKLOG plan.md M16 row but "starter recipes not"
   done). Wire it to scaffold the template crate plus a minimal iOS/Android shell
   with the registration calls pre-filled. This is the *single highest-leverage DX
   investment* on the post-v1 list and it should be on the v1 list — the framework's
   selling line in `aim.md` §1 ("one-shot a working Nostr application") is the thing
   `nmp init` does, and shipping v1 without it ships a framework that nobody can
   use without reading the Chirp source.

This is the friction point Notes was supposed to expose, but Notes opted out of the
seams and now stands as evidence that the friction is high enough to defeat motivation
to use the seams.

---

## Is the v1 definition honest?

**Short answer: no — three of the four remaining blockers have non-falsifiable closing
conditions, and the pending decisions sit on architectural prerequisites that have not
been built.**

The blockers per `plan.md` §TL;DR:

- **F-01 IndexedDB store** — clearly defined, falsifiable (port persistence, run a
  reload test). Honest. The work is real but the criterion is clean.
- **F-02 DM cold-start receive-side** — `BACKLOG.md:488` already states
  "device-level acceptance test against live relays (product QA, not CI-gatable)."
  V-15 closed the missing nightly workflow, but the test that proves F-02 done
  is **still a manual run** because the test fixture needs two real accounts on
  two devices. Until that fixture exists in CI, F-02 closes when a human says so.
  **Not falsifiable.**
- **F-04 Zap E2E** — same shape as F-02 (needs a live NWC wallet). Plus, finding 2
  above: the action chain breaks at zap.rs:202, so even if a human runs it once and
  it works, the framework offers no contract that protects against the chain breaking
  silently in the future. **Not falsifiable.**
- **F-05 codegen Swift pilot** — finding 8 above: scope is "blocked on emitter
  extensions" but the v1 criterion does not say which extensions. **Closing
  condition ambiguous.**

The pending decisions:

- **PD-033-A (Notes rewrite)** — V-37 blocks the rewrite (the three substrate
  prerequisites do not exist). V-37 itself is tagged "needs ADR before work begins."
  So PD-033-A is blocked on an ADR that has not been written, which means the v1
  criterion #4 ("Stateful second-app spike is run") **cannot close on the current
  trajectory**. Honest path: either acknowledge V-37 is itself a v1 blocker (it
  becomes F-08), or drop PD-033-A from the v1 list and write a one-paragraph
  rationale.
- **Cross-platform claim (v1 criterion #6)** — `plan.md` says "Either wasm runs a
  real `NmpApp` actor on a Web Worker, or 'cross-platform' is rewritten as 'iOS +
  macOS + Android'." Stages 2-3c of nmp-wasm are merged; F-01 IndexedDB remains. This
  is the cleanest v1 item on the list — but the choice between the two options
  ("really cross-platform" vs. "rewrite the marketing") has not been made. It will
  be a politically loaded decision when it finally happens.
- **Real-relay nightly (V-15 DONE) + perf gate (V1 exit #8 DONE)** — both closed.
  These are wins. The perf gate is the only one of the eight "exit criteria" with
  an automated, falsifiable signal.

**Biggest risk to shipping v1.** Not F-01, not the bespoke FFI surface, not the
codegen — those are mechanical. The biggest risk is the closure of PD-033-A. Until
the framework can demonstrate a second app *without bypassing its own seams* — which
requires V-37's three new substrate affordances, which require an ADR that has not
been started — v1 ships with the framework's central thesis unproven. The honest
calls are (a) acknowledge V-37 as a v1 blocker (promote to F-08), or (b) drop the
"framework thesis proven" exit criterion and ship v1 as "iOS Chirp on a kernel,
framework status to-be-determined."

---

## One contrarian take

**The framework's core claim — "make it nearly impossible to build a broken Nostr
application" (`aim.md` §1) — guards the wrong layer. The kernel is safe; the FFI seam
is not. And the FFI seam is where every NMP app lives.**

The architecture's central protection — actor-owned state, typed `ActionModule`s,
single-writer projections, doctrine-lint D0-D14 — applies inside Rust. It does not
apply when a Swift author registers a raw-event tap and parses JSON in
`JSONSerialization`. The Notes spike proved this: 96 LOC of Swift defeated D3 outbox
routing, kernel-owned formatting, lifecycle gating, and the codegen contract — without
once stepping outside the framework's public ABI. The framework was satisfied; the
*app* was broken.

**What would falsify this.** A rewrite of `apps/notes/` that demonstrates the
framework's seams produce a working app **without** the developer having to think about
ordering, observer lifecycle, or projection registration boilerplate. If V-37 is built
and the rewrite drops below 100 LOC of Swift while staying correct on D3 / D8 / D5,
the thesis holds. If the rewrite balloons to 300 LOC because the developer has to
re-implement Chirp's `register_dm_runtime` / `register_zap_receipts_runtime` /
`set_coverage_hook` boilerplate to get a useful timeline, the thesis is falsified —
the framework guards the kernel but does not produce shippable apps without Chirp-grade
plumbing.

The bet I'd make: the rewrite balloons. The boilerplate Chirp carries — 403 LOC of
`register.rs` for one app — is the framework's *real* cost, not the FFI symbol count.
A framework that demands 400 LOC of registration code per app is not "one-shot a
working Nostr application"; it's "fork Chirp." That's what NMP currently is, and the
v1 exit criteria do not measure it.

---

## Summary table

| Finding | Severity | Tracked? | Recommended action |
|---------|----------|----------|-------------------|
| 1. NIP-23/51/94/96 absent + untracked | v1-A (mute) / post-v1 (rest) | NO | Add §5 rows; mute list possibly v1-A |
| 2. Zap correlation chain breaks at LNURL | MEDIUM | NO (V-41 covers crate location, not contract) | Document chain contract in dispatch-actions.md; add `Stage::WalletPaid` |
| 3. No decrypt-only crate for iOS NSE | v1-A if DMs ship | NO (aim.md §7 open) | Add `nmp-nip59-decrypt-only` crate row |
| 4. No `LogicalInterest::SocialTimeline` substrate seam | v1-B framework | Partially (V-37c, but framed wrong) | Promote V-37c to own item |
| 5. No offline action queue | v1-A blocker for social | NO (aim.md §7 open) | Add row or drop from aim.md |
| 6. Snapshot built-in cluster unbounded; D5 silently violated | HIGH | NO | Rewrite D5 or gate built-in cluster on open views |
| 7. Framework guarantees end at FFI seam (Notes proof) | STRUCTURAL | Partially (PD-033-A) | Add aim.md §1 caveat + contributor doc on escape hatches |
| 8. F-05 codegen is 17% covered, claims "v1 QUALITY" | MEDIUM | F-05 (but scope is ambiguous) | Split F-05a/F-05b; drop v1-quality framing on Stage 3 |
| 9. DX friction: 403 LOC of Chirp register boilerplate | HIGH | NO | `nmp-app-template` crate + finish `nmp init` |
| 10. V-37 ADR is the actual PD-033-A blocker | v1-A | YES (V-37) but not framed this way | Promote V-37 to F-08 v1 blocker, or drop PD-033-A from v1 |
| 11. F-02/F-04 closing conditions non-falsifiable | HIGH | F-02/F-04 closed by human judgement | Define automated acceptance fixture or accept manual sign-off explicitly |

---

**30-day call.** Two items, in order: (1) write V-37's ADR and build the three
substrate affordances (snapshot projector context arg, generic snapshot pull path,
`LogicalInterest::SocialTimeline`); (2) build `nmp-app-template` + finish `nmp init`
so Notes' rewrite is a `cp -r nmp-app-template apps/notes` + 50 LOC of edits, not 96
LOC of bespoke Swift. Without (1) the framework thesis stays unproven. Without (2)
the friction that produced the bypassing Notes spike will produce another one.
