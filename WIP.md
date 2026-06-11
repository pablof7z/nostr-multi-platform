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

- 2026-06-11 — **PR-B FINAL: stop emitting payload:Value** (closes #991/#979). Worktree: agent-a329ca748cf7215bc. Branch: `pr-b-final-zero-payload-emission`. Promoting `decode_claimed_events`/`ClaimedEventsModel` to pub; rewiring nmp-gallery TUI+desktop to typed sidecars; zeroing payload emission; adding codegen-drift CI gate.

- 2026-06-11 — **F-02 closure-gate** (issue #977). Branch: `feat/f02-cold-start-real-kernel-fix`. Worktree: agent-aa07b090a7b004806. Fix: `on_dm_relays_changed` enqueues `DmRelayListChanged` trigger in wildcard ingest arm when `DmInboxRelayLookup` cache transitions. Integration test: `real_relay_nip17_cold_start_kernel` passes against `wss://relay.primal.net` in 1.71s.
- 2026-06-11 — **PR-B: final consumer typed-first migration** (toward #991/#979). Worktree: agent-ad2618eb77e4ab642. Completed: chirp-tui `FeatureSnapshot::from_flatbuffer` and chirp-desktop `decode_snapshot_typed` now read typed-first from the Tier-3 `SnapshotEnvelope` + per-projection typed sidecars (incl. `nmp.nip17.dm_inbox` + `resolved_profiles` for desktop) — neither shell reads `payload:Value`. Promoted to `pub`: the identity/views/outbox decode cluster (accounts/active_account/configured_relays/settings_hub/profile/author_view/thread_view/publish_outbox/outbox_summary) + `resolved_profiles`; added `RelayStatusEntry` to `SnapshotEnvelope` (relay_statuses decode). Schema field marked `payload:Value (deprecated)`. **Emission NOT zeroed**: blocked on migrating the `nmp-gallery` TUI+desktop shells (still decode `payload:Value` → `resolved_profiles`/`claimed_events`, no typed path) — that is the follow-up that lets `encode_snapshot_with_envelope` drop the payload. All scoped tests + doctrine lint + wasm green; full nmp-core lib suite 1256 passed.
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
