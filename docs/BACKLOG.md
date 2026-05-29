# NMP Backlog

> Tracker for active violations, pending user decisions, and the ordered v1 feature backlog.
> Supersedes `docs/perf/pending-user-decisions.md` (append-only history log, kept for audit)
> and the former `docs/arch-review-queue.md` (deleted after open items were folded here).
>
> Companion files:
> - [`WIP.md`](../WIP.md) — live tracker for work currently on a branch (in-flight)
> - [`docs/plan.md`](plan.md) — overarching plan (milestones, doctrine, where we are)
>
> Verified against `origin/master` **9fe2aced** (2026-05-27). Update this file
> in every PR that touches an item listed here. (Cleanup pass 2026-05-27 — completed
> items removed; see git history for prior state.)

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

### V-57 · 2026-05-26 architecture audit follow-up queue [HIGH · priority tracker]

**ARCH ASSESSMENT CLOSED 2026-05-27.** Codex confirmed **ARCHITECTURE IS IN VERY GOOD STANDING** (all 6 checks passed) against master commit `7213d7ba` (PR #656). Two additional P1 violations found by the assessment were fixed and merged: D0 — `swap_nip17_dm_inbox_observer` renamed to `swap_dm_inbox_observer` in `AppHost` substrate trait (PR #654); D6 — `display::short_npub` removed from `publish_outbox` kernel projection (PR #655). P2–P6 items below remain as ongoing debt tracked here.

**Scope:** this is the canonical roll-up for the six-agent architecture audit run on
2026-05-26. PR #578 removes the duplicate planning/status authorities; the remaining
findings below are ordered by architectural risk. When a slice gets a dedicated V/PD entry
or a fixing PR, remove or strike that bullet here instead of creating a parallel plan.

**Priority order:**
2. **P2 — centralise Nostr kind constants in `nmp-core`.** _Direction changed
   2026-05-27._ The original framing treated `nmp-core` naming `1059` / `10002`
   as a D0 leak; the owner reframed this on 2026-05-27 — integer kind numbers
   are wire-protocol data, not app/protocol *nouns*, and centralising the
   integers in one place removes the duplication risk without growing the
   kernel's semantic surface.

   **Stage 1 — DONE.** `crates/nmp-core/src/kinds.rs` is the new canonical
   workspace registry for the kind integers `nmp-core` actively names
   (`KIND_PROFILE_METADATA`, `KIND_SHORT_TEXT_NOTE`, `KIND_CONTACT_LIST`,
   `KIND_REACTION`, `KIND_CHAT_MESSAGE`, `KIND_GIFT_WRAP`, `KIND_RELAY_LIST`).
   `actor/commands/{publish,relays}.rs`, `actor/commands/identity.rs`,
   `kernel/{discovery,publish_outbox,requests/profile}.rs`, and
   `subs/recompile.rs` all use the constants from this module — no production
   `nmp-core` code path holds a hand-rolled `1059` / `10002` literal any
   more. The doc-prose and log strings in `publish.rs` no longer name `NIP-17`,
   kind `10050`, or `Marmot`; the kind:1059 D10 guard now refers to "the
   author's public-relay outbox" in substrate-neutral terms.

   **Next step (Stage 2).** Migrate the remaining private duplicates in
   `nmp-nip59` (`KIND_GIFT_WRAP`), `nmp-nip17` (`KIND_DM_RELAY_LIST` +
   `KIND_CHAT_MESSAGE`), `nmp-nip29`, `nmp-nip57`, `nmp-marmot`,
   `nmp-router::publish_relay_list::KIND_RELAY_LIST`, and `nmp-wot` to
   re-export from `nmp_core::kinds` once the dependency edges are confirmed
   compatible with the boundary spec. Out of scope for the current slice.

   **Files still needing migration (2026-05-29 audit):** `nmp-nip59`
   `KIND_GIFT_WRAP`; `nmp-nip17` `KIND_DM_RELAY_LIST` + `KIND_CHAT_MESSAGE`;
   `nmp-nip57` `KIND_ZAP_REQUEST` + `KIND_ZAP_RECEIPT`; `nmp-marmot`
   `KIND_GIFT_WRAP`; `nmp-router` `KIND_RELAY_LIST`. Note: `nmp-nip29`'s
   `KIND_CHAT_MESSAGE` (value `9`) is a different kind from `nmp-nip17`'s
   `KIND_CHAT_MESSAGE` (value `14`) and should stay crate-local — it is not a
   duplicate of the canonical registry constant.
3. **P3 — move Chirp shell business logic behind Rust-owned actions/projections.**
   ~~`apps/chirp/chirp-tui/src/commands.rs:169-234` resolves lightning addresses in the
   TUI~~: **FIXED** — now routes through `runtime.zap()`. ~~`apps/chirp/chirp-tui/src/runtime_commands.rs:249-269`
   bypasses the action door for Marmot~~: **ACCEPTABLE** — `marmot_register_active` is
   identity setup, not a reactive dispatch bypass.
   `ios/Chirp/Chirp/Features/RelaySettingsView.swift:159-177` **CURRENT:** dispatches two
   protocol publishes while tracking only one correlation id. **Next step:** expose a
   composite Rust action / action-stage projection for the relay-settings publish.
4. ~~**P4 — make wasm use the same snapshot and error contract as native.**~~
   **DONE (2026-05-29 audit):** all 5 cited TODO markers resolved. Wasm is
   post-v1 per user direction 2026-05-29.
5. ~~**P5 — close native update-loop and envelope discipline gaps.**~~
   **DONE (2026-05-29 audit):** Gallery polling now properly handles disconnect
   (`IllegalStateException` pattern); the `recv_timeout` two-arm pattern on the
   Rust side is correct.
6. **P6 — strengthen enforcement so these regressions trip earlier.**
   V-12 already tracks oversized boundary files; the new gap is doctrine-lint coverage for
   dependency direction and app-noun leakage. **Next step:** add a dependency-graph/layer
   lint covering upward edges such as `nmp-router -> nmp-ffi` and `nmp-signer-broker -> nmp-core`,
   plus explicit allowlists for sanctioned adapter crates.

### V-68 · Core/planner still carry kind:1/6 social subscription policy [HIGH · D0 violation]

**Verified 2026-05-28:** `nmp-core` and `nmp-planner` still contain social-client
subscription defaults that belong in NIP/app modules:

- `crates/nmp-core/src/kernel/requests/profile.rs:532-550` hardcodes selected-author
  note/repost requests as `{"kinds":[1,6], ...}`.
- `crates/nmp-core/src/kernel/requests/thread.rs:217-223` hardcodes recursive thread
  reply requests as `{"kinds":[1,6], ...}`.
- `crates/nmp-core/src/kernel/ingest/mod.rs:623-650` fires mailbox-change routing
  traces with `&[1, 6]` as the canonical content kind set.
- `crates/nmp-planner/src/interest.rs:183-189` exposes
  `InterestShape::timeline_for` as a generic-looking constructor while internally
  selecting `[1, 6]`.

**Why this is a violation:** `{1, 6}` is a social/NIP-01 timeline policy, not
substrate policy. `nmp-core` and `nmp-planner` may carry caller-supplied `kinds`
as filter data, but they must not choose app defaults. Defaults like "follow-list
timeline means kind:1 + kind:6" belong in `nmp-nip01`, `nmp-nip02`,
`nmp-app-template`, or an app crate such as Chirp.

**Required fix:** move the remaining social subscription constructors and trace
inputs out of `nmp-core`/generic planner APIs. Keep the existing
`ActorCommand::OpenContactListSubscription { kinds }` direction: hosts or NIP
modules declare the kind set, and the substrate registers/executes those kinds as
data. Tests covering follow-feed behavior must use arbitrary host-declared kinds,
not `{1, 6}`, unless the test is explicitly scoped to a NIP-01/NIP-18 module.

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

### V-14 · Bunker has no reconnect — relay flap silently bricks the session [MEDIUM] — **DONE** (PR #431)

**Remaining:** step b — host-visible `BunkerConnectionState` projection (Connected / Connecting / TransportLost) so the host shell can surface a non-silent indicator.

**Deadline:** before v1-A. Either this is fixed or `aim.md` and v1 copy drop
NIP-46 as a v1 sign-in method.

---

### V-37 · Snapshot output seam doesn't support non-Chirp apps reading kernel state [HIGH]

**Verified (2026-05-24 — Notes rewrite investigation):** PD-033-A requires Notes to be
rewritten against "real framework seams (LogicalInterest, kernel-owned timeline projection,
handshake gate)." Code-grounded inspection found the current framework does not expose those
seams generically:

1. **`NmpSnapshotProjector` is zero-arg** (`crates/nmp-ffi/src/snapshot.rs:39`):
   ```rust
   pub type NmpSnapshotProjector = unsafe extern "C" fn() -> *const c_char;
   ```
   The callback receives no kernel-state argument and no context pointer. A registered
   projector must obtain state through side-channels (raw event observers, separate globals).
   There is no mechanism for the kernel to *push* a typed view to a non-Chirp app.

2. **No generic `nmp_app_snapshot`** — only `nmp_app_chirp_snapshot` exists
   (`apps/chirp/nmp-app-chirp/src/ffi/snapshot.rs:14`), typed to `*mut ChirpHandle`.
   A non-Chirp app has no pull path either. (As of the 2026-05-29 audit,
   `nmp_app_chirp_snapshot` is now `#[deprecated]` per ADR-0037.)

3. **No follow-set-aware `LogicalInterest` seam without `nmp-nip02`** — subscribing to
   "kind:1 from the active user's follow set, outbox-routed" requires `nmp-nip02`'s
   `FollowListProjection`. A second app that doesn't want Chirp's full NIP-02 stack has no
   lightweight path to the canonical social feed.

**Impact:** PD-033-A cannot be closed by a rewrite alone — the prerequisites don't exist.
Any honest "rewrite Notes" attempt will rediscover these three gaps and either (a) use the
same raw-event bypass again, or (b) pull in all of `nmp-nip02` as a hidden Chirp dependency.
V-37 is the *blocker* for PD-033-A, not a separate concern.

**Required:** Add three affordances before attempting the rewrite:
- (a) `NmpSnapshotProjector` gains a `*const c_void` context pointer (or is replaced by a
  richer registration model);
- (b) a generic `nmp_app_get_snapshot(app, namespace) -> *mut c_char` pull path;
- (c) a `LogicalInterest::FollowSetKind1` variant (or equivalent) in a substrate crate
  that does not pull in Chirp-level NIP-02 machinery.

These are new framework affordances — they require an ADR before implementation
(ffi-surface-freeze gate). Tag: **needs ADR before work begins**.

