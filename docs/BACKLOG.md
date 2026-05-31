# NMP Backlog

> Tracker for active violations, pending user decisions, and the ordered v1 feature backlog.
> Supersedes `docs/perf/pending-user-decisions.md` (append-only history log, kept for audit)
> and the former `docs/arch-review-queue.md` (deleted after open items were folded here).
>
> Companion files:
> - [`WIP.md`](../WIP.md) — live tracker for work currently on a branch (in-flight)
> - [`docs/plan.md`](plan.md) — overarching plan (milestones, doctrine, where we are)
>
> Verified against `origin/master` **c295efcc** (2026-05-29). Update this file
> in every PR that touches an item listed here. (Cleanup pass 2026-05-27 — completed
> items removed; see git history for prior state.)
>
> Reconciled against `origin/master` **c295efcc** 2026-05-29 — removed items closed
> during the v0.1.0/0.1.1 backend sweep (V-46, V-58, V-61–V-67, V-69–V-72, V-74–V-75,
> V-77, V-79, V-84–V-86, V-92, V-96); renumbered duplicate V-68-iOS to V-106;
> deduplicated V-82; resolved PD-041.

---

## FUNDAMENTAL RULE

**Any mock, stub, or "for now" hack that deviates from perfect architectural execution is
completely forbidden and must be fixed immediately.**

Corollary for multi-week fixes: staging is allowed, but the staging plan must be written here
and progress must advance each sprint. A staged fix that has not moved in two sprints is
treated as an immediate-fix violation.

---

## For Autonomous Agents

**Pick the topmost item in Section 4 (Feature Backlog) that does NOT appear in Section 2
(In Flight).** Do not start a Section 4 item already in progress. Section 1 (Active
Violations) takes priority over Section 4 — if a Section 1 item has no open branch, create
one before picking Section 4 work. Never start two items that touch overlapping files without
explicit coordination.

---

## Section 1 — Active Violations

Code-verified structural violations on current HEAD. Count must only decrease. No new entry
without a `file:line` citation confirmed against the current tree.

### V-57 · Remaining kind-constant duplicates to migrate to nmp-kinds [LOW · cleanup]

Kind-constant centralisation is partially done (`nmp-kinds` Layer-0 crate exists; seven
constants migrated; `nmp-core::kinds` and `nmp-nip59::kinds` are now re-exports). The
following duplicates are still crate-local and must be migrated when each NIP crate is
next refactored:

- `nmp-nip57` — `KIND_ZAP_REQUEST` / `KIND_ZAP_RECEIPT`
- `nmp-nip17` — `KIND_DM_RELAY_LIST`
- `nmp-nip51` — `KIND_MUTE_LIST`
- `nmp-router` — `KIND_BLOCKED_RELAYS`
- `nmp-nip17/src/inbox.rs:75` — `KIND_CHAT_MESSAGE` (u16, used against `rumor.kind.as_u16()`; needs cast, separate change)
- `nmp-nip29` — `KIND_CHAT_MESSAGE = 9` (distinct semantic from registry `= 14`; stays crate-local unless semantics are unified)

**Open items from the 2026-05-26 audit that remain:**

- **P3 — move Chirp shell business logic behind Rust-owned actions/projections.**
  `ios/Chirp/Chirp/Features/RelaySettingsView.swift:159-177` dispatches two
  protocol publishes while tracking only one correlation id. **Next step:** expose a
  composite Rust action / action-stage projection for the relay-settings publish.
- **P6 — strengthen enforcement so these regressions trip earlier.**
  V-12 already tracks oversized boundary files; the new gap is doctrine-lint coverage for
  dependency direction and app-noun leakage. **Next step:** add a dependency-graph/layer
  lint covering upward edges such as `nmp-router -> nmp-ffi` and `nmp-signer-broker -> nmp-core`,
  plus explicit allowlists for sanctioned adapter crates.

### V-68 · Core/planner still carry kind:1/6 social subscription policy [HIGH · D0 violation · Stage 2-author+Stage 3 OPEN]

**Stages 1 and Stage 2 thread-half landed (2026-05-29/30).** `nmp-planner/src/interest.rs`,
`nmp-core/src/kernel/ingest/mod.rs`, and `nmp-core/src/kernel/requests/thread.rs` no longer
carry the `{1, 6}` literal. Remaining open sites:

- ⏳ **OPEN (Stage 2 author-half)** `crates/nmp-core/src/kernel/requests/profile.rs:~532-550`
  still hardcodes selected-author note/repost requests as `{"kinds":[1,6], ...}`.
  Deferred until the iOS peer agent lands `ActorCommand::OpenAuthor { kinds }` +
  `NmpCore.h` + `KernelBridge.swift` churn.

**Why this is a violation:** `{1, 6}` is a social/NIP-01 timeline policy, not
substrate policy. `nmp-core` and `nmp-planner` may carry caller-supplied `kinds`
as filter data, but they must not choose app defaults. Defaults like "follow-list
timeline means kind:1 + kind:6" belong in `nmp-nip01`, `nmp-nip02`,
`nmp-app-template`, or an app crate such as Chirp.

