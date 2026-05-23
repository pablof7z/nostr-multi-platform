# NMP Backlog

> Tracker for active violations, pending user decisions, and the ordered v1 feature backlog.
> Supersedes `docs/perf/pending-user-decisions.md` (append-only history log, kept for audit)
> and `docs/arch-review-queue.md`.
>
> Companion files:
> - [`WIP.md`](../WIP.md) — live tracker for work currently on a branch (in-flight)
> - [`docs/plan.md`](plan.md) — overarching plan (milestones, doctrine, where we are)
>
> Verified against HEAD **20a3794f** (2026-05-23). Update this file in every PR that touches
> an item listed here.

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

### V-01 · nmp-wasm stub — multi-platform claim is false [CRITICAL · staged fix in progress]

**Verified:** `crates/nmp-wasm/Cargo.toml` has zero `nmp-core` dependency (only
`nmp-chirp-config`, `serde`, `serde_json`, `wasm-bindgen`). `src/runtime.rs` is a ~295-line
stub with a hardcoded `"browser-local"` pubkey and synthesized snapshot JSON.

**Impact:** `apps/chirp-web` and `.github/workflows/chirp-web.yml` build against this stub.
Every "one source of truth, four delivery paths" claim is structurally false until this is
fixed. Each sprint that adds chirp-web features on top of the stub incurs divergence that
makes the eventual fix harder.

**Staged fix plan:**
- Phase 1a ✅ DONE: `native` Cargo feature added to `nmp-core` gating tungstenite, ureq,
  heed, nostr-database, mio, rustls, chrono/clock. `mod ffi` gated behind
  `#[cfg(feature = "native")]`. Default build unchanged. (commit `5e36e158`)