**V-37 is the actual PD-033-A blocker (review #18 finding 10):** the ADR for these
three affordances has not been written. Until the ADR exists and the affordances are
built, PD-033-A cannot close without re-using the Notes raw-event bypass. Either
promote V-37 to a v1 blocker or drop PD-033-A from the v1 exit criteria with a
written rationale. V-45 splits sub-item (c) into its own tracked item.

---

### V-42 · NIP-23 / NIP-51 / NIP-94 / NIP-96 absent from crates and untracked [HIGH · v1-A for mute · post-v1 for rest]

**Evidence:** `ls crates/` shows `nmp-nip{01,02,17,29,42,57,59,65}` only.
`crates/nmp-content-fixtures/src/dto.rs:186-213` defines a `Nip51List` DTO for tests
but no production projection exists. kind:30023 appears in `crates/nmp-core/src/tags.rs`
only as a constant — no decoder, no projection, no action module.

- **NIP-51 mute lists** — v1-A safety-relevant. A user has no way to suppress
  harassment from within an app built on NMP. The `BlockListView` in Chirp is absent
  from the iOS shell (`grep -r "BlockListView" ios/Chirp/` returns nothing).
  Prerequisite: only a `KernelEventObserver` projection + kind:10000/10001 decoder.
  Effort: ~1 day.
- **NIP-23 long-form articles** — post-v1. kind:30023 constant already in `tags.rs`.
  Need: decoder + `KernelEventObserver` projection. Effort: ~2 days.
- **NIP-94 / NIP-96 file metadata + media servers** — post-v1. Ships in every modern
  client for HEIC vs JPEG, dimensions, MIME, SHA-256. Need: `imeta` tag parser + action
  for upload. Effort: ~2 days per NIP.

**Recommended action:** promote NIP-51 mute list to v1-A backlog as its own item;
add one-line §5 rows for NIP-23 / NIP-94 / NIP-96.

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

---

### V-45 · No `LogicalInterest::SocialTimeline` substrate seam [CLOSED — replaced by ADR-0036 composition-root approach]

**Resolved (V-80 rung 4, 2026-05-28):** `LogicalInterest::SocialTimeline` was
deliberately NOT added. The composition-root approach (`ActiveFollowSet`
closure-based predicate in `nmp-nip02`, wired at `nmp-app-template`) supersedes
this item. See ADR-0036. This entry is closed.

**Evidence (extracted from V-37c):** every "show me notes from people I follow" app
needs this pattern. Today it requires reading 30+ lines of Chirp's
`apps/chirp/nmp-app-chirp/src/ffi/register.rs:370-403` to assemble the follow-list
wiring. The substrate offers no affordance for the most common Nostr-client read
pattern. `aim.md` §1 says "one-shot a working Nostr application" — this is the
one affordance a social read app needs.

**Recommended action:** design `LogicalInterest::SocialTimeline { viewer: Pubkey, kinds: Vec<u16> }`
that pulls in the follow-set automatically and routes through the outbox planner.
Drop V-37(c) as a sub-item; track here separately.

---

### V-46 · Snapshot built-in projection cluster is unbounded — D5 silently violated [RESOLVED]

**Resolved** by PR fix/v46-snapshot-d5-bounding (2026-05-29).

**Option (b) implemented** — view-dependent keys moved to a "only-if-view-subscribed"
branch in `crates/nmp-core/src/kernel/update/projections.rs`:

- `timeline`, `inserted`, `updated`, `removed`: emitted only when
  `follow_feed_kinds` is non-empty (i.e., the shell has called
  `nmp_app_open_timeline` / `ActorCommand::OpenContactListSubscription`).
- `author_view`: emitted only when `author_view()` returns `Some(...)`.
- `thread_view`: emitted only when `thread_view()` returns `Some(...)`.

**Shell tolerance verified** before change: iOS `SnapshotProjections` declares all
six keys as `Optional` (Swift `?`); Android `SnapshotProjections` uses default
`emptyList()` / nullable; TUI reads via `.get(key)` returning `Option`; web uses
`Array.isArray(...)` guard and `?? undefined` optional chain — all shells are
absence-tolerant. D5 doctrine text updated to describe the static vs.
view-dependent cluster split.

---

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

### V-50 · Relay routing is a single hardwired NIP-65 algorithm in `nmp-core` — must become a per-kind dispatch table in `nmp-relay-pool` [HIGH · post-v1]

**Evidence:** `crates/nmp-core/src/kernel/outbox.rs` (447 LOC) implements one routing
strategy — consult kind:10002 write relays for all event kinds. This is correct for kind:1
public notes but wrong for everything else. The kernel has no per-kind routing dispatch at all.

**The full picture — routing is kind-specific:**

Different event shapes route to completely different relay sets, none of which are kind:10002:

| Event shape | Relay source | Kind |
|---|---|---|
| Public notes (kind:1/6/7/…) | Author's NIP-65 write relays | kind:10002 |
| DMs (kind:14/1059) | **Recipient's** DM inbox | kind:10050 |
| NIP-29 group events | Group relay from `h` tag | (tag-derived) |
| Marmot/MLS group events | MLS group relay | (group state) |
| Drafts | Author's private storage relay | TBD |
| Long-form (kind:30023) | Author's write relays | kind:10002 (default) |
| NIP-51 sets | Author's write relays | kind:10002 (default) |

NIP-51 documents the full taxonomy of kind-specific relay lists: kind:10002 (general),
kind:10050 (DM), kind:30002 (named relay sets), kind:10009 (group relay lists), etc. The
routing algorithm must dispatch on event kind (and sometimes tags like `h`) to consult
the right relay list kind for the right pubkey.

Today this dispatch does not exist — `kernel/outbox.rs` hardwires kind:10002 for every
publish, and V-39/V-40 show that DM relay routing leaks into the kernel as a special case
rather than being handled by the dispatch table that should own it.

**Correct design — `crates/nmp-relay-pool/`:**

A new crate (analogous to applesauce's `relay` package) owning:
1. **Per-kind routing dispatch table:** given an unsigned event, select the right relay
   list kind and target pubkey, then resolve to a concrete relay URL set.
2. **`MailboxCache` implementation** (currently `InMemoryMailboxCache` in `crates/nmp-router/src/cache.rs`, marked
   "Phase 2: replace with nmp-router implementation" — that future destination is here).
3. **The NIP-65 `publish_relay_list` ActionModule** from `crates/nmp-router/src/publish_relay_list.rs`
   (that file is too thin to stand alone; absorb it here).
4. **Relay pool lifecycle** — connect/disconnect/reconnect, not just routing math.

`nmp-core` substrate defines `trait OutboxRouter` + `trait MailboxCache`; the kernel holds
injected `Arc<dyn>` of each. `nmp-relay-pool` provides the concrete implementations.

`crates/nmp-router/src/publish_relay_list.rs` is deleted after the ActionModule migrates.

**Migration difficulty: MEDIUM-HARD.** The `MailboxCache` trait seam exists; `OutboxRouter`
trait needs to be designed carefully to express the per-kind dispatch without leaking NIP
knowledge into the substrate. The per-kind dispatch table itself is new design work, not just
a refactor. Prerequisite for V-39 (DM routing) and V-40 (DM ingest) clean migrations.

**Phase: post-v1.** Prerequisite for any competing outbox strategy and for NIP-17 DM routing
to leave the kernel cleanly. Pairs with V-38/V-39/V-41 (open-ActorCommand seam).

---

### V-51 · No structural observability on routing decisions — apps can't surface "why did event Y go to relay B?" [HIGH] — **Phases 1, 2, 4, 5 DONE; phase 3 pending**

**Phase 1 — substrate observer + bounded projection** ✅ PR #457 merged
(efe72537). `RoutingTraceObserver` trait + `RoutingTraceProjection`
bounded ring buffer (capacity 64 per stream) in `nmp-core`; both
`nmp_router::GenericOutboxRouter` and `nmp-core`'s default router fan
out to the observer.

**Phase 2 — FFI/wasm snapshot surface** ✅ PR #476. New FFI symbol
`nmp_app_recent_routing_decisions` (heap-owned, freed via
`nmp_app_free_string`) returns a stable schema-versioned JSON document
(`schema_version: 1`) listing recent publishes + subscriptions with
per-URL lane attribution. Wasm sibling
`NmpWasmRuntime::recent_routing_decisions()` returns the identical
payload shape via a `wasm-bindgen` method backed by
`KernelReducer::recent_routing_decisions_json`. JSON shape lives in
`nmp_core::kernel::routing_trace_dto` (consumer-side renderer; substrate
types stay free of `serde::Serialize`). `NmpCore.h` updated; CI drift
gate passes.

**Phase 4 — validation harness** ✅ PR #461 merged (b9e0fc15).
`chirp-repl routing-trace` subcommand + `cargo test -p nmp-testing
--test routing_trace_real_nostr -- --ignored` integration test that
fetches pablof7z's real NIP-65 from `wss://relay.damus.io` and asserts
`Nip65/Read` lane attribution. `scripts/validate-routing.sh` shell
smoke.

**Phase 5 — kernel-router observability cut-over** ✅ PR #462 merged
(1dbff579). Kernel calls injected `OutboxRouter` on subscription
dispatch sites + kind:10002 ingest; chirp wires `GenericOutboxRouter`
via `set_routing_substrate`. **Caveat**: this is *observe-only* — the
kernel still picks REQ relays via cache helpers. Make-substrate-honest
follow-up promotes the router to the decision authority.

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

### V-52 · Single-relay browsing — read events from one relay only, with cache origin tracking [HIGH · v1 DX]

**What we want:** an app must be able to scope an interest to a single specific relay URL
("show me what *this* relay has"). When a subscription is scoped that way:

- REQs and `NEG-OPEN` (NIP-77 negentropy) are sent ONLY to that relay, never to any
  outbox/inbox/indexer set the router would otherwise pick.
- The cache must be queryable for events known to have originated from that specific
  relay. We need a per-event provenance signal — for each cached event, did it (also)
  arrive from relay X? Today's `Provenance` lane (lane 3 in `nmp-router`) already
  carries relay-origin URLs in events' tag set, but the cache index can't be queried
  by "events seen on relay X" as a primary lookup.
- A scoped subscription does NOT cause an unscoped re-broadcast. The router treats
  the relay scope as an `explicit_targets` override (similar to lane 5) and does not
  add discovery/AppRelay fallbacks.

**Why this matters:** every modern Nostr client has a "browse this relay" or "switch
relay" affordance (relay-trawler, what's-on-this-relay debugging, single-relay reads
for private/paid relays). Today an NMP app has no structural way to express it —
the router always fans out via outbox/inbox.

**Code-grounded surfaces to extend:**

- `crates/nmp-core/src/substrate/routing.rs` — `RoutingContext` already has
  `explicit_targets: Option<BTreeSet<Url>>`, but there is no parallel `LogicalInterest`
  shape for the subscribe side. Add a `LogicalInterest::SingleRelay { url, inner }` or
  an `interest.scope_relays: Option<BTreeSet<Url>>` field that the router will honour
  in lane 5 on the subscribe path (today lane 5 is publish-only in `nmp-router`).
- `crates/nmp-store/` — cache lookup needs a `by_relay(url)` index, OR
  `EventStore::list_events_seen_on(relay, filter)`. The relay-origin provenance set
  already lives in `Provenance` events; the store must expose a primary lookup by
  any one relay URL.
- `crates/nmp-router/src/router.rs` lane 5 — extend the `ClassRouted` lane to cover
  the subscribe path when `interest.scope_relays.is_some()`. Today the subscribe-side
  lane 5 is empty (see PR #483).
- FFI: surface a `nmp.subscribe_scoped_to_relay(url, filter, ...)` action namespace
  so apps can request it without learning the substrate types.
- Chirp: expose this as a UI affordance — a relay picker in any timeline view that,
  when set, runs the same view bound to a single-relay scoped subscription. The
  routing-trace inspector (V-51) already shows the lane attribution, so this
  surface lights up "you are looking at relay X" naturally.

**Acceptance test:** a chirp-repl flow `chirp-repl browse --relay wss://relay.damus.io
--kind 1 --limit 100` returns exactly the kind:1 events the cache has stamped with
that relay's URL, drains REQ messages only to that relay, and never fans out to other
relays even when the active account has a NIP-65 write set covering them.

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

### V-54 · NIP-46 onboarding still blocks the actor thread [MEDIUM · remote-signer UX] (related: GH #611 AccountsView polling, GH #612 op.wait blocks actor)

**Verified:** `crates/nmp-core/src/actor/commands/identity.rs:826`, `:864`, and
`:1019` still call the synchronous `sign_active` path while publishing the
initial kind:0 metadata, kind:10002 relay list, and kind:3 follows during
`create_account`. `sign_active` is bounded by `REMOTE_SIGN_TIMEOUT` (5s), but
a remote signer can still stall the actor during account creation.

**Impact:** the non-blocking signing path exists for normal publish/react/follow
flows, but onboarding remains a residual blocking path for bunker accounts.

**Correct fix:** move the three cold-start publishes onto the existing
`sign_active_nonblocking` / `PendingSign` settlement path, preserving explicit
cold-start relay targets and D6 toast surfaces for "no cold-start relay".

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

### V-59 · `EventStore` trait missing kernel clock injection — `SystemTime::now()` in watermarks and queries [LOW · correctness]

**Verified:**
- `crates/nmp-store/src/types/watermark.rs:59-61` — inline note: "the `EventStore` trait does not yet thread the kernel clock into the store … this is a known transitional site pending the store-clock plumbing tracked for a later milestone."
- `crates/nmp-store/src/lmdb/query.rs:433` and `src/mem/query.rs:373` — same note verbatim; `SystemTime::now()` substituted for the missing kernel clock.

**Impact:** watermark timestamps and query "current time" are sourced from the OS wall clock, not the kernel's monotonic clock. This creates subtle divergence in test environments (where the kernel clock can be controlled) and in long-running sessions where clock skew could affect expiry and ordering logic.

**Correct fix:** thread a `ClockSource` or `Instant`-provider through the `EventStore` trait so all time reads inside the store use the same clock as the rest of the kernel.

---

### V-60 · LMDB `gc_step` never evicts — LRU eviction not implemented [MEDIUM · resource management]

**Verified:** `crates/nmp-store/src/lmdb/gc.rs:8-10` — module comment: "LRU eviction is not implemented in this milestone — `Mem` doesn't have one either; `gc_step` reports `lru_evicted = 0`. Future work tracked under M4 GC tuning."

**Impact:** a long-running session that ingests a high-throughput feed will grow the LMDB store without bound. The GC step runs on each tick but evicts nothing; no byte or event-count budget is enforced.

**Correct fix:** implement an LRU policy in `gc_step` — track last-access time per event, evict the least-recently-read events when the store exceeds a configurable byte or event-count ceiling. The `mem` store needs the same policy for test consistency. Prerequisite: `EventStore` clock injection (V-59) so eviction timestamps are kernel-clock-sourced.

---

### V-56 · Content-level profile mentions do not feed profile discovery [MEDIUM · v1 UX]

**Verified:** `crates/nmp-core/src/kernel/ingest/timeline.rs:132-138` only feeds
event tags (`p`/`e`/`q`) and the note author into discovery/profile fetch.
`crates/nmp-core/src/subs/unknown_ids.rs:121-128` already exposes
`UnknownIds::note_pubkey`, and `crates/nmp-content/src/tokenizer.rs:206-210`
extracts `nostr:npub1...` / `nostr:nprofile1...` mentions, but no production
ingest path connects those content-extracted pubkeys to `UnknownIds`.

**Impact:** a visible `nostr:npub1...` mention that is not also present in a
`p` tag may render indefinitely without a kind:0 profile fetch.

**Correct fix:** add a D8-clean content-mention demand producer that extracts
profile pubkeys during ingest and records them through the existing
`UnknownIds::note_pubkey` path. The kind:0 re-fetch after kind:10002 arrival is
already implemented via `refresh_profile_after_mailbox`; do not duplicate that
part of the deleted scratch plan.

---

### V-61 · Marmot `PendingGroupChange::drop` silently clears unresolved MLS commit [HIGH · MLS state divergence]

**Verified:** `crates/nmp-marmot/src/service.rs:172` — `PendingGroupChange::drop` clears the pending MLS commit on drop without surfacing an error. If a panic or early return drops the guard before the commit is resolved, the group's local MLS state is mutated as if the commit were applied, but no kind:445/`commit` event reaches the relay.

**Impact:** group state diverges from the relay-published timeline. Subsequent members joining or syncing see a different epoch than this client. There is no recovery path short of leaving and rejoining the group.

**Correct fix:** the drop path must either (a) roll the pending change back to pre-commit state, or (b) record a typed `MarmotError::OrphanedCommit` on the kernel error toast and refuse further group sends until the operator resolves the divergence. Silent drop is not a doctrined option.

---

### V-62 · Marmot keyring failure silently installs in-memory mock store [HIGH · MLS secret durability]

**Verified:** `crates/nmp-marmot/src/ffi.rs:228-269` — nested fallback: real keyring open fails → delete-and-retry path → on second failure installs the in-memory mock store with no host-visible signal. MLS group secrets thereafter live only in process memory.

**Impact:** the user believes MLS groups are persistent; on next launch the secret material is gone and every group is unjoinable. There is no toast, no error code returned to the host, no metric. Once secrets are lost, group membership cannot be recovered.

**Correct fix:** surface a typed `MarmotInitError::KeyringUnavailable` to the host; let the app decide whether to block group features, prompt the user, or fall back to an explicit ephemeral session. Never install the mock store as an implicit decision inside `ffi.rs`.

---

### V-63 · NIP-47 outbound payment serialization uses `unwrap_or_default` — payment frame can be empty string [HIGH · silent payment failure]

**Verified:** `crates/nmp-nip47/src/runtime.rs:222`, `:267`, `:460` — `serde_json::to_string(...).unwrap_or_default()` on the REQ/EVENT/CLOSE frames. If serialization fails, the resulting `""` is pushed onto the outbound queue. For `pay_invoice` (line 460) the `pending_payments` map is populated before the broken frame is sent, so the correlation_id is registered as inflight while the relay never receives a payment request.

**Impact:** the user's pay-invoice action stage hangs indefinitely (or until wallet disconnect drains it as "wallet disconnected", which is a misleading reason). No error surfaces. This is also the most likely path through which a malformed bolt11 or amount could vanish without diagnosis.

**Correct fix:** propagate serialization failure as `Err(NwcError::EncodeFailure)`; never send `""` frames; emit a `record_action_failure` for the correlation_id before dropping the request. Apply the same fix to the REQ filter and CLOSE frames so subscription bring-up failures are visible.

---

### V-64 · NIP-47 has no `pending_payments` timeout sweep — orphaned responses silently dropped [MEDIUM · silent stuck state]

**Verified:** `crates/nmp-nip47/src/runtime.rs:392` — the `(_, None) => {}` arm in the response-correlation handler silently discards wallet responses that arrive with no matching `pending_payments` entry. `crates/nmp-nip47/src/runtime.rs:48` defines `pending_payments: HashMap<String, Option<String>>` and the only drain site is `wallet_disconnect_inner` (`:260`). There is no periodic sweep that times out inflight pay_invoice correlations.

**Impact:** if a wallet response arrives after the client has already evicted the entry (or never registers one due to V-63), the response is silently consumed. Combined with the absence of a timeout, a payment can sit "pending" until the user manually disconnects the wallet, at which point the failure reason surfaced to the user is the unrelated string "wallet disconnected".

**Correct fix:** add a wall-clock-gated sweep that records `record_action_failure(cid, "wallet timeout (>Ns)")` for entries older than a configured TTL (default 90 s, kernel-clock-sourced once V-59 lands). Replace the `_ => {}` arm with a `tracing::warn!` and a `WalletAnomaly::OrphanResponse` counter so the receive-without-correlation case becomes observable rather than dropped.

---

### V-65 · `NOSTRCONNECT_DEFAULT_RELAY_URL = "wss://relay.damus.io"` hardcoded in `nmp-core` [MEDIUM · D0 leak + third-party dependency]

**Verified:** `crates/nmp-core/src/actor/relay_roles.rs:5` — `pub const NOSTRCONNECT_DEFAULT_RELAY_URL: &str = "wss://relay.damus.io";` is a hardcoded third-party relay URL used as a fallback when a user without configured write relays initiates a `nostrconnect://` handshake (NIP-46).

**Impact:** (1) D0 — `nmp-core` should not contain protocol-named third-party endpoints. (2) reliability — if `damus.io` rate-limits or goes offline, every NMP build worldwide that hasn't onboarded write relays cannot complete NIP-46 client-initiated handshakes. (3) policy — choice of bootstrap relay is an app/operator decision, not a framework constant.

**Correct fix:** move the default to host-supplied policy. The host registers a `NostrConnectBootstrapRelays` capability (single URL or weighted list) via `NmpApp::register_capability`; the actor reads from the capability slot. nmp-core retains no hardcoded URL. Until removed, the existing constant must at minimum be flagged on the doctrine-lint banned-token list.

---

### V-66 · `FALLBACK_CONTENT_RELAY` / `FALLBACK_INDEXER_RELAY` activate silently when relay rows are empty [MEDIUM · D3 violation + masked config bug]

**Verified:** `crates/nmp-core/src/kernel/mod.rs:1417,1420` — when `relay_edit_rows` is empty the kernel substitutes `FALLBACK_CONTENT_RELAY` / `FALLBACK_INDEXER_RELAY` for the active routing set. The substitution is silent (no toast, no log, no slot delta) so the host has no way to tell whether the user has zero configured relays or whether their configuration was wiped.

**Impact:** the user appears to be online (publishes succeed against the fallback), but they are publishing to a relay they did not consent to. If their actual relay rows were lost (e.g. keyring re-init, V-62), the loss is invisible until they notice their followers no longer see their notes.

**Correct fix:** distinguish "no rows" from "rows present but degraded". When `relay_edit_rows` is empty the kernel must publish `KernelDiagnostic::NoConfiguredRelays` and either (a) refuse to publish with a typed `NoTargets` error (matches V-50/V-51 fail-closed direction) or (b) require the host to explicitly opt in via a `BootstrapRelaysCapability`. Hardcoded URL constants in nmp-core must not be the production path.

---

### V-67 · Kernel init silently degrades to in-memory store on LMDB open failure [MEDIUM · silent durability loss]

**Verified:** `crates/nmp-core/src/kernel/mod.rs:872` — LMDB open failure during kernel init falls through to an ephemeral in-process store. No `KernelDiagnostic` is emitted; no host callback is invoked. The kernel reports itself as healthy.

**Impact:** the user runs the app, ingests events, publishes notes — and on the next launch every locally-stored event is gone (because the LMDB file was never opened, so the in-memory store was the only persistence layer). Notifications, drafts, profile cache, follow list — all transient without warning. This is one of the harder failure modes to diagnose because everything else works.

**Correct fix:** LMDB open failure must surface as `KernelInitError::StoreUnavailable { reason }` to the host. The host decides whether to retry, fall back, or block startup. nmp-core never installs an ephemeral store as an implicit default. Same posture as V-62 (Marmot keyring): silent mock installation is not a doctrined option.

---

### V-68 · iOS Chirp hardcoded 21,000 msat (21 sat) zap default — production UX hazard [MEDIUM · v1-A Chirp UX]

**Verified:** `ios/Chirp/Chirp/Bridge/KernelModel.swift:446` — `func zap(...) { amountMsats: UInt64 = 21_000, ... }` with a doc-comment at `:433-434` stating "defaults to 21,000 msats (21 sats) until an amount picker lands." Every zap dispatch from the iOS shell that doesn't explicitly pass an amount sends 21 sats.

**Impact:** users expecting a richer zap amount (e.g. 1,000 / 5,000 / 21,000 sats) send 21 sats because no picker exists. The default is in production iOS, not behind a feature flag, and the doc-comment promises a picker that has not landed. This is a user-facing UX defect, not framework debt.

**Correct fix:** ship the amount picker (sheet with 21 / 100 / 500 / 1k / 5k / 21k presets + custom field) and remove the function default. The Rust side (`nmp_nip57::zap`) already accepts `amount_msats`; the gap is purely Swift UI. Until the picker ships, the default should be an explicit `nil` that forces the host to surface a sheet rather than silently dispatching 21 sats.

---

### V-70 · `hex_to_bytes32` returns all-zeros on malformed hex — `RawEvent::id_bytes/pubkey_bytes` produce valid-shape but wrong IDs [LOW · sharp-edge API]

**Verified:** `crates/nmp-store/src/types/ids.rs:20-34` — `hex_to_bytes32(s)` returns `[0u8; 32]` whenever `s.len() != 64` or any byte is non-hex. `crates/nmp-store/src/types/events.rs:38-46` exposes this via `RawEvent::id_bytes()` and `RawEvent::pubkey_bytes()` with doc-comments that admit the silent-zero behaviour.

**Impact:** `VerifiedEvent::try_from_raw` gates inserts behind Schnorr verification so a malformed `RawEvent` cannot enter the store with a zero ID/pubkey under normal flow. But any caller (current or future) that reads `id_bytes`/`pubkey_bytes` from an unverified `RawEvent` (e.g. for prefiltering, indexing, debug telemetry) silently receives the all-zeros sentinel. The all-zeros pubkey is a valid 32-byte shape, so downstream comparisons (`== local_pubkey`) cannot distinguish "decode failure" from "actually zero".

**Correct fix:** change the signature to `fn hex_to_bytes32(s: &str) -> Option<[u8; 32]>` and propagate through `RawEvent::id_bytes/pubkey_bytes` as `Option<EventId>` / `Option<PubKey>`. Callers must handle the `None` arm explicitly. This is a one-shot mechanical refactor confined to `nmp-store`.

---

### V-71 · `nip65_resolver` module doc claims `tracing::debug!` logging that the code does not perform [LOW · false documentation + missing observability]

**Verified:** `crates/nmp-router/src/nip65_resolver.rs:30` — module doc states "bad-shape kind:10002 tags (missing url, non-wss) are logged via `tracing::debug!` and skipped". A `grep tracing|debug!|warn!` of the file returns only the doc-comment itself; no tracing call exists.

**Impact:** (1) documentation lies — a maintainer reading the module believes there is observability that doesn't exist. (2) the V-51 routing-trace inspector cannot attribute "kind:10002 tag was malformed" as a cause of an empty outbox set, because the malformed-tag path is silent. Diagnosing "why didn't my note publish?" is harder than the doc implies.

**Correct fix:** add the `tracing::debug!(target: "nmp.router.nip65", url=?, reason=?, "skipping malformed kind:10002 tag")` calls at the two skip sites (missing URL, non-`wss://`), and surface a `RouterAnomaly::MalformedRelayTag` counter into V-51's routing-trace snapshot so the inspector can attribute empty-set outcomes.

---

### V-73 · `register.rs` falls back to empty `Pubkey` on null/invalid viewer_pubkey — anonymous register with no host signal [LOW · silent identity bug]

**Verified:** `apps/chirp/nmp-app-chirp/src/ffi/register.rs:114` — null or malformed `viewer_pubkey` is replaced with `Pubkey::default()` (32 zero bytes) and the register call proceeds. No error is returned to Swift.

**Impact:** the iOS host believes it registered a logged-in user; the Rust side proceeds with the all-zeros pubkey as the active viewer. Personal-timeline projections, NIP-65 outbox resolution, and DM inbox filtering all run against the zero-pubkey "anonymous" identity. The user appears to be logged in to themselves but is treated as the canonical empty account by every Rust subsystem.

**Correct fix:** the C-ABI `nmp_app_chirp_register` must return `NmpRegisterStatus::InvalidViewerPubkey` on null or non-32-byte input; Swift surfaces the failure to the onboarding flow. There is no doctrined reason for a register call with an invalid identity to silently succeed as anonymous.

---

### V-75 · Router Lane 7 (AppRelay) catch-all silent — V-51 routing-trace cannot attribute empty-outbox causes [LOW · routing observability]

**Verified:** `crates/nmp-router/src/router.rs:250-260` and `:377-388` — the AppRelay catch-all fires when all prior lanes (NIP-65, hint cache, recipient inbox, etc.) produce empty sets. No diagnostic is emitted for which lane attempted what; the catch-all is the silent terminator of an empty publish set.

**Impact:** V-51's routing-trace inspector can show "event Y went to relay B via lane N" but cannot show "lanes 1–6 returned empty for reason R". When a publish appears to succeed against an app relay that the user didn't configure, the user has no way to find out why their NIP-65 write relays were skipped.

**Correct fix:** each lane emits a typed `RouteAttempt { lane, outcome }` into the routing-trace ring buffer, including empty-set outcomes. The Lane 7 fallback explicitly attributes itself as `Lane::AppRelayFallback` so the V-51 inspector can show the empty-cause chain. This is a strict extension of V-51, not a duplicate.

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

### V-79 · NIP-47 wallet connection has no heartbeat and no reconnect — connection can stale silently [LOW · wallet connection resilience]

**Verified:** `crates/nmp-nip47/src/runtime.rs` — no `ping`, `health`, `interval`, `heartbeat`, `reconnect`, `backoff`, or `tokio::time` symbols. On `UNAUTHORIZED` / `RESTRICTED` error codes (`runtime.rs:398-399`) the connection `status` is set to `"error"` but no reconnect is attempted. There is no periodic liveness probe; a wallet that goes offline after the initial handshake leaves the connection in `"ready"` indefinitely. V-14 (which would be the natural home for this) is scoped to NIP-46 bunker reconnect and is marked DONE — NIP-47 NWC is a separate transport with no equivalent tracker.

**Impact:** the user sees the wallet status pill as "ready" while the wallet is in fact unreachable; the first outbound `pay_invoice` after the connection stales fails with a transport error that the user can't pre-empt. There is no diagnostic surface to attribute the failure to a stale connection (the user reads it as a wallet bug). This is the wallet-side analogue of the relay-flap pattern V-14 fixed for bunker.

**Correct fix:** mirror V-14's design for NIP-47 — (a) periodic `get_info` heartbeat at a low cadence (~30s) while the wallet UI is visible, (b) on three consecutive failures, transition `status` to `"connecting"` and re-establish the subscription, (c) project a `nmp.nwc.connection_state` field (Connected / Reconnecting / TransportLost) so the host shell can render a non-silent indicator. Implementation must reuse the relay-flap reconnect primitives from V-14 rather than introducing a parallel timer subsystem.

---

### V-80 · Home feed is thread-roots-only with reply attribution [HIGH · v1 PRODUCT-MODEL FIX]

> **Numbering note (2026-05-28):** the design doc
> [`docs/perf/op-centric-feed-architecture.md`](perf/op-centric-feed-architecture.md)
> §8 drafted this entry as "V-59". V-59 (and every number through V-79) was
> already assigned by the time rung 1 landed, so this entry takes the next
> free number, **V-80**. The design doc's internal "V-59" references all
> point to *this* item.

**Status:** spec proposed 2026-05-27 in
[`docs/perf/op-centric-feed-architecture.md`](perf/op-centric-feed-architecture.md).
**Rung 1 (Stage 0 kernel substrate additions) landed 2026-05-28** — five pure
kernel additions with no consumer yet (see WIP.md / the rung-1 PR).
**Rung 2 (Stage 1 — lossless `TimelineBlock::Standalone`) landed 2026-05-28**
— `Standalone(EventId)` reshaped to `Standalone { id, root: Option<ThreadPointer> }`;
the grouper's chain-length-1 path (`grouper.rs:367`) and the module-collapse
removal path now preserve the resolved root pointer, closing the
root-dropping bug. Every Rust + Swift consumer of the serialized shape was
patched atomically. Behavior delta: chirp-tui's ↳ "reply in thread"
indicator now fires for `Standalone` reply rows (it previously only lit for
`Module` blocks). Home feed still rides `ModularTimelineProjection` (the
projection swap is rung 7).
**Rung 3 (Stage 2 — generic `RootIndexedFeed` engine in `nmp-feed`) landed
2026-05-28** — `trait AttributionPayload` (associated `type Profile`, the B1
dep-cycle fix), `struct RootIndexedFeed<R, A, C>` state machine
(`KernelEventObserver` + `FeedController`), `RootCard<C, A>` /
`RootFeedSnapshot<C, A>` (raw `Vec<A>` attribution, no `attribution_total` —
Q1), `ClaimRequest{Claim,Release}` carrying a `ThreadPointer` (codex M2).
Capabilities are closures, not traits (D7): follow predicate, event lookup,
claim sink. ADR-0035. CI grep gate
(`crates/nmp-testing/tests/op_feed_doctrine_lint.rs`) enforces zero
protocol/profile tokens in `crates/nmp-feed/src/`. V-81 resolved via option
(a) — the engine treats `event_claim_released` as non-terminal (see V-81).
Engine ships **unwired** with 17 synthetic tests; Chirp unchanged, master
green. Rungs 4–7 remain.
**Rung 4 (Stage 3a — `ActiveFollowSet` follow-set producer in `nmp-nip02`)
landed 2026-05-28** — `struct ActiveFollowSet` (`crates/nmp-nip02/src/active_follow_set.rs`)
with `Arc<RwLock<BTreeSet<String>>>` internal state, an internal
`KernelEventObserver` that rebuilds the set from the active account's kind:3
(author-gated, self-inclusion mirroring `contacts.rs::sync_follow_feed_interests`
lines 162-164), and the explicit account-change seam
`notify_account_changed()` (rebuilds on switch, clears on logout). Public API
is closures-only — **no `FollowSetLookup` trait** (B1/§3-D override):
`follows() -> Vec<String>`, `predicate() -> Arc<dyn Fn(&str) -> bool + Send +
Sync>` (captures a clone of the internal `Arc<RwLock<…>>`, so a handed-out
predicate reflects later set changes live), `on_change(Box<dyn Fn() + …>)`.
Constructor takes the kernel's `ActiveAccountSlot` (re-exported via
`nmp_core::slots`), **not** `&NmpApp` — that keeps `nmp-nip02` on `nmp-core`
only (no new `nmp-feed` edge, no production `nmp-ffi` edge; `cargo tree -p
nmp-nip02` unchanged). ADR-0036 records the composition-root expansion
decision (no planner `SocialTimeline` seam). Producer ships **unwired** with
12 synthetic tests; rungs 5 (`nmp-nip01` instance) + 6 (`nmp-app-template`
composition) consume it. Chirp unchanged, master green. Rungs 5–7 remain.
**Rung 5 (Stage 3b — `nmp-nip01` OP-feed instance) landed 2026-05-28** —
`crates/nmp-nip01/src/op_feed/` binds the generic engine to NIP-10:
`Nip10ReplyAttribution` implements `nmp_feed::AttributionPayload` with
`type Profile = ProfileDisplay` (raw pubkey + reply id + raw `created_at` +
`Option<String>` display-name/picture mirrors per the 2026-05-25 display-
separation doctrine; `refresh_for_profile` mirrors
`ModularTimelineProjection::refresh_author_cards`). `register_op_feed(viewer,
follow_predicate, event_lookup, claim_sink) -> Arc<OpFeedEngine>` constructs
`RootIndexedFeed<Nip10Resolver, Nip10ReplyAttribution, TimelineEventCard>`;
`build_actor_claim_sink(dispatch)` encodes the `ThreadPointer` as a `nostr:`
URI (`nevent` for `Event`, `naddr` for `Address`; `External` terminal) and
dispatches the existing `ActorCommand::ClaimEvent` / `ReleaseEvent` (the
Rust seam behind `nmp_app_claim_event`, never the `extern "C"` symbol). A
new public `TimelineEventCard::from_event_for_op_feed(root, target)` is the
stateless card-builder reuse seam (the private `from_event` needs kernel-
internal caches). **Spec-vs-code drift surfaced & followed:** (1) the
engine is `RootIndexedFeed<R, A, C>` with a 7-arg constructor, not the
doc's `<R, A>`; (2) `register_op_feed` takes `nmp-core` primitives, **not**
`&NmpApp` (same `nmp-ffi`-edge inversion rung 4 rejected — composition root
rung 6 does the `NmpApp` registration); (3) `event_lookup` is
`Fn(&EventId)`, not the doc's `Fn(&str)`; (4) `from_event` is private with a
5-arg signature, so a new public stateless helper was added. Instance ships
**unwired in production** (only tests register `"nmp.feed.home"`; Chirp keeps
`ModularTimelineProjection` until rung 7) with 13 tests covering repost
L-1…L-5, claim-URI shape, profile refresh, snapshot shape, and the V-81
non-terminal release signal. `cargo test -p nmp-nip01` 108 pass;
`cargo build -p nmp-app-chirp` clean; doctrine-lint smoke 42 pass. Rungs
6–7 remain.
**Rung 6 (Stage 5 — `nmp-app-template` composition root) landed 2026-05-28** —
`crates/nmp-app-template/src/op_feed_defaults.rs` adds
`register_op_feed_defaults(app: &NmpApp, viewer: Pubkey, active_account_slot:
ActiveAccountSlot) -> Arc<OpFeedEngine>`. It constructs
`nmp_nip02::ActiveFollowSet` over the slot, registers it as a
`KernelEventObserver`, builds the engine via `nmp_nip01::register_op_feed`
(follow predicate = `ActiveFollowSet::predicate()`, claim sink =
`build_actor_claim_sink` over `app.actor_sender()`, no-op `event_lookup`),
registers the engine as both a `KernelEventObserver` (ingest) and a
`FeedController` under `"nmp.feed.home"` (output), and wires a self-detecting
`on_change` callback that resets the engine ONLY on an account switch (the
pubkey actually changed), never on a kind:3 update (the predicate is live).
**CRITICAL DECISION — no `expand_follow_timeline_interests`:** the design doc
§5 Stage 5 / ADR-0036 sketch per-follow `LogicalInterest` registration at the
composition root "mirroring `sync_follow_feed_interests`," but the kernel
**still owns** `sync_follow_feed_interests`
(`crates/nmp-core/src/kernel/ingest/contacts.rs:119`), which already registers
those interests on the active account's kind:3 and on identity change.
Re-registering at the composition root would be **duplicate REQ
subscriptions** — so this rung registers the engine + follow-set observer
ONLY, no interest expansion. The doc predates the kernel keeping
`sync_follow_feed_interests`. **Spec-vs-code drift followed:** (1) the
function takes an explicit `active_account_slot` param — `NmpApp` exposes no
synchronous `ActiveAccountSlot` accessor (kernel makes its own at
`mod.rs:1406`, never threaded back); a thin accessor is filed as **V-82**;
(2) `event_lookup` is a no-op `|_| None` — no synchronous event-by-id read API
on `NmpApp`; the engine's L-2 re-key fallback keeps it correctness-preserving;
the optimization is filed as **V-83**; (3) `send_cmd` is crate-private, so the
claim sink dispatches through `actor_sender()`. **`register_op_feed_defaults`
is NOT called by `register_defaults` and NOT wired into Chirp this rung** —
defined + tested only (4 integration tests:
`crates/nmp-app-template/tests/op_feed_defaults_test.rs`); rung 7 makes Chirp
call it and removes the `ModularTimelineProjection` registration. `cargo test
-p nmp-app-template` 7 pass (4 new + 3 existing); `cargo build -p
nmp-app-chirp` clean; doctrine-lint smoke 42 pass. Master green; Chirp
unchanged. **Rung-7 note:** the engine's repost cards key the root slot by
`target_id` (`card.id == target_id`, `ingest.rs:101`), differing from
`ModularTimelineProjection`'s wrapper-id keying — rung 7's chirp-tui /
codegen swap must account for this.

**MIGRATION COMPLETE (2026-05-29).** Rung 7 (Chirp cutover, #747) plus the
ADR-0038 typed-`NOFS` ladder all landed: B1 typed schema/encoder/emission
(#752), B2 chirp-tui typed decoder (#753), B3 iOS decoder (#755), B4 Android
decoder (#757), plus V-82 (#745) + V-83 (#756). The OP-centric home feed is
**LIVE on master**: chirp-tui reads via the typed `NOFS` path; iOS/Android
read via the generic `RootFeedSnapshot` fallback (their typed decoders ship
**decoder-only** — wiring them into render needs a Swift/Kotlin NFCT
content-tree decoder, tracked as **V-84/V-85** below). Behavior verified
through the real production composition + projection + render via integration
and unit tests (`op_feed_defaults_test`, `op_feed_repost_hydration_test`,
chirp-tui snapshot/render-parity, B1 golden-wire, B4 Kotlin golden 5/5).
**Live tmux / iOS-sim runtime confirmation was blocked by environment only**
(unsigned-binary macOS keychain prompt; incomplete Xcode-26-beta `UIUtilities`
framework stubs in `/tmp/LocalFrameworks` + missing `docs/dev/xcode26-workarounds.md`)
— reproducible by a developer in a configured GUI/Xcode env. See
[`docs/perf/pending-user-decisions.md`](perf/pending-user-decisions.md).

**Evidence:** today's home feed (chirp-tui left pane, Chirp iOS home) shows
replies as standalone feed rows. PR #710 added a ↳ "reply in thread"
indicator as a partial mitigation, but the product model the user wants is
different: **feed = thread roots only; follows' replies attribute back to
their root**. A follow's reply to a non-followed OP should surface the OP
with a "↳ Alice replied" badge. Reply rows never stand alone.

Today's code drops the root pointer on chain-length-1 standalone blocks
(`crates/nmp-threading/src/grouper.rs:367`), defeats attribution at the
threading layer, and lacks any mechanism to fetch a non-followed root id
into the local store (the existing thread-hydration logic
`enqueue_thread_hydration_from_event` only fires when a thread detail view
is open — `crates/nmp-core/src/kernel/ingest/timeline.rs`).

**Architectural shape:** the engine `RootIndexedFeed<R: ParentResolver, A:
AttributionPayload>` lives in `nmp-feed` (generic substrate, zero protocol
knowledge). `nmp-nip01` provides the NIP-10 instance
(`Nip10ReplyAttribution` + `register_op_feed`); a future `nmp-nip22`
provides the kind:1111 instance covering ALL non-kind:1 root kinds
(NIP-23, NIP-94, NIP-99, podcasts, …). One engine, two foreseeable
instances; no per-kind state-machine explosion.

**Rung 1 (DONE 2026-05-28) — kernel substrate additions in `nmp-core`:**
1. `Kernel::active_timeline_authors()` — public typed read accessor over
   the `timeline_authors` projection (raw pubkeys).
2. `pre_kind3_buffer` — bounded staging map that parks kind:1/kind:6 events
   whose author is not yet followed, replayed by
   `sync_follow_feed_interests` once the author is followed.
3. `OneshotApi::request` gains a `hints: Vec<RelayHint>` parameter;
   `claim_event` seeds the initial REQ with the URI's NIP-19 relay TLVs.
4. `event_claim_released` — bounded ring projection + in-process
   `EventClaimReleasedObserver`; `complete_unknown_oneshot` clears claim
   state and pushes the id on EOSE-no-match.
5. `release_event` calls `release_claim_expansion` on refcount-zero (codex
   M3).
All five are pure additions with no consumer yet; master behavior is
unchanged.

**Prerequisite:** V-45 (`LogicalInterest::SocialTimeline` substrate seam) +
`FollowSetLookup` capability — delivered in a later rung per the doc's §5
Stage 0 (note: the doc's §5 "Stage 0" is the V-45/FollowSetLookup work; the
rung-1 PR that landed is a DIFFERENT five-addition decomposition than the
doc's §5 text — see the PR's spec-drift report).

**Recommended action:** seven-rung PR ladder per
[`docs/perf/op-centric-feed-architecture.md`](perf/op-centric-feed-architecture.md)
§5–§6. Net add ~1,700 LOC across `nmp-threading`, `nmp-core` (substrate
seam only), `nmp-planner`, `nmp-feed` (engine), `nmp-nip01` (instance),
`nmp-app-template`, and `apps/chirp/`. Net delete ~250 LOC (partial-chain
machinery in chirp-tui + hand-rolled follow-set wiring in nmp-app-chirp).
Two new ADRs: ADR-0033 (`FollowSetLookup` capability) and ADR-0034
(generic root-indexed feed engine in `nmp-feed`; protocol-specific
instances in NIP crates).

**Open user decisions** carried to `docs/perf/op-centric-feed-architecture.md`
§7: Q1 (attribution cap + deletion semantics), Q2 (LogicalInterest enum vs
discriminator), Q3 (repost behavior under OP-centric model), Q4
(self-replies), Q5 (NIP-22 scope deferred to post-v1), Q6 (root-hydration
latency trade-off). All have flagged defaults if the user is unavailable.

**Out of scope (post-v1):** the `nmp-nip22` instance over kind:1111
comment trees. Implementation is ~150 LOC (one `ParentResolver` impl
plus one `AttributionPayload` impl plus one wiring helper); engine code
is zero new lines. Tracked separately when `nmp-nip22` crate is created.

---

### V-82 · `NmpApp` does not expose the kernel's active-account slot — OP-feed composition root (rung 7) + Chirp cannot read the live active account [MEDIUM · sub-item of V-80, rung-7 prerequisite] — LANDED 2026-05-28

**Origin (rung-6 finding):** the kernel owns the authoritative
`ActiveAccountSlot` (`Arc<Mutex<Option<String>>>`, the active account's hex
pubkey, written by the actor reducer on sign-in / account-switch / logout).
The kernel "makes its own, never threaded back" — `NmpApp` exposed no
accessor — so host code (the V-80 OP-feed composition root at rung 7, and
Chirp) could not read the real slot to seed `ActiveFollowSet::new` or drive
`ActiveFollowSet::notify_account_changed` on an account switch.

**Fix (LANDED):** `NmpApp::active_account_handle(&self) -> ActiveAccountSlot`
in `nmp-ffi` (`crates/nmp-ffi/src/lib.rs`). Single source of truth, no
divergent mirror: `nmp_app_new` constructs the slot once and hands the SAME
`Arc` to the kernel at actor startup via the new
`Kernel::with_storage_path_and_account_slot` constructor (the kernel's
internal `Arc::clone` — including the test-support outbox resolver — references
the supplied slot, so no internal consumer diverges). The `Reset` dispatch arm
rebuilds the kernel through the same constructor with the actor-held slot, so
the shared handle survives a state wipe (mirrors the routing-trace re-publish
contract). The actor reducer remains the sole writer (D4); reads happen
through the host handle. Substrate-clean: the slot holds a raw pubkey `String`
— no NIP noun, D0 stays clean (generic identity plumbing).

**Tests:** 3 nmp-ffi tests driving REAL sign-in / account-switch / Reset
through the actor (not a direct slot poke), incl. an `Arc::as_ptr` identity
check that rules out two divergent slots and a Reset-then-sign-in survival
test (`crates/nmp-ffi/src/active_account_handle_tests.rs`). `cargo test -p
nmp-ffi` 61 pass; `cargo test -p nmp-core --lib` 997 pass; doctrine-lint smoke
42 pass; `cargo build -p nmp-app-template -p nmp-app-chirp` clean.

**Spec-vs-code drift:** the kernel's `ActiveAccountSlot` construction is at
`crates/nmp-core/src/kernel/mod.rs` ~line 1413 (`new_active_account_slot()`),
not ~1406 (the repo moved); the kernel already had a `active_account_handle()`
accessor (`kernel/mod.rs` ~line 1340) — the gap was only the `NmpApp` → kernel
*sharing* at construction, which this item closes.

---

### V-81 · `event_claim_released` signal fires on Phase-1 EOSE, not final give-up — rung-3 consumer must not drop pending attribution early [MEDIUM · sub-item of V-80, blocks rung 3 correctness]

**Origin:** rung-1 (PR #727, commit `171090d3`) added the
`event_claim_released` ring buffer + in-process observer so the OP-feed
engine learns when a root claim resolves to nothing. The push currently
fires inside `complete_unknown_oneshot` on **Phase-1 EOSE**.

**Risk:** Phase-1 EOSE is *not* the final "this event will never arrive"
verdict. Claim expansion (Phase-2 relay retargeting, the W5/W7 hint path)
may still be in flight against other relays. If the rung-3 engine treats
the `event_claim_released` signal as terminal, it will drop the buffered
`pending_attributions[root_id]` while Phase-2 is still trying — so Bob's
OP arrives, but the "Alice replied" badge was already discarded. The user
sees a root card with no attribution even though attribution was known.

**Correct fix (decide in rung 3):** either (a) the engine ignores the
release signal until a true `terminate_claim` (all phases exhausted), or
(b) move the ring push from Phase-1 EOSE to `terminate_claim` in
`nmp-core` as a rung-1 follow-up. (b) is cleaner if `terminate_claim` is
the single authoritative give-up point; (a) keeps rung 1 untouched but
puts the burden on every future consumer. The rung-3 implementer (the
`nmp-feed` `RootIndexedFeed` engine) MUST resolve this before wiring the
release observer to attribution eviction. See
[`docs/perf/op-centric-feed-architecture.md`](perf/op-centric-feed-architecture.md)
§3-K for the buffering model this protects.

**Resolution (rung 3, 2026-05-28): option (a) implemented.**
`RootIndexedFeed::on_event_claim_released` is a no-op beyond a diagnostic
`AtomicU64` counter — it does NOT drop `pending_attributions`. (This
supersedes the design doc §3-D, which predates V-81 and said to drop on the
signal.) A pending attribution survives a release signal and is dropped only
when the root actually arrives (drain) or the bounded map evicts it under D5
pressure. Proven by
`v81_release_signal_does_not_drop_pending_attribution`. Recorded in ADR-0035.
**Option (b) — moving the `nmp-core` ring push from Phase-1 EOSE to
`terminate_claim` — remains a possible rung-1 follow-up.** It is no longer
load-bearing for OP-feed correctness (the engine is robust to the current
Phase-1-EOSE behavior), so this item is downgraded to a cleanup. If pursued,
it would let the engine treat the signal as terminal and proactively emit
`Release` + drop pending, instead of relying on arrival/eviction.

---

### V-82 · `NmpApp::active_account_handle()` accessor for the OP-feed composition root [LOW · sub-item of V-80, rung-7 prerequisite]

**Origin:** rung 6 (Stage 5) needs the kernel's `ActiveAccountSlot` to
construct `nmp_nip02::ActiveFollowSet`. `NmpApp` exposes **no** synchronous
accessor for it: the kernel constructs its `active_account_handle` internally
(`crates/nmp-core/src/kernel/mod.rs:1406`) and never threads a clone back to
`NmpApp` (unlike `relay_edit_rows`, which `NmpApp` owns and injects via
`run_actor_with_observers`). So `register_op_feed_defaults` takes the slot as
an explicit parameter this rung.

**Fix:** add `NmpApp::active_account_handle(&self) -> ActiveAccountSlot`
mirroring the `relay_edit_rows_handle` pattern — `NmpApp` owns the slot
(created in `nmp_app_new`), passes a clone into `run_actor_with_observers`,
and the actor binds it onto the kernel (a new `Kernel::set_active_account_handle`
setter replacing the kernel's internal `new_active_account_slot()`). That is a
small `nmp-core`/actor change, deliberately out of rung-6 scope. Once landed,
`register_op_feed_defaults` can drop its explicit slot parameter and read the
slot off `app`. **Rung 7 (Chirp cut-over) needs this** to obtain the slot at
its `register_op_feed_defaults` call site, so this should land with or before
rung 7.

---

### V-83 · OP-feed `event_lookup` reads the kernel event store (replace no-op closure) [LOW · sub-item of V-80, optimization only] — LANDED 2026-05-29 (#756)

**Origin:** rung 6 wires the engine's
`event_lookup: Arc<dyn Fn(&EventId) -> Option<KernelEvent>>` as a no-op
`|_| None`. There is **no synchronous event-by-id read API on `NmpApp`** — the
kernel's `EventStore::get_by_id` (`crates/nmp-store/src/events.rs:149`) lives
on the actor thread and is never published back to `NmpApp`. The no-op is
**correctness-preserving** for the OP feed: the engine's L-2 fallback holds an
attribution against the (unresolved) wrapper id and re-keys it when the wrapper
later arrives via the observer fan-out (§3-L step 2); L-5 shows the placeholder
card until the target arrives.

**Fix (optimization, not correctness):** expose a kernel-owned, thread-safe
event-by-id read handle on `NmpApp` (an `Arc<dyn EventStore>` clone, or a
typed `Kernel::event_by_id` accessor surfaced like `relay_edit_rows_handle`),
and wire it into the `event_lookup` closure so the engine can resolve a
locally-cached parent/target immediately instead of waiting for the observer
re-key. Only matters for repost L-2/L-5 cold-start latency. **DONE 2026-05-29 (#756):**
landed via a publish-back `EventStoreSlot` + `NmpApp::event_by_id` (single-writer
actor, Reset-survivable); the no-op closure is replaced and repost L-2/L-5
hydration is exercised by `op_feed_repost_hydration_test.rs`.

---

### V-84 · iOS Swift NFCT (content-tree) decoder — wire the iOS typed NOFS read into render [MEDIUM · ADR-0038 rollout tail · post-v1]

**Origin (B3, #755):** iOS ships the typed `NOFS` decoder **decoder-only**. It
is not wired into the render because iOS has **no Swift NFCT content-tree
decoder** — a typed read would yield blank tweet bodies (`contentTree`
unfillable). iOS therefore renders the home feed via the generic `Value`
`RootFeedSnapshot` path (correct, live). Pre-existing gap (the NFTS predecessor
had it too). **Fix:** build a Swift decoder for the embedded `NFCT` content-tree
buffer, then flip `TypedHomeFeedDecoder` into the render preference so iOS gets
the typed hot-path. Behavior is unaffected until then (generic fallback).

### V-85 · Android Kotlin NFCT decoder — wire the Android typed NOFS read into render [LOW · ADR-0038 rollout tail · post-v1]

**Origin (B4, #757):** same as V-84 for Android (gallery). The Kotlin `NOFS`
decoder ships decoder-only (golden test 5/5); Android renders via the generic
fallback. Needs a Kotlin NFCT decoder to wire the typed render. Non-blocking.

### V-86 · `ci/check-flatbuffers-version-pins.sh` glob misses the Chirp `nmp/{nip01,feed}` Kotlin tree [LOW · CI hygiene]

**Origin (B4, #757):** the pin-check glob does not cover the newly-added
Android `nmp/nip01` + `nmp/feed` generated Kotlin bindings (pinned `25.2.10`).
It also misses `android/app/src/main/java/nmp/transport/` (9 files) — the main
Android Chirp app's transport layer is **completely unchecked** by the pin glob,
not just the Chirp `nmp/{nip01,feed}` trees. Pre-existing-shaped gap; extend the
glob so the Android NOFS/timeline/feed bindings **and the `nmp/transport` tree**
are pin-checked in CI.

---

> **Provenance — V-87 … V-105 (2026-05-29 GH-issue audit, issues #600–#630).**
> These nineteen entries fold the 31 open GitHub issues from the offline-first /
> doctrine audit into Section 1. Every citation below was re-confirmed against
> HEAD (`c5302157`) before being recorded — per the Section 1 invariant, no entry
> asserts a live violation that the current tree does not exhibit. Where an issue's
> originally-filed `file:line` had drifted, the citation is corrected here; where
> the described violation is **already fixed at HEAD**, the entry says so and the
> action is to close the stale GH issue rather than re-open a phantom violation.

### V-87 · D1 startup violations cluster [HIGH · pre-v1 · issues #600–#606]

The D1 / offline-first contract (`docs/product-spec/offline-first.md` §1–§6):
the first rendered frame must not depend on relay I/O or relay connectivity.
Seven candidate sites were filed; HEAD-verified status below.

1. **#600 — ALREADY FIXED AT HEAD. Close the issue.**
   `crates/nmp-core/src/actor/dispatch.rs:443-451` — the `Start` arm now calls
   `emit_now` (`:444`) **before** `spawn_missing_relays` (`:445`), with the
   explicit comment "first snapshot must reach the shell before any relay TCP
   connection is dialed, so emit_now precedes spawn_missing_relays". The order
   the issue asked for is already in place. No live violation; mark #600 resolved.
2. `crates/nmp-core/src/actor/mod.rs:1176` [#601] — the actor blocks on
   `command_rx.recv()` (`let first_command = match command_rx.recv()`) before
   constructing the Kernel. No snapshot can emit until the host sends a command;
   a host that waits for the first snapshot before sending `Start` deadlocks.
   **Confirmed live.**
3. `crates/nmp-core/src/actor/relay_mgmt.rs:178-188` [#602] — `maybe_send_startup`
   (`:178`) early-returns unless `all_relays_connected(connected_relays)` (`:188`,
   helper at `:51`) is true. One tardy lane (e.g. Indexer) delays Content-lane
   startup REQs indefinitely. **Confirmed live.**
4. **#603 — CITATION STALE. Re-scope before fixing.** The filed citation
   `apps/nmp-gallery/tui/src/live.rs:161-195` (`bootstrap()` chaining six
   `recv_timeout` loops) does **not** exist at HEAD: `live.rs` is 217 lines, has
   no `bootstrap` fn and no `recv_timeout` call. The polling bootstrap appears to
   have been refactored out. Re-audit the gallery TUI live path (`live.rs`,
   `embed_host.rs`) for any remaining pre-first-frame blocking loop and re-file
   with a HEAD-accurate citation, or close #603.
5. `ios/Chirp/Chirp/Features/HomeFeedView.swift:101` [#604] — empty
   `blocks`/`items` renders `ChirpPlaceholder(…)` until the first kernel tick;
   the shell cannot distinguish "no events" from "not yet ticked". **Confirmed
   live** (placeholder branch present; copy now differs from the originally-filed
   string — see V-99).
6. **#605 — CITATION STALE.** `ios/Chirp/Chirp/Features/ThreadScreen.swift` (202
   lines) does **not** contain the string "Fetching notes from the relay network"
   anywhere in the iOS tree, and the `threadView == nil` hard-gate at `:30-64` is
   not present as filed. Re-audit `ThreadScreen.swift` for the current loading
   gate and re-file with a HEAD-accurate `file:line`, or close #605. (See V-99 —
   the user-facing-copy half of this issue is also stale.)
7. `crates/nmp-core/src/kernel/types.rs:184` [#606] — `ProfileCard.has_profile:
   bool` is consumed as a render gate at
   `ios/Chirp/Chirp/Features/ProfileView.swift:142,168` (`profile?.hasProfile ==
   true`). It trains callers to block fields on relay data. **Confirmed live**
   (the iOS gate is real; the originally-filed gallery `live.rs:419` cite is
   stale — `live.rs` is only 217 lines — so the gallery half needs re-citing).

**Required fix:** Items 2–3 require kernel/actor changes to emit the first
snapshot before any network I/O or relay-connectivity gate. Items 5, 7 require
shell changes: render immediately with placeholders, never gate on relay state.
Item 1 is done (close #600). Items 4, 6 need re-citation against HEAD or closure.

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

### V-89 · Sentinel API values cause double-stamping — P2 type-safety gaps [MEDIUM · issues #608 #609 #610]

Three builders require sentinel (zero/empty) inputs that callers must not replace
with real values, with no type-level enforcement of the distinction:

1. **#608 — CITATION STALE; re-scope to the real seam.** The filed cite
   `crates/nmp-nip59/src/wrap.rs:41-55` (dual-public `gift_wrap` /
   `gift_wrap_with_signer`) does **not** match HEAD: `wrap.rs` exposes only
   `unwrap_gift_wrap` (`:33`); there is no `gift_wrap(sender: &Keys, …)` free
   function. The signer seam is `gift_wrap_with_signer` at
   `crates/nmp-nip59/src/signer_seal.rs:234` (re-exported `lib.rs:38`). PR #760
   already made `wallet_connect` `pub(crate)`. Re-file #608 against the actual
   `signer_seal.rs` API surface, or close it if the dual-path no longer exists.
2. `crates/nmp-nip17/src/lib.rs:107` [#609] — `build_dm_rumor(input,
   sender_pubkey: &str)` writes `pubkey: sender_pubkey.to_string()` (`:125`);
   action-executor call sites must pass `sender_pubkey = ""` and a real value
   double-stamps. No type enforcement. **Confirmed live.**
3. `crates/nmp-nip57/src/build.rs:117-122`, `action.rs:200` [#610] —
   `ZapRequestBuilder::build(author, created_at)` (`build.rs:117`, writes
   `pubkey: author.into()` at `:155`) is called with `String::new(), 0` at
   `action.rs:200`; `created_at = 0` is the documented D7 sentinel (`action.rs:199`)
   and real values double-stamp. **Confirmed live.**

**Correct fix:** Each function should accept `Option<T>` or use a builder/typestate
split (`Unsigned*` type at action call time, signed variant post-actor-signing).

### V-90 · Actor thread blocking during remote-signer operations [HIGH · D8 violation · issues #612 #613]

Two D8 violations (no blocking on the actor thread):

1. `crates/nmp-nip17/src/dm_send.rs:221` [#612] — `ProtocolCommand::run` calls
   `op.wait(nmp_nip59::GIFT_WRAP_TOTAL_TIMEOUT)`, blocking up to the 12 s gift-wrap
   budget for the remote-signer response on the actor thread, stalling the kernel
   loop for all other commands. **Confirmed live.**
2. `crates/nmp-ffi/src/capability.rs:56` (`nmp_app_dispatch_capability`), invoked
   in-actor via `self.dispatch_capability(&req)` at
   `crates/nmp-ffi/src/lib.rs:1524,1541` [#613] — the registered platform
   capability callback runs synchronously on the actor thread; iOS Keychain
   blocks hundreds of ms. **Confirmed live** (filed `lib.rs:1399` drifted; the
   real in-actor call sites are `:1524,:1541`).

**Note:** Related to V-54 (NIP-46 onboarding blocks the actor thread, at
`identity.rs:826,864,1019`). V-90 covers two additional blocking paths not in
V-54's scope.

**Correct fix:** Move both operations off the actor thread. The protocol command
must use a non-blocking async channel; capability dispatch must queue the callback
and settle via a dedicated capability thread.

### V-91 · Android nativeNextUpdate blocks calling thread 250ms per poll [MEDIUM · P2/P3 · issue #614]

**Verified:** `crates/nmp-android-ffi/src/lib.rs:185` —
`Java_org_nmp_android_KernelBridge_nativeNextUpdate*` (declared `:163,:172`) calls
`s.rx.recv_timeout(Duration::from_millis(250))`, forcing Kotlin into a polling
drain loop with a 250 ms blocking budget per call. iOS uses a push model (callback
on the listener thread); Android should match. The `recv_timeout` polling pattern
is a D8 violation at the FFI boundary.

**Correct fix:** Replace the `recv_timeout` polling pattern with a push-based
callback notification model matching the iOS `set_update_callback` architecture.

### V-92 · Relay reconnect backoff never resets after a healthy session [LOW · reliability · issue #615]

**Verified:** `crates/nmp-network/src/relay_worker/mod.rs:152` initializes
`backoff = RELAY_RECONNECT_DELAY_INITIAL`; the mid-session-drop branch at
`:183-184` carries the explicit comment "Do NOT reset backoff here", and backoff
only ever doubles (capped at `RELAY_RECONNECT_DELAY_MAX`). No reset path exists
after a sustained `Connected` state, so a relay that drops after a long healthy
session re-enters at the maximum backoff. **Confirmed live.**

**Correct fix:** Reset backoff to `RELAY_RECONNECT_DELAY_INITIAL` after a
configurable minimum connected duration (e.g. 5 minutes). Related to V-58 (backoff
blind to close reason) — that fix and this fix compose naturally.

### V-93 · Kernel constructor blocks synchronously on LMDB open and pending load [MEDIUM · D1/P3 · issue #617]

**Verified:** `crates/nmp-core/src/kernel/mod.rs:1020` (`build_event_store`, LMDB
open) and `:1098` (`load_profile_intents`, which walks all pending publish records
via `publish_store.load_pending()` at `:1102`) run synchronously in the construction
path. A slow LMDB open or a large publish-intent backlog delays kernel construction
and therefore blocks the first snapshot emit. Related to V-67 (LMDB silent
degradation) but a distinct startup-latency issue. **Confirmed live.**

**Correct fix:** Defer the publish-intent load to after the first snapshot is
emitted; open LMDB asynchronously or on a background task that resolves before the
first publish command needs it.

### V-94 · 10+ must-call-before-`nmp_app_start` constraints have no type enforcement [MEDIUM · P3 · issue #618]

**Verified:** multiple `crates/nmp-ffi/src/lib.rs` setup symbols must be wired
before `nmp_app_start` for correct behavior, but ordering is documented in prose
only — `nmp_app_set_storage_path` (slot doc `lib.rs:345`; omission permanently
loses storage), `set_coverage_hook` (`lib.rs:1160`; a late call is silently
ignored), `nmp_app_set_update_callback` (`lib.rs:207`), and the REQ-frame setters.
No compile-time or runtime check prevents calling `Start` before these are wired.
**Confirmed live.**

**Correct fix:** Introduce a builder/configuration type (`NmpAppConfig`) that must
be fully constructed before `nmp_app_start` can be called. At minimum, add a
runtime assertion that emits a `KernelDiagnostic::MissingSetup` before the first
tick.

### V-95 · WalletRuntime initialization order not type-enforced — OnceLock error risk [MEDIUM · P2/P3 · issue #619]

**Verified:** `crates/nmp-nip47/src/runtime.rs:107` (`install_wallet_runtime`)
populates a process-global `OnceLock` read by `active_wallet_runtime()` (`:80`
doc); `WalletConnectModule`/`WalletDisconnectModule`/`WalletPayInvoiceModule`
`execute` read it and return a runtime error when `install_wallet_runtime` was
never called (`:114` doc). The type system does not prevent dispatch before
installation. **Confirmed live.**

**Correct fix:** Pass `Arc<Mutex<Option<WalletRuntime>>>` as a field on each module
struct (injected at registration time via `nmp-app-template`), eliminating the
global and making the initialization order visible from the type signature.

### V-96 · NIP-57 bolt11-fetch path consolidation [MEDIUM · P1 · issue #620] — PARTIALLY RESOLVED AT HEAD

**Verified:** the two migration-artifact paths the issue targets are **already**
`pub(crate)` at HEAD: `crates/nmp-nip57/src/lnurl/mod.rs:419`
(`pub(crate) fn fetch_lnurl_invoice_blocking`) and `:559`
(`pub(crate) fn fetch_bolt11_for_zap`). The architecturally-correct in-kernel path
`FetchLnurlInvoiceCommand` is the only public bolt11 surface (`lib.rs:30`
re-export; used by `action.rs:35`). The "three public paths" framing no longer
holds — crate-external callers can only reach the protocol command.

**Remaining work:** confirm no crate-internal caller still routes through the two
`pub(crate)` blocking helpers outside `FetchLnurlInvoiceCommand::run`; if clean,
close #620. If an internal bypass remains, delete it. This is verification-only,
not a structural re-architecture.

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

### V-100 · iOS WalletView validates the NIP-47 URI scheme in Swift [LOW · P5 · issue #625]

**Verified:** `ios/Chirp/Chirp/Features/WalletView.swift:209-211` —
`schemeLooksValid(_:)` checks `trimmed.lowercased().hasPrefix("nostr+walletconnect://")`
in Swift and gates the connect button (`.disabled(!schemeLooksValid(uri))` at
`:192`). Protocol URI validation belongs in Rust. **Confirmed live.**

**Correct fix:** Remove the Swift-side validation. `dispatch_action(
"nmp.nwc.connect", {"uri": …})` should validate and surface a typed
`ActionError::InvalidNwcUri` in the action-lifecycle projection.

### V-101 · iOS NIP-29 group relay URL hardcoded in Swift `@State` [LOW · P5 · issue #626]

**Verified:** `ios/Chirp/Chirp/Features/NewGroupSheet.swift:26` —
`@State private var publicRelayUrl = "wss://relay.groups.nip29.com"`, a
compile-time third-party URL baked into Swift state. **Confirmed live.**

**Correct fix:** Surface a default NIP-29 relay URL through the kernel
configuration projection so it can be updated without a client release. Related to
V-65 (NOSTRCONNECT_DEFAULT_RELAY_URL) — same category.

### V-102 · `TimelineEventCard`/`ModularTimelineSnapshot` are app-domain types exported from a protocol crate [MEDIUM · D0 · issue #627]

**Verified:** `crates/nmp-nip01/src/timeline_projection.rs:44`
(`pub struct TimelineEventCard`) and `ModularTimelineSnapshot` in the same module
embed app-layer concerns (display-name formatting, picture URLs, timeline
windowing, feed cursor) but are exported from a protocol crate. "Timeline" and
"feed card" are app nouns forbidden from NIP crates under D0. **Confirmed live.**

**Correct fix:** Move these types to `crates/nmp-app-template/` or a new
`crates/nmp-social-feed/` crate. The protocol crate retains only the raw event
data types.

### V-103 · Missing D1 bootstrap regression test [MEDIUM · test coverage · issue #628]

**Verified:** `docs/product-spec/offline-first.md` §7 (line 80–82) mandates that
every viewer-class app have a smoke test that boots the kernel with **zero relay
connectivity** and verifies the first rendered frame is produced from local-store
content alone. No `d1_bootstrap`-style test exists in `crates/nmp-testing/tests/`.
**Confirmed: gap is live.**

**Correct fix:** Add `crates/nmp-testing/tests/d1_bootstrap.rs` that (1) seeds LMDB
with events, (2) boots the kernel with no relay URLs configured, (3) asserts
`make_update` emits a non-empty snapshot before any relay connection is attempted.

### V-104 · Six `e2e_full_pipeline` tests are unimplemented stubs [MEDIUM · test coverage · issue #629]

**Verified:** `crates/nmp-testing/tests/e2e_full_pipeline.rs` — all six integration
tests are `#[ignore]` stubs whose bodies are `todo!("implement once …")`
(`:83,:123,:164,:203,:244,:292`). The milestones the stubs wait on (M2, M3, M8) are
marked DONE in `docs/plan.md`. The six cover `cold_open_profile_view_full_pipeline`
(`:61`), `kind3_update_rewires_subscriptions`, `publish_roundtrip_via_outbox`,
`negentropy_skips_redundant_req` (`:181` — the core D1/D2 regression),
`auth_required_for_read_flow`, and `monotonic_rev_under_concurrent_ingests`.
**Confirmed live.**

**Correct fix:** Implement each test. `negentropy_skips_redundant_req` in
particular is load-bearing for D1/D2 doctrine and must pass before v1 ships.

### V-105 · Test infra: `wait_for_snapshot_predicate` uses untyped substring scanning [LOW · test hygiene · issue #630]

**Verified:** `crates/nmp-testing/tests/nip46_bunker_signing.rs:191` (and `:229,
:237`) probe snapshot JSON blobs with `frame.contains("\"signer_kind\":\"nip46\"")`
because no typed "signer registered" observable exists; and
`crates/nmp-testing/tests/publish_unsigned_event.rs:146-149` runs a ~300 ms blind
`recv_timeout` drain loop to "ensure the Start completes". **Confirmed live.**

**Correct fix:** (1) Add an `ActorEvent::SignerRegistered` variant or a signer-state
field in `UpdateEnvelope` so tests can use a typed `recv()`. (2) Add an
`ActorCommand::Barrier` equivalent to replace the drain loop.

---

Work currently on a branch lives in [`WIP.md`](../WIP.md). Agents must check that file
before picking up Section 4 work to avoid duplicating an in-progress task.

---

## Section 3 — Pending User Decisions

Items that cannot be resolved autonomously. An agent that encounters one of these must log
its finding in the decision thread below and move on to the next item, not block.

### PD-033-A · Framework thesis — second non-social app — CLOSED BY DELETION (2026-05-28)

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

### PD-041 · Marmot/NWC scope reconciliation — NEEDS OWNER DECISION

Earlier temporal planning deferred M9 DMs/messaging and M12 Wallet to post-v1,
but Marmot/MLS (`nmp-marmot`, `nmp-nip29`, `nmp-nip59`) and
NWC + NIP-57 (`nmp-nwc`, `nmp-nip57`) were subsequently built and wired.

**Decision needed:** either formally accept the built Marmot/NWC surfaces into
the v1 support matrix, or label them experimental/Labs/post-v1 so `docs/plan.md`,
`docs/plan/post-v1.md`, product copy, and validation expectations do not imply
contradictory scope.

---

## Section 4 — V1 Feature Backlog

Ordered by blocking priority. Items earlier in the list unblock items below them. An
autonomous agent picks the topmost item not already in Section 2.

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
open item not already in Section 2. PR #588 closes F-CR-01 and F-CR-06; the next
highest-value open item is F-CR-02, because Android must join `ContentTreeWire`
before the Compose registry can replace the old embed card.

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

#### F-CR-01 · Rust `EmbedKindProjection` + `EmbeddedEventEnvelope` [DONE · PR #588]

New module `crates/nmp-content/src/embed_projection/`. Creates the typed envelope
(`EmbeddedEventEnvelope`) and variant enum (`EmbedKindProjection`) that carry
per-kind projection data across the wire to all three platforms.

Variants: `ShortNote`, `Article`, `Highlight`, `Profile`, `Unknown`. The `Unknown`
variant carries `kind: u32`, raw `tags: Vec<Vec<String>>`, `content: String`,
`content_tree: ContentTreeWire`, and `alt_text` — enough for native kind handlers
to extract any custom field without a Rust-side change.

Also adds `RenderContextWire` (serialisable form of `nmp-content::RenderContext`)
and `resolve_embed_projection(event, ctx)` — the single `match event.kind` dispatch
point in the workspace. All fields follow ADR-0032 (raw protocol data, no formatted
strings).

Add golden fixture JSONs under `crates/nmp-content-fixtures/fixtures/embed/` for
each variant. Tests in `nmp-content` pin the serde round-trip.

**Dependencies:** none. **Scope:** medium. **Status:** implemented by PR #588.

#### F-CR-02 · Android gallery → `ContentTreeWire` migration [PREREQUISITE · Android]

Migrate `android/gallery/` off `ContentTreeDto` / `SegmentDto` / `MarkdownNodeDto`
onto `ContentTreeWire` / `WireNode` (already the iOS + TUI wire format).

- Rename `SegmentDtoView.kt` → `WireNodeView.kt`; rewrite against `WireNode` arena
  indexing.
- Delete `SegmentDto.kt`, `ContentTreeDto.kt`, `MarkdownNodeDto.kt`.
- Update `EmbedEntry.rendered` field type from `ContentTreeDto?` to `ContentTreeWire?`.
- `WireNode.EventRef` arm calls `EmbeddedEvent` composable (wired in F-CR-07).

Run `./gradlew :gallery:test` to verify no regressions.

**Dependencies:** F-CR-01 (envelope shape needed to scope the conversion).
**Scope:** medium.

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

**Dependencies:** F-CR-01. **Scope:** medium-large.

#### F-CR-06 · TUI `NostrKindRegistry` + `EmbeddedEvent` widget [DONE · PR #588]

New registry component at `crates/nmp-cli/registry/tui/content-kind-registry/`.

- `NostrKindRegistry` — `HashMap<u32, Arc<dyn KindRenderer>>` plus typed slots;
  `resolve(&projection)` returns the right renderer.
- `KindRenderer` trait — `render(…, area, buf)` + `preferred_height(…, width)`.
- `EmbeddedEvent` widget — `impl Widget`, wraps chosen renderer in
  `EmbedChromeContainer` (left-border + indent).
- Update `nostr_content_view.rs` `WireNode::EventRef` arm to call `EmbeddedEvent`.
- Add default short-note and unknown renderers bound in `make_default()`. The
  short-note renderer reuses the existing content-tree render path instead of
  carrying a second quote-card implementation.

**Dependencies:** F-CR-01. **Scope:** medium. **Status:** implemented by PR #588.

#### F-CR-07 · Android `NostrKindRegistry` + `EmbeddedEvent` composable [HIGH · Android]

New registry component at `crates/nmp-cli/registry/compose/content-kind-registry/`.

- `NostrKindRegistry` — `CompositionLocal`, holds typed `KindRenderer` slots plus
  `Map<Int, KindRenderer>` for open-ended dispatch.
- `KindRenderer` — `fun interface` with `@Composable fun Render(…)`.
- `EmbeddedEvent` — `@Composable`, receives `EmbeddedEventEnvelope?`, calls registry,
  wraps in `EmbedChromeContainer`.
- Delete `android/gallery/src/main/java/org/nmp/gallery/ui/EmbedCard.kt`.
- Wire `WireNode.EventRef` in `WireNodeView.kt` to `EmbeddedEvent`.

**Dependencies:** F-CR-01, F-CR-02. **Scope:** medium-large.

#### F-CR-09 · `content-kind-30023` — Long-form article handler [MEDIUM · all platforms]

Per-platform kind handler components that bind `EmbedKindProjection::Article` to a
proper article preview card (title, summary, hero image, author, read-time). Derived
from the existing `ArticlePreview` composable in Android's old `EmbedCard.kt`; new for
iOS and TUI. Independently installable: `nmp add component swiftui/content-kind-30023`.

**Dependencies:** F-CR-05, F-CR-06, F-CR-07. **Scope:** medium.

#### F-CR-10 · `content-kind-9802` — NIP-84 highlight handler [SMALL · all platforms]

Left-accent bar + italic highlighted text + source footer. New `crates/nmp-nip84/`
crate for the `HighlightProjection` decoder. Independently installable.

**Dependencies:** F-CR-05, F-CR-06, F-CR-07. **Scope:** small.

#### F-CR-11 · `content-kind-0` — Profile card handler [SMALL · all platforms]

Avatar + display name + npub chip + about preview. No new crate needed (profile data
already in kernel projections). Independently installable.

**Dependencies:** F-CR-05, F-CR-06, F-CR-07. **Scope:** small.

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
| `bootstrap_urls_for_role` test-only fallback | `FALLBACK_CONTENT_RELAY` / `FALLBACK_INDEXER_RELAY` are unconditional in production |
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
