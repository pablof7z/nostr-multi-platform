# NMP Backlog

> Tracker for active violations, pending user decisions, and the ordered v1 feature backlog.
> Supersedes `docs/perf/pending-user-decisions.md` (append-only history log, kept for audit)
> and `docs/arch-review-queue.md`.
>
> Companion files:
> - [`WIP.md`](../WIP.md) — live tracker for work currently on a branch (in-flight)
> - [`docs/plan.md`](plan.md) — overarching plan (milestones, doctrine, where we are)
>
> Verified against HEAD **76bc8547** (2026-05-23). Update this file in every PR that touches
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
- Stage 3 (NEXT): Migrate remaining M1 `req()` call sites in `profile.rs` / `thread.rs`.
- Stage 4: Delete the M1 `req()` helper once all call sites are migrated.

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

**Staged fix plan:**
Production splits of actor/mod.rs, dispatch.rs, kernel/mod.rs, ffi/mod.rs are post-v1
(ActorCommand closed enum analysis required — Opus review #10).

### V-13 · Broker relay client uses polling — violates D8 / no-polling doctrine [MEDIUM]

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
- Stage 2: Rewrite `TungsteniteRelayClient::run_worker` to use it; delete the
  polling loop.
- **Deadline:** before v1-A (any user sign-in via bunker hits this path).

### V-14 · Bunker has no reconnect — relay flap silently bricks the session [MEDIUM]

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
  that lands).
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

### V-16 · `SearchView.swift` is dead code that ships in the Chirp binary [MEDIUM] — **DONE** (PR #427 pending CI)

**Verified:** `ios/Chirp/Chirp/Features/SearchView.swift:3` defines `struct SearchView` and is
compiled into `Chirp.app` (`project.pbxproj:468`). Zero `SearchView()` call sites exist in the
iOS target. `RootShell.swift:7` documents "search tab removed (Search deferred to toolbar button
on HomeFeed)" but no HomeFeed toolbar button to `SearchView` exists. The view is also mis-named:
it is an "open hex pubkey or event id" form (lines 31–47), not a search feature.

**Correct fix:** delete `SearchView.swift` and remove it from `project.pbxproj`, or wire it
back into HomeFeed's toolbar and rename to `OpenByIdView`.

### V-17 · `MarmotMemberList::snapshot` returns `Vec::new()` — no group-member visibility [HIGH]

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

### V-20 · `dmRelativeTime` in Swift — thin-shell doctrine violation [MEDIUM]

**Verified:** `ios/Chirp/Chirp/Features/DmListView.swift:284` defines
`func dmRelativeTime(_ unixSecs: UInt64) -> String`. `DmConversationView.swift:161` and
`DmListView.swift:127` both call it. The thin-shell rule (aim.md §2) says relative-time
formatting is Rust-owned; `DiagnosticsView.swift` itself cites this rule. DM messages bypass it.

**Correct fix:** add `createdAtDisplay: String` to `DmMessage` in
`crates/nmp-nip17/src/inbox.rs` (Rust-side formatting), delete `dmRelativeTime` from Swift.

---

## Section 2 — In Flight

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
| Follow / Unfollow / React ActionModules app-local in `nmp-app-chirp` (Opus direction review #10 escape path) | 2026-05-24: lifted to `crates/nmp-nip02/` (NIP-02 follow list + NIP-25 reactions). Chirp's `register_chirp_actions` now delegates to `nmp_nip02::register_actions(app)`. Any Nostr app on top of NMP wires the social graph with a single call (mirrors `nmp_nip17::register_actions` / `nmp_nip57::register_actions` / `nmp_nip65::register_actions`). The deleted `Chirp{Follow,Unfollow,React}Module` impls are now `FollowModule` / `UnfollowModule` / `ReactModule` in `nmp-nip02`; namespaces (`nmp.follow`, `nmp.unfollow`, `nmp.nip25.react`) and JSON shapes unchanged — migration is binary-compatible for every existing host. |
