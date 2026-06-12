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

- 2026-06-12 — **PR-W2: build and deploy real nmp-wasm** (advances #1150 web track). PR #1176 (OPEN). Branch: `feat/chirp-web-build-real-wasm`. Builds and deploys the real `nmp-wasm` artifact and deletes the stale stub artifact.

- PR **#1014** — nmp-conformance scanner skill + catalog (v0 seed). **Draft** (author-gated);
  all checks green after the 2026-06-11 re-run cleared the v58 flake.

## Stale-entry purge log

2026-06-11: removed every "Active" entry dated 2026-05-24..28 — all their branches are
deleted and the work is merged (verified: PRs #460, #468, #484, #503, #517 merged;
branches `worktree-nmp-gallery-desktop`, `codex/*-zap-cleanup`, `feat/chirp-tui-zap-command`,
`feat/component-update`, `feat/step-12-nmp-marmot-return-to-crates`,
`refactor/substrate-d0-leaks-batch`, etc. gone). Also dropped the dead
`docs/BACKLOG.md` link (file no longer exists; the queue lives in GitHub Issues).

2026-06-12: purged every merged "Active" entry — verified each PR is MERGED via `gh pr view`: #1153, #1141, #1120, #1101, #1082 (PR-numbered), plus #1165 (`ci/nip55-proof-lanes-stage2-followups`), #1157 (`chore/nmp-codegen-drop-redundant-must-use`), #1134 (`adr-0048-nip55-external-signer`), #1110 (`chore/nmp-core-remove-dead-seed-accounts`), #1102 (`adr-0045-rev2-single-mechanism-cache-serve`), #1096 (`fix/kernel-ram-tier-bounded-1088`), #1080 (`feat/f02-cold-start-real-kernel-fix`), and the codebase-audit session (#1040–#1054, all merged). Kept the draft #1014 entry; added open PR #1176 (PR-W2 real nmp-wasm).
