# NMP Backlog

> **Single source of truth** for all active violations, in-flight work, pending decisions, and
> the ordered feature backlog. Supersedes `docs/perf/pending-user-decisions.md` (append-only
> history log, kept for audit), `docs/arch-review-queue.md`, and `WIP.md`.
>
> Verified against HEAD **596bba29** (2026-05-23). Update this file in every PR that touches
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
  `nmp-wasm/Cargo.toml`. `cargo check -p nmp-wasm` (native) passes; wasm32 target blocked by
  secp256k1-sys C build (tracked separately — needs pure-Rust secp backend or further feature-gating).
- Phase 1c ✅ DONE (PR #341): `RelayFrame` enum introduced; `actor/` and `relay_worker/` gated
  behind `#[cfg(feature = "native")]`; `cargo check -p nmp-core --no-default-features` passes.
- Stage 2: replace stub relay transport with `gloo-net`/`web-sys` WebSocket.
- Stage 3: port persistence to IndexedDB-backed `nostr-database` impl.

No chirp-web features may be added until Phase 1c lands.

### V-02 · nmp-marmot in crates/ — application subsystem misplaced [DONE]

**Verified:** `crates/nmp-marmot/` exists (~4,096 LOC). ADR-0025 explicitly states Marmot
opts out of the NMP substrate seam. It is an application feature, not a protocol primitive.
Protocol crates live in `crates/`; application feature bundles belong in `apps/`.

Moved to `apps/marmot/nmp-app-marmot/` — crate name unchanged (`nmp-marmot`). All dependent
path references updated (nmp-repl, nmp-app-chirp, nmp-testing). All `cargo check` passes.

### V-03 · ~~wallet_status app noun in nmp-core~~ CLOSED — see Appendix

### V-04 · Two subscription systems coexist — D4 single-writer violation [MEDIUM · pending user decision]

**Verified:** `crates/nmp-core/src/kernel/mod.rs:361` documents that the M1 hand-rolled
`req()` path is still authoritative. The `InterestRegistry`/`LogicalInterest` infrastructure
is live but parallel, not a replacement. Two systems maintaining separate state for the same
concern violates D4 (single-writer-per-fact).

**Correct fix:** designate `InterestRegistry` as canonical; migrate all M1 `req()` call sites
to it; delete the hand-rolled path. See PD-033-C for the user decision that gates this.

### V-05 · D2 enforcement gap — coverage_hook never installed [HIGH · staged fix required]

**Verified:** `crates/nmp-core/src/subs/mod.rs:93-116` — `coverage_hook` (the seam for
enforcing D2: "negentropy before REQ") is **never installed** in the production kernel.
`Kernel::with_publish_store` (`kernel/mod.rs:535`) wires `set_watermark_fn` (T129) but
makes no call to `SubscriptionLifecycle::set_coverage_hook`. Neither `actor::run_actor` nor
the `nmp-core/src/ffi` app surface installs it either.

**Impact:** D2 is convention-only. Every plan flows straight to a raw `REQ` — no
negentropy/set-reconciliation pre-pass runs in production. The sentinel test
`subs::coverage_hook_tests::d2_production_kernel_installs_coverage_hook` is `#[ignore]`d.

**Why the fix is staged:** a coverage-gate policy must depend on `nmp-core`, so a
`nmp-core → policy-crate` import is both a D0 app-noun leak AND a dep cycle. The
correct fix requires a higher-level **assembly crate** that can depend on both
`nmp-core` and the policy crate and installs the hook at kernel-construction time.

**Staged fix plan:**
- Stage 1: Create `nmp-coverage-gate` crate with the D2 policy logic (negentropy
  threshold, back-off rules). No `nmp-core` dep.
- Stage 2: Assembly point (app FFI crate or a new `nmp-app-base` crate) depends on
  both `nmp-core` and `nmp-coverage-gate`, installs the hook via
  `SubscriptionLifecycle::set_coverage_hook` at `Kernel::with_publish_store` time.
- Stage 3: Remove `#[ignore]` from the sentinel test; it becomes a CI gate.

**Deadline:** Stage 1 must land before NMP v1 ships. D2 is a protocol-correctness
requirement (negentropy reduces relay load and correctly handles large follow lists).

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

---

## Section 2 — In Flight

Work currently on a branch. Agents must not duplicate these tasks.

| ID | Description | Branch | Status |
|----|-------------|--------|--------|
| B-7 | fix(nmp-core): route kind:9735 to KernelEventObserver + #p bootstrap subscription (F-04) | agent in worktree | PR #342 pending CI merge |
| B-8 | test(nmp-app-chirp): DM inbox FFI round-trip — unignore dm_inbox_full_round_trip_through_ffi | `worktree-agent-ae8ca2fe461608b8a` | PR #344 pending CI merge |

> B-1–B-4 all merged to master (PRs #331–#337). B-5 merged PR #341. B-6 merged PR #340. Phase 1b merged PR #343. WIP.md superseded by this file.

---

## Section 3 — Pending User Decisions

Items that cannot be resolved autonomously. An agent that encounters one of these must log
its finding in the decision thread below and move on to the next item, not block.

### PD-033-A · Framework thesis — second non-social app

Each app requires: protocol crate + projection crate + 4–6 C-ABI symbols + payload types +
Swift decoder. A second non-social app built using only generic `dispatch_action` + kernel
projections (no new C-ABI symbols, no new projection crate) would validate or falsify the
framework claim in one sprint.

**Decision needed:** next sprint priority, or continue adding Chirp features first?

### PD-033-C · Two subscription systems (gates V-04 fix)

M1 hand-rolled `req()` path vs `InterestRegistry`. Migrate M1 to `InterestRegistry` and
delete, or designate one canonical first and delete the other in a follow-up?

### PD-037 · Stale branch confirmation

Branch `codex/worker1-nip17-dm-inbox-relays` may be dead (work shipped via PR #237 + #300).
Confirm and delete, or identify what remains.

---

## Section 4 — V1 Feature Backlog

Ordered by blocking priority. Items earlier in the list unblock items below them. An
autonomous agent picks the topmost item not already in Section 2.

### F-01 · Fix V-01 Phase 1b/1c — wire nmp-wasm to nmp-core proper [V1 BLOCKER]

Phase 1a, 1b, 1c all done (PRs #341, #343). `nmp-wasm` now depends on
`nmp-core` (default-features = false). Next: wire `NmpApp` into `WasmRuntime`
and delete the stub runtime. Blocked on secp256k1-sys wasm32 C build (pure-Rust
secp backend or further feature-gating needed).

No `chirp-web` feature work until stub runtime is deleted.

### F-02 · DM cold-start receive-side verification [V1 BLOCKER]

Gift-wrap **send** landed; kind:10050 relay-list publish is wired. The **receive** side on a
fresh install has not been verified end-to-end. A new user who signs in for the first time
must receive DMs before NIP-17 can be called done.

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

**Structural gaps fixed (B-7, in progress):**
1. `handle_event` `_` wildcard never called `notify_event_observers` → kind:9735 events never
   reached `ZapsAggregateProjection`. Fix: add kind:9735 arm in `kernel/ingest/mod.rs`.
2. No kind:9735 subscription interest registered at bootstrap. Fix: add `#p <viewer>` REQ in
   `active_account_bootstrap_requests`.

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
| Cashu / nutzaps (NIP-60/61) | NWC + NIP-57 cover the v1 zap use case |
| nmp-codegen full Swift bridge | Pilot (F-05) must land first to prove the pattern |
| Second non-social app | PD-033-A decision needed first |

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
