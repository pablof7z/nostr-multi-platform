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
> in every PR that touches an item listed here.

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

**Scope:** this is the canonical roll-up for the six-agent architecture audit run on
2026-05-26. PR #578 removes the duplicate planning/status authorities; the remaining
findings below are ordered by architectural risk. When a slice gets a dedicated V/PD entry
or a fixing PR, remove or strike that bullet here instead of creating a parallel plan.

**Priority order:**
1. **P1 — CLOSED: remove upward dependencies and app policy from reusable crates.**
   Closed by PR #580, which removed the `nmp-signer-broker` dependency on `nmp-core`/`nmp-ffi`;
   PR #583 moved the Gallery app crate from `crates/` to `apps/`; PR #586 moved
   Chirp identity/keyring policy out of `nmp-marmot`; and PR #587 moved router
   registration onto `nmp_core::substrate::{ActionRegistrar, AppHost}` so
   `nmp-router` and `nmp-app-template` no longer need `nmp-ffi` for reusable Rust
   registration. No P1 slices remain; the next V-57 item is P2 below.
2. **P2 — move protocol policy back out of `nmp-core`.**
   `crates/nmp-core/src/actor/commands/relays.rs:29-35` duplicates NIP-65 kind `10002`
   publishing policy; `crates/nmp-core/src/actor/commands/publish.rs:16-21`, `:297-305`,
   and `:377-389` hardcode kind `1059` gift-wrap routing policy. **Next step:** protocol
   crates should declare publish/routing constraints as typed policy data; core should enforce
   the abstract constraint without naming NIP kinds.
3. **P3 — move Chirp shell business logic behind Rust-owned actions/projections.**
   `apps/chirp/chirp-tui/src/commands.rs:169-234` resolves lightning addresses and builds
   zap input in the TUI; the host-side zap auto-pay loop was removed so Rust owns the
   LNURL → `nmp.wallet.pay_invoice` chain; `apps/chirp/chirp-tui/src/runtime_commands.rs:249-269`
   still bypasses the canonical action door for Marmot; and
   `ios/Chirp/Chirp/Features/RelaySettingsView.swift:159-177` dispatches two protocol
   publishes while tracking only one correlation id. **Next step:** expose composite Rust
   actions/action-stage projections for zap, Marmot rich results, and relay-settings publish.
4. **P4 — make wasm use the same snapshot and error contract as native.**
   `crates/nmp-wasm/src/snapshot.rs:80-84` ignores the `KernelReducer` and hand-builds a
   status-shaped snapshot; `crates/nmp-wasm/src/lib.rs:70-78` exposes public errors as
   `Result<JsValue, JsValue>`; `crates/nmp-wasm/src/lib.rs:165-170` rejects a promise for
   invalid JSON. **Next step:** route wasm snapshots through the canonical kernel update path
   and return caller-visible failures as data envelopes.
5. **P5 — close native update-loop and envelope discipline gaps.**
   `apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/bridge/GalleryModel.kt:70-75`
   polls for updates; `apps/nmp-gallery/nmp-app-gallery/src/android.rs:221-228` returns `null` on
   `recv_timeout`; and `ios/Chirp/Chirp/Bridge/KernelBridge.swift:603-618` identifies panic
   frames by substring before decoding the envelope. **Next step:** use blocking/pushed update
   delivery with lifecycle cancellation and decode one typed `UpdateEnvelope` first.
