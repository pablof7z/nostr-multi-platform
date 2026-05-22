# NMP Backlog

> **Single source of truth** for all active violations, in-flight work, pending decisions, and
> the ordered feature backlog. Supersedes `docs/perf/pending-user-decisions.md` (append-only
> history log, kept for audit), `docs/arch-review-queue.md`, and `WIP.md`.
>
> Verified against HEAD **9f227710** (2026-05-23). Update this file in every PR that touches
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

### V-01 · nmp-wasm stub — multi-platform claim is false [CRITICAL · staged fix allowed]

**Verified:** `crates/nmp-wasm/Cargo.toml` has zero `nmp-core` dependency (only
`nmp-chirp-config`, `serde`, `serde_json`, `wasm-bindgen`). `src/runtime.rs` is a ~295-line
stub with a hardcoded `"browser-local"` pubkey and synthesized snapshot JSON.

**Impact:** `apps/chirp-web` and `.github/workflows/chirp-web.yml` build against this stub.
Every "one source of truth, four delivery paths" claim is structurally false until this is
fixed. Each sprint that adds chirp-web features on top of the stub incurs divergence that
makes the eventual fix harder.

**Staged fix plan:**
- Stage 1 (next sprint): add `nmp-core` dep behind `cfg(target_arch = "wasm32")`; construct
  `NmpApp` and call `nmp_app_dispatch_action`; delete synthesized-JSON stub. No relay
  transport yet. This alone makes the single-kernel thesis testable.
- Stage 2: replace stub relay transport with `gloo-net`/`web-sys` WebSocket.
- Stage 3: port persistence to IndexedDB-backed `nostr-database` impl.

No chirp-web features may be added until Stage 1 lands.

### V-02 · nmp-marmot in crates/ — application subsystem misplaced [HIGH · staged fix allowed]

**Verified:** `crates/nmp-marmot/` exists (~4,096 LOC). ADR-0025 explicitly states Marmot
opts out of the NMP substrate seam. It is an application feature, not a protocol primitive.
Protocol crates live in `crates/`; application feature bundles belong in `apps/`.

**Fix path:**
1. Create `apps/marmot/nmp-app-marmot/` with its own `Cargo.toml`
2. Move `crates/nmp-marmot/src/` → `apps/marmot/nmp-app-marmot/src/`
3. Update workspace `Cargo.toml` and all dependents

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
- Stage 1: Surface the limitation to the user — when active signer is remote and
  `clear_auth_signer` runs, emit a relay-status note that AUTH relays are not supported
  for bunker accounts (replaces silent failure with visible degradation).
- Stage 2: Broker side — expose `sign_auth_challenge(challenge, relay_url)` RPC.
- Stage 3: `sync_kernel_auth_signer` — for remote signers, install a
  `AuthSignerFn`-compatible closure that drives the broker RPC synchronously.

**Deadline:** Stage 1 (visibility) must land before v1. Stages 2-3 are post-v1.

### V-07 · Zap relay selection in Swift — D0 policy leak [HIGH · immediate fix]

**Verified:** `ios/Chirp/Chirp/Bridge/KernelModel.swift:405-421` — the `zap()` call
filters `relayEditRows` (the sender's own configured relays) to build the
`relays` tag for the kind:9734 zap request. This is policy (which relays the LN
provider should publish the kind:9735 receipt to), not rendering. A hardcoded
fallback to `wss://relay.damus.io` + `wss://nos.lol` is also embedded in Swift.

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

---

## Section 2 — In Flight

Work currently on a branch. Agents must not duplicate these tasks.

| ID | Description | Branch | Commits ahead of master |
|----|-------------|--------|------------------------|
| B-2 | feat(chirp-web): parity shell | `codex/chirp-web-parity-polish` | 1 |

> B-1 (`wt-nip17-fix`) and B-3 (agent worktree) no longer exist on remote — merged or
> abandoned. WIP.md entries for PR-G (action_stages), PR-I (relay slots), chirp-tui-spec,
> and chirp-repl-author-names show zero commits ahead of master — merged or abandoned.
> WIP.md is now superseded by this file.

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

### F-01 · Fix V-01 Stage 1 — wire nmp-wasm to nmp-core [V1 BLOCKER]

See V-01 staged fix plan. Stage 1 is the immediate deliverable: `nmp-core` dep added,
`NmpApp` constructed, `dispatch_action` reachable, stub runtime deleted.

No `chirp-web` feature work until this lands.

### F-02 · DM cold-start receive-side verification [V1 BLOCKER]

Gift-wrap **send** landed; kind:10050 relay-list publish is wired. The **receive** side on a
fresh install has not been verified end-to-end. A new user who signs in for the first time
must receive DMs before NIP-17 can be called done.

**Acceptance test:** fresh account → receive a gift-wrapped kind:1059 from a second account →
message appears in the `nmp.nip17.dm_inbox` snapshot projection.

### F-03a · NIP-65 kind:10002 publish coverage — verify auto-trigger fires on sign-in [V1 QUALITY]

`PublishRelayListAction` (`nmp.nip65.publish_relay_list`) is registered in the chirp app FFI.
The actor auto-publishes kind:10002 via `maybe_publish_relay_list_after_edit`
(`actor/dispatch.rs:117`) whenever `AddRelay` / `RemoveRelay` are dispatched. This covers
the relay-settings-change path.

**Gap to verify:** on first sign-in with an existing account (kind:10002 fetched from relay,
NOT added via `AddRelay`), the auto-trigger never fires — only explicit `AddRelay` calls do.
A returning user may have correct relays but never re-advertise them.

**Verification test:** sign in with a pre-existing account that has a kind:10002 on relay →
confirm the actor does NOT re-publish kind:10002 (correct; the relays came from the wire, not
from an AddRelay call). Then add/remove a relay in settings → confirm kind:10002 is published.

### F-03b · First-launch defaults — empty timeline [V1 BLOCKER]

A new account sees an empty timeline with no discovery surface. No default kind:3 follow list
is seeded on first sign-in.

**Fix:** on first account creation (or when kind:3 is absent after initial sync), either seed
a curated default follow list or present a discovery/onboarding surface before the main
timeline.

### F-04 · Zap E2E round-trip verification [V1 BLOCKER]


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

### F-07 · Fix V-02 — move nmp-marmot to apps/ [CLEANUP]

See V-02 fix path. Can be done in parallel with F-05.

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