- Phase 1b ✅ DONE (PR #343): `nmp-core = { default-features = false }` dep wired in
  `nmp-wasm/Cargo.toml`. **Regression noted 2026-05-23:** between PR #343 and Stage 2,
  five `super::format_timestamp` / `super::now_hms` / `super::UNIX_EPOCH` imports in
  `kernel/{nostr,mod,update,status,publish_outbox}.rs` were left ungated even though the
  items they pulled from are `#[cfg(feature = "native")]`. `cargo check -p nmp-wasm`
  failed on HEAD until Stage 2 (PR pending) restored the gates.
- Phase 1c ✅ DONE (PR #341): `RelayFrame` enum introduced; `actor/` and `relay_worker/` gated
  behind `#[cfg(feature = "native")]`; `cargo check -p nmp-core --no-default-features` passes
  (after the Stage 2 import-gating fixup above).
- Stage 2 ✅ DONE (PR #372 — merged 2026-05-23): `WasmRuntime` rewritten on `nmp_core::KernelReducer` — the
  pure protocol kernel now drives `Start`/`Stop`/snapshot envelopes. `LocalNote` stub deleted.
  `cargo check --target wasm32-unknown-unknown -p nmp-wasm` passes; relay transport remains
  Stage 3 (app-level intents return `browser_actor_driver_missing` honestly).
- Stage 3 (read path) ✅ DONE (PR #375 — merged 2026-05-23): `BrowserRelayDriver` in `nmp-wasm` owns one
  `web_sys::WebSocket` per (URL, role) pair; inbound frames flow through
  `KernelReducer::handle_relay_frame` → kernel state; outbound fans back over the same sockets.
  Shared substrate primitives (backoff constants, jitter, HTTP-denial classifier) moved into
  always-compiled `nmp_core::relay_protocol`. `RelayFrame` / `OutboundMessage` / `RelayRole`
  promoted to `pub`. The native `relay_worker` thread is unchanged. Auto-reconnect uses the
  exact same exponential backoff + per-URL jitter constants the native worker does.
- Stage 3b (write path): app-level `AppAction` writes (PublishNote / React / Follow / Unfollow)
  still return `browser_actor_driver_missing` — signing requires the identity runtime + bunker
  hooks (`actor::commands::sign_in_*`) that live behind `feature = "native"`. Wire IndexedDB
  store + identity runtime; deliver async snapshot push to JS via `js_sys::Function` callback.
  An in-flight branch (`feat/wasm-stage3b-write-path`) prototypes the NIP-07 signer + snapshot
  push callback; not yet merged to master.

No chirp-web features may be added until Stage 3b lands.

### V-02 · nmp-marmot in crates/ — application subsystem misplaced [DONE]

**Verified:** `crates/nmp-marmot/` exists (~4,096 LOC). ADR-0025 explicitly states Marmot
opts out of the NMP substrate seam. It is an application feature, not a protocol primitive.
Protocol crates live in `crates/`; application feature bundles belong in `apps/`.

Moved to `apps/marmot/nmp-app-marmot/` — crate name unchanged (`nmp-marmot`). All dependent
path references updated (nmp-repl, nmp-app-chirp, nmp-testing). All `cargo check` passes.

### V-03 · ~~wallet_status app noun in nmp-core~~ CLOSED — see Appendix

### V-04 · Two subscription systems coexist — D4 single-writer violation [MEDIUM · staged fix in progress]

**Verified:** `crates/nmp-core/src/kernel/mod.rs:361` documents that the M1 hand-rolled
`req()` path is still authoritative. The `InterestRegistry`/`LogicalInterest` infrastructure
is live but parallel, not a replacement. Two systems maintaining separate state for the same
concern violates D4 (single-writer-per-fact).

**Correct fix:** designate `InterestRegistry` as canonical; migrate all M1 `req()` call sites
to it; delete the hand-rolled path.

**Staged fix plan (PD-033-C):**
- Stage 1 ✅ DONE (PR #368 — merged 2026-05-23): Deleted M1 dual-write (`self.req(...)`) from
  `kernel/discovery.rs` (`drain_unknown_oneshots`). Required three load-bearing pieces: the
  deletion itself, a `CompileTrigger::ViewOpened` enqueue (drain_tick short-circuits on empty
  inbox), and a planner sub_id bridge (`OneshotApi::request` → `(OneshotToken, InterestId)`,
  `register_planner_wire_frames` re-keys `oneshot_subs` from the planner-assigned `sub-<hash>`
  id). All 1040 nmp-core tests pass.
- Stage 2: Migrate remaining M1 `req()` call sites (bootstrap REQs, profile claims, etc.).
- Stage 3: Delete the M1 `req()` helper once all call sites are migrated.

### V-05 · D2 enforcement gap — coverage_hook never installed [DONE]

**Verified FIXED (PR #347 — merged 2026-05-23):** All three stages complete.
- Stage 1 ✅ (PR #346): `nmp-coverage-gate` crate with pure policy data.
- Stage 2 ✅ (PR #347): `NmpApp` grows `coverage_hook` slot; `run_actor_with_observers`
  threads it through; `ActorContext` carries it; `Reset` arm re-installs it. `nmp-app-chirp`
  installs `CoverageGate::default()` closure in `nmp_app_chirp_register`.
- Stage 3 ✅ (PR #347): `#[ignore]`d sentinel replaced with `d2_coverage_hook_slot_round_trips`
  real CI test. 5/5 coverage-hook tests pass.

The backstop closure enforces `max_relay_connections = 30` cap (redundant with
`apply_selection`'s built-in cap — proves seam wired). Full negentropy steering (the
original D2 intent) is a future follow-up once negentropy relay infra is available.

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

### V-07 · Zap relay selection in Swift — D0 policy leak [DONE]

**Verified FIXED:** PR #331 (`fix(zap): auto-select recipient relays from kind:10002 (V-07)`)
resolved this. `inject_recipient_relays` in `actor/commands/zap.rs` now looks up the
recipient's kind:10002 write relays from the kernel cache. Swift passes an empty `relays`
array; relay selection is fully Rust-owned.

**D0 violation:** "if you would write an `if` statement in Swift that decides
what the app should *do*, that logic belongs in Rust" (AGENTS.md §Architecture).

**Correct fix:**
- Make `ZapInput.relays` optional (`Option<Vec<String>>` or accept empty vec as
  "auto-select from recipient's kind:10002").
- In `handle_fetch_lnurl_invoice` (zap.rs), when relays is empty:
  1. Look up recipient's mailbox in kernel's `author_relay_lists`.
  2. Fall back to the actor's configured indexer/content relays.
  3. Fall back to two compile-time defaults if nothing is available.
- Remove relay-selection logic from `KernelModel.swift`; pass empty array.

**Deadline:** before v1. This makes zap receipt routing correct: the kind:9734
`relays` tag tells the LN provider where to broadcast the kind:9735 receipt — the
correct answer is the RECIPIENT's write/both relays from their kind:10002 (so the
receipt lands where the recipient listens). Using the sender's own relays is the
wrong set and produces an under-informed zap flow.

### V-09 · `nmp-app-chirp/src/ffi.rs` split — [DONE]

**Verified FIXED:** PR #332 split `ffi.rs` into `ffi/mod.rs`, `ffi/actions.rs`,
`ffi/handle.rs`, `ffi/helpers.rs`, `ffi/register.rs`, `ffi/snapshot.rs`, `ffi/tests.rs`.
All production sub-modules are within the 500-LOC ceiling.

**Follow-up (V-09b) ✅ DONE:** PR #339 split `ffi/tests.rs` (790 LOC) into
`tests/{mod,helpers,register,social,nip29,nip17,nip57}.rs`. Every sub-file is under
the 500-LOC ceiling. All 32 lib tests pass.

### V-12 · Five production files above 500-LOC ceiling [MEDIUM · staged]

**Verified (2026-05-23):**
- `crates/nmp-core/src/actor/mod.rs` — 1487 LOC
- `crates/nmp-core/src/actor/dispatch.rs` — 1477 LOC
- `crates/nmp-core/src/kernel/mod.rs` — 1373 LOC
- `crates/nmp-core/src/actor/commands/identity.rs` — ~1211 LOC production (1320 total incl. tests)
- `crates/nmp-core/src/actor/commands/wallet.rs` — 657 LOC production (wallet tests extracted in PR #376)

All five breach AGENTS.md §File-size rule ("hard 500-LOC ceiling; test sections extracted to `#[path]` subfiles").

**Staged fix plan (independent per file):**

**(a) identity.rs test extraction ✅ DONE (commit on master 2026-05-23):**
`mod nip46_onboarding_tests` (108 LOC) extracted to `identity/nip46_onboarding_tests.rs`
via `#[path = "identity/nip46_onboarding_tests.rs"]`. Production section is 1211 LOC and
needs its own follow-up production split.

**(b) identity.rs production split:**
A 3-way split was analyzed: BunkerHandshake dto/types + IdentityRuntime + free functions.
Requires reshuffling visibility of `BunkerHandshakeSlot` across modules — a real refactor,
not just a split. Defer until after v1.

**(c) actor/mod.rs, actor/dispatch.rs, kernel/mod.rs:**
Need command/concern grouping analysis (see Opus direction review #10: the `ActorCommand`
enum's closed-enum lock-in is the real issue, not just LOC). Do not split blindly — group by
cohesion. Post-v1.

**Deadline:** identity.rs test extraction done. Production splits of mod.rs, dispatch.rs,
kernel/mod.rs, and wallet.rs are post-v1.

---

## Section 2 — In Flight

Work currently on a branch lives in [`WIP.md`](../WIP.md). Agents must check that file
before picking up Section 4 work to avoid duplicating an in-progress task.

---

## Section 3 — Pending User Decisions

Items that cannot be resolved autonomously. An agent that encounters one of these must log
its finding in the decision thread below and move on to the next item, not block.

### PD-033-A · Framework thesis — second non-social app — CONFIRMED 2026-05-23

**Decision settled (PR #377 — merged 2026-05-23):** `apps/notes/` is a minimal NIP-01 note
client (read kind:1, publish kind:1, sign-in via nsec or NIP-46 bunker) built entirely on
substrate seams already exported by `nmp-core` + `nmp-signer-broker`. **Zero new C-ABI
protocol symbols** — the only `#[no_mangle]` introduced is `nmp_app_notes_init` (empty
app-registration marker; binary would still link without it). Swift surface is **299 LOC**
(under the ≤300 LOC budget). Rust surface is 25 LOC of code (plus docs + 2 tests).

The framework thesis — generic `dispatch_action` + kernel projections + signer-broker can
host a second non-social app without any new protocol crate — is now proven for both the
**read path** (verified earlier by `apps/longform/`) and the **stateful write path** (this
spike: publish kind:1 + NIP-46 bunker sign-in, both via existing seams).

**Original framing kept for history:** each protocol-bound app (e.g. Chirp social) requires
a protocol crate + projection crate + 4–6 C-ABI symbols + payload types + Swift decoder.
PD-033-A asked whether the substrate could host an app that needs none of those. Answer: yes,
when the app is built on generic seams (raw_event_observer + dispatch_action + signer-broker).

### PD-033-C · Two subscription systems (gates V-04 fix) — DECISION MADE

Decision (2026-05-23): migrate M1 `req()` call sites to InterestRegistry and delete the
hand-rolled path, staged. Stage 1 complete (PR #368). See V-04 staged fix plan above.

### PD-037 · Stale branch confirmation

Branch `codex/worker1-nip17-dm-inbox-relays` may be dead (work shipped via PR #237 + #300).
Confirm and delete, or identify what remains.

---

## Section 4 — V1 Feature Backlog

Ordered by blocking priority. Items earlier in the list unblock items below them. An
autonomous agent picks the topmost item not already in Section 2.

### F-01 · Fix V-01 Stage 3b — IndexedDB store + write path + async snapshot push [V1 BLOCKER]

Phase 1a/1b/1c (PRs #341/#343) + Stage 2 (PR #372) + Stage 3 read path (PR #375) all merged
to master. `WasmRuntime` now drives the pure `KernelReducer` AND owns a pool of
`BrowserRelayDriver`s (`web_sys::WebSocket`-backed, one per (URL, role) pair) with the same
exponential backoff / jitter / HTTP-401/403 classification the native worker uses.
Inbound frames route through `KernelReducer::handle_relay_frame`; outbound
fans back over the same sockets via the runtime's relay-pool sink. The read
path (relay → kernel → snapshot projection) is functional end-to-end.

**Stage 3b remaining scope (V1 BLOCKER for chirp-web write features):**
1. **IndexedDB store.** Port persistence to an IndexedDB-backed
   `nostr-database` impl (the native `nmp-nostr-lmdb` fork stays native-only).
   Right now the kernel runs entirely in memory and resets on page reload.
2. **Write path (signing).** `AppAction` writes (PublishNote / React / Follow /
   Unfollow) still return `browser_actor_driver_missing`. Wire identity
   runtime + a wasm-compatible signer (browser-keychain or NIP-46 bunker
   broker) so the kernel's publish engine can produce kind:1/6/7/3 events.
3. **Async snapshot push.** Relay-driven kernel mutations don't yet push
   a fresh snapshot to JS — the host pulls by dispatching a kernel action.
   Add a `js_sys::Function` callback channel through the wasm-bindgen wrapper
   so the relay-driver sink can emit a `WorkerEvent::Update` directly when
   the kernel changes.
4. **Multi-role bootstrap parsing.** Stage 3 spawns a single Content-lane
   driver per URL regardless of the bootstrap's declared role string
   (`"indexer"`, `"both,indexer"`, ...). Kernel routing is by URL (T105) so
   the wire path is correct, but `RelayHealth` diagnostics for pure-indexer
   URLs land on the wrong lane. Parse the role string in
   `nmp-wasm::relay_pool::spawn_drivers` and open one driver per declared
   role per URL (mirrors the native `spawn_missing_relays` behaviour).

secp256k1-sys wasm32 C build remains environmentally gated on
`CC_wasm32_unknown_unknown=clang` (CI sets this; local builds need
homebrew LLVM on macOS).

No `chirp-web` write features may be added until Stage 3b lands.

### F-02 · DM cold-start receive-side verification [V1 BLOCKER]

Gift-wrap **send** landed; kind:10050 relay-list publish is wired. The **receive** side on a
fresh install has not been verified end-to-end. A new user who signs in for the first time
must receive DMs before NIP-17 can be called done.

**Rust-layer pipeline verified (PR #344 — merged):** `nmp_app_inject_signed_event_json` injects
a real signed kind:1059 gift-wrap through `IngestPreVerifiedEvents` → `notify_raw_event_observers`
→ `DmInboxProjection`. `nmp_app_read_projection_json("nmp.nip17.dm_inbox")` confirms the message
appears in the snapshot. The `dm_inbox_full_round_trip_through_ffi` test passes (no longer ignored).
The test also gates that cold-start `nip17_local_keys` seed path works without calling `Start`.

**Remaining:** device-level acceptance test against live relays (product QA, not CI-gatable).

**Acceptance test:** fresh account → receive a gift-wrapped kind:1059 from a second account →
message appears in the `nmp.nip17.dm_inbox` snapshot projection.

### F-03a · NIP-65 kind:10002 publish coverage [VERIFIED ✅]

`maybe_publish_relay_list_after_edit` (`actor/dispatch.rs:117`) is called only from `AddRelay`
and `RemoveRelay` actor commands — never from sign-in or `ingest_relay_list`. A returning user
with an existing kind:10002 does NOT re-publish on sign-in (correct behavior: relays came from
the wire, not from user action). The relay-settings-change path is covered.

Verified 2026-05-23: `sign_in_nsec` and `sign_in_bunker` (`actor/commands/identity.rs`) have
no `maybe_publish_relay_list_after_edit` call. Explicit relay mutations (`AddRelay`/`RemoveRelay`)
do re-publish as intended.

### F-03b · First-launch defaults — empty timeline [DONE]

`create_account` (`actor/commands/identity.rs:778`) calls `prepopulate_seed_contacts` with
`DEFAULT_FOLLOWS` (fiatjaf + npub1l2vyh47…), then `publish_initial_follows` publishes kind:3
to cold-start relays. New accounts have an immediately-populated follow feed. Verified on
HEAD `3e370bb5`.

### F-04 · Zap E2E round-trip verification [V1 BLOCKER]

**Structural gaps fixed (B-7 — merged PR #342):**
1. `handle_event` `_` wildcard never called `notify_event_observers` → kind:9735 events never
   reached `ZapsAggregateProjection`. Fixed: kind:9735 arm added in `kernel/ingest/mod.rs`.
2. No kind:9735 subscription interest registered at bootstrap. Fixed: `#p <viewer>` REQ added in
   `active_account_bootstrap_requests` (5 tests covering F-02 + F-04 pass).

`ZapAction` is implemented and registered. `ZapsAggregateProjection` is registered. The full
round-trip — dispatch zap → `FetchLnurlInvoice` → bolt11 toast → `WalletPayInvoice` → NWC
`pay_invoice` → kind:9735 receipt → `ZapsAggregateProjection` update — has not been verified
against a live NWC wallet.

**Acceptance test:** connect real NWC wallet → tap zap → bolt11 invoice received via toast →
NWC `pay_invoice` fires → kind:9735 receipt ingested and reflected in `nmp.nip57.zaps`.

### F-05 · nmp-codegen Swift Decodables pilot [V1 QUALITY]

`crates/nmp-codegen` (1,212 LOC) has a working `generate_modules` CLI. `KernelBridge.swift`
is 1,988 LOC of handwritten counterpart types — a maintenance surface that diverges on every
snapshot field change.

**Pilot deliverable:** generate Swift `Codable` structs for `TimelineBlock` and `KernelUpdate`
from the `nmp.toml` manifest; delete the handwritten counterparts in `KernelBridge.swift`.
Proves the loop before extending to the full bridge.

### F-06 · ~~CI lint: freeze C-ABI surface~~ CLOSED — see Appendix

### F-07 · Fix V-02 — move nmp-marmot to apps/ [DONE]

Completed — see V-02. Moved to `apps/marmot/nmp-app-marmot/`.

---

## Section 5 — Post-V1

Deliberately deferred. Do not start until Section 4 is complete.

| Item | Why deferred |
|------|-------------|
| Blossom uploads/downloads (M10) | No `nmp-blossom` crate; no blocking user need |
| Web-of-Trust (M13) | No architecture decision; not user-blocking |
| UniFFI migration (M14) | Raw C-ABI works; multi-sprint, high churn |
| Cashu wallet (NIP-60) + nutzaps (NIP-61) | NWC + NIP-57 cover the v1 zap use case; nutzap UX layer requires Cashu wallet primitives first. `crates/nmp-nip60` / `crates/nmp-nip61` do not exist on master. |
| `nmp-codegen` full Swift bridge | Pilot (F-05) must land first to prove the pattern |
| Second non-social app (shipped product) | PD-033-A decision needed first; the v1 spike is a thesis test, not a shipped product |
| Android parity with iOS Chirp | Android Chirp shell exists but lacks feature parity with iOS; v1 ships iOS-first. Parity work blocked on UniFFI (M14) to avoid hand-maintaining two FFI surfaces. |
| Nostr-aware UI component registry | Curated reusable UI primitives — components, builders, complete blocks — distributed à la NDK's `svelte/registry` (`/Users/pablofernandez/Work/NDK-nhlteu/svelte/registry`). Blocked on (a) stable snapshot projection contracts so registry components have a versioned surface to bind against, and (b) target-platform decision (SwiftUI registry vs. multi-target via UniFFI views vs. web-only via `nmp-wasm`). Naming TBD; provisional `nmp-ui-registry`. |

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
