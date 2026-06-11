# WIP — Active Work In Flight

> **Live tracker** for work currently on a branch (agent worktrees, in-progress PRs).
> Update this file when you start work, and remove the entry when the PR merges.
>
> Related surfaces:
> - GitHub Issues (sorted by `priority:*` labels) — violations, pending decisions, ordered feature queue
> - [`docs/plan.md`](docs/plan.md) — overarching plan (milestones, doctrine, where we are)

## Architecture migration ladder — complete

The 12-step crate-boundary migration from
[`docs/architecture/crate-boundaries.md`](docs/architecture/crate-boundaries.md) §5 is
**done on master** (verified 2026-06-11): all steps 1–12 merged, including step 7
(`nmp-nip47`, PR #460), the step-8 Pool phases (push-model `Pool` API, `BrowserRelayDriver`
in `nmp-network/src/browser_driver.rs`, `nmp-signer-broker` riding `nmp_network::Pool`),
and step 12 (`crates/nmp-marmot` back under `crates/`). Per-step status detail now lives in
the spec itself; this file no longer mirrors it.

## Active

- 2026-06-11 — **F-02 closure-gate** (issue #977). Branch: `feat/f02-cold-start-real-kernel-fix`. Worktree: agent-aa07b090a7b004806. Fix: `on_dm_relays_changed` enqueues `DmRelayListChanged` trigger in wildcard ingest arm when `DmInboxRelayLookup` cache transitions. Integration test: `real_relay_nip17_cold_start_kernel` passes against `wss://relay.primal.net` in 1.71s.
- 2026-06-11 — **PR-B: typed-first migration step** (toward #991/#979). Branch: `feat/pr-b-delete-payload-value`. Worktree: agent-a07dd3b0c5a2269b5. Completed: chirp-tui `SharedSnapshot` migrated to typed-first (envelope + typed sidecars); new public APIs (`decode_snapshot_envelope`, `decode_snapshot_typed_projections`, relay_diagnostics/action_stages public decoders). Blocked on: kernel-built-in projections without typed schemas (accounts, active_account, profile, configured_relays, settings_hub) consumed by chirp-tui `FeatureSnapshot` and chirp-desktop. Full payload:Value removal deferred to follow-up PR.
- 2026-06-11 — **Codebase audit session** (`nmp-codebase-audit-2026-06` tenex proposal).
  MERGED: #1040 (executor flake → TOCTOU fix), #1041 (NIP-59 independent wrap timestamp),
  #1042 (planner 64-bit filter hash), #1046 (router fail-closed on empty NIP-65 write set),
  #1047 (kind:6 repost preview raw-JSON), #1048 (nmp-nwc → base64 crate), #1049
  (addr_tombstones GC purge), #1050 (relay_worker edge-triggered read starvation + v58
  test determinism), #1051 (update-callback quiescence gate / UAF), #1052 (DM send
  single-terminal contract), #1054 (relay_worker event honesty: terminal Closed on
  control-drop, ping-flush-gated pong timeout, write-interest fix). Session complete —
  all audit PRs merged, master CI green. New issues
  filed: #1043 (NIP-57 nostrPubkey), #1044 (dual free-string symbols, decision),
  #1045 (display helpers in projections). The stale `fix/zap-success-feedback` orphan
  was verified patch-equivalent to master (`git cherry` "-") and removed.
- PR **#1014** — nmp-conformance scanner skill + catalog (v0 seed). **Draft** (author-gated);
  all checks green after the 2026-06-11 re-run cleared the v58 flake.

## Stale-entry purge log

2026-06-11: removed every "Active" entry dated 2026-05-24..28 — all their branches are
deleted and the work is merged (verified: PRs #460, #468, #484, #503, #517 merged;
branches `worktree-nmp-gallery-desktop`, `codex/*-zap-cleanup`, `feat/chirp-tui-zap-command`,
`feat/component-update`, `feat/step-12-nmp-marmot-return-to-crates`,
`refactor/substrate-d0-leaks-batch`, etc. gone). Also dropped the dead
`docs/BACKLOG.md` link (file no longer exists; the queue lives in GitHub Issues).
