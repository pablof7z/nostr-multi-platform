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

- 2026-06-12 — **Remove dead seed_accounts test fixture from nmp-core**. Branch: `chore/nmp-core-remove-dead-seed-accounts`. Worktree: /home/pablo/Work/nostrmultiplatform (root checkout). Removes `SeedAccount` struct and `seed_accounts()` fn — zero call sites, `#[cfg(test)] #[allow(dead_code)]` items carried over from 2026-05-18 kernel mod split.

- 2026-06-12 — **ADR-0045 Rev 2: single-mechanism cache-serve (owner correction)**. Branch: `adr-0045-rev2-single-mechanism-cache-serve`. Worktree: agent-a229248b7c12f575c. Supersedes §9 staged-by-domain rollout; amends decision to ONE always-on store-serve seam (cold/warm/offline/online, every LogicalInterest); preserves Rev 1 technical findings; updates issue #1086; restates v1 recommendation as "does universal cache-serve gate v1?".

- 2026-06-12 — **Release nmp-v0.4.0** (version bump 0.3.0→0.4.0, CHANGELOG). Branch: `release/nmp-v0.4.0`. PR #1101. Worktree: agent-a86437222f2953564. C-ABI break (4 symbols removed by #1100) + Android dark fix (#1092) warrant minor bump over 0.3.1.

- 2026-06-12 — **RAM-tier eviction for events/profiles/seed_contacts** (closes #1088). Branch: `fix/kernel-ram-tier-bounded-1088`. Worktree: agent-a93463be1bec749f1. New module `kernel/ram_eviction.rs` + 12 TDD tests in `kernel/ram_eviction_tests.rs`. HWMs: events=1000, profiles=2000, seed_contacts=32. Piggybacked on `run_gc_step` (separate call site from #1085).


- 2026-06-11 — **PR-B FINAL: stop emitting payload:Value** (closes #991/#979). Worktree: agent-a329ca748cf7215bc. Branch: `pr-b-final-zero-payload-emission` (PR #1082, review round 2). Done: gallery TUI+desktop on typed sidecars; emission zeroed; Rust flatc bindings REGENERATED (deprecated `payload` accessors gone) + `ci/check-rust-flatc-drift.sh` gate wired into codegen-drift.yml; `decode_snapshot_payload`/`decode_snapshot_with_typed`/`encode_snapshot_value` DELETED with all ~20 workspace readers migrated to `SnapshotEnvelope` + typed sidecars (`UpdateEnvelope::Snapshot` now carries `SnapshotEnvelope`); chirp-tui/chirp-desktop real-encoder round-trip tests added; nmp-wasm emits Tier-3 typed frames.

- 2026-06-11 — **F-02 closure-gate** (issue #977). Branch: `feat/f02-cold-start-real-kernel-fix`. Worktree: agent-aa07b090a7b004806. Fix: `on_dm_relays_changed` enqueues `DmRelayListChanged` trigger in wildcard ingest arm when `DmInboxRelayLookup` cache transitions. Integration test: `real_relay_nip17_cold_start_kernel` passes against `wss://relay.primal.net` in 1.71s.
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
