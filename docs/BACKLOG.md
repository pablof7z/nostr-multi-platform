# NMP Backlog

> **Single source of truth** for all active violations, in-flight work, pending decisions, and
> the ordered feature backlog. Supersedes `docs/perf/pending-user-decisions.md` (append-only
> history log, kept for audit), `docs/arch-review-queue.md`, and `WIP.md`.
>
> Verified against HEAD **710e73ee** (2026-05-22). Update this file in every PR that touches
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

---

## Section 2 — In Flight

Work currently on a branch. Agents must not duplicate these tasks.

| ID | Description | Branch | Commits ahead of master |
|----|-------------|--------|------------------------|
| B-1 | fix(nip17): fail-closed DM relay routing | `wt-nip17-fix` | 1 |
| B-2 | feat(chirp-web): parity shell | `codex/chirp-web-parity-polish` | Multiple |
| B-3 | WASM plan doc + label cleanup | `.claude/worktrees/agent-a69e36db1d66908af` | 3 |

> WIP.md entries for PR-G (action_stages), PR-I (relay slots), chirp-tui-spec, and
> chirp-repl-author-names show zero commits ahead of master — those branches have been
> merged or abandoned. WIP.md is now superseded by this file.

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

### F-03 · First-launch defaults — empty timeline [V1 BLOCKER]

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

### F-06 · CI lint: freeze C-ABI surface [V1 QUALITY]

177 `#[no_mangle] extern "C"` symbols currently (134 in `nmp-core/src/ffi/`, 43 in
`nmp-app-chirp/src/`). A CI lint that fails the build when a new `#[no_mangle]` is added
without an allowlist entry prevents the dispatch_action seam from eroding further.

**Fix:** add a `cargo xtask` or shell step in CI that counts `#[no_mangle]` symbols and fails
if the count increases without a matching allowlist update.

### F-07 · Fix V-02 — move nmp-marmot to apps/ [CLEANUP]

See V-02 fix path. Can be done in parallel with F-05/F-06.

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