**Stage 2 (author-view + thread-reply) — the remaining production behavior.**
These two sites carry live behavior and CANNOT reuse the follow-feed's
host-declared `Kernel::follow_feed_kinds`: `nmp_app_open_author`
(`ProfileView.swift`) and `nmp_app_open_thread` fire **independently** of
`nmp_app_open_timeline`/`OpenContactListSubscription`, so a deep-link can open an
author/thread before the home feed ever declared its kinds — borrowing
`follow_feed_kinds` would request zero kinds and silently break profile/thread
views. The correct seam is **per-call kinds**, extending the existing
`OpenContactListSubscription { kinds }` pattern: add `kinds: BTreeSet<u32>` to
`ActorCommand::OpenAuthor` and `ActorCommand::OpenThread`, thread them through
`Kernel::open_author` / `open_thread` → `author_requests` / thread reply builder,
and have the FFI symbols (`nmp_app_open_author` / `nmp_app_open_thread`) accept the
kind set from the host. Because kinds arrive *with* the call there is no ordering
problem — the Swift `ProfileView`/thread call site declares them exactly as
`nmp_app_open_timeline` declares `{1, 6}` today. Cost is FFI + `NmpCore.h` +
`KernelBridge.swift` churn (iOS blast radius), which is why it is staged.

  *Rejected alternative:* a single app-level `content_kinds` field set once at app
  init would avoid the ABI churn, but only if an init hook is guaranteed to run
  before any view opens, and it must be a **separate** field from
  `follow_feed_kinds` (overloading conflates "no declared kinds" with "follow-feed
  inactive"). Per-call kinds is the safe default; pursue the field only if that
  init guarantee is proven cheap.

**Stage 3 (finalizer):** once Stage 2 lands and no `[1, 6]` literal remains in
`nmp-core`/`nmp-planner` production code, add a doctrine-lint rule banning hardcoded
social content-kind sets (`[1, 6]` / `[1u32, 6u32]`) in those crates' non-test
source so the door stays closed. Do NOT add the lint before Stage 2 — `profile.rs`
and `thread.rs` still carry the literal and would fail the build.

**Required fix (general):** move the remaining social subscription constructors and
trace inputs out of `nmp-core`/generic planner APIs. Keep the existing
`ActorCommand::OpenContactListSubscription { kinds }` direction: hosts or NIP
modules declare the kind set, and the substrate registers/executes those kinds as
data. Tests covering follow-feed behavior must use arbitrary host-declared kinds,
not `{1, 6}`, unless the test is explicitly scoped to a NIP-01/NIP-18 module.

**Note (out of scope, separate cleanup):** `crates/nmp-core/src/kernel/ingest/event.rs`
and `ingest/eose.rs` are orphan files (not declared as modules in `ingest/mod.rs`,
so not compiled). `event.rs` contains a stale duplicate of `on_mailbox_changed`
still carrying `&[1, 6]`; it is dead code and was left untouched to keep this PR
scoped. A follow-up should delete the orphan files.

### V-06 · NIP-42 AUTH incompatible with NIP-46 remote signers [MEDIUM · staged fix required]

**Verified:** `crates/nmp-core/src/actor/commands/identity.rs:700` —
`sync_kernel_auth_signer` clears the auth signer when a remote NIP-46 signer is active
(`kernel.clear_auth_signer()`). The broker's ephemeral key cannot sign NIP-42 challenges
as the user's pubkey.

**Impact:** users authenticating via bunker (NIP-46) cannot sign NIP-42 AUTH challenges
with their own pubkey. They can still connect to and read from relays that accept
unauthenticated connections, but they cannot pass AUTH-required relay gates as themselves.
This is a silent failure: no toast, no indicator.

**Why the fix is staged:** the broker must expose a `sign_event(kind:22242)` RPC path;
then `AuthSignerFn` needs a sync-compatible adapter that round-trips through the broker's
one-shot channel. This is non-trivial broker work.

**Staged fix plan:**
- Stage 1 ✅ DONE: When active signer is remote and `clear_auth_signer` runs, toast
  "Relays requiring NIP-42 authentication are not supported with bunker accounts yet."
  Only fires on the transition from having auth capability to losing it (not on every
  `sync_kernel_auth_signer` call). See `identity.rs:703-717`.
- Stage 2: Broker side — expose `sign_auth_challenge(challenge, relay_url)` RPC.
- Stage 3: `sync_kernel_auth_signer` — for remote signers, install a
  `AuthSignerFn`-compatible closure that drives the broker RPC synchronously.

**Deadline:** Stages 2-3 are post-v1.

### V-08 · DM inbox silent failure for bunker accounts [MEDIUM · staged fix required]

**Verified:** `crates/nmp-nip17/src/inbox.rs:205` — `DmInboxProjection::snapshot()` returns
`DmInboxSnapshot::empty()` when `local_keys` is `None` (i.e. the active account uses a
remote NIP-46 signer). A host cannot distinguish "no signer yet" from "remote signer
that cannot unseal gift-wraps."

**Impact:** bunker (NIP-46) users see an empty DM inbox with no explanation. The host
must choose between "show loading indicator forever" or "show empty state as if no DMs
exist" — both are wrong. Silent degradation with no user-visible signal.

**Staged fix plan:**
- Stage 1 ✅ DONE: Added `remote_signer_unsupported: bool` (with `#[serde(default)]`) to
  `DmInboxSnapshot`. When `local_keys` is `None`, `snapshot()` sets it `true`. The flag is
  included in the snapshot JSON so Swift can read it. Backward compatible (old decoders
  read `false` for the missing field).
- Stage 2 ✅ DONE: `DmListView` checks `store.remoteSignerUnsupported` and shows a
  `bunkerUnsupportedState` banner with "DMs unavailable – bunker accounts cannot decrypt
  messages yet." The compose button is disabled in this state.
- Stage 3: ADR-0026 Phase 2 follow-up: implement `unwrap_gift_wrap` via remote signer RPC,
  delete the flag.

**Deadline:** Stage 3 is post-v1.

### V-12 · Production files above 500-LOC ceiling [MEDIUM · ongoing test extraction]

*Production splits needed (no test section to extract; post-v1). LOC refreshed
from the 2026-05-29 audit:*
- `crates/nmp-core/src/kernel/mod.rs` — 2358 LOC (grew significantly)
- `crates/nmp-core/src/actor/dispatch.rs` — 1967 LOC
- `crates/nmp-core/src/actor/mod.rs` — 1852 LOC
- `crates/nmp-nostr-lmdb/src/store/lmdb/mod.rs` — 1495 LOC
- `crates/nmp-core/src/actor/commands/identity.rs` — ~1223 LOC production
- `crates/nmp-core/src/actor/commands/publish.rs` — 816 LOC (no test section)

*Removed from this list:*
- `crates/nmp-core/src/ffi/mod.rs` — no longer exists; migrated to `nmp-ffi`.
- `crates/nmp-core/src/kernel/update.rs` — now 282 LOC, under ceiling (FIXED). The
  view-projection cluster was split into the new
  `crates/nmp-core/src/kernel/update/projections.rs` (275 LOC, under ceiling).
- `crates/nmp-core/src/publish/engine.rs` — now 458 LOC, under ceiling.
- `crates/nmp-core/src/kernel/relay_diagnostics.rs` — now 420 LOC, under ceiling.

### V-14 · Bunker reconnect — iOS/Android consumption of `BunkerConnectionState` projection [MEDIUM]

**Step b DONE (PR feat/v14-bunker-connection-state).** The kernel now emits `projections["bunker_connection_state"]` with `state`/`is_connected`/`is_reconnecting`/`is_failed`/`reason` derived from real Pool relay-lifecycle events. D0 clean, D4 compliant, tested.

**Remaining:** iOS/Android shells must consume the projection — show a reconnecting indicator, prompt re-auth on `is_failed`. Swift `BunkerConnectionState` Decodable stub needed in `KernelBridge.swift`; Android equivalent in the Kotlin bridge. Deadline: before v1-A (without host consumption the silent-brick UX is unchanged for the user).

**Deadline:** before v1-A.

### V-107 · Migrate gallery + marmot consumers off bespoke pull-snapshot symbols onto the canonical projection seam [HIGH · PRIORITIZED FOR AWARENESS]

**RATIFICATION LANDED — ADR-0039 (2026-05-29):** the push-vs-pull decision this
item was gated on is now made — ADR-0039 mandates the push seam and **rescinds the
ADR-0025 Step-12 read-leg sanction** that kept the Marmot pull symbols alive. Gallery
chain already removed (PR #791); `nmp_app_chirp_snapshot` already gone (PR #733/#766).
**Now unblocked:** migrate the live Marmot read-leg (`nmp_marmot_snapshot`,
`nmp_marmot_group_messages`) per the recipe below, then delete the symbols.

**Surfaced 2026-05-29 (podcast-player polling incident).** A downstream app
(`/Users/pablofernandez/Work/podcast-player`) independently reinvented the
`nmp_app_*_snapshot` *pull* accessor + 500 ms `Task.sleep` poll loop — a D8
violation — because the canonical reactive seam is **undocumented in the
builder-guide** and the nearest in-repo examples are bespoke pull symbols. This
is the same anti-pattern recurring (ADR-0025 / ADR-0037 deprecation target), and
it will keep recurring in every new app until the live in-repo consumers are
migrated and the positive pattern is taught.

**The bespoke pull-snapshot cluster (this repo) — refined by the 2026-05-29
`snapshot-projection-cleanup` workflow (8-agent fan-out):**
- `nmp_marmot_snapshot` (`crates/nmp-marmot/src/ffi.rs:422`; header `NmpCore.h:487`)
  and `nmp_marmot_group_messages` (`ffi.rs:435`; `NmpCore.h:488`) — **genuinely
  live** (real callers: chirp-repl, chirp-tui, `MarmotBridge.swift`, `nmp-app-chirp`
  re-export). **These are the real migrate-first work.**
- `nmp_app_gallery_snapshot` (+`_free`) — **NOT a live consumer**: the Kotlin
  `gallerySnapshot()` and Swift `gallerySnapshotJSON()` wrappers are defined but
  have **zero call sites**; the symbol returns only `{schema, alive, projections:{}}`
  (liveness already covered by `nmp_app_is_alive` + the push frame). It is **dead
  code** → removed in **PR #791** (no migration needed).
- `nmp_app_chirp_snapshot` — **already deleted** from master (PR #733/#766,
  commit `242802d7`), before this workflow. No action; mentioned only to close the
  lead.

**The work tracked here (NOT auto-dispatched):** migrate the two live **Marmot**
read-leg consumers onto the canonical seam — `register_snapshot_projection` →
`KernelSnapshot::projections` → pushed FlatBuffers `SnapshotFrame` read from
`projections[key]` in the host `apply()` — then **remove** the bespoke pull
symbols. `nmp_marmot_group_messages` is parameterized by `group_id_hex`, so its
migration carries the one real design choice (fold per-group message tails into
`nmp.marmot.snapshot` keyed by group id, vs. project active-group tails); resolve
in the ADR amendment. Drive each to completion; no half-landed state. These are
real shell changes, gated on orchestrator review, not run by an autonomous agent.
The ADR-0025 Step-12 sanction that keeps the Marmot read leg alive cites the
**now-deleted** Chirp pull precedent — so that sanction must be re-decided here too.

**Prerequisite — satisfied by ADR-0039 (2026-05-29):** ADR-0039 resolved the
push-vs-pull architectural question, mandating the push seam
(`register_snapshot_projection`). The earlier "blocked on V-37" framing is
obsolete — ADR-0039 confirmed the push seam already satisfies what V-37's
affordances were meant to provide.

**Related tracking (do not duplicate):** PD-039 (bespoke `nmp_app_*` symbol
retirement calendar; gallery/marmot pulls fall under it), PD-041 (Marmot
formally in the v1 support matrix), V-87 item 4 (stale
`apps/nmp-gallery/tui/src/live.rs:161-195` citation — re-audit before touching
gallery). Positive builder-guide guidance for the seam is being added by the
same 2026-05-29 workflow (root-cause fix for the recurrence).

---

### V-42 · NIP-23 / NIP-94 / NIP-96 absent from crates and untracked [post-v1]

**NIP-51 mute lists landed (2026-05-30).** `crates/nmp-nip51/` ships `MuteListProjection`.

**Follow-up (tracked here):** `nmp-wot` independently parses kind:10000 `p` tags in
`WotGraph::ingest_mute_list` for trust-scoring. Consolidating both onto `nmp-nip51`'s
decode (making `nmp-wot` depend on `nmp-nip51`) is a clean-up step, not v1 scope.

- **NIP-23 long-form articles** — post-v1. kind:30023 constant already in `tags.rs`.
  Need: decoder + `KernelEventObserver` projection. Effort: ~2 days.
- **NIP-94 / NIP-96 file metadata + media servers** — post-v1. Ships in every modern
  client for HEIC vs JPEG, dimensions, MIME, SHA-256. Need: `imeta` tag parser + action
  for upload. Effort: ~2 days per NIP.

---

### V-44 · No decrypt-only crate for iOS Notification Service Extension [v1-A if DMs ship · post-v1 Android]

**Evidence:** `aim.md` §7 open design question #5 (open since the start). No
`UNNotification` imports anywhere in `ios/` — Chirp ships NIP-17 DMs but users do
not receive push notifications when backgrounded.

`crates/nmp-nip59/` has the gift-wrap codec but exposing it requires linking the full
`nmp-core` static lib (actor, storage, relay code). Apple caps NSE binaries at 24 MB
total; the full kernel link far exceeds that.

**Recommended action:** add `crates/nmp-nip59-decrypt-only/` exposing a single function
`unwrap_gift_wrap(envelope_json: &str, local_nsec: &str) -> Result<String, String>`.
No actor, no storage, no relay code. Target: ~2 MB static lib.

### V-49 · F-05 codegen coverage is ~20% (9/45 structs as of 2026-05-29 audit) — "v1 QUALITY" label is misleading [MEDIUM · clarity fix]

**Evidence (code-grounded):** `ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift`
— 258 LOC, 8 generated structs. `ios/Chirp/Chirp/Bridge/KernelBridge.swift` — 1,895 LOC,
~40 handwritten `Decodable` structs. Coverage: 8/48 ≈ 17%. The remaining 40 are exactly
the types that change most often (snapshot payload, multi-state enums, projection clusters)
and benefit most from codegen. They're all blocked on tagged-enum support + `legacy_default`
override + per-field Swift-type overrides — each a separate architectural step.

**Recommended action:** split F-05 into "F-05a: Stage 1+2+3-partial (DONE)" + "F-05b:
tagged-enum emitter + full sweep (post-v1)"; drop "V1 QUALITY" framing on Stage 3.
The v1 pilot was a proof-of-concept — call it that.

---

### V-50 · Relay pool lifecycle + MailboxCache unification [post-v1 · residual from shipped nmp-router]

Per-kind routing shipped as `nmp-router` (2026-05-29). Two open residuals with unresolved decisions:

1. Optional `nmp-router`→`nmp-relay-pool` rename + relay-pool *lifecycle* ownership
   (connect/reconnect — genuinely new, exists nowhere).
2. Unify the planner-side mixed `MailboxCache` (NIP-65 + NIP-17) with the NIP-65-only
   `substrate::MailboxCache` (the V-40 follow-up named in `substrate/routing.rs:10-26`).

**Phase: post-v1.** Pairs with V-38/V-39/V-41 (open-ActorCommand seam).

---

### V-51 · No structural observability on routing decisions — apps can't surface "why did event Y go to relay B?" [HIGH] — **Phase 3 pending**

**Phases 1, 2, 4, 5 landed** (PRs #457, #476, #461, #462). Substrate observer,
FFI snapshot surface, validation harness, and kernel observability cut-over are all
in place.

**Phase 3 (Chirp inspector UI)** — not started. Pending the iOS / web
shell consumers of the phase 2 JSON payload (a `RoutingInspectorView`
long-press target on `ChirpEventCard` / publish-status row + a debug
toolbar toggle on the wasm host) plus `chirp-tui` relay diagnostics. Every
connected relay row must expose role, active wire-subscription count, durable
session EVENT count, and enough status to explain a zero count as either no
REQ, active REQ with no matches, EOSE/no matches, or a routing/configuration
anomaly. `chirp-tui` Settings must render the full active relay inventory
rather than only configured app relays; group rows by runtime category/source;
let the user select any relay; and show why the client is connected, current
wire subscriptions with exact raw REQ filters, per-sub and session event
counts, EOSE/close/error state, and traffic/reconnect counters. The title bar
and preview relay pane must label total vs preview counts consistently.
Indexer relays are part of this acceptance criterion: for discovery kinds
(`0`, `3`, `10002`, and other `10000..19999` lists), configured indexers must
be visibly targeted or the diagnostics must show why they were not.

---

### V-52 · LMDB relay index for `list_events_seen_on` [LOW · follow-up from single-relay browsing feature]

Single-relay browsing shipped (PR feat/v52-single-relay-browsing, 2026-05-30). The
`MemEventStore` has an O(1) `relay_url→event_ids` index; LMDB returns
`StoreError::NotSupported` until a secondary relay_url→event_ids B-tree index is added.
Callers can fall back to a provenance-scan in the meantime.

---

### V-53 · iOS Swift sweep for raw-data projection doctrine (ADR-0032) [MEDIUM · follow-up to ec8decad / display-separation PR]

**What we did:** ADR-0032 (commit ef9a9e50) records the raw-data projection
doctrine — the kernel and the four Layer-4 NIP projections now ship raw
protocol data; presentation layers own all formatting.
Rust shells (chirp-tui), the web TS shell, and the Android model +
TimelineScreen Compose row are aligned with the new doctrine.

**What's open:** the iOS Swift shell still reads the deleted projection fields
(146 sites across 17 files):
- Bridge Decodables: `KernelBridge.swift`, `ModularTimelineBridge.swift`,
  `MarmotBridge.swift`, `DmBridge.swift`, `FollowListBridge.swift`,
  `GroupChatBridge.swift`, `WalletBridge` slot in `KernelModel.swift`,
  `TimelineBlock.swift`, `Bridge/Generated/KernelTypes.generated.swift`.
- View files: `Features/{DmConversationView, ProfileView,
  MarmotGroupChatView, DmListView, GroupChatView, MarmotGroupsView,
  InvitesView, WalletView, AccountsView}.swift`,
  `Components/{ModularBlockView, ProfileNoteRow, ThreadNoteRow,
  NoteRowView}.swift`.

**Why deferred from the doctrine PR:** the Swift sweep is verifiable only in
Xcode (no `swiftc` build in the agent environment). Shipping unverified
`Decodable` changes risks runtime JSON-decode failures the agent cannot catch.
The Rust crates compile + test clean + codegen-drift clean, so the doctrine
landing is durable; this V-entry tracks the surface that still needs a human
Xcode pass.

**Status note (2026-05-29 audit):** the helper namespace already exists as
`ios/Chirp/Chirp/Extensions/PubkeyFormatting.swift` (NOT `DisplayFormat.swift`).
Its existing helpers cover the same `shortPubkey` / `relativeAgo` /
`avatarInitials` / `avatarColor` functionality. The remaining work is wiring the
still-formatted fields through it, not adding a new namespace. Remaining
formatted fields: `RelayDiagnosticsWireSub` (6 fields), `RelayDiagnosticsRow`
(7 fields), `ThreadView` (2 fields), `PublishOutboxItem` (5 fields),
`PublishOutboxRelay` (3 fields), and the `BunkerHandshake` labels.

**Approach:**
1. Reuse the existing `PubkeyFormatting.swift` helpers (`shortPubkey(_ hex: String)`,
   `relativeAgo(_ unixSeconds: UInt64)`, `avatarInitials(_ hex: String)`,
   `avatarColor(_ hex: String)`; 8+8 / `Xs/Xm/Xh/Xd ago` buckets / djb2 — matches
   the canonical `nmp_core::display::*` algorithms the Rust shells use). Mirrors
   the equivalent shell-side helpers added to `chirp-tui` in the doctrine PR.
2. For every Bridge Decodable file, drop the now-deleted CodingKeys + struct
   fields. Where the field becomes `Optional` (`authorDisplayName`,
   `authorPictureUrl`, `displayName`, `pictureUrl`, etc.), use `String?` +
   `decodeIfPresent`.
3. For every view file, replace reads of the deleted formatted fields with
   the locally-formatted equivalent via `DisplayFormat.*` over the raw
   `author_pubkey` / `created_at`.
4. Each step compiles + runs in Xcode before the next — the agent
   environment cannot verify, so this is human-in-the-loop work.

**Spec:** ADR-0032 §"Migration guidance for existing shell consumers".

**Out of scope** for this V-entry (deliberately): codegen Swift port of
`nmp-display` (separate ADR follow-up); generated Swift `Decodable` updates
in `Bridge/Generated/KernelTypes.generated.swift` regenerate automatically
once `nmp-codegen`'s Swift emitter is taught the new shape (the
`gen modules --check` gate against `apps/fixture/nmp.toml` is currently
green because the fixture types do not include these projection shapes).

### V-55 · `LocalKeySigner` cannot zero all `nostr::Keys` secret copies [MEDIUM · upstream-blocked]

**Verified:** `crates/nmp-signers/src/signers/local.rs:35-46` documents that
`nostr::Keys` retains the secret in private `secp256k1::SecretKey` and cached
`Keypair` storage that NMP cannot wipe. NMP keeps a redundant
`Zeroizing<[u8; 32]>` copy, which reduces but does not eliminate recoverable
secret material in freed memory.

**Correct fix:** upgrade to upstream `nostr` / `secp256k1` support that exposes
a zeroizable key type or mutable erasure hook, then delete the partial-mitigation
comment and prove all in-memory secret copies wipe on drop. Until upstream support
exists, do not claim full zeroization for local-key accounts.

### V-106 · iOS Chirp hardcoded 21,000 msat (21 sat) zap default — production UX hazard [MEDIUM · v1-A Chirp UX]

**Verified:** `ios/Chirp/Chirp/Bridge/KernelModel.swift:446` — `func zap(...) { amountMsats: UInt64 = 21_000, ... }` with a doc-comment at `:433-434` stating "defaults to 21,000 msats (21 sats) until an amount picker lands." Every zap dispatch from the iOS shell that doesn't explicitly pass an amount sends 21 sats.

**Impact:** users expecting a richer zap amount (e.g. 1,000 / 5,000 / 21,000 sats) send 21 sats because no picker exists. The default is in production iOS, not behind a feature flag, and the doc-comment promises a picker that has not landed. This is a user-facing UX defect, not framework debt.

**Correct fix:** ship the amount picker (sheet with 21 / 100 / 500 / 1k / 5k / 21k presets + custom field) and remove the function default. The Rust side (`nmp_nip57::zap`) already accepts `amount_msats`; the gap is purely Swift UI. Until the picker ships, the default should be an explicit `nil` that forces the host to surface a sheet rather than silently dispatching 21 sats.

---


### V-73 · `register.rs` falls back to empty `Pubkey` on null/invalid viewer_pubkey — anonymous register with no host signal [LOW · silent identity bug]

**Verified:** `apps/chirp/nmp-app-chirp/src/ffi/register.rs:114` — null or malformed `viewer_pubkey` is replaced with `Pubkey::default()` (32 zero bytes) and the register call proceeds. No error is returned to Swift.

**Impact:** the iOS host believes it registered a logged-in user; the Rust side proceeds with the all-zeros pubkey as the active viewer. Personal-timeline projections, NIP-65 outbox resolution, and DM inbox filtering all run against the zero-pubkey "anonymous" identity. The user appears to be logged in to themselves but is treated as the canonical empty account by every Rust subsystem.

**Correct fix:** the C-ABI `nmp_app_chirp_register` must return `NmpRegisterStatus::InvalidViewerPubkey` on null or non-32-byte input; Swift surfaces the failure to the onboarding flow. There is no doctrined reason for a register call with an invalid identity to silently succeed as anonymous.

---

### V-76 · `web/chirp` silently falls back to `InProcessNmpClient` on Worker construction failure [LOW · web production degradation]

**Verified:** `web/chirp/src/nmp/client.ts:43-47` — Worker construction failure is caught and the client downgrades to `InProcessNmpClient`, which runs nmp-wasm on the main thread. No console warning, no telemetry, no UI signal.

**Impact:** a user on a browser that fails to construct the Worker (CSP misconfiguration, Safari Lockdown Mode, restricted enterprise environment) sees a Chirp web app that "works" but blocks the main thread on every kernel tick. Performance is silently degraded; the diagnostic surface is empty.

**Correct fix:** the catch arm must `console.warn` with the Worker error and set a `nmp.client.runtime = "in_process_fallback"` field on the diagnostic snapshot so the host can render an unobtrusive "performance-degraded mode" banner. Production builds may additionally choose to refuse the fallback and surface an error to the user.

---

### V-78 · NIP-57 zap signing requires local keys — bunker (NIP-46) accounts cannot zap [MEDIUM · bunker feature gap]

**Verified:** `crates/nmp-nip57/src/lnurl/mod.rs:195-211` — `ZapAction::execute` short-circuits with a toast (`"zap requires a local-keys account; bunker signing for kind:9734 is not yet implemented (ADR-0026 Phase 2 follow-up)"`) when `ctx.active_local_keys()` returns `None`. This is the same ADR-0026 Phase 1 cutline as V-08 (DM unwrap) and V-06 (NIP-42 AUTH), but a separate code path — the broker has no `sign_zap_request(kind:22242→9734)` RPC and the actor thread has no sync-compatible adapter for it.

**Impact:** users authenticated via bunker can read zaps (kind:9735 receipts decode without keys) but cannot send a zap. The failure is non-silent (toast fires) so this is not a silent-fail violation, but it is a v1-A feature gap that is currently invisible from the BACKLOG. V-08 covers DMs and V-06 covers AUTH; zaps were missing as a tracked sibling.

**Staged fix plan:**
- Stage 1: surface the bunker-zap gap in onboarding / zap UI before the user attempts a zap (currently they only learn at zap time via toast).
- Stage 2: broker side — expose `sign_zap_request(unsigned_kind_9734)` RPC. Companion to V-06 Stage 2 (the broker is the same target; both bunker-sign paths land in the same RPC table).
- Stage 3: `ZapAction::execute` — when `active_local_keys()` is `None`, drive the broker RPC synchronously through the same one-shot channel pattern as V-06.

**Deadline:** Stages 2-3 are post-v1. Either this is fixed or v1 copy drops "send zaps" as a v1 capability for bunker accounts.

---

> **Provenance — V-87 … V-105 (2026-05-29 GH-issue audit, issues #600–#630).**
> These nineteen entries fold the 31 open GitHub issues from the offline-first /
> doctrine audit into Section 1. Every citation below was re-confirmed against
> HEAD (`c5302157`) before being recorded — per the Section 1 invariant, no entry
> asserts a live violation that the current tree does not exhibit. Where an issue's
> originally-filed `file:line` had drifted, the citation is corrected here; where
> the described violation is **already fixed at HEAD**, the entry says so and the
> action is to close the stale GH issue rather than re-open a phantom violation.

### V-87 · D1 startup violations cluster — iOS/shell legs open [HIGH · pre-v1 · issues #603–#606]

The D1 / offline-first contract (`docs/product-spec/offline-first.md` §1–§6):
the first rendered frame must not depend on relay I/O or relay connectivity.

Items #600, #601, #602 (kernel half) resolved in PR fix/v87-kernel-d1-startup
(2026-05-30). Remaining open items:

1. **#603 — CITATION STALE. Re-scope before fixing.** The filed citation
   `apps/nmp-gallery/tui/src/live.rs:161-195` (`bootstrap()` chaining six
   `recv_timeout` loops) does **not** exist at HEAD: `live.rs` is 217 lines, has
   no `bootstrap` fn and no `recv_timeout` call. Re-audit the gallery TUI live
   path (`live.rs`, `embed_host.rs`) for any remaining pre-first-frame blocking
   loop and re-file with a HEAD-accurate citation, or close #603.
2. `ios/Chirp/Chirp/Features/HomeFeedView.swift:101` [#604] — empty
   `blocks`/`items` renders `ChirpPlaceholder(…)` until the first kernel tick;
   the shell cannot distinguish "no events" from "not yet ticked". **DEFERRED
   (iOS/shell leg).**
3. **#605 — CITATION STALE.** `ios/Chirp/Chirp/Features/ThreadScreen.swift` (202
   lines) does **not** contain the string "Fetching notes from the relay network"
   anywhere in the iOS tree, and the `threadView == nil` hard-gate at `:30-64` is
   not present as filed. Re-audit `ThreadScreen.swift` for the current loading
   gate and re-file with a HEAD-accurate `file:line`, or close #605. (See V-99 —
   the user-facing-copy half of this issue is also stale.) **DEFERRED (iOS/shell
   leg).**
4. `crates/nmp-core/src/kernel/types.rs:184` [#606] — `ProfileCard.has_profile:
   bool` is consumed as a render gate at
   `ios/Chirp/Chirp/Features/ProfileView.swift:142,168` (`profile?.hasProfile ==
   true`). It trains callers to block fields on relay data. **Confirmed live**
   (the iOS gate is real; the originally-filed gallery `live.rs:419` cite is
   stale — `live.rs` is only 217 lines — so the gallery half needs re-citing).
   **DEFERRED (iOS/shell leg).**

### V-88 · View payload `state` string invites render-gating [MEDIUM · P2/D1 · issue #607]

**Verified:** `crates/nmp-core/src/kernel/types.rs:240` (`AuthorViewPayload`,
struct at `:238`) and `:277` (`ThreadViewPayload`, struct at `:274`) each carry
`pub(super) state: String` with values `"queued"`/`"opening"`/`"ready"`. The
`"ready"` value structurally invites `if state == "ready" { render() }` — the
offline-first anti-pattern. Subscription-lifecycle state is an internal kernel
concern and must not surface on the view payload as a render gate.

**Correct fix:** Remove `state` from `AuthorViewPayload`/`ThreadViewPayload`;
always emit whatever local data is available; move lifecycle state to a
debug/diagnostics-only channel.


### V-111 · Actor thread retains a *callable* blocking-sign primitive [LOW · D8-hardening]

**Verified:** the blocking `sign_active` (`crates/nmp-core/src/actor/commands/identity.rs:693`,
does `.wait(REMOTE_SIGN_TIMEOUT)` when `active_remote()` is `Some`) still has 3
production call sites — all inside `create_account`/`publish_initial_follows`
(925/963/1118). They are safe *today* only because `create_account` activates a
local key first (enforced by `debug_assert!(active_remote().is_none())`), so the
`.wait()` branch is never reached. But the blocking primitive remains callable
from actor-thread code: a future caller that signs while a bunker is active
would silently re-introduce an actor freeze.

**Correct fix (separate refactor, its own blast radius — NOT bundled with V-90):**
make it structurally impossible for the actor thread to block on signing — either
delete `sign_active` and route its 3 callers through `sign_active_nonblocking`
(then the park arms become live, not dead), or `#[cfg(test)]`-gate `sign_active`
(only tests at 295/581/877 use the blocking form) so production actor code
physically cannot call it. Pick after measuring whether the nonblocking
conversion of `create_account` is worth the park-arm complexity.

### V-91 · Android nativeNextUpdate blocks calling thread 250ms per poll [MEDIUM · P2/P3 · issue #614]

**Verified:** `crates/nmp-android-ffi/src/lib.rs:185` —
`Java_org_nmp_android_KernelBridge_nativeNextUpdate*` (declared `:163,:172`) calls
`s.rx.recv_timeout(Duration::from_millis(250))`, forcing Kotlin into a polling
drain loop with a 250 ms blocking budget per call. iOS uses a push model (callback
on the listener thread); Android should match. The `recv_timeout` polling pattern
is a D8 violation at the FFI boundary.

**Correct fix:** Replace the `recv_timeout` polling pattern with a push-based
callback notification model matching the iOS `set_update_callback` architecture.

### V-93 · Kernel constructor blocks synchronously on LMDB open and pending load [MEDIUM · D1/P3 · issue #617]

**DESIGN PRODUCED (2026-05-29, ADR-pending).** Defer ONLY the publish-intent load
(not the LMDB open), resolved as a one-shot step **on the actor thread** after the
first snapshot emit (never a background thread — that framing self-inflicts the
construction races). Measurement-first: confirm `load_pending_us` actually dominates
`construction_us` before implementing, else the deferral fixes the wrong cost. Needs
ADR.


**Verified:** `crates/nmp-core/src/kernel/mod.rs:1020` (`build_event_store`, LMDB
open) and `:1098` (`load_profile_intents`, which walks all pending publish records
via `publish_store.load_pending()` at `:1102`) run synchronously in the construction
path. A slow LMDB open or a large publish-intent backlog delays kernel construction
and therefore blocks the first snapshot emit. Related to V-67 (LMDB silent
degradation) but a distinct startup-latency issue. **Confirmed live.**

**Correct fix:** Defer the publish-intent load to after the first snapshot is
emitted; open LMDB asynchronously or on a background task that resolves before the
first publish command needs it.

### V-94 · `nmp_app_start` ordering — C-ABI runtime guard for non-Rust hosts [MEDIUM · P3 · issue #618]

Rust composition root is now compile-time-enforced (`NmpAppBuilder<S>` typestate,
`crates/nmp-app-template/src/builder.rs`; design: `docs/design/v94-app-config-ordering.md`).

**Open:** Swift/Kotlin hosts driving raw C-ABI symbols get no compile guarantee —
add a runtime `KernelDiagnostic::LateWiring` (design doc §3.2).

### V-95 · WalletRuntime initialization order not type-enforced — OnceLock error risk [MEDIUM · P2/P3 · issue #619]

**DESIGN PRODUCED (2026-05-29, ADR-pending).** Candidate B: relocate the wallet runtime
to app-scoped actor-side host-extension state and **delete the process-global
`OnceLock`**, without touching the `ActionModule` trait. Goal reframe (load-bearing):
"type-enforced init order" is unachievable at the stringly-typed `dispatch_action` site
— the real win is deleting the global. Use a **substrate-generic** typed-extension
accessor, NOT a wallet-named getter on `nmp-core`'s `ProtocolCommandContext` (D0 hazard).
Needs ADR to ratify B-vs-A.


**Verified:** `crates/nmp-nip47/src/runtime.rs:107` (`install_wallet_runtime`)
populates a process-global `OnceLock` read by `active_wallet_runtime()` (`:80`
doc); `WalletConnectModule`/`WalletDisconnectModule`/`WalletPayInvoiceModule`
`execute` read it and return a runtime error when `install_wallet_runtime` was
never called (`:114` doc). The type system does not prevent dispatch before
installation. **Confirmed live.**

**Correct fix:** Pass `Arc<Mutex<Option<WalletRuntime>>>` as a field on each module
struct (injected at registration time via `nmp-app-template`), eliminating the
global and making the initialization order visible from the type signature.

### V-97 · Four sign-in paths to the same "activate local account" operation [MEDIUM · P1 · issue #622]

**Verified:** `crates/nmp-ffi/src/lib.rs:1496` (`sign_in_nsec`), `:1502`
(`restore_local_nsec_from_keyring`), `:1518` (`sign_in_local_nsec_with_keyring`),
and the C-ABI `nmp_app_signin_nsec` (`lib.rs:80`) all activate a local account
with subtly different key-storage semantics (the latter three funnel into
`sign_in_nsec`). No structural signal guides new app authors to the correct path.
**Confirmed live.**

**Correct fix:** Consolidate behind one public path (`sign_in_with_local_nsec(
keyring: bool)`); make the others `pub(crate)` or document them as internal
migration shims with explicit deprecation.

### V-98 · iOS WalletView switches on raw Rust wire status strings [MEDIUM · P5/V-53 · issue #623]

**Verified:** `ios/Chirp/Chirp/Features/WalletView.swift:71`
(`status.status.capitalized` — Swift reformats a wire key for display), `:93`
(`status.status == "connecting"` — branches on the raw protocol string), and
`:69,:73` feeding `statusColor(_:)` (`:108`) which switches on the wire string
(e.g. `case "connecting"` at `:111`) for theme color. **Confirmed live.** Three
P5 violations.

**Correct fix:** Rust projection emits `statusLabel: String`, `statusTone: String`
(e.g. "warning"/"success"/"idle"). Related to V-53 (ADR-0032 iOS sweep) — fold
this site into that sweep.

### V-99 · iOS UI copy references the relay network [LOW · P5/D1 · issue #624] — CITATIONS STALE

**Status:** both filed citations describe user-facing copy that does **not** exist
at HEAD. `ios/Chirp/Chirp/Features/ThreadScreen.swift:58-59` does not contain
"Fetching notes from the relay network" (no such string anywhere in the iOS tree),
and `HomeFeedView.swift:113` no longer carries the "Loading your timeline…"
subtitle as filed. The copy appears to have already been changed.

**Required fix:** Re-audit the iOS loading/placeholder copy at HEAD
(`HomeFeedView.swift` placeholder branch `:101`, `ThreadScreen.swift` loading
state) for any remaining relay-dependency phrasing and re-file #624 with accurate
`file:line`, or close it. The doctrine (never communicate relay-dependency to
users; offline-first copy only) stands regardless. Paired with V-87 items 5–6.

### V-101 · iOS NIP-29 group relay URL hardcoded in Swift `@State` [LOW · P5 · issue #626]

**Verified:** `ios/Chirp/Chirp/Features/NewGroupSheet.swift:26` —
`@State private var publicRelayUrl = "wss://relay.groups.nip29.com"`, a
compile-time third-party URL baked into Swift state. **Confirmed live.**

**Correct fix:** Surface a default NIP-29 relay URL through the kernel
configuration projection so it can be updated without a client release. Related to
the hardcoded-URL-in-substrate category (V-65 fixed in PR; same pattern).

### V-102 · `TimelineEventCard`/`ModularTimelineSnapshot` are app-domain types exported from a protocol crate [MEDIUM · D0 · issue #627]

**Verified:** `crates/nmp-nip01/src/timeline_projection.rs:44`
(`pub struct TimelineEventCard`) and `ModularTimelineSnapshot` in the same module
embed app-layer concerns (display-name formatting, picture URLs, timeline
windowing, feed cursor) but are exported from a protocol crate. "Timeline" and
"feed card" are app nouns forbidden from NIP crates under D0. **Confirmed live.**

**Correct fix:** Move these types to `crates/nmp-app-template/` or a new
`crates/nmp-social-feed/` crate. The protocol crate retains only the raw event
data types.

### V-108 · `ChirpTests/NoteContentRenderingTests.swift` references removed `noteContentGroups`/old `.inline` API — whole ChirpTests target fails to compile [MEDIUM · test rot · ChirpTests not CI-gated]

**Verified:** `ios/Chirp/ChirpTests/NoteContentRenderingTests.swift` calls
`noteContentGroups(tree)` (`:36`) and asserts against `ContentGroup.inline([1,2,3])`
plus an `embedDepth:` argument (`:75`). None of those symbols exist in the app
target anymore: commit `98dcd313` ("refactor(ios/chirp): align Swift consumers
with ADR-0032 raw-data doctrine") renamed the function to `nostrContentGroups`
(`Chirp/Components/NostrContent/NostrContentGrouping.swift:36`) and changed the
enum to `NostrContentGroup.inline(level:children:)` (`:10`). The stale test was
never updated, so the **entire `ChirpTests` target fails to compile** with
`Cannot find 'noteContentGroups' in scope` / `Type 'Equatable' has no member
'inline'` / `Extra argument 'embedDepth' in call`.

**Why it survived:** `ChirpTests` is **not gated in CI** — no `.github/workflows/*`
invokes `xcodebuild`/`-scheme Chirp`/`ChirpTests` (the iOS smoke suite runs only
under `NMP_SMOKE=1`, and `SmokeScenariosTests` self-`XCTSkip`s otherwise). The
break has been latent since `98dcd313`.

**Impact:** any agent running the Chirp Swift unit suite hits a target-wide compile
failure and cannot run *any* ChirpTests class (e.g. the new
`ProfileNameFallbackTests`) without first neutralizing this file locally.

**Correct fix:** re-derive the assertions under the new `nostrContentGroups` /
`NostrContentGroup.inline(level:children:)` semantics (group count + `level`
values changed; do NOT mechanically swap names — the old `groups.count == 2` /
`.inline([1,2,3])` expectations encode the pre-98dcd313 grouping and would be a
compiling-but-wrong test). Separately, decide whether `ChirpTests` should be
CI-gated so this class of rot is caught at the PR boundary rather than latently.

---

### V-109 · Android does not build or expose Marmot/MLS [MEDIUM · platform gap]

**Verified blockers (three distinct layers):**

**(a) Build:** `android/app/build.gradle.kts` passes `cargoNdk` only `build --release`
with no `--features marmot` flag. The marmot feature is never compiled into the Android
native library.

**(b) Cargo:** `nmp-android-ffi/Cargo.toml` pulls `nmp-app-chirp` with
`default-features = false`, which explicitly excludes the `marmot` feature. Even if
cargoNdk were amended, the Rust build would not include Marmot code.

**(c) UI/FFI surface:** Zero Marmot/MLS/NIP-29 UI or FFI in `android/app/src/` — no
Groups tab, no key-package or welcome screens, no MLS-related FFI calls. The iOS
`justfile` passes `--features marmot` to every build target; Android never does.

**Contrast with iOS:** iOS wires the marmot feature explicitly (`--features marmot` in
the justfile), links `libnmp_app_chirp.a` which includes `nmp_marmot_*` symbols, and
ships a Groups tab backed by that FFI surface. Android has no equivalent path.

**Correct fix:** (1) Add `--features marmot` to the `cargoNdk` invocation in
`android/app/build.gradle.kts`. (2) Enable `marmot` in `nmp-android-ffi/Cargo.toml`
(or pass it through cargoNdk flags). (3) Build the Android Groups UI: key-package
publish, pending-welcome accept, group message send/receive — mirroring the iOS
`GroupsView` / `MarmotViewModel` surface. The Rust MLS runtime is already proven
correct (in-process round-trip verified 2026-05-31 via `NMP_MARMOT_MOCK_KEYRING=1`);
the gap is entirely in the Android build wiring and UI layer.

---

### V-110 · `KernelAction::OpenView` silently no-ops — general view-lifecycle seam unwired [MEDIUM]

**Provenance:** Identified while fixing V-110's first known consumer (the Marmot key-package
fetch mis-wire, fixed in `fix/marmot-keypackage-fetch-miswire`).

**What is wrong:** The `OpenView` reducer arm in `crates/nmp-core/src/kernel_action.rs:51` echoes
`KernelUpdate::ViewOpened { namespace, key }` without compiling the named view's declared
`dependencies()` into any registry interests. No relay subscription is opened. The arm is
annotated as a placeholder ("Lifecycle / view variants have no resolver yet") but fails
**silently** — callers receive the expected `ViewOpened` update and have no signal that
nothing happened on the wire.

**Known consumers and impact:**
- `crates/nmp-marmot/src/projection/state.rs` — `request_key_package_fetch` (Marmot leg A): **FIXED** in this PR by bypassing `OpenView` and routing through `push_interest(key_package_lookup_interest(pk))`, consistent with legs B/C.
- `crates/nmp-marmot/src/fetch.rs` — `nmp_marmot_fetch_key_packages` C-ABI: **FIXED** in the same PR.
- `crates/nmp-core/src/actor/tick.rs:156` — UI view opens from tick dispatch.
- `crates/nmp-core/src/wasm/dispatch_routing.rs:94` — WASM dispatch path.

**Correct fix (separate PR):** Either (a) wire `OpenView` to look up the named view's
`dependencies()` in the view registry and compile them into kernel interests (the full
view-lifecycle story — generalises `OpenUri`'s pattern), or (b) as a minimum safety net,
**make the stub FAIL-LOUD**: `trace!`/`warn!` with a stable message such as
`"OpenView({namespace}, {key}) has no resolver — interest not compiled"` so the next
mis-wiring surfaces immediately instead of vanishing. Option (b) alone is a one-line change
that eliminates the silent-drop hazard without building the full resolver.

**Do NOT fix in the same PR as the marmot re-wire** — `OpenView` may be load-bearing for
other consumers in the iOS tick path. Treat as a follow-on MEDIUM item.

---

Work currently on a branch lives in [`WIP.md`](../WIP.md). Agents must check that file
before picking up Section 4 work to avoid duplicating an in-progress task.

---

## Section 3 — Pending User Decisions

Items that cannot be resolved autonomously. An agent that encounters one of these must log
its finding in the decision thread below and move on to the next item, not block.

### PD-033-A · Framework thesis — second non-social app — RE-OPENED AS BUILDABLE (2026-05-29, ADR-0039)

**UNBLOCKED — zero new affordances required (2026-05-29, ADR-0039).** The reassessment
inverts the long-standing "V-37 blocks PD-033-A" framing. The push projection seam
already satisfies every property the deleted Notes app violated: kernel-owned
projection (no D5 shell parsing), handshake-gated sign-in (via the existing
`projections["bunker_handshake"]`), and D3 outbox routing (not a raw-event tap), all
read off the pushed frame. None of V-37's three affordances need to be built (see
ADR-0039 §3). The **podcast-player** is the live candidate — to be built on the push
seam (deleting its current bespoke `nmp_app_podcast_snapshot` pull symbol + 500 ms
poll). History of the deletion-closure retained below.


**Original closure (PR #377 — merged 2026-05-23):** `apps/notes/` is a minimal NIP-01 note
client, 299 LOC Swift, 25 LOC Rust, zero new C-ABI protocol symbols. Closed as "confirmed."

**Re-opened (Opus direction review #13 — 2026-05-24):** Code-grounded inspection of the
artifact found it does NOT use the framework's defining properties:

- `NotesBridge.swift:74` calls `nmp_app_register_raw_event_observer` with a kind:1 filter
  only — this is a raw event *tap* (every ingested kind:1 fans out regardless of author).
  D3 outbox routing is bypassed entirely; `KindFilter` (`raw_event_observer.rs:92`) has no
  author dimension.
- `NoteModel.swift:14` parses the NIP-01 event JSON in Swift (`JSONSerialization →
  [String: Any]`). The first anti-pattern (D5: never parse protocol data in the shell).
- `NotesBridge.swift:84` orders the timeline in Swift (insertion-order keyed off arrival,
  not `created_at`). The kernel owns no timeline view for this app.
- `TimelineView.swift:30, 36–38` formats timestamps + shortens pubkeys in Swift.
- `NotesBridge.swift:36–37` sets `isSignedIn = true` synchronously with no handshake-
  success gate.

**Resolution (user decision 2026-05-28):** `apps/notes/` deleted, along with the
superseded read-only spike `apps/longform/`. The framework thesis remains **unproven**
for stateful non-social apps — the substrate does not yet expose the three affordances
required (`NmpSnapshotProjector` context pointer, generic `nmp_app_get_snapshot` pull
path, `LogicalInterest::FollowSetKind1` or equivalent). PD-033-A is closed with the
explicit acknowledgement that the framework is not yet expressive enough to host an
honest second app. The thesis may be revisited when V-37 (snapshot output seam for
non-Chirp apps) and V-45 (`LogicalInterest` follow-set variant) land.

### PD-039 · Bespoke FFI deprecation calendar (D11 expansion) — DECISION MADE 2026-05-23

**Decision settled (this PR):** the bespoke `nmp_app_*` C-ABI surface in
`crates/nmp-core/src/ffi/` is sorted into four categories. The calendar fixes
which symbols are migration debt vs. permanent by doctrine, the migration
cadence, and the doctrine reviewers apply to new additions. Companion to v1
exit criterion #7 in [`docs/plan.md`](plan.md#v1-exit--what-has-to-be-true-to-ship).

**Inventory on 2026-05-23 (HEAD `4fd656dd`, 48 symbols total):** 1 canonical
(`nmp_app_dispatch_action`); 1 already a thin shim over `dispatch_action`
(`nmp_app_wallet_pay_invoice`); 26 structural permanent under Theme A
(lifecycle / callbacks / capability sockets / observer + projection
registration / NWC connection lifecycle / publish control plane / liveness
probe / action-stage acks); 4 test-only (`cfg(feature = "test-support")`); **16
migration debt** (user-intent verbs that send `ActorCommand::*` directly).

**Rule (in force from 2026-05-23):** No new `nmp_app_*` symbol may be added
without a merged ADR. The CI gate
[`ci/check-ffi-surface-freeze.sh`](../ci/check-ffi-surface-freeze.sh)
(`.github/workflows/ffi-surface-freeze.yml`) rejects net-additions by default;
genuinely-structural additions are exempted via `ADR_OVERRIDES` (precedent:
`nmp_app_is_alive` / ADR-0028).

**Cadence — target zero migration-debt symbols at v1-B:**
- Batch 1 (pre-v1-A): 0 deletions — every debt symbol has a live Swift caller.
- Batch 2 (v1-A → v1-B, ~2/quarter): identity (5) + relay-edit (2) = 7
  symbols migrate to `nmp.identity.*` / `nmp.relays.*` namespaces.
- Batch 3 (v1-B): 9 view/subscription-registry mutations migrate to
  `nmp.timeline.*` (or 2 reclassify as structural — `claim_profile` /
  `release_profile` are handle refcounts, not actions).

**Definition of done per migrated symbol:** body becomes a thin
`dispatch_action_json(Some(app), "<namespace>", &json)` shim (the pattern
`nmp_app_wallet_pay_invoice` already follows; `ffi/wallet.rs:119`). The
C-ABI symbol is retained for byte-stable Swift compatibility; only the body
changes. Net-zero ABI churn.

Full per-symbol inventory, Theme A doctrine, batch-by-batch namespace map, and
adjacent hygiene items (header drift in `NmpCore.h`; signer-broker /
nmp-app-chirp symbols outside this calendar's scope) live in
[`docs/architecture-audit/ffi-deprecation-calendar.md`](architecture-audit/ffi-deprecation-calendar.md).

### PD-041 · Marmot/NWC scope reconciliation — RESOLVED 2026-05-29

**Decision (2026-05-29):** Marmot/MLS (`nmp-marmot`, `nmp-nip29`, `nmp-nip59`)
and NWC + NIP-57 (`nmp-nwc`, `nmp-nip57`) are formally accepted into the v1
support matrix. They are fully built and wired; the V-61–V-64/V-79 backend
sweep confirmed production quality. `docs/plan.md` and product copy treat
these as v1 capabilities.

Earlier temporal planning deferred M9 DMs/messaging and M12 Wallet to post-v1,
but these surfaces were subsequently built, wired, and swept for silent-failure
violations in the v0.1.0/0.1.1 backend pass. The decision to include them is
recorded here as closed.

---

## Section 4 — V1 Feature Backlog

Ordered by blocking priority. Items earlier in the list unblock items below them. An
autonomous agent picks the topmost item not already in Section 2.

### F-00 · Unify app directory layout — `apps/<app>/{ios,android,desktop,tui,web}` + `apps/<app>/crates/` [PRIORITY · repo structure]

**Problem:** `ios/Chirp/` and the monolithic `android/` project (containing both `:app`
for Chirp and `:gallery` for Gallery) live at the repo root, while `apps/nmp-gallery/`
already hosts its own `{ios,android,desktop,tui}` platform subdirectories. The result is
two conflicting layout conventions in the same repo.

**Target layout:**
```
apps/
  chirp/
    crates/          # app-specific Rust crates (nmp-app-chirp, nmp-chirp-config)
    ios/             # ← move from ios/Chirp/
    android/         # ← extracted from android/app/ (standalone Gradle project)
    desktop/         # already in place
    tui/             # already in place
    repl/            # already in place
  nmp-gallery/
    crates/          # ← move nmp-app-gallery here
    ios/             # already in place
    android/         # already in place (standalone; supersedes android/gallery/)
    desktop/         # already in place
    tui/             # already in place
  fixture/
    crates/          # ← move fixture-todo-core, nmp-app-fixture here
```
Top-level `ios/` and `android/` directories are deleted after migration.

**Key complication — Android is a monolithic multi-module Gradle project:**
`android/settings.gradle.kts` includes both `:app` (Chirp) and `:gallery` (Gallery) as
subprojects in a single Gradle build. `apps/nmp-gallery/android/` already exists as a
standalone Gallery Gradle project, so gallery is partially duplicated. The migration must:
1. Extract Chirp's `:app` module into a standalone Gradle project at `apps/chirp/android/`.
2. Confirm whether `android/gallery/` is a live build target or superseded by
   `apps/nmp-gallery/android/`; consolidate to the latter and delete `android/gallery/`.
3. Delete the top-level `android/` wrapper once both sub-projects are self-contained.

**Full migration checklist:**
1. **iOS** — Move `ios/Chirp/` → `apps/chirp/ios/`; update `ios/Chirp/project.yml` root
   path, all `xcodegen` spec paths, Xcode scheme files, and `DerivedData` references.
   Regenerate `project.pbxproj` via `xcodegen generate`. Update `justfile` targets and
   `ci/check-ffi-header-drift.sh` / `ci/check-flatbuffers-version-pins.sh`.
2. **Android** — Create a standalone Gradle project at `apps/chirp/android/` with its own
   `settings.gradle.kts` (include `:app` only). Move source from `android/app/`. Update
   JNI / `.cargo/config.toml` library output paths. Audit `android/gallery/` vs
   `apps/nmp-gallery/android/` and delete the redundant copy.
3. **Rust crates** — Move `apps/chirp/{nmp-app-chirp,nmp-chirp-config}` →
   `apps/chirp/crates/`; `apps/nmp-gallery/nmp-app-gallery` →
   `apps/nmp-gallery/crates/nmp-app-gallery`; `apps/fixture/{nmp-app-fixture,fixture-todo-core}`
   → `apps/fixture/crates/`. Update workspace `Cargo.toml` members to use glob paths
   (`apps/chirp/crates/*`, etc.) and fix all inter-crate `path = "…"` dependencies.
4. **CI** — Update every path in `ci/check-ffi-header-drift.sh`,
   `ci/check-flatbuffers-version-pins.sh`, and any GitHub Actions workflow files.
5. **Justfile** — Update `ios/Chirp/…` and `android/…` references.
6. **Docs / README** — Update path references in `docs/` and top-level `README.md`.
7. **Verification** — `cargo test -p nmp-app-chirp`, `cargo test -p nmp-testing --test
   doctrine_lint_smoke`, Xcode build clean, Android `./gradlew :app:build` from new location.

**Prerequisite for:** nothing — purely structural, no feature dependency. Do not let this
block v1 work; tackle between feature slices.

---

### F-01 · Fix V-01 — IndexedDB store [V1 BLOCKER · partial]

All prior stages merged (chirp-web now supports NIP-07 PublishNote end-to-end).

**Remaining scope (still V1 BLOCKER):**
1. **IndexedDB store.** Port persistence to an IndexedDB-backed `nostr-database` impl.
   Kernel runs in-memory only and resets on page reload. Requires sync/async model decision
   (write-behind queue + in-memory cache vs. warm-boot-from-IDB on Start).

secp256k1-sys wasm32 C build remains environmentally gated on
`CC_wasm32_unknown_unknown=clang` (CI sets this; local builds need homebrew LLVM on macOS).

No `chirp-web` features requiring persistence across reloads may be added until IndexedDB lands.

### F-02 · DM cold-start receive-side verification [V1 BLOCKER]

Gift-wrap **send** landed; kind:10050 relay-list publish is wired. The **receive** side on a
fresh install has not been verified end-to-end. A new user who signs in for the first time
must receive DMs before NIP-17 can be called done.

**Rust-layer pipeline verified (PR #344 — merged):** `nmp_app_inject_signed_event_json` injects
a real signed kind:1059 gift-wrap through `IngestPreVerifiedEvents` → `notify_raw_event_observers`
→ `DmInboxProjection`. `nmp_app_read_projection_json("nmp.nip17.dm_inbox")` confirms the message
appears in the snapshot. The `dm_inbox_full_round_trip_through_ffi` test passes (no longer ignored).
The test also gates that cold-start `active_local_keys` seed path works without calling `Start`.

**Remaining:** device-level acceptance test against live relays (product QA, not CI-gatable).

**Acceptance test:** fresh account → receive a gift-wrapped kind:1059 from a second account →
message appears in the `nmp.nip17.dm_inbox` snapshot projection.

### F-04 · Zap E2E round-trip verification [V1 BLOCKER]

`ZapAction` is implemented and registered. `ZapsAggregateProjection` is registered. The full
round-trip — dispatch zap → `FetchLnurlInvoice` → bolt11 toast → `WalletPayInvoice` → NWC
`pay_invoice` → kind:9735 receipt → `ZapsAggregateProjection` update — has not been verified
against a live NWC wallet.

**Acceptance test:** connect real NWC wallet → tap zap → bolt11 invoice received via toast →
NWC `pay_invoice` fires → kind:9735 receipt ingested and reflected in `nmp.nip57.zaps`.

### F-05 · nmp-codegen Swift Decodables pilot [V1 QUALITY]

`crates/nmp-codegen` (1,212 LOC) has a working `generate_modules` CLI. `KernelBridge.swift`
was 1,988 LOC of handwritten counterpart types — a maintenance surface that diverges on every
snapshot field change.

**Remaining Stage 3 work (all blocked on emitter extensions):**

- `KernelSnapshot` (Swift `KernelUpdate`, `KernelBridge.swift:721`): needs a per-field
  Swift-type override mechanism so the `HashMap<String, serde_json::Value>` `projections`
  field can render as the existing generated `SnapshotProjections?` rather than an
  `[String: AnyDecodable]`. Also depends on the `legacy_default` flag (v6 plan §4d) for
  `updateKind` / `relayStatus`-style backward-compat optionality and on a place to host the
  20+ computed accessors (`var walletStatus`, `var profile`, etc.) that currently live on
  the hand-written struct (move them to an `extension KernelUpdate` in
  `KernelBridge.swift`).
- Tagged-enum support (`TimelineBlock` family in `TimelineBlock.swift`, `ActionStage`,
  `Nip46Onboarding.StageKind`): the emitter currently rejects non-flat-record schemas with
  `Unsupported`; needs the `oneOf` / `anyOf` rendering path.
- `legacy_default` override flag (v6 plan §4d) for forward/backward-compat fields the
  current Rust shape requires but older snapshots omitted.

These are each their own architectural step and merit separate PRs.

**Coverage note (V-49):** 8 generated structs / ~48 total Decodables = ~17% coverage.
The "v1 QUALITY" label applies to Stage 1+2+3-partial; Stage 3 remainder (tagged enums,
legacy_default, full sweep) is effectively post-v1. Consider renaming to "F-05a (DONE) /
F-05b (post-v1)" so the v1 claim is scoped accurately.

### F-08 · App-owned component registry + content rendering kits [V1 DX]

Promoted from the post-v1 bucket by user direction on 2026-05-25. This is the
M16 developer-experience track for reusable source components that apps can
install, own, customize, and update later.

Core product promise: registry components are reference-driven and reactive per
[`product-spec/overview-and-dx.md` §5.4](product-spec/overview-and-dx.md#54-registry-components-reference-first-reactive-ui).
App screens pass Nostr references plus styling/callbacks; installed components
own the platform lifecycle that claims, observes, hydrates, redraws, and releases
Rust-owned projections. Screens must not reimplement per-row profile/embed
hydration just because they render a component.

**Plan:** [`docs/plan/m16-component-registry.md`](plan/m16-component-registry.md).

**Status:** First implementation slice in progress: a built-in offline
component registry, `nmp add component`, `nmp.components.lock`, dependency
resolution, duplicate-install rejection, and the `swiftui/content-minimal`
fixture kit.

**Initial scope:**

- Component manifest and lock-file model for app-owned source installation.
- `nmp add component` and `nmp update component` over a local offline registry.
- Optional jsrepo-compatible export after the NMP-native path works.
- iOS SwiftUI content-rendering kits over `ContentTreeWire`.
- Android Compose content-rendering kits with matching names and fallback
  behavior.
- Renderer variants such as minimal mentions, avatar mentions, rich
  press-and-hold profile preview, compact quote cards, rich quote cards, media
  grids, and markdown/article rendering.

**Acceptance:** a clean app can install a content kit, render the shared content
fixtures, customize one renderer in app-owned source, update the upstream kit,
and preserve the local customization. For reactive components, the same app can
pass only a Nostr reference plus styling/callbacks and does not call
`claim_profile`, `release_profile`, `claim_event`, or `release_event` from the
feature screen.

**Progress (2026-05-25):** M16-C1 step "freeze the content fixtures / wire
contract" landed — `crates/nmp-content-fixtures/fixtures/wire/<id>.json`
holds 38 committed `ContentTreeWire` JSON golden files covering text
(`S-T01..S-T10`), mentions (`S-M01..S-M03`), quotes (`S-M04..S-M09`), lists
(`S-A03..S-A05`), media (`S-MD01..S-MD03`), links (`S-L01..S-L03`),
hashtags (`S-H01..S-H03`), and fallback edge cases (`S-E01..S-E07`).
Regenerate via `cargo run -p nmp-content-fixtures --bin build-wire-fixtures`;
drift is caught at test time by `tests/wire_fixtures.rs::wire_goldens_match`
(byte-exact pin + orphan-file guard). iOS and Android decoders consume this
exact byte set as the M16 cross-platform wire-contract truth.

**Kind-dispatch sub-track (ADR-0034):** the next M16 slice is the kind-dispatched
content rendering system.
Architectural decisions: [`ADR-0034`](decisions/0034-kind-dispatch-content-rendering.md).
Items F-CR-01 through F-CR-12 below are ordered by dependency. Pick the topmost
open item not already in Section 2. F-CR-01 and F-CR-06 landed (PR #588). The next highest-value open item is
F-CR-02, because Android must join `ContentTreeWire` before the Compose registry
can replace the old embed card.

#### F-CR-00 · Reference-driven reactive component contract [HIGH · all platforms]

Before expanding more user/content/embed components, make the registry contract
match the product promise in `docs/plan/m16-component-registry.md`: app screens
pass references; components own platform lifecycle; Rust owns truth and policy.

- Define the host adapter each platform exposes to copied source components:
  profile claim/release, embedded-event claim/release, projection observation,
  and redraw/update delivery.
- Update user-profile components so the primary API is reference-first
  (`pubkey` / `npub` / `nprofile`) with hydrated projection inputs retained only
  for previews, tests, and already-resolved composition.
- Update embedded-event/content components so lifecycle lives in the component
  or shared registry host, not in each feed/thread screen.
- Update recipes and web registry copy that currently teach per-screen maps or
  manual hydration as the normal path.

**Acceptance:** a clean SwiftUI, Compose, or TUI screen can render an avatar or
embedded event by passing a reference and local styling/callbacks only. No
feature screen directly calls claim/release for that reference; those lifecycle
calls are owned by the installed component or the one-time registry host adapter.
Registry demos/previews use one canonical set of real relay-backed references
from `apps/nmp-gallery/showcase-references.json` across SwiftUI, Compose, TUI,
and desktop; visible hydrated profile/content/media values come from Rust-owned
projections or neutral fallback from the exact reference, never invented fixture
identities or event payloads.

**Dependencies:** source-of-truth update in product spec and M16 plan. **Scope:**
medium-large.

#### F-CR-02 · Android gallery → `ContentTreeWire` migration [PREREQUISITE · Android]

Migrate `android/gallery/` off `ContentTreeDto` / `SegmentDto` / `MarkdownNodeDto`
onto `ContentTreeWire` / `WireNode` (already the iOS + TUI wire format).

- Rename `SegmentDtoView.kt` → `WireNodeView.kt`; rewrite against `WireNode` arena
  indexing.
- Delete `SegmentDto.kt`, `ContentTreeDto.kt`, `MarkdownNodeDto.kt`.
- Update `EmbedEntry.rendered` field type from `ContentTreeDto?` to `ContentTreeWire?`.
- `WireNode.EventRef` arm calls `EmbeddedEvent` composable (wired in F-CR-07).

Run `./gradlew :gallery:test` to verify no regressions.

**Dependencies:** F-CR-01 (landed, PR #588). **Scope:** medium.

#### F-CR-05 · iOS `NostrKindRegistry` + `EmbeddedEvent` + `EmbedChromeContainer` [HIGH · iOS]

New registry component at `crates/nmp-cli/registry/swiftui/content-kind-registry/`.

- `NostrKindRegistry` — `ObservableObject`, holds typed renderer slots (`ShortNote`,
  `Article`, `Highlight`, `Profile`) plus `[UInt32: UnknownKindRenderer]` for open-ended
  kind dispatch. `register(_:forKind:)` wires any kind number without touching core.
- `EmbeddedEvent` — SwiftUI view, receives `EmbeddedEventEnvelope?`, consults registry,
  wraps in `EmbedChromeContainer`.
- `EmbedChromeContainer` — generic `<Content: View>` wrapper providing border,
  indent, depth visual weight, and collapse placeholder. Knows nothing about content.
- Built-in `DefaultShortNoteRenderer` and `DefaultUnknownRenderer` (promoted from
  current `NostrQuoteCard` logic) bound via `makeDefault()`.
- Update `NostrContentView.swift` `EventRef` arm to use `EmbeddedEvent`.
- Deprecate (one release) `quoteCardProvider` closure API.

**Dependencies:** F-CR-01 (landed, PR #588). **Scope:** medium-large.

#### F-CR-07 · Android `NostrKindRegistry` + `EmbeddedEvent` composable [HIGH · Android]

New registry component at `crates/nmp-cli/registry/compose/content-kind-registry/`.

- `NostrKindRegistry` — `CompositionLocal`, holds typed `KindRenderer` slots plus
  `Map<Int, KindRenderer>` for open-ended dispatch.
- `KindRenderer` — `fun interface` with `@Composable fun Render(…)`.
- `EmbeddedEvent` — `@Composable`, receives `EmbeddedEventEnvelope?`, calls registry,
  wraps in `EmbedChromeContainer`.
- Delete `android/gallery/src/main/java/org/nmp/gallery/ui/EmbedCard.kt`.
- Wire `WireNode.EventRef` in `WireNodeView.kt` to `EmbeddedEvent`.

**Dependencies:** F-CR-01 (landed, PR #588), F-CR-02. **Scope:** medium-large.

#### F-CR-09 · `content-kind-30023` — Long-form article handler [MEDIUM · all platforms]

Per-platform kind handler components that bind `EmbedKindProjection::Article` to a
proper article preview card (title, summary, hero image, author, read-time). Derived
from the existing `ArticlePreview` composable in Android's old `EmbedCard.kt`; new for
iOS and TUI. Independently installable: `nmp add component swiftui/content-kind-30023`.

**Dependencies:** F-CR-05, F-CR-06 (landed, PR #588), F-CR-07. **Scope:** medium.

#### F-CR-10 · `content-kind-9802` — NIP-84 highlight handler [SMALL · all platforms]

Left-accent bar + italic highlighted text + source footer. New `crates/nmp-nip84/`
crate for the `HighlightProjection` decoder. Independently installable.

**Dependencies:** F-CR-05, F-CR-06 (landed, PR #588), F-CR-07. **Scope:** small.

#### F-CR-11 · `content-kind-0` — Profile card handler [SMALL · all platforms]

Avatar + display name + npub chip + about preview. No new crate needed (profile data
already in kernel projections). Independently installable.

**Dependencies:** F-CR-05, F-CR-06 (landed, PR #588), F-CR-07. **Scope:** small.

#### F-CR-12 · Nested-embed regression fixtures + golden tests [MEDIUM · all platforms]

Five fixture scenarios (one-deep article quote, cycle, depth limit, unknown kind,
highlight) with per-platform snapshot/buffer/instrumented tests. Verifies the recursion
guard, the `Unknown` fallback, and the article + highlight handlers end-to-end.

**Dependencies:** F-CR-09, F-CR-10, F-CR-11. **Scope:** medium.

#### F-CR-04 · Delete legacy embed wire-shapes (tail PR) [CLEANUP]

After all registries are live and golden tests pass: delete `EmbedEntry.article`,
`EmbedEntry.list`, `ArticleHeaderDto`, `ListDto`, `ListRowDto` from
`nmp-content-fixtures::dto`; delete `content-quote-card` registry components on all
three platforms; delete `ios/.../NostrQuoteCard.swift`; delete `android/.../EmbedDto.kt`
residuals. One dedicated cleanup PR per AGENTS.md §no hacks.

**Dependencies:** F-CR-12 passing on CI. **Scope:** small (deletions only).

### F-09 · Event relay provenance UI — "received from" view [V1 DX · all platforms]

Show the user which relays delivered a given event. The data is already tracked: `EventStore::provenance_for(event_id)` (`crates/nmp-store/src/events.rs:288`) returns `Vec<ProvenanceEntry>` with `relay_url`, `first_seen_ms`, `last_seen_ms`, and a `primary: bool` flag (up to 32 relays per event, persisted in LMDB).

**Status (2026-05-29 audit):** the `relay_count` badge exists on iOS (`NoteRowView`). Still missing: the full `relay_provenance` list on `TimelineEventCard`, the "Received from" detail view, and the Android / TUI implementations.

**Required work:**

1. **Expose provenance in the projection** — `TimelineItem` already carries `relay_count: u32`. Add a `relay_provenance: Vec<String>` field (list of relay URLs) to `TimelineItem` and `TimelineEventCard`. Populate from `store.provenance_for(&event.id)` in `Kernel::timeline_item` (`crates/nmp-core/src/kernel/update.rs:464`). Keep `relay_count` as the cheap badge signal; `relay_provenance` is the detail payload. Consider making it opt-in via a projection flag to avoid bloating every timeline row snapshot.

2. **iOS Chirp** — long-press or info sheet on any note row opens a "Received from" list showing relay URLs with first-seen timestamps. Tapping a relay URL copies it or navigates to relay diagnostics.

3. **Android Chirp** — same UX as iOS: bottom sheet or dialog on long-press.

4. **chirp-tui** — `?` key or dedicated pane shows relay provenance for the selected event. Already has `DiagnosticsView` precedent.

5. **chirp-web** — tooltip or expandable row section.

**Note:** `relay_count: u32` is already on `TimelineItem` and rendered in iOS (`NoteRowView`). Step 1 is the only Rust change; steps 2–5 are pure presentation work per platform.

### F-10 · Canonical FlatBuffers runtime update transport [V1 INFRA · in progress]

**Status (2026-05-29 audit):** the generic FlatBuffers `Value` tree is the
mandatory primary transport; typed projections are deployed as sidecars for the
feed (`NOFS` / `NFTS`). There is no `FullState` / `ViewBatch` typed root yet, and
the JSON `Value` tree remains the main generic interchange shape.

**ADR-0038 rollout progress (2026-05-29):** V-84 (iOS Swift NFCT decoder, PR
#762), V-85 (Android Kotlin NFCT decoder, PR #764), and V-86 (CI glob fix,
PR #781) are all LANDED at HEAD — the typed path is now the live preferred path
on iOS, Android, and TUI; the Android `nmp/` tree is fully pin-checked in CI.
ADR-0038 rollout is complete.

Replace the Rust-to-frontend JSON update payload with one canonical
FlatBuffers schema for `FullState`, `ViewBatch`, and side-effect frames.
UniFFI remains the generated binding, object lifecycle, callback, and
capability surface; it is not the hot payload format.

**Rule:** no production JSON runtime fallback. JSON remains valid only for
Nostr relay frames, diagnostics/golden fixtures, historical raw-C migration
shims, and explicit test tooling.

**Acceptance:**

- iOS, Android, desktop, and wasm shells consume the same FlatBuffers update
  schema.
- The stale-`rev` guard, snapshot-default path, and `ViewBatch` delta path are
  preserved across all shells.
- Legacy JSON update callback code is deleted or isolated behind documented
  migration/test-only entry points.

**PR #582 measurement:** local debug `snapshot_perf_firehose_gate` on 2026-05-26
with 1,000 synthetic events and `visible_limit=500`: master JSON frame
`payload_bytes=480296`, `make_update_us=18016`, `serialize_us=11361`; PR #582
generic FlatBuffers value tree `payload_bytes=873200`, `make_update_us=42075`,
`serialize_us=35501`. This is still below the 4 Hz tick budget and existing CI
ceilings, but it confirms the generic value tree is an interim transport shape;
typed snapshot tables are the next F-10 performance step if foreground logs show
`make_update_us` or payload size approaching budget.

---

### iOS Component — Gallery Extraction Candidates

Swift reusable component candidates identified in `ios/Chirp/`. Every one recreates existing gallery primitives or is missing the lifecycle that the registry pattern requires. Extract as `nmp-gallery` components once F-08 composition root lands. Entries track blockers and acceptance criteria per component.

#### [V-??] Extract ChirpAvatar as NostrAvatar gallery primitive

**File:** `ios/Chirp/Chirp/Theme/ChirpTheme.swift` (circular avatar with picture + identicon fallback).

**Why it matters:** every social Nostr app recreates this avatar. Chirp's version handles picture + fallback identicon but lacks the `claimProfile` / `releaseProfile` lifecycle that the registry pattern requires.

**Blocker:** F-08 Stage 1 (composition root + claim/release adapter).

**Acceptance:** extract as `nmp-gallery` SwiftUI component; wires through registry host for claim/release; Chirp imports from gallery instead of local Theme.

#### [V-??] Extract ChirpNpubChip as NostrNpubChip gallery primitive

**File:** `ios/Chirp/Chirp/Features/ProfileView.swift` (copyable npub chip — truncated display + copy-to-clipboard + 2s checkmark animation).

**Why it matters:** functional duplicate of `nmp-gallery`'s `NostrNpubChip`. Every Nostr app needs this interaction.

**Blocker:** F-08 Stage 1 (gallery component model).

**Acceptance:** extract as gallery component; Chirp uses gallery version; animation and copy behavior identical.

#### [V-??] Extract ChirpNip05Badge as NostrNip05Badge gallery primitive

**File:** `ios/Chirp/Chirp/Features/ProfileView.swift` (checkmark + NIP-05 identifier, failable on empty).

**Why it matters:** nearly identical to gallery's `NostrNip05Badge`. Standardizing prevents divergence as more apps ship.

**Blocker:** F-08 Stage 1.

**Acceptance:** extract with failable init pattern; Chirp uses gallery version.

#### [V-??] Extract ChirpUserCard as NostrUserCard gallery primitive

**File:** `ios/Chirp/Chirp/Features/ProfileView.swift` (avatar + name + NIP-05 badge composite).

**Why it matters:** three-part header is the canonical user card every social Nostr app builds. Composable from the atomic pieces above (avatar, npub chip, NIP-05 badge).

**Blocker:** F-08 Stage 1; completion of the three atomic pieces above.

**Acceptance:** composition built from extracted avatar + name + badge; Chirp wires the three via gallery host; no local profile-header duplication.

#### [V-??] Extract ChirpRelayRow as NostrRelayRow gallery primitive

**File:** `ios/Chirp/Chirp/Features/RelaySettingsView.swift` (icon + monospaced URL + role badge).

**Why it matters:** common to all NMP apps with relay settings. Gallery already has `NostrRelayList`; extract the row as the base primitive with optional connection-status dot.

**Blocker:** F-08 Stage 1.

**Acceptance:** `NostrRelayRow` component with role badge and optional status indicator; `NostrRelayList` refactored to use it; Chirp imports from gallery.

#### [V-??] Extract NoteActionsRow as gallery primitive

**File:** `ios/Chirp/Chirp/Components/NoteRowView.swift` (reply/repost/like/zap action bar).

**Why it matters:** compound component every social Nostr app recreates. Strong gallery candidate once action/zap interaction patterns stabilize.

**Blocker:** F-08 (registry) + post-v1 action/dispatch stability.

**Status:** post-v1. Action/zap interactions will be hardened by then.

---

## Section 5 — Post-V1

Deliberately deferred. Do not start until Section 4 is complete.

| Item | Why deferred |
|------|-------------|
| NIP-23 long-form articles (`nmp-nip23`) | kind:30023 constant exists in `tags.rs`; no decoder/projection. ~2 days when framework is stable. |
| NIP-51 lists / bookmarks / mute (see V-42) | Mute list is v1-A safety item (promote there); bookmarks/pins/communities are post-v1. |
| NIP-94 / NIP-96 file metadata + servers | `imeta` tag parser + upload action needed; ships in all modern clients. |
| Blossom uploads/downloads (M10) | No `nmp-blossom` crate; no blocking user need |
| Web-of-Trust (M13) | No architecture decision; not user-blocking |
| UniFFI migration (M14) | Raw C-ABI works; multi-sprint, high churn |
| Cashu wallet (NIP-60) + nutzaps (NIP-61) | NWC + NIP-57 cover the v1 zap use case; nutzap UX layer requires Cashu wallet primitives first. `crates/nmp-nip60` / `crates/nmp-nip61` do not exist on master. |
| `nmp-codegen` full Swift bridge | Pilot (F-05) must land first to prove the pattern |
| Second non-social app (shipped product) | PD-033-A decision needed first; the v1 spike is a thesis test, not a shipped product |
| Android parity with iOS Chirp | Android Chirp shell exists but lacks feature parity with iOS; v1 ships iOS-first. Parity work blocked on UniFFI (M14) to avoid hand-maintaining two FFI surfaces. |
| Additional Nostr-aware component packs | Content rendering moved to F-08 / M16. Post-v1 packs cover broader reusable app blocks such as account switchers, diagnostics inspectors, full thread screens, auth blocks, and non-content templates. |
| Raw-data projection follow-ups | ADR-0032 is canonical. Post-v1 work may add a shared `nmp-display` helper/codegen surface, a doctrine-lint rule for banned display helpers in projections, and a review of free-form metadata fallbacks. |
| Chirp TUI approach-B visual refresh | The top-level scratch plans were deleted. If this work resumes, track it as a scoped TUI UX item here or in WIP while a branch is active; preserve existing `chirp-tui` runtime/bridge/command wiring and keep rendering modules under the LOC ceiling. |
| Indexer-republish follow-ups | The default composition installs `nmp_router::IndexerRepublishPolicy` through `nmp-core`'s generic raw-event forwarding seam. Deferred add-ons are runtime toggles, telemetry, and parameterized replaceable support only if product demand appears. |
| Chirp TUI unfinished interactions | `apps/chirp/chirp-tui/src/input.rs:350,431,433,523` — repost, group-discover, add-relay, add-account, and DM-open are all `// not yet wired (post-v1)` no-ops. Mirror: `ios/Chirp/Chirp/Components/NoteRowView.swift:225` repost is also a no-op. Wire once the corresponding `dispatch_action` namespaces exist. |
| `nmp-content` Phase-2 claim dependency channel | `crates/nmp-content/src/embed_registry/mod.rs:26` — `// Phase 2: expose the claim-driven dependency channel`. The embed registry currently resolves claims synchronously; the async demand-producer path for late-arriving embedded events is not exposed to callers. |
| wasm32 test infrastructure | `crates/nmp-wasm/src/lib.rs:200` — no `wasm-bindgen-test` harness set up. The entire wasm publish path and signer-slot dispatch lack automated coverage. Set up `wasm-pack test --headless` in CI and migrate the `// TODO: wasm32 tests TBD` stubs into real tests. |
| `web/registry` CodeBlock placeholder | `web/registry/src/components/CodeBlock.tsx:39` renders `"This component is being built — check back soon."` in the web registry UI. Replace with a real syntax-highlighted code block (e.g. `shiki` or `prism`) once the registry UI is active. |

---

## Appendix — Closed / Verified Fixed

Recorded so Opus reviews do not re-flag these as violations.

| Item | Fixed at |
|------|---------|
| NIP-17 wire schema `nmp.dm.*` → `nmp.nip17.*` | Correct on HEAD: `nmp-nip17/src/action.rs:51`, `dm_relay_list.rs:121` |
| Bunker DM gated out (ADR-0026 Phase 2 inert) | `identity.rs:491` — `active_signer_for_seal()` returns `RemoteSignerForSeal` |
| ZapAction stub | Fully implemented; `FetchLnurlInvoice` enqueued; registered in chirp ffi |
| D0 `chirp.follow` / `chirp.unfollow` in nmp-core | Not present in `kernel/update.rs` on HEAD |
| NIP-29 dormant admin executors (11 stubs) | Removed; 5 live action modules remain |
| correlation_id discarded in KernelBridge.swift | Fully handled via `@discardableResult` intent chain |
| `bootstrap_urls_for_role` test-only fallback | V-66 fixed: fallback still operates but `no_configured_relays: true` is now emitted in the KernelUpdate snapshot when active-account + empty rows; host can surface a banner |
| V-03 `wallet_status` app noun in `Kernel` struct | Fixed: no typed field in `KernelSnapshot`; surfaced via host-registered `"wallet"` snapshot projection (`kernel/types.rs:741`) |
| D0 `chirp.follow`/`chirp.unfollow` hardcoded in `nmp-core` | Confirmed removed: zero occurrences in `crates/nmp-core/` (verified 2026-05-23) |
| F-06 CI lint: freeze C-ABI surface | Already shipped: `ci/check-ffi-surface-freeze.sh` + `.github/workflows/ffi-surface-freeze.yml`; ADR-override process live |
| V-07 zap relay selection D0 leak | PR #331: `inject_recipient_relays` in zap.rs; Swift passes empty relays array |
| V-09 ffi.rs LOC violation | PR #332: split into ffi/ sub-modules; all production files within 500-LOC ceiling |
| V-02 nmp-marmot in crates/ | PR #337: moved to `apps/marmot/nmp-app-marmot/` |
| `chirp.follow_list` projection key | Commits 570b7d2a + 5742c7fe: renamed to `nmp.follow_list` across all consumers |
| dm_inbox test chirp shape | Commit 282665c9: test updated for `remote_signer_unsupported` field in V-08 Stage 1 |
| marmot_local_nsec → mls_local_nsec | PR #334: D0 rename complete |
| ChirpAction → AppAction in nmp-wasm | PR #333: D0 rename complete |
| V-05 D2 enforcement gap — coverage_hook never installed | PR #347: `NmpApp::set_coverage_hook` seam wired; `CoverageGate::default()` installed in `nmp_app_chirp_register`; all 3 stages complete |
| WalletPayInvoice dispatch_action bypass | PR #361 (2026-05-23): `WalletPayInvoiceModule` registered under `"nmp.wallet"` namespace; `nmp_app_wallet_pay_invoice` rewritten as thin `dispatch_action_json` wrapper. Zero direct-FFI bypasses of the dispatch_action seam remain. |
| ADR-0025 Marmot bespoke FFI exception — FULLY RETIRED | PR #363 (Rust seam), PR #367 (iOS dispatch_action migration), PR #370 (deleted `nmp_marmot_dispatch` C symbol + REPL/TUI migrated to `MarmotHandle::dispatch` Rust method). Zero `extern "C" fn nmp_marmot_dispatch` in workspace. |
| Follow / Unfollow / React ActionModules app-local in `nmp-app-chirp` (Opus direction review #10 escape path) | 2026-05-24: lifted to `crates/nmp-nip02/` (NIP-02 follow list + NIP-25 reactions). Chirp's `register_chirp_actions` now delegates to `nmp_nip02::register_actions(app)`. Any Nostr app on top of NMP wires the social graph with a single call (mirrors `nmp_nip17::register_actions` / `nmp_nip57::register_actions` / `nmp_nip65::register_actions`). The deleted `Chirp{Follow,Unfollow,React}Module` impls are now `FollowModule` / `UnfollowModule` / `ReactModule` in `nmp-nip02`; namespaces (`nmp.follow`, `nmp.unfollow`, `nmp.nip25.react`) and JSON shapes unchanged — migration is binary-compatible for every existing host. |
| V-38 · NIP-47 NWC wallet stack out of nmp-core | `crates/nmp-nip47/` created; `wallet/` and `actor/commands/wallet.rs` deleted from nmp-core; Cargo.toml dep removed |
| V-43 · Zap correlation_id chain | correlation_id threaded through in nmp-nip57 (was nmp-core/zap.rs, now nmp-nip57/lnurl/mod.rs) |
| F-11 · Versioned releases + nmp upgrade | `release/nmp-release.toml`, `nmp upgrade`, `nmp doctor`, `nmp init --nmp-version` all implemented |
| V-46 · D5 snapshot bounding | PR #770 + #779; `snapshot_projections_with_publish_cluster` in `nmp-core/src/kernel/update/projections.rs` gates timeline/author_view/thread_view on open views. Verified at HEAD. |
| V-58 · Close-reason backoff | PR #778; `BackoffClass`/`SetBackoffHint` in `nmp-network/src/relay_worker/mod.rs`. Verified at HEAD. |
| V-61 · Marmot orphaned-commit | PR #772; `MarmotError::OrphanedCommit` in `nmp-marmot/src/service.rs`. Verified at HEAD. |
| V-62 · Marmot keyring-unavailable | PR #772; `keyring_unavailable` field in `nmp-marmot/src/projection/payload.rs`. Verified at HEAD. |
| V-63 · NIP-47 encode failures | PR #774; `encode_frame` in `nmp-nip47/src/runtime.rs`. Verified at HEAD. |
| V-64 · NIP-47 sweep expired payments | PR #774; `sweep_expired_payments` in `nmp-nip47/src/runtime.rs`. Verified at HEAD. |
| V-65 · NOSTRCONNECT bootstrap capability | PR #780; `NostrConnectBootstrapRelaySlot` in `nmp-core/src/slots.rs`; no hardcoded `wss://relay.damus.io` in `relay_roles.rs` at HEAD. Verified. |
| V-66 · NoConfiguredRelays diagnostic | PR #782; `no_configured_relays` field in `KernelSnapshot` (`kernel/types.rs:835`). Verified at HEAD. |
| V-67 · LMDB store-unavailable diagnostic | PR #769; `store_open_failure` field in `KernelSnapshot` (`kernel/types.rs:822`). Verified at HEAD. |
| V-69 · LMDB orphan-index counter | PR #767; `StoreAnomalySnapshot`/`orphan_index_entries` in `nmp-nostr-lmdb/src/store/lmdb/mod.rs`. Verified at HEAD. |
| V-70 · hex_to_bytes32 returns Option | PR #775; `pub(super) fn hex_to_bytes32(s: &str) -> Option<[u8; 32]>` in `nmp-store/src/types/ids.rs`. Verified at HEAD. |
| V-71 · nip65_resolver tracing | PR #759; `tracing::debug!` calls at both malformed-tag skip sites in `nmp-router/src/nip65_resolver.rs`. Verified at HEAD. |
| V-72 · Signer KindOutOfRange | PR #771; `KindOutOfRange` variant in `nmp-signer-iface/src/error.rs`; `local.rs` returns it. Verified at HEAD. |
| V-74 · NWC URI UnknownParam | PR #768; `UnknownParam` variant in `nmp-nwc/src/parse.rs`. Verified at HEAD. |
| V-75 · Router lane attribution | PR #777; `RouteAttempt`/`RoutingLane` in `nmp-core/src/substrate/routing_trace.rs`. Verified at HEAD. |
| V-77 · Dead MakeInvoice removed | PR #768; `MakeInvoice` absent from `nmp-nwc/src/types.rs`. Verified at HEAD. |
| V-79 · NWC heartbeat/reconnect | PR #783; `NwcConnectionState`/`tick_heartbeat` in `nmp-nip47/src/runtime.rs`. Verified at HEAD. |
| V-84 · iOS NFCT decoder + render wiring | PR #762; `TypedHomeFeedDecoder.swift` complete `decodeContentTree`; `KernelBridge.swift:533` wired; typed path live. Verified at HEAD. |
| V-85 · Android NFCT decoder + render wiring | PR #764; `TypedHomeFeedDecoder.kt` `decodeContentTree` wired; `KernelModel.kt:131-133` confirmed. Verified at HEAD. |
| V-86 · Flatbuffers pin-check Android coverage | PR #781; `ci/check-flatbuffers-version-pins.sh` covers full `android/app/src/main/java/nmp/` tree. Verified at HEAD. |
| V-92 · Relay backoff reset after healthy session | Commit 5da5942c; `RELAY_BACKOFF_RESET_AFTER_SECS` reset at line ~426 in `nmp-network/src/relay_worker/mod.rs`. Verified at HEAD. |
| V-96 · NIP-57 bolt11 consolidation | Already `pub(crate)` at HEAD: `fetch_lnurl_invoice_blocking` (`:419`) + `fetch_bolt11_for_zap` (`:559`) in `nmp-nip57/src/lnurl/mod.rs`; no external callers confirmed. Close GH #620. |