6. **P6 — strengthen enforcement so these regressions trip earlier.**
   V-12 already tracks oversized boundary files; the new gap is doctrine-lint coverage for
   dependency direction and app-noun leakage. **Next step:** add a dependency-graph/layer
   lint covering upward edges such as `nmp-router -> nmp-ffi` and `nmp-signer-broker -> nmp-core`,
   plus explicit allowlists for sanctioned adapter crates.

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
- Stage 3b ✅ DONE (PR #378 — merged 2026-05-23): NIP-07 signer + async snapshot push.
  `Nip07Signer::sign()` on wasm32 bridges `window.nostr.signEvent(...)` via
  `wasm-bindgen-futures::spawn_local` with cached-pubkey cross-check. Two-state honest error:
  `signer_not_installed` vs. `publish_path_not_wired`. `NmpWasmRuntime::set_snapshot_callback`
  pushes a JSON frame to JS after every inbound relay frame. New files: `dispatch_routing.rs`,
  `signer_slot.rs`, `snapshot.rs` (all under 500-LOC ceiling).
- Stage 3c ✅ DONE (PR #385 — merged 2026-05-24): `KernelReducer::publish_signed_event` with
  correlation_id threading; `nmp_signers::sign_event_via_extension` (async, wasm32+wasm-feature);
  `publish_path.rs` (268 LOC); `NmpWasmRuntime::dispatch_app_action_async` Promise wrapper;
  extracted `nip07/wasm.rs` via `#[path]`. chirp-web now supports NIP-07 PublishNote end-to-end.
  Multi-role bootstrap parsing done (roles_for_entry + spawn_drivers). See F-01 for IndexedDB.

No chirp-web persistence features may be added until F-01 IndexedDB lands.

### V-02 · nmp-marmot in crates/ — application subsystem misplaced [DONE]

**Verified:** `crates/nmp-marmot/` exists (~4,096 LOC). ADR-0025 explicitly states Marmot
opts out of the NMP substrate seam. It is an application feature, not a protocol primitive.
Protocol crates live in `crates/`; application feature bundles belong in `apps/`.

Moved to `apps/marmot/nmp-app-marmot/` — crate name unchanged (`nmp-marmot`). All dependent
path references updated (nmp-repl, nmp-app-chirp, nmp-testing). All `cargo check` passes.

### V-03 · ~~wallet_status app noun in nmp-core~~ CLOSED — see Appendix

### V-04 · Two subscription systems coexist — D4 single-writer violation [DONE — PR #430]

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
- Stage 2 precursor — planner Case C bootstrap-content inbox extension
  (PR pending — `worktree-agent-adff1381808c9be39`): adds a gated fallback in
  `planner::compiler::partition::case_c_p_tags::route_bootstrap_content_inbox`
  for `Tailing + Global + #p (Nip65ReadRelays)` interests whose tagged pubkeys all
  lack a cached NIP-65 inbox. Mirrors M1's `req(RelayRole::Content, …)` cold-start
  emission for the self zap-receipts subscription (`kind:9735 #p=[self_pk]`,
  `kernel/requests/startup.rs`). Without this gate, Stage 2 deletion of the M1
  helper would silently lose every #p Tailing REQ until kind:10002 arrives — the
  F-04 zap-receipts contract would break on every cold-start sign-in. NIP-17 DM
  routing is intentionally EXCLUDED (gift-wraps must stay fail-closed). All 1065
  nmp-core tests pass.
- Stage 2 ✅ DONE (PR #422 — merged 2026-05-24): Migrated the 4 remaining `self.req(...)` call
  sites in `kernel/requests/startup.rs::active_account_bootstrap_requests` (self
  kind:0/3/10002/10050 via Indexer) onto `InterestRegistry::ensure_sub` +
  `CompileTrigger::ViewOpened`. Added `Kernel::drain_lifecycle_outbound()` in
  `kernel/outbox.rs` so the wasm `KernelReducer` drains inline (no actor idle loop).
  `KernelReducer::handle_relay_connected` now calls `drain_lifecycle_outbound` after startup.
  1067 nmp-core tests pass. `Kernel::req` now has zero in-tree callers (kept under
  `#[allow(dead_code)]` — PD-033-C will retire it in Stage 4).
- Stage 3 ✅ SUBSUMED by Stage 2 (PR #422 — merged 2026-05-24): the audit before
  Stage 4 confirmed `profile.rs` / `thread.rs` carry no `self.req(...)` callers;
  the production helper migration is complete after Stage 2.
- Stage 4 ✅ DONE: Deleted the M1 `req()` helper from `kernel/requests/mod.rs` and the
  `ONESHOT_SUB_PREFIX` retirement-gate constant from `kernel/discovery.rs`. The lone
  remaining test caller (`auth_tests.rs::nip42_kernel_auth_required_for_read`)
  was migrated to `req_for_relay`; the discovery retirement-gate test inlines the
  `"oneshot-disc-"` literal instead of referencing the deleted constant.

### V-05 · D2 enforcement gap — coverage_hook never installed [DONE]

**Verified FIXED (PR #347 — merged 2026-05-23):** All three stages complete.
- Stage 1 ✅ (PR #346): `nmp-coverage-gate` crate with pure policy data.
- Stage 2 ✅ (PR #347): `NmpApp` grows `coverage_hook` slot; `run_actor_with_observers`
  threads it through; `ActorContext` carries it; `Reset` arm re-installs it. `nmp-app-chirp`
  installs `CoverageGate::default()` closure in `nmp_app_chirp_register`.
- Stage 3 ✅ (PR #347): `#[ignore]`d sentinel replaced with `d2_coverage_hook_slot_round_trips`
  real CI test. 5/5 coverage-hook tests pass.

The backstop closure enforces `max_relay_connections = 30` cap (redundant with
`apply_selection`'s built-in cap — proves seam wired). Client-side NIP-77 steering
now handles eligible large one-shot author/kind REQs through `nmp-nip77`; non-exact
filters, tailing subscriptions, and relays that reject negentropy fall back to raw
REQs.
The reusable gate now counts author × kind fanout per filter (`2 kinds × 25 authors`
and `3 kinds × 20 authors` both qualify at the default threshold of 50) and still
requires known relay support before choosing reconciliation.

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

### V-12 · Production files above 500-LOC ceiling [MEDIUM · ongoing test extraction]

*Test-extractable — first batch (all merged 2026-05-24):*
- ~~`crates/nmp-core/src/actor/relay_mgmt.rs`~~ — 806 → 374 LOC (PR #394)
- ~~`crates/nmp-core/src/actor/commands/raw_event_observer.rs`~~ — 833 → 479 LOC (PR #398)
- ~~`crates/nmp-core/src/actor/commands/dm.rs`~~ — 680 → 457 LOC (PR #395)
- ~~`crates/nmp-core/src/actor/commands/zap.rs`~~ — 682 → 429 LOC (PR #401)
- ~~`crates/nmp-core/src/kernel/outbox.rs`~~ — 713 → 387 LOC (PR #399)
- ~~`crates/nmp-core/src/publish/state.rs`~~ — 516 → 348 LOC (PR #396)
- ~~`crates/nmp-core/src/relay.rs`~~ — 516 → 341 LOC (PR #397)
- ~~`crates/nmp-nip65/src/lib.rs`~~ — 569 → 265 LOC (PR #388)
- ~~`crates/nmp-nostr-lmdb/src/lib.rs`~~ — 1144 → 269 LOC (PR #400)

*Test-extractable — second batch (most merged 2026-05-24; remaining PRs pending CI):*
- ~~`crates/nmp-codegen/src/swift.rs`~~ — 918 → 593 LOC (PR #402)
- ~~`crates/nmp-core/src/kernel/ingest/mod.rs`~~ — 706 → 549 LOC (PR #403)
- ~~`crates/nmp-core/src/planner/compiler/partition/case_a_authors.rs`~~ — 712 → 253 LOC (PR #404)
- ~~`crates/nmp-core/src/actor/commands/event_observer.rs`~~ — 543 → 340 LOC (PR #405)
- ~~`crates/nmp-core/src/planner/compiler/mod.rs`~~ — 864 → 372 LOC (PR #406)
- ~~`crates/nmp-core/src/kernel/relay_diagnostics.rs`~~ — 611 → 542 LOC (PR #407; production alone 539 LOC — post-v1 split needed)
- ~~`crates/nmp-content/src/markdown.rs`~~ — 580 → 424 LOC (PR #408)
- ~~`crates/nmp-core/src/nip19.rs`~~ — 568 → 476 LOC (PR #409)
- ~~`crates/nmp-core/src/planner/compiler/partition/case_c_p_tags.rs`~~ — 604 → 163 LOC (PR #410)
- ~~`crates/nmp-core/src/kernel/action_registry.rs`~~ — 937 → 353 LOC (PR #411)
- ~~`crates/nmp-testing/bin/doctrine-lint/rules/d10.rs`~~ — 725 → 336 LOC (PR #412)
- ~~`crates/nmp-testing/bin/doctrine-lint/rules/d11.rs`~~ — 618 → 351 LOC (PR #415)
- ~~`crates/nmp-testing/bin/doctrine-lint/rules/d12.rs`~~ — 569 → 337 LOC (PR #414)
- ~~`crates/nmp-testing/bin/doctrine-lint/rules/d15.rs`~~ — 672 → 474 LOC (PR #413)

*Production splits needed (no test section to extract; post-v1):*
- `crates/nmp-core/src/ffi/mod.rs` — 1559 LOC
- `crates/nmp-nostr-lmdb/src/store/lmdb/mod.rs` — 1495 LOC
- `crates/nmp-core/src/actor/mod.rs` — 1488 LOC
- `crates/nmp-core/src/actor/dispatch.rs` — 1477 LOC
- `crates/nmp-core/src/kernel/mod.rs` — 1386 LOC
- `crates/nmp-core/src/actor/commands/identity.rs` — ~1211 LOC production
- `crates/nmp-core/src/kernel/update.rs` — 983 LOC
- `crates/nmp-core/src/publish/engine.rs` — 827 LOC (already has extracted tests.rs)
- `crates/nmp-core/src/actor/commands/publish.rs` — 803 LOC (no test section)
- `crates/nmp-core/src/kernel/relay_diagnostics.rs` — 539 LOC production (tests extracted PR #407)

**Completed test extractions:**
- handle.rs, signer_seal.rs, view.rs (commit 34fc71a1 — 2026-05-23)
- action_stages.rs, planner/selection.rs, substrate/bounded.rs, nmp-nip65/src/lib.rs, publish/action.rs (PR #388 — 2026-05-24)
- identity.rs (commit e79f7a90); wallet.rs (PR #376)
- relay_mgmt.rs, raw_event_observer.rs, dm.rs, zap.rs, outbox.rs, publish/state.rs, nmp-nostr-lmdb/lib.rs, relay.rs (PRs #394-#401 — 2026-05-24)
- swift.rs, ingest/mod.rs, case_a_authors.rs, event_observer.rs, compiler/mod.rs, nip19.rs (PRs #402-#406, #409 — 2026-05-24)
- relay_diagnostics.rs, markdown.rs, case_c_p_tags.rs, action_registry.rs, doctrine-lint d10 (PRs #407-#412 — 2026-05-24)
- doctrine-lint d11/d12/d15 (PRs #413-#415 — 2026-05-24)
- nmp-nip29/projection/group_chat.rs (813 → 358 LOC), nmp-nip01/timeline_projection.rs (590 → 275 LOC), nmp-core/kernel/identity_state.rs (569 → 471 LOC) — third batch — 2026-05-24
- nmp-router/src/router.rs (703 → 242 LOC), nmp-core/src/substrate/routing.rs (531 → 346 LOC), nmp-core/src/substrate/protocol.rs (745 → 519 LOC; still 19 LOC over — production split deferred) — fourth batch (PR #480) — 2026-05-25

**Staged fix plan:**
Production splits of actor/mod.rs, dispatch.rs, kernel/mod.rs, ffi/mod.rs are post-v1
(ActorCommand closed enum analysis required — Opus review #10).

### V-13 · Broker relay client uses polling — violates D8 / no-polling doctrine [MEDIUM] — **DONE** (PR #431 — Stage 1; step 8 phase D — Stage 2 dedupe)

**Verified:** `crates/nmp-signer-broker/src/relay_client.rs:103` calls
`set_read_timeout(&mut socket, Duration::from_millis(100))`. The worker loop at
`:154–217` interleaves `cmd_rx.try_recv()` with a short-timeout blocking read.
This is exactly the pattern banned by `crates/nmp-core/src/relay_worker/no_polling_tests.rs:1–35`,
which asserts that `set_read_timeout`, `Duration::from_millis(50)`, and `.try_recv()`
are absent from `relay_worker/{mod,io_ready,socket_io}.rs`. The banned-token test
does not cover the broker because it is a different crate, but the doctrine
(`feedback_no_polling`, AGENTS.md §No polling — ever) is project-wide.

**Impact:** 100 ms polling at 4 Hz snapshot cadence means the broker thread burns
CPU on every tick whether or not the bunker relay has sent anything. On mobile,
this contributes to battery drain on any session with a remote signer.

**Correct fix:** extract a generic readiness-driven `RelayConnection` type (the
primitives are already partially factored in `relay_protocol.rs` by PR #375) and
replace `TungsteniteRelayClient::run_worker` with it. Both the native relay worker
and the broker then depend on the same shared primitive.

**Staged fix plan:**
- Stage 1: Extract `nmp-relay-conn` crate (or `relay_protocol` extension) with a
  readiness-driven tungstenite socket loop — no polling, no `set_read_timeout`.
  Stage 1 MUST also bound the connect handshake (`tungstenite::connect` is
  blocking with the OS-level TCP timeout, ~60 s; this leaks into both
  `nmp-core::relay_worker` and `nmp-signer-broker::relay_client` as a
  cancel-during-connect stall). Pattern: resolve URI host/port, call
  `TcpStream::connect_timeout`, install non-blocking + readiness-driven TLS
  handshake on the resulting socket.
- Stage 2: Rewrite `TungsteniteRelayClient::run_worker` (PR #431 — DONE for
  the readiness loop; still inlines its own boilerplate) to depend on the
  shared crate; delete the duplicated mio/readiness code in `relay_client.rs`.
  PR #431 already drains Shutdown between connect attempts as a partial
  mitigation for the residual stall.
- **Status (2026-05-25):** Stage 2 dedupe shipped under step 8 phase D
  (`docs/architecture/crate-boundaries.md` §5 step 8). `relay_client.rs` is
  now a thin `nmp_network::Pool` adapter (`PoolRelayClient`); the duplicate
  ~700-line mio/tungstenite readiness loop is gone, the broker's Cargo.toml
  no longer names `tungstenite` / `mio` / `rustls` directly, and one
  readiness-driven WebSocket implementation lives in the workspace
  (`nmp_network::relay_worker`). The connect-timeout (Stage 1 residual) is
  partially mitigated by `PoolRelayClient::connect`'s 10 s
  CONNECT_BUDGET, which bounds the broker's per-URL try window so the
  multi-relay cycle pivots within bounded time. The deeper fix (bound the
  TCP/TLS handshake inside `nmp_network::relay_worker::open_relay_socket`)
  still applies workspace-wide and remains open.
- **Deadline:** before v1-A (any user sign-in via bunker hits this path).

### V-14 · Bunker has no reconnect — relay flap silently bricks the session [MEDIUM] — **DONE** (PR #431)

**Verified:** `crates/nmp-signer-broker/src/relay_client.rs` exposes only `send`
and `shutdown`. `broker.rs:114` exposes only `cancel`. Neither file has a
reconnect path. `run_worker` returns on any read or write failure
(`relay_client.rs:159, 194, 213`). When that thread dies every subsequent
`signer.sign()` call times out after `REMOTE_SIGN_TIMEOUT` (5s) with a generic
backend error. V-06 and V-08 post-v1 items cover NIP-42 / DM decryption — they
do not cover basic transport resilience. **This gap is unticketed.**

**Impact:** NIP-46 is listed as a v1 sign-in method in `aim.md` §4.6. Any user
who signs in via bunker and experiences an intermittent relay drop ends up in a
state where every publish attempt silently fails until they re-sign-in. No UI
surface for "bunker connection lost" exists because the broker has no state for it.

**Correct fix:**
- (a) Reconnect loop in `TungsteniteRelayClient` with the same exponential
  backoff + per-URL jitter constants from `relay_protocol.rs`.
- (b) A `BunkerConnectionState::TransportLost { reconnect_in_ms }` variant (or
  equivalent) on the broker's status projection so the host shell can surface a
  non-silent indicator.

**Staged fix plan:**
- Stage 1: Add `BunkerConnectionState` enum (Connected / Connecting /
  TransportLost) to broker; expose it via the broker's status callback.
- Stage 2: Implement the reconnect loop (can share V-13 Stage 1 primitive once
  that lands). **Status (2026-05-24):** PR #431 implements an autonomous
  reconnect loop with jittered exponential backoff inside
  `TungsteniteRelayClient` (V-14 step a) and adds subscription replay so the
  REQ frame survives a flap. The UI-visible `BunkerConnectionState` projection
  (step b) is NOT yet wired — the host shell still gets only `"ready"` /
  `"failed"` from the handshake stage.
- **Status (2026-05-25):** Step 8 phase D migrated the reconnect machinery
  onto the shared `nmp_network::Pool` primitive (V-13 Stage 2 dedupe).
  Mid-session reconnect with jittered exponential backoff now lives in
  `nmp_network::relay_worker`; the broker's dispatcher replays installed
  subscriptions on every fresh `PoolEvent::Opened` so REQ-survives-flap
  is preserved end-to-end. V-14 step b (host-visible
  `BunkerConnectionState`) is still pending and not blocked by this dedupe.
- **Deadline:** before v1-A. Either this is fixed or `aim.md` and v1 copy drop
  NIP-46 as a v1 sign-in method.

### V-15 · Real-relay test suite never runs in CI — v1 live-relay claims are unfalsifiable [HIGH] — **DONE** (commit 41feec14)

**Verified:** `.github/workflows/test.yml:41` runs `cargo test --workspace --exclude ...` without
`--ignored`. Every integration test in `crates/nmp-testing/tests/` (`real_relay_smoke.rs`,
`real_relay_connect.rs`, `real_relay_nip42.rs`, `real_relay_outbox.rs`, `real_relay_replan.rs`,
`real_relay_soak.rs`) is `#[ignore = "real-relay smoke (run with --ignored)"]`. CI has never
opened a socket to a real relay. The "Damus round-trip kind:1" test at `real_relay_smoke.rs:99`
is the most basic possible end-to-end proof and has not run on a CI box since it was written.

**Impact:** F-02 ("DM cold-start receive-side verification") and F-04 ("Zap E2E round-trip") are
listed as v1 blockers but their acceptance criteria live only in developer heads. There is no
place a passing or failing result is recorded automatically. The publish path, AUTH path, outbox
routing, NIP-77 negentropy sync, and gift-wrap inbox could all be broken on HEAD right now with
no CI signal.

**Correct fix:** add `.github/workflows/real-relay-nightly.yml` that runs the `real_relay_*`
suite on a cron (e.g. nightly), posts results, and gates v1 on the suite being green. Add a
"skip if relay unreachable" guard so the nightly is not flaky on relay downtime. The infrastructure
already exists — this is one new workflow file.

**Deadline:** before declaring F-02 or F-04 closed. Until this workflow exists, the v1 exit
criterion for F-02/F-04 is literally unevaluable.

### V-16 · `SearchView.swift` is dead code that ships in the Chirp binary [MEDIUM] — **DONE** (PR #427 merged)

**Verified:** `ios/Chirp/Chirp/Features/SearchView.swift:3` defines `struct SearchView` and is
compiled into `Chirp.app` (`project.pbxproj:468`). Zero `SearchView()` call sites exist in the
iOS target. `RootShell.swift:7` documents "search tab removed (Search deferred to toolbar button
on HomeFeed)" but no HomeFeed toolbar button to `SearchView` exists. The view is also mis-named:
it is an "open hex pubkey or event id" form (lines 31–47), not a search feature.

**Correct fix:** delete `SearchView.swift` and remove it from `project.pbxproj`, or wire it
back into HomeFeed's toolbar and rename to `OpenByIdView`.

### V-17 · `MarmotMemberList::snapshot` returns `Vec::new()` — no group-member visibility [HIGH] — **DONE** (PR #429 merged)

**Verified:** `apps/marmot/nmp-app-marmot/src/view/views.rs:270` — `MemberListView::snapshot`
returns `MemberListPayload { members: Vec::new() }` with a comment "Authoritative member set is
MDK-side; the service/actor layer fills…". `MarmotGroupChatView.swift` has no UI to view group
members. The Invite sheet (`MarmotInviteSheet.swift`) accepts npubs but the user cannot see who
is already in the group.

**Impact:** For an encrypted-group product, "who can read my messages" is the most
safety-critical question. The answer is currently "the user cannot see." Marmot/MLS is listed as
"what works on master" in `plan.md` §TL;DR — that is true at the kernel layer, false at the UX
layer.

**Correct fix:** wire `MDK::get_members()` into `MemberListView::snapshot`; add a members sheet
to `MarmotGroupChatView`'s toolbar. Gate v1-A Marmot on this.

**Deadline:** before v1-A if Marmot ships. Otherwise move Marmot to a "Labs" tab with a
disclaimer and remove it from the v1-A feature set.

### V-18 · `PublishOutcome::FailedAfterRetries` has no `set_last_error_toast` — silent failure [DONE — PR #426]

**Verified:** `crates/nmp-core/src/publish/state.rs:172` defines the terminal failure variant.
The only `set_last_error_toast` call paths from the publish/sign chain were sign-step failures,
broker timeouts, and relay-management ops — no settle-time toast for a post that failed on every
relay. **Fixed:** PR #426 adds the toast in `apply_engine_completions` when `status == "failed"`.

### V-19 · `DiagnosticsView` (474 LOC) ships to all users via Settings [DONE — PR #425]

**Verified:** `ios/Chirp/Chirp/Features/SettingsHubView.swift:43` exposed Diagnostics under a
"Developer" section accessible to every user — no debug build flag. **Fixed:** PR #425 wraps the
Developer section in `#if DEBUG` and deletes the stale Roadmap DisclosureGroup.

### V-20 · `dmRelativeTime` in Swift — thin-shell doctrine violation [DONE — PR #428]

**Verified:** `ios/Chirp/Chirp/Features/DmListView.swift:284` defined
`func dmRelativeTime(_ unixSecs: UInt64) -> String`. `DmConversationView.swift:161` and
`DmListView.swift:127` both called it. The thin-shell rule (aim.md §2) says relative-time
formatting is Rust-owned; `DiagnosticsView.swift` itself cites this rule. DM messages bypassed
it. **Fixed:** PR #428 added `created_at_display: String` to `DmMessage` in
`crates/nmp-nip17/src/inbox.rs` (computed at every snapshot tick via `display::format_ago_secs`)
and deleted `dmRelativeTime` from Swift.

### V-22 · `GroupChatView.relativeTime` in Swift — thin-shell doctrine violation [DONE]

**Verified:** `ios/Chirp/Chirp/Features/GroupChatView.swift:257` defined
`func relativeTime(_ unixSecs: UInt64) -> String` using `RelativeDateTimeFormatter`. The kind:9
NIP-29 group-chat row at line 213 called it for every message timestamp — the same thin-shell
violation V-20 fixed for DMs. **Fixed:** added `created_at_display: String` to
`GroupChatMessage` in `crates/nmp-nip29/src/projection/group_chat.rs`, computed at every
snapshot tick via a `format_ago_secs` helper that mirrors `nmp_nip17::display::format_ago_secs`
byte-for-byte (deliberate micro-duplication — a NIP crate should not depend on another NIP
crate just for a trivial bucketed-time formatter). Swift view binds the field directly and the
`relativeTime` Swift helper is deleted.

### V-23 · `WalletView` thin-shell doctrine violations — **DONE** (PR #434)

`WalletView.swift` computed `balanceSats` (msats÷1000), formatted it with
`.formatted()`, and abbreviated `walletNpub` using a private `shortNpub()` function.
All three moved to Rust: `WalletStatus` now carries `balance_sats`, `balance_sats_display`,
`wallet_npub_short`, `is_ready`, and `is_connected`.

### V-24 · `AccountsView` + `JoinGroupView` thin-shell doctrine violations — **DONE** (PR #435)

**Verified:** `ios/Chirp/Chirp/Features/AccountsView.swift:68,90-93` abbreviated npubs in Swift
via a private `shortNpub(_:)` helper (`<first10>…<last6>`). `ios/Chirp/Chirp/Features/JoinGroupView.swift:156-178`
computed `initials`, `displayName`, and `subtitle` from `DiscoveredGroup` projection data inside
the SwiftUI row view — first-two-char uppercase, name/groupId fallback, and visibility-glyph +
pluralized member-count assembly all lived in Swift. aim.md §2 thin-shell rule places all
display formatting in Rust.

**Fixed:** `AccountSummary` (`crates/nmp-core/src/kernel/identity_state.rs`) gains a
`npub_short: String` field computed by a new `account_npub_short(&str)` helper. The algorithm
mirrors `profile_npub_short` in `kernel/update.rs` byte-for-byte (deliberate micro-duplication —
`identity_state.rs` does not reach into `update.rs`'s private helpers; V-22 precedent).
`DiscoveredGroup` (`crates/nmp-nip29/src/projection/discovered.rs`) gains `initials`,
`display_name`, and `subtitle` fields populated by a new `finalize_display_fields` pass in
`DiscoveredGroupsProjection::snapshot`. Visibility glyphs (`#` / `🔒`) and pluralization
(`"1 member"` / `"N members"`) live in Rust. iOS views bind the new fields verbatim;
`shortNpub` and the three `JoinGroupView` computed properties are deleted. Swift codegen
(`KernelTypes.generated.swift`) regenerated to surface `npubShort`.

### V-25 · `GroupChatView` pubkey-derived display strings in Swift — **DONE** (PR #436)

**Verified:** `ios/Chirp/Chirp/Features/GroupChatView.swift` carried three pubkey-derived display
computations the host had no business doing: `shortPubkey(_:)` at line 183 (`"\(hex.prefix(8))…\(hex.suffix(8))"`,
called from the chat-row header at line 209 and the reply banner at line 102), `var initials`
at line 253 (`String(message.pubkey.prefix(2)).uppercased()`), and the avatar colour slice
`String(message.pubkey.prefix(6))` at line 203. The first two are the same class of
abbreviated-identity formatting V-22 moved to Rust for timestamps; the third was worse — a
different algorithm from `nmp_nip17::display::avatar_color_hex` / `nmp_marmot::projection::display::avatar_color_hex`,
so the **same author** rendered with a **different avatar tint** in DMs vs. NIP-29 group chat.

**Fixed:** added three fields to `GroupChatMessage` in `crates/nmp-nip29/src/projection/group_chat.rs`
— `author_display`, `author_initials`, `author_color_hex` — populated at ingest from
`KernelEvent::author` via three new helpers (`pubkey_display`, `pubkey_initials`, `avatar_color_hex`).
The colour helper is **byte-identical** to `nmp_nip17::display::avatar_color_hex` (djb2 over the
last 6 bytes), deliberate micro-duplication for the same reason `format_ago_secs` is duplicated
(a NIP crate must not depend on another NIP crate just to share a trivial helper). Swift
`GroupChatMessage` mirror in `KernelBridge.swift` gains the three matching camelCase properties;
the view binds them directly; the three Swift display helpers are deleted. A pinned-vector test
locks the djb2 output so an algorithm drift cannot silently change every group-chat avatar.

**Behaviour change called out:** the avatar tint for every existing group-chat row will shift
once on first run — that's the consistency fix, not a regression.

### V-27 · `ModularBlockView` pubkey-derived display strings in Swift — **DONE**

**Verified:** `ios/Chirp/Chirp/Components/ModularBlockView.swift` carried four pubkey/timestamp-derived
display computations the host had no business doing: `defaultInitials(pubkey:)` at line 319
(`String(pubkey.prefix(2))` — first-two-chars initials), `defaultColor(pubkey:)` at line 323
(`"#" + String(pubkey.prefix(6))` — a **different** algorithm from `nmp_nip17::display::avatar_color_hex`
and the V-25 nmp-nip29 helper, so the same author rendered with a different tint in the modular
timeline vs. DMs vs. NIP-29 group chat), `displayPubkey(item:card:)` at line 250 (`"\(hex.prefix(6))…\(hex.suffix(4))"`
— abbreviated hex for the Twitter-style secondary-identifier slot), and `relativeTime(card:)` at line 264
(`"\(Int(delta))s/m/h/d"` from `card.createdAt` — relative time in Swift, with a different dialect
from `nmp_nip17::display::format_ago_secs` and the V-22/V-25 helpers).

**Fixed:** added five fields to `TimelineEventCard` in `crates/nmp-nip01/src/timeline_projection.rs`
— `created_at_display`, `author_avatar_initials`, `author_avatar_color`, `author_pubkey_short`,
`author_display_name` — populated at `from_event` via four file-local helpers
(`format_ago_secs`, `pubkey_initials`, `avatar_color_hex`, `pubkey_display`). The colour helper is
**byte-identical** to `nmp_nip17::display::avatar_color_hex` (djb2 over the last 6 bytes), and the
short-pubkey helper uses the V-25 `8…8` algorithm so the same author renders with the same tint and
the same abbreviated handle in every surface. Deliberate micro-duplication for the same reason
`format_ago_secs` is duplicated (a NIP crate must not depend on another NIP crate just to share a
trivial helper). `Inner::refresh_author_cards` was extended to keep the flat `author_display_name`
mirror in sync when a kind:0 arrives after a note is ingested. Swift `ChirpEventCard` in
`TimelineBlock.swift` gains the five matching camelCase properties; `ModularBlockView` binds them
directly; the four Swift display helpers are deleted; the test-only `ChirpEventCard(...)`
constructor in `NoteContentRenderingTests.swift` supplies fixture values for the new fields.

**Behaviour change called out:** every modular-timeline row's avatar tint, avatar initials,
secondary-identifier caption, and relative-time string will shift once on first run — the tint
becomes consistent with DMs/NIP-29/Marmot (V-25 fix), initials change from raw-hex-prefix to
uppercase (matching every other surface), the handle abbreviation widens from `6…4` to `8…8`
(matching V-25), and the timestamp dialect changes from `"5s"` to `"5s ago"` (matching V-20/V-22).
None are regressions; all are the consistency fix.

### V-28 · `shortPubkey` / `shortID` / `relativeTime` Swift helpers — thin-shell doctrine violation — **DONE**

**Verified:** three remaining display-string helpers survived the V-22–V-27 sweep in the
same family of thin-shell violations.

- `ios/Chirp/Chirp/Components/NoteEntityViews.swift:124` called `relativeTime(createdAt: card.createdAt)`
  inside `embeddedCard` even though V-27 had already added `createdAtDisplay: String` to
  `ChirpEventCard`. The Swift helper at lines 241-247 carried its own `"Xs/m/h/d"` dialect
  distinct from the kernel's `"X ago"` dialect.
- `ios/Chirp/Chirp/Components/NoteRowView.swift:112` called a private `shortPubkey(_ hex:)`
  returning `"\(hex.prefix(6))…\(hex.suffix(4))"` — the same Twitter-style secondary identifier
  V-27 had already solved on `ChirpEventCard.author_pubkey_short` (and V-25 on
  `nmp-nip29`), but `TimelineItem` itself had no equivalent field.
- `ios/Chirp/Chirp/Features/ComposeView.swift:151` defined `shortID(_:)` and called it from
  the reply banner (`replyBanner(for: replyToID)` line 133) on a raw 64-char event id, with
  the same `prefix(6)…suffix(4)` algorithm.

**Fixed:** added two fields to `TimelineItem` in `crates/nmp-core/src/kernel/types.rs` —
`author_pubkey_short` and `short_id` — populated at `Kernel::timeline_item` construction in
`crates/nmp-core/src/kernel/update.rs` via a new `kernel::nostr::short_hex_display(value)`
helper that produces the cross-surface `<first 8>…<last 8>` abbreviation. The new helper is
distinct from `short_pubkey_display` above (which carries the `npub ` prefix and `..`
separator used by the kernel's own author display fallback) and matches the algorithm
already in `nmp_nip01::timeline_projection::pubkey_display`,
`nmp_nip17::display::pubkey_short`, and `nmp_nip29::projection::group_chat::pubkey_display`
so the same author / event id renders the same abbreviation across every surface.

Also extended `TimelineEventCard` in `crates/nmp-nip01/src/timeline_projection.rs` with a
new `short_id: String` field populated via the existing `pubkey_display` helper (works on any
hex string) so the synthetic `TimelineItem` builder in
`ios/Chirp/Chirp/Components/ModularBlockView.swift:288` could bind the abbreviation through
without slicing the raw id — same precedent as V-27's `author_pubkey_short` addition.

Swift codegen (`KernelTypes.generated.swift`) regenerated to surface `authorPubkeyShort` and
`shortId` on `TimelineItem`. `ChirpEventCard` in `TimelineBlock.swift` gains `shortId` +
matching `CodingKey`. `ComposeView` gains a parallel `replyToShortID: String? = nil`
parameter the reply banner binds verbatim — the publish path still receives the raw
`replyToID` (the Rust kernel needs the full 64-char id to publish the NIP-10 `e` tag).
`NoteRowView.swift:70` and `ThreadScreen.swift:57/108/200` pass `item.shortId` through
(via `ReplyTarget.shortID`). All three Swift helpers (`relativeTime`, `shortPubkey`,
`shortID`) are deleted.

**Behaviour change called out:** the secondary-identifier abbreviation in the home feed
row and the reply-banner caption widen from `<first 6>…<last 4>` to `<first 8>…<last 8>` —
deliberate consistency fix, same disclosure pattern V-27 / V-25 already used. The embedded
event card's timestamp dialect changes from `"5s"` to `"5s ago"` (matches V-20/V-22/V-27).

**Out of scope (V-29 follow-up):** `NoteEntityViews.swift:88-93` `authorProfile(for:)`
fallback (`initials: String(pubkey.prefix(2))`, `colorHex: "#" + String(pubkey.prefix(6))`)
— requires Rust to emit fallback `MentionProfile` entries for all referenced pubkeys, not
just known authors. `NoteEntityViews.swift:263-266` `shortEntity` — used by both the
fallback initials path and the mention label path; deletable once V-29 lands.
`MarmotGroupsView.swift:159` `PublicGroupRow.initials` — needs `GroupChatSnapshot`
group-level display fields (V-30). `DiagnosticsView.swift:440` `shortID` — diagnostics is
already `#if DEBUG` gated by V-19; cleanup deferred.

### V-26 · `AccountAvatar` extension display logic in Swift — thin-shell doctrine violation — **DONE** (PR #438)

**Verified:** `ios/Chirp/Chirp/Components/AccountAvatar.swift` defined
`extension AccountSummary { var avatarInitials: String; var avatarColorHex: String }`,
computing both display strings in-view. `ComposeView.swift:76-77`, `HomeFeedView.swift:125-126`,
and `AccountsView.swift:60-61` all bound the extension properties. `avatarInitials` did
first-char-of-each-word + bech32-body fallback; `avatarColorHex` used a hard-coded six-colour
palette indexed by a unicode-scalar `&* 31 &+ value` hash. The colour case was the same class
of violation V-25 fixed for `GroupChatMessage` — a different algorithm from
`nmp_nip17::display::avatar_color_hex` / `nmp_marmot::projection::display::avatar_color_hex`
/ `nmp_nip29::projection::group_chat::avatar_color_hex`, so the **same author** rendered with
a **different avatar tint** in the Accounts toolbar / compose row / row avatars vs. DMs vs.
NIP-29 group chat.

**Fixed:** added two fields to `AccountSummary` in `crates/nmp-core/src/kernel/identity_state.rs`
— `avatar_initials` and `avatar_color_hex` — populated at construction in
`actor::commands::identity::sync_accounts_from_identity` via two new helpers
(`account_avatar_initials`, `account_avatar_color_hex`). The colour helper is **byte-identical**
to `nmp_nip17::display::avatar_color_hex` (djb2 over the last 6 bytes of the hex pubkey,
`{:06X}` mask), deliberate micro-duplication for the same reason `account_npub_short` is
duplicated in this module (V-22 / V-24 / V-25 precedent — `identity_state.rs` must not gain a
cross-crate dependency on a NIP crate for a trivial helper). The `Kernel::accounts_enriched`
path also re-runs `account_avatar_initials` whenever a kind:0 `display_name` lands so the
placeholder initials don't stay stuck on the npub-body fallback after enrichment. Swift codegen
(`KernelTypes.generated.swift`) regenerated to surface `avatarInitials` + `avatarColorHex` as
`let` fields on `AccountSummary`; iOS views bind them verbatim; the
`Components/AccountAvatar.swift` file (which only contained the extension) is deleted along
with its four `project.pbxproj` entries (V-16 precedent). A pinned-vector test
(`account_avatar_color_hex_matches_pinned_djb2_vector`, same `"08E60C"` output as the V-25 nip29
vector) locks the djb2 output so an algorithm drift cannot silently change every account
avatar tint.

**Behaviour change called out:** the avatar tint for every existing account row will shift
once on first run — that's the consistency fix (Accounts toolbar avatar now matches the same
author's DM and group-chat tint), not a regression. Same disclosure pattern as V-25.

---

### V-35 · `dispatch_action` namespace catalog missing — framework is undiscoverable — **DONE** (docs/dispatch-actions.md)

**Verified (Opus direction review #16 — 2026-05-24):** The C-ABI surface (48 symbols in
`crates/nmp-core/src/ffi/`) is wire transport, not the developer-facing API. The real API is
the `dispatch_action` namespace catalog. Known namespaces are scattered across action-module
files: `nmp.publish` (`nmp-nip01/src/action.rs`), `nmp.nip17.*` (`nmp-nip17/src/action.rs`),
`nmp.nip57.*` (`nmp-nip57/src/action.rs`), `nmp.nip65.*` (`nmp-nip65/src/action.rs`),
`nmp.follow` / `nmp.unfollow` / `nmp.nip25.react` (`nmp-nip02/src/action.rs`),
`nmp.wallet.pay_invoice` (`nmp-nip57` wallet module). No catalog file exists.

**Impact:** A third developer cannot find what to call, what JSON shape each namespace
expects, or which projections to subscribe to. PD-039 inventories the C-ABI *symbols*; nothing
inventories the *contracts*. The Notes spike demonstrates the failure mode: `NotesBridge.swift`
bypassed `dispatch_action` and wrote raw event handling in Swift because the correct entry point
was undiscoverable.

**Fix:** Create `docs/dispatch-actions.md` — a single catalog of every registered namespace,
its JSON request shape, and the projection event it drives. Should be auto-verifiable: a CI
script greps the action-module files and asserts the catalog is not stale. Companion to F-05
codegen (typed Swift dispatch API is the end state; the markdown catalog is the immediate
legibility fix).

**Staleness risk (open):** Every new `ActionModule` registration can silently make
`docs/dispatch-actions.md` stale within 2 PRs. No CI drift gate exists today.
The real fix is F-05 codegen sweep — until that lands, every PR adding an `ActionModule`
must manually update the catalog. Tracked under F-05.

---

### V-36 · `nmp-signer-broker` reimplements NIP-46 without an ADR [MEDIUM] — **DONE** (ADR-0031)

**Verified (Opus direction review #16 — 2026-05-24):** `aim.md §3` names `nostr-connect`
(the rust-nostr NIP-46 crate) as the dependency. NMP shipped `nmp-signer-broker` instead —
a hand-rolled NIP-46 relay transport (`crates/nmp-signer-broker/`). No ADR was written to
justify this divergence from the canonical dependency. Post-hoc fixes V-06 (NIP-42
incompatibility), V-13 (polling relay client), V-14 (no reconnect), V-08 (DM gift-wrap) are
all *fix* tickets on a *should-this-exist* question.

**Impact:** The framework's own corollary "Use rust-nostr, not scratch crypto" was violated
without writing down why. Every future NIP-46 bug is evaluated against a codebase whose
existence is an undocumented divergence from doctrine.

**Required:** Write `docs/adr/ADR-NNNN-signer-broker.md` documenting (a) why `nostr-connect`
was insufficient for the bunker relay-multiplexing model, (b) what NIP-46 features
`nmp-signer-broker` owns that `nostr-connect` does not (multi-relay broadcast, `mio`
event-loop integration), and (c) the long-term exit: either upstream the missing features to
`nostr-connect` and delete the crate, or declare it canonical and track it as maintained
infrastructure.

---

### V-37 · Snapshot output seam doesn't support non-Chirp apps reading kernel state [HIGH]

**Verified (2026-05-24 — Notes rewrite investigation):** PD-033-A requires Notes to be
rewritten against "real framework seams (LogicalInterest, kernel-owned timeline projection,
handshake gate)." Code-grounded inspection found the current framework does not expose those
seams generically:

1. **`NmpSnapshotProjector` is zero-arg** (`crates/nmp-core/src/ffi/snapshot.rs:39`):
   ```rust
   pub type NmpSnapshotProjector = unsafe extern "C" fn() -> *const c_char;
   ```
   The callback receives no kernel-state argument and no context pointer. A registered
   projector must obtain state through side-channels (raw event observers, separate globals).
   There is no mechanism for the kernel to *push* a typed view to a non-Chirp app.

2. **No generic `nmp_app_snapshot`** — only `nmp_app_chirp_snapshot` exists
   (`apps/chirp/nmp-app-chirp/src/ffi/snapshot.rs:14`), typed to `*mut ChirpHandle`.
   A non-Chirp app has no pull path either.

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

### V-38 · NIP-47 NWC wallet stack wrongly in `nmp-core` [HIGH · post-v1 · staged fix required]

**Verified:** the entire NIP-47 Nostr Wallet Connect runtime lives inside
`nmp-core`, with an inverted dependency direction that no other NIP-crate in
the workspace exhibits:

- `crates/nmp-core/Cargo.toml:90` — `nmp-nwc = { path = "../nmp-nwc", optional = true }`.
  Every other NIP crate (`nmp-nip02`, `nmp-nip17`, `nmp-nip57`, `nmp-nip65`) goes
  `nip-crate → nmp-core`; only NWC inverts this so `nmp-core → nmp-nwc`. The
  module docstring at `actor/commands/wallet.rs:6` says the quiet part out
  loud: *"D0: nmp-core may depend on nmp-nwc (the protocol crate). The
  inverse is not true."* That is exactly the inversion the substrate doctrine
  forbids — the kernel must be the substrate every protocol crate adapts to,
  never a consumer of protocol semantics.
- `crates/nmp-core/src/actor/commands/wallet.rs` (716 LOC) — `WalletRuntime`,
  `WalletConnection`, `WalletStatus`, `WalletStatusSlot`, the kind:23194
  builder, the kind:23195 response handler, NWC URI parse, NIP-04 encrypt
  bridge.
- `crates/nmp-core/src/wallet/mod.rs` + `wallet/action.rs` — `WalletAction`
  enum + `WalletPayInvoiceModule` `ActionModule` impl, registered in
  `kernel/action_registry.rs:347`. Mounted at `lib.rs:45` as a top-level
  `pub mod wallet` of `nmp-core`.
- `crates/nmp-core/src/actor/mod.rs:540,546,569` — three protocol-noun
  variants on the closed `ActorCommand` enum (`WalletConnect`,
  `WalletDisconnect`, `WalletPayInvoice`) gated on `feature = "wallet"`.
- `crates/nmp-core/src/actor/dispatch.rs:737,749` — dispatch arms.
- `crates/nmp-core/src/ffi/wallet.rs` — three bespoke C-ABI symbols
  (`nmp_app_wallet_connect`, `nmp_app_wallet_disconnect`,
  `nmp_app_wallet_pay_invoice`).

**Correct destination:** a new `crates/nmp-nip47/` that depends on both
`nmp-core` and `nmp-nwc`. Direction flips from `nmp-core → nmp-nwc` (today) to
`nmp-nip47 → nmp-core` and `nmp-nip47 → nmp-nwc` (post-fix). The Theme A
discriminator in PD-039 classifies wallet connection lifecycle as permanent
bespoke FFI — those C symbols stay byte-stable, only their bodies become thin
shims.

**Migration difficulty: HARD.** Three substrate seams must land first:
1. Open `ActorCommand` for protocol crates (Opus direction review #10 — prerequisite
   shared by V-39, V-40, V-41).
2. Relay-text handler plug-in seam for the NWC relay role.
3. Wallet-status `Arc<Mutex<_>>` slot wiring via `NmpApp` extension points.

**Staged fix plan:** Stage 1 (open-ActorCommand seam, shared with V-39/V-40/V-41) →
Stage 2 (create `nmp-nip47`, move all wallet code) → Stage 3 (thin-shim FFI bodies) →
Stage 4 (delete `feature = "wallet"` from `nmp-core/Cargo.toml`).

**Deadline:** post-v1.

---

### V-39 · NIP-17 DM send handler + `SendGiftWrappedDm` `ActorCommand` variant in `nmp-core` [HIGH] — **DONE** (PR #458 merged 2026-05-24, commit 852750b2)

**Closed by PR #458** (combined with V-40): `SendGiftWrappedDmCommand` moved
into `crates/nmp-nip17/src/dm_send.rs` and dispatches through the
`ProtocolCommand` substrate seam (PR #448). `actor/commands/dm.rs` and the
`ActorCommand::SendGiftWrappedDm` enum variant are deleted. Bunker NIP-46
signing regressed and is tracked under V-08 (Phase 2 follow-up: add
`SignerForSealCapability` to `ProtocolCommandContext`).

#### Original violation (kept for archaeology)

**Verified:** the NIP-17 gift-wrap send orchestration lives entirely in
`nmp-core`, even though a dedicated `nmp-nip17` crate exists and already
depends on `nmp-core` (`crates/nmp-nip17/Cargo.toml:15`):

- `crates/nmp-core/src/actor/commands/dm.rs` (457 LOC) — `send_gift_wrapped_dm`
  resolves the active `SignerForSeal`, calls `nmp_nip59::gift_wrap_with_signer`
  twice (recipient + self-copy), and dispatches each kind:1059 envelope
  through `publish_signed_event`. The handler's entire purpose — gating
  kind:1059 publish on the receivers' kind:10050 DM-inbox relays
  (`required_dm_relays` → `DmRelayNotReady`) — is a literal NIP-17 §2 wire
  rule, not a substrate concern.
- `crates/nmp-core/src/actor/mod.rs:460` — `ActorCommand::SendGiftWrappedDm`
  variant carries `recipient_pubkey: String` and an `UnsignedEvent` rumor.
- `crates/nmp-core/src/actor/dispatch.rs:568` — dispatch arm.

**Correct destination:** `crates/nmp-nip17/`. Move `send_gift_wrapped_dm` to
`nmp-nip17/src/dm_send.rs` as a `DmSendModule: ActionModule`. The
`ActorCommand::SendGiftWrappedDm` variant deletes. FFI surface unchanged —
DM send already routes through `nmp_app_dispatch_action` under `nmp.nip17.send`.

**Migration difficulty: MEDIUM-HARD.** Needs the open-ActorCommand seam (V-38
Stage 1) + a `SignerForSealCapability` trait on the actor context.

**Staged fix plan:** Stage 1 (ride V-38 Stage 1 + add `SignerForSealCapability`)
→ Stage 2 (move `dm.rs` to `nmp-nip17`, delete `ActorCommand::SendGiftWrappedDm`).

**Deadline:** post-v1. F-02 ships on the current layout.

---

### V-40 · NIP-17 kind:10050 ingest + `dm_relay_lists` cache wrongly in kernel [MEDIUM] — **DONE** (PR #458 merged 2026-05-24, commit 852750b2)

**Closed by PR #458** (combined with V-39): kind:10050 parser and DM-inbox
relay cache moved into `crates/nmp-nip17/`: `Kind10050Parser` implements
the substrate `IngestParser` trait (PR #447), `DmRelayCache` is owned by
`nmp-nip17` and exposed back to the kernel via the substrate
`DmInboxRelayLookup` trait so the kernel never names NIP-17 nouns. The
old `Kernel::dm_relay_lists` field and `kernel/ingest/dm_relay_list.rs`
are deleted.

#### Original violation (kept for archaeology)

**Verified:** kernel state and ingest logic for NIP-17's DM-inbox relay
mechanism live in `nmp-core`:

- `crates/nmp-core/src/kernel/ingest/dm_relay_list.rs` (107 LOC) — parses
  kind:10050 `["relay", <wss-url>]` tags into `dm_relay_lists`. Module docstring
  at line 5 names NIP-17 §2 by spec section — pure protocol semantics.
- `crates/nmp-core/src/kernel/mod.rs:386` — `Kernel` struct carries
  `dm_relay_lists: HashMap<String, Vec<String>>`. The comment at `:382` cites
  "NIP-17 gift-wrap envelopes."
- `crates/nmp-core/src/kernel/outbox.rs:169` — `Kernel::recipient_dm_relays`
  reader, called by V-39's `send_gift_wrapped_dm`.
- `crates/nmp-core/src/kernel/ingest/mod.rs:397` — kind:10050 match arm in
  the kernel's kind-dispatch table alongside routing kinds (0/3/10002).
- `crates/nmp-core/src/subs/CompileTrigger::DmRelayListChanged` — kernel
  recompile trigger named after a NIP-17 noun.

The contrast: kinds 0/3/10002 drive the outbox router — a substrate primitive
every Nostr app uses. kind:10050 drives NIP-17-specific routing. The kernel
is not entitled to know it.

**Correct destination:** `crates/nmp-nip17/`. kind:10050 parsing moves to
`nmp-nip17/src/dm_relay_list_ingest.rs`; `dm_relay_lists` cache becomes a
NIP-17-owned projection; the outbox router consults it through a generic
projection-lookup hop.

**Migration difficulty: MEDIUM.** Needs an "input-side projection" seam — a
NIP crate registers `(kind, parser_fn)` with the kernel ingest dispatcher.
This is the input-side counterpart to the existing snapshot-projection output seam.

**Staged fix plan:** Stage 1 (input-side projection seam) → Stage 2 (move
`dm_relay_list.rs` to `nmp-nip17`, delete `Kernel::dm_relay_lists` and
kind:10050 match arm) → Stage 3 (generalise or remove
`CompileTrigger::DmRelayListChanged`).

**Deadline:** post-v1. F-02 ships with kind:10050 still in the kernel.

---

### V-41 · NIP-57 zap LNURL handler + `FetchLnurlInvoice` `ActorCommand` variant in `nmp-core` [HIGH] — **DONE** (PR #456 merged 2026-05-24, commit c9fc728f)

**Closed by PR #456**: `FetchLnurlInvoiceCommand` moved into
`crates/nmp-nip57/src/lnurl/` and dispatches through the `ProtocolCommand`
substrate seam (PR #448). `actor/commands/zap.rs` (429 LOC) +
`zap_lnurl.rs` (252 LOC) deleted; `ActorCommand::FetchLnurlInvoice` enum
variant deleted; `ureq` dropped from `nmp-core`'s deps. The
`ZapAction` action module in `nmp-nip57` now dispatches
`ActorCommand::Protocol(Box::new(FetchLnurlInvoiceCommand{...}))`.

**Caveat (carried into "make substrate honest" follow-up):** this PR
widened `ProtocolCommandContext` from 1 to 7 positional closure args
(`now_secs`, `author_write_relays`, `bootstrap_discovery_relays`,
`active_local_keys`, `record_action_stage_requested`,
`command_sender_clone`). Direct cache accessors on the context
(`author_write_relays`, `bootstrap_discovery_relays`) are arguably
routing concerns that should go through the router. Scheduled to be
revisited and bundled into capability traits.

#### Original violation (kept for archaeology)

**Verified:** the NIP-57 LNURL-pay round-trip orchestration lives in
`nmp-core`, even though `crates/nmp-nip57/` exists and already owns the
zap-request builder, the kind:9735 receipt decoder, the aggregate
projection, and the `ZapAction` `ActionModule`:

- `crates/nmp-core/src/actor/commands/zap.rs` (429 LOC) — `handle_fetch_lnurl_invoice`
  resolves the active `Keys`, signs the kind:9734 zap request, spawns an HTTP
  worker, and runs the two-leg LNURL-pay round-trip. Every one of these is a
  NIP-57 concern.
- `crates/nmp-core/src/actor/commands/zap_lnurl.rs` (252 LOC) — pure
  LUD-01/LUD-06/LUD-16/bolt11 helpers the kernel has zero need to host.
- `crates/nmp-core/src/actor/mod.rs:610` — `ActorCommand::FetchLnurlInvoice`
  variant. NIP-57 protocol noun on the kernel's command enum.
- `crates/nmp-core/src/actor/dispatch.rs:773` — dispatch arm.
- `nmp_nip57::ZapAction::execute` (`crates/nmp-nip57/src/action.rs:176`) already
  enqueues `ActorCommand::FetchLnurlInvoice` — the action side is already in the
  right crate; only the handler side leaked into `nmp-core`.

**Correct destination:** `crates/nmp-nip57/`. Move `zap.rs` + `zap_lnurl.rs` to
`nmp-nip57/src/lnurl/`. Delete `ActorCommand::FetchLnurlInvoice` and the dispatch
arm. FFI surface unchanged — zap already routes through `nmp_app_dispatch_action`
under `nmp.nip57.zap`.

**Migration difficulty: MEDIUM-HARD.** Same two seams as V-39: the open-ActorCommand
seam (V-38 Stage 1) + local-signer access via substrate trait.

**Staged fix plan:** Stage 1 (ride V-38 Stage 1 + V-39 Stage 1) → Stage 2 (move
`zap.rs` + `zap_lnurl.rs` to `nmp-nip57/src/lnurl/`, delete the ActorCommand
variant) → Stage 3 (confirm wallet auto-pay chain still works via
`nmp.wallet.pay_invoice` dispatch).

**Deadline:** post-v1. F-04 (zap E2E) ships on the current layout.

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

### V-43 · Zap `dispatch_action` multi-step chain has no contract — `correlation_id: None` at wallet dispatch [MEDIUM · post-v1]

**Evidence:** `crates/nmp-core/src/actor/commands/zap.rs:202`:

```rust
let _ = command_tx.send(ActorCommand::WalletPayInvoice {
    bolt11: bolt11.clone(),
    amount_msats: Some(amount_msats),
    correlation_id: None,     // ← chain breaks here
});
// ...
if let Some(cid) = correlation_id {
    let _ = command_tx.send(ActorCommand::RecordActionSuccess { correlation_id: cid });
}
```

The original zap correlation closes `RecordActionSuccess` the moment the LNURL provider
returns a valid bolt11 — before the wallet pays or the kind:9735 receipt arrives. The
wallet pay runs under a separate, anonymous correlation_id. A host that dispatches
`nmp.nip57.zap` sees `Success` ~200 ms after LNURL responds, regardless of whether
payment happens.

`crates/nmp-core/src/kernel/publish_cmd.rs:233-236` — `action_lifecycle_projection`
does not collapse two correlation_ids into one chain, so there is no framework-level
way to observe the full zap outcome.

**Recommended action:** document `nmp.nip57.zap` as a multi-step chain contract in
`docs/dispatch-actions.md`; either (a) keep the original `correlation_id` open until
kind:9735 receipt arrives, or (b) introduce `Stage::Bolt11Received` /
`Stage::WalletPaid` / `Stage::ReceiptObserved` on the `action_stages` substrate.
Option (b) generalises to every future multi-step dispatch. Prerequisite: V-41 Stage 1.

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

### V-45 · No `LogicalInterest::SocialTimeline` substrate seam [MEDIUM · v1-B framework readiness]

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

### V-46 · Snapshot built-in projection cluster is unbounded — D5 silently violated [HIGH · pre-v1 doctrine fix]

**Evidence:** `crates/nmp-core/src/kernel/update.rs:267-440` —
`snapshot_projections_with_publish_cluster` unconditionally inserts on every tick:
`publish_queue`, `publish_outbox`, `outbox_summary`, `relay_edit_rows`,
`relay_role_options`, `settings_hub`, `accounts`, `active_account`, `profile`,
`timeline`, `author_view`, `thread_view`, `inserted`, `updated`, `removed`,
`relay_diagnostics`, `mention_profiles` — plus all host-registered projections.

D5 (`plan.md:43`) reads "snapshots bounded by open views." The built-in cluster is
not bounded. Even with zero open views, the cluster carries 17+ keys including
`relay_diagnostics` (rolls every relay + every wire sub) and `mention_profiles`
(walks every visible item).

The perf gate (`perf_tests.rs:128`) runs against `Kernel::new()` with zero registered
host projections — it does not exercise the full cluster.

**Recommended action:** either (a) rewrite D5 to "bounded by a static cluster gated by
`snapshot_perf_firehose_gate` + open-view-dependent payloads", or (b) move genuinely
view-dependent keys (`author_view`, `thread_view`, `timeline`, `inserted`, `updated`,
`removed`) into a "only-if-view-subscribed" branch. Option (b) is doctrine-honest.

---

### V-47 · `register_raw_event_observer` gives FFI callers a lane that defeats all D1/D3/D5/D8 guarantees [MEDIUM · pre-v1 doc fix] — **DONE**

**Evidence:** `crates/nmp-core/src/ffi/raw_event_tap.rs` — `nmp_app_register_raw_event_observer`
with no doc warning. `apps/notes/ios/Notes/Bridge/NotesBridge.swift:73-76` registers it
without ceremony. The Notes spike proved 96 LOC Swift defeats D3 outbox routing,
kernel-owned formatting, lifecycle gating, and codegen contracts without leaving the
public ABI.

Three other escape hatches exist: `inject_pre_verified_events`, `inject_signed_event_json`,
and the host-supplied `NmpSnapshotProjector` callback.

**Resolution:** `aim.md` §1 updated with escape-hatch caveat; `docs/escape-hatches.md`
created cataloguing all four escape hatches (raw event tap, snapshot projector, action
module seam, test-only injectors) with decision tree. `raw_event_tap.rs` module doc
updated with `## Escape-hatch caveat` section listing all bypassed doctrines.

---

### V-48 · No `nmp-app-template` crate — second-app developer must read 403 LOC of Chirp to understand registration [HIGH · v1 DX] — **DONE** (PR #467 merged 2026-05-24, commit a079d95f)

**Closed by PR #467**: `crates/nmp-app-template/` ships `register_defaults(&mut NmpApp)` plus a `runtimes` module (DM-inbox + zap-receipts reconcilers). Chirp's `nmp-app-chirp/src/` shrank from 1003 → 456 LOC (−547, −55%) by deleting `dm_runtime.rs` and `zap_receipts_runtime.rs` and routing through the template. The integration test in `crates/nmp-app-template/tests/register_defaults.rs` constructs a real `NmpApp` and proves every canonical action namespace (`nmp.follow`, `nmp.unfollow`, `nmp.nip25.react`, `nmp.nip17.send`, `nmp.nip17.publish_relay_list`, `nmp.nip57.zap`, `nmp.nip65.publish_relay_list`) resolves through `dispatch_action`.

#### Original requirement (kept for archaeology)



**Evidence:** `apps/chirp/nmp-app-chirp/src/ffi/register.rs` — 403 LOC.
`docs/dispatch-actions.md` documents *what to call* but not *what to register first*.
The ordering matters (action registration before `&NmpApp` borrow; observer
registration before `nmp_app_start`); ordering violations fail silently. The smallest
existing app (`apps/notes/`) opts out of the framework's seams — so the smallest
example is also the wrong example.

`aim.md` §4.14 names `nmp init`; `crates/nmp-cli` exists but starter recipes are absent.

**Recommended action:** (1) `nmp-app-template` crate with canonical wiring (action
registry, default projections for kind:1 + profiles, coverage hook); (2) wire
`nmp init <appname>` in `nmp-cli` to scaffold the template + minimal iOS shell.
This is the highest-leverage DX investment before shipping v1 if the framework's
§1 claim ("one-shot a working Nostr application") is to hold.

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
2. **`MailboxCache` implementation** (currently `InMemoryMailboxCache` in nmp-core, marked
   "Phase 2: replace with nmp-nip65 implementation" — that future destination is here).
3. **The NIP-65 `publish_relay_list` ActionModule** from `crates/nmp-nip65/` (that crate is
   too thin to stand alone; absorb it here).
4. **Relay pool lifecycle** — connect/disconnect/reconnect, not just routing math.

`nmp-core` substrate defines `trait OutboxRouter` + `trait MailboxCache`; the kernel holds
injected `Arc<dyn>` of each. `nmp-relay-pool` provides the concrete implementations.

`crates/nmp-nip65/` is deleted after the ActionModule migrates.

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
toolbar toggle on the wasm host).

#### Original requirement (kept for archaeology)



**Evidence (architecture-grounded):** the substrate primitive already
exists. `nmp_core::substrate::RoutedRelaySet` (PR #449) attributes every
URL to one or more `RoutingSource` lanes (`Nip65{direction}`, `Hint`,
`Provenance`, `UserConfigured`, `ClassRouted`, `Indexer`,
`AppRelay{mode}`) precisely so a later "why-this-relay" answer is
reconstructable. But there is no observation seam that surfaces those
attributions outside the router call site, no projection that aggregates
them into a snapshot field, and no FFI surface that exposes them. Today
an app can see *what* frames went out (via the `RawEventObserver` lane
— see V-47 — or wire-frame tracing) but not *why* those relay choices
were made.

**What v1 needs:**

- **Substrate seam.** A `RoutingTraceObserver` (or similar) on
  `nmp-router` that fires per `route_publish` / `route_subscription`
  call carrying `(EventOrInterest, RoutedRelaySet, RoutingContext-summary)`.
  The kernel installs it once at composition time. Per-call cost must be
  opt-in (debug builds + explicitly enabled in release) — D8 (no
  per-event alloc) applies; the observer trait takes references the
  router already has.
- **Projection.** A bounded ring buffer projection in `nmp-core` (size
  capped per D5) that records the last N routing decisions per
  `(event_id | interest_id)` keyed by their correlation. Each entry: the
  inputs the router saw (kind, pubkey, tag highlights,
  `explicit_targets` was-set), the output set per URL with its
  `RoutingSource` lane(s), and a timestamp. Snapshot-bound so the FFI /
  wasm surface gets it for free.
- **App-side API.** A `recent_routing_decisions(filter)` snapshot field
  with stable shape. Optional `route_explain(event_or_interest)` action
  for ad-hoc replay against current state.
- **Chirp peek-under-hood UI.** A `RoutingInspectorView` (debug toolbar
  + long-press on any timeline item / publish-status row) that
  displays: the input (kind, author, tags), the output set (one row per
  resolved URL), the lane attribution per URL (e.g., "Nip65/Write +
  AppRelay/Fallback" with a tooltip explaining each lane), and the
  blocked-relay drops. Same shell as `DiagnosticsView` (gated by
  `#if DEBUG` + V-19 pattern) but the projection itself ships in release
  so a user can opt into diagnostics without a debug build.
- **Validation tie-in.** This is the substrate the validation programs
  (route-trace subcommand, chirp-repl smokes, NIP-29 group / DM smokes
  the user requested 2026-05-24) read from to assert "router routed to
  the expected URLs and attributed them to the expected lanes." Without
  the seam, validation tests have to wire ad-hoc dyn-router wrappers;
  with it, validation reads the same projection the UI reads, ensuring
  the UI's claims are testable.

**Doctrine:** D5 (bounded snapshot — projection size cap mandatory), D7
(kernel reports, host renders — the UI does not call the router itself),
D8 (no per-event alloc — observer fires from data already on the
stack), D11 (one door per write surface — observer is the read door for
routing state, paired with the existing publish-engine status
projection).

**Recommended action:**

1. **Phase 1 — substrate** (post-step-3 cut-over so the seam has a real
   single producer): add `RoutingTraceObserver` trait to
   `nmp_core::substrate`, an `Arc<dyn RoutingTraceObserver>` slot on
   `Kernel`, and the kernel-side ring-buffer projection. New tests
   exercising lane attribution + bounded retention.
2. **Phase 2 — FFI/wasm surface:** snapshot-bound
   `recent_routing_decisions` field. Swift codegen emits the DTO.
3. **Phase 3 — Chirp inspector UI:** `RoutingInspectorView` long-press
   target on `ChirpEventCard` / publish-status row. Debug toolbar
   "show routing trace" toggle.
4. **Phase 4 — validation harness:** route-trace CLI subcommand on
   `nmp-repl` and the chirp-repl / chirp-tui smokes that consume the
   same projection.

**Phase: v1.** Without V-51 the architecture-migration validation goal
("end-to-end verify the router routes correctly against real pubkeys")
has no structural answer — every test has to roll its own observer.
Chirp's "peek under the hood" UI is the durable user-facing payoff;
the validation harness is the developer-facing one.

---

### V-49 · F-05 codegen coverage is ~17% — "v1 QUALITY" label is misleading [MEDIUM · clarity fix]

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
Rust shells (chirp-tui, nmp-desktop), the web TS shell, and the Android model +
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

**Approach:**
1. Add a `DisplayFormat.swift` namespace with `shortPubkey(_ hex: String)`,
   `relativeAgo(_ unixSeconds: UInt64)`, `avatarInitials(_ hex: String)`,
   `avatarColor(_ hex: String)` static helpers (8+8 / `Xs/Xm/Xh/Xd ago`
   buckets / djb2 — matches the canonical `nmp_core::display::*` algorithms
   the Rust shells use). Mirrors the equivalent shell-side helpers added to
   `chirp-tui` / `nmp-desktop` in the doctrine PR.
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

### V-54 · NIP-46 onboarding still blocks the actor thread [MEDIUM · remote-signer UX]

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

Work currently on a branch lives in [`WIP.md`](../WIP.md). Agents must check that file
before picking up Section 4 work to avoid duplicating an in-progress task.

---

## Section 3 — Pending User Decisions

Items that cannot be resolved autonomously. An agent that encounters one of these must log
its finding in the decision thread below and move on to the next item, not block.

### PD-033-A · Framework thesis — second non-social app — NEEDS REVALIDATION

**Original closure (PR #377 — merged 2026-05-23):** `apps/notes/` is a minimal NIP-01 note
client, 299 LOC Swift, 25 LOC Rust, zero new C-ABI protocol symbols. Closed as "confirmed."

**Re-opened (Opus direction review #13 — 2026-05-24):** Code-grounded inspection of the
artifact found it does NOT use the framework's defining properties:

- `NotesBridge.swift:74` calls `nmp_app_register_raw_event_observer` with a kind:1 filter
  only — this is a raw event *tap* (every ingested kind:1 fans out regardless of author).
  D3 outbox routing is bypassed entirely; `KindFilter` (`raw_event_observer.rs:92`) has no
  author dimension.
- `NoteModel.swift:14` parses the NIP-01 event JSON in Swift (`JSONSerialization →
  [String: Any]`). The architectural bible's first anti-pattern.
- `NotesBridge.swift:84` orders the timeline in Swift (insertion-order keyed off arrival,
  not `created_at`). The kernel owns no timeline view for this app.
- `TimelineView.swift:30, 36–38` formats timestamps + shortens pubkeys in Swift.
- `NotesBridge.swift:36–37` sets `isSignedIn = true` synchronously with no handshake-
  success gate.

**The 299 LOC count is accurate; the proof is not.** Notes proves the substrate *can be
bypassed* cheaply — not that the framework guidance produces correct apps.

**Required to re-close:** rewrite `apps/notes/` so it (a) registers a `LogicalInterest` for
kind:1 from the active user's follow set (outbox-routed through the planner, D3), (b)
consumes a kernel-owned timeline projection (no JSON in Swift, no list ordering in Swift),
and (c) gates `isSignedIn` on a real handshake-success callback. If that requires new
framework affordances, those affordances are the real v1-A gap. Milestone: 30-day call from
Opus direction review #13.

### PD-033-C · Two subscription systems (gates V-04 fix) — DECISION MADE

Decision (2026-05-23): migrate M1 `req()` call sites to InterestRegistry and delete the
hand-rolled path, staged. Stage 1 complete (PR #368). See V-04 staged fix plan above.

### PD-037 · Stale branch confirmation — CLOSED 2026-05-23

`codex/worker1-nip17-dm-inbox-relays` does not exist on the remote. Already deleted.

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

`docs/plan/scope-adjustments-2026-05-18.md` deferred M9 DMs/messaging and M12
Wallet to post-v1, but Marmot/MLS (`nmp-marmot`, `nmp-nip29`, `nmp-nip59`) and
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

All prior stages merged. Stage 3c (PR #385 — 2026-05-24) wired the publish path:
`KernelReducer::publish_signed_event`, `sign_event_via_extension` (async wasm32),
`dispatch_app_action_async` Promise wrapper. chirp-web now supports NIP-07 PublishNote
end-to-end (kind:1 write via NIP-07 signer, correlation_id settlement, per-relay terminals).

**Multi-role bootstrap parsing: DONE (PR #385 — 2026-05-24).** `nmp-wasm::relay_pool::roles_for_entry`
parses `"content"` / `"indexer"` / `"both"` / `"both,indexer"` strings; `spawn_drivers` opens one
`BrowserRelayDriver` per `(URL, role)` pair. Indexer-lane `RelayHealth` diagnostics are now
correctly bucketed.

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
   `active_account_bootstrap_requests` (5 tests covering F-02 + F-04 pass). Note: the kind:9735
   REQ was later D0-migrated to a host-side `ZapReceiptsRuntimeController` in `nmp-app-chirp`
   (PR #421) — the subscription is now pushed as a generic `LogicalInterest` via
   `nmp_nip57::self_zap_receipts_interest`. The planner's cold-start bootstrap fallback
   (`Tailing + Global + Nip65ReadRelays`) ensures receipts still flow before kind:10002 lands.

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

**Status:** Stage 1 (7 flat-record types) **DONE**. Stage 2 (`SnapshotProjections` registry)
**DONE**. Stage 3 partial — `TimelineItem` migrated to the generated header (this PR);
the handwritten Swift counterpart + custom decoder were deleted. The synthetic-construction
call site in `ModularBlockView` was updated to feed the now-non-optional `authorPictureUrl`
/ `authorAvatarSource` directly.

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

### F-06 · ~~CI lint: freeze C-ABI surface~~ CLOSED — see Appendix

### F-07 · Fix V-02 — move nmp-marmot to apps/ [DONE]

Completed — see V-02. Moved to `apps/marmot/nmp-app-marmot/`.

### F-08 · App-owned component registry + content rendering kits [V1 DX]

Promoted from the post-v1 bucket by user direction on 2026-05-25. This is the
M16 developer-experience track for reusable source components that apps can
install, own, customize, and update later.

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
and preserve the local customization.

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
content rendering system. Implementation plan: [`docs/plan/m16-kind-dispatch.md`](plan/m16-kind-dispatch.md).
Architectural decisions: [`ADR-0034`](decisions/0034-kind-dispatch-content-rendering.md).
Items F-CR-01 through F-CR-12 below are ordered by dependency. Pick the topmost
open item not already in Section 2. PR #588 closes F-CR-01 and F-CR-06; the next
highest-value open item is F-CR-02, because Android must join `ContentTreeWire`
before the Compose registry can replace the old embed card.

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

### F-10 · Canonical FlatBuffers runtime update transport [V1 INFRA · in progress]

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

**Required work:**

1. **Expose provenance in the projection** — `TimelineItem` already carries `relay_count: u32`. Add a `relay_provenance: Vec<String>` field (list of relay URLs) to `TimelineItem` and `TimelineEventCard`. Populate from `store.provenance_for(&event.id)` in `Kernel::timeline_item` (`crates/nmp-core/src/kernel/update.rs:464`). Keep `relay_count` as the cheap badge signal; `relay_provenance` is the detail payload. Consider making it opt-in via a projection flag to avoid bloating every timeline row snapshot.

2. **iOS Chirp** — long-press or info sheet on any note row opens a "Received from" list showing relay URLs with first-seen timestamps. Tapping a relay URL copies it or navigates to relay diagnostics.

3. **Android Chirp** — same UX as iOS: bottom sheet or dialog on long-press.

4. **chirp-tui** — `?` key or dedicated pane shows relay provenance for the selected event. Already has `DiagnosticsView` precedent.

5. **chirp-web** — tooltip or expandable row section.

**Note:** `relay_count: u32` is already on `TimelineItem` and rendered in iOS (`NoteRowView`). Step 1 is the only Rust change; steps 2–5 are pure presentation work per platform.

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
