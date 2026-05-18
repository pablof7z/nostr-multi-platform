# T-podcast-gap-3 — `docs/orchestration-policy-podcast-forcing-function.md` missing

> **Surfaced by:** T157 step 1 onboarding
> **Filed:** 2026-05-18
> **Status:** Open — likely a missing doc, not a code gap

## Symptom

The T157 task brief instructs the rolling Android podcast agent to:

> "Read first: `docs/orchestration-policy-podcast-forcing-function.md`
> (the contract you're operating under)"

That file does not exist in the repo:

```bash
$ find docs -maxdepth 3 -name "*forcing*" -o -name "*orchestration-policy*"
(no matches)
```

The closest doc is `docs/perf/orchestration-log.md` (the durable
heartbeat trail), but that's an event log, not a contract document.

## Impact

The agent has no canonical document spelling out the contract it's
operating under. In its absence, T157 was executed by reading:

- The task brief itself (the orchestrator-injected prompt)
- `docs/design/podcast-app-rebuild.md` + `docs/design/podcast/inventory.md`
- The doctrine letters (D0, D5, D6, D8) inferred from existing
  `crates/nmp-core/` + `crates/nmp-android-ffi/` source comments
- `AGENTS.md` (file-size guidance)
- `MEMORY.md` (user memory index — autonomous mode, agent push protocol,
  worktree isolation)

That set was enough to ship a defensible iteration. But the explicit
"forcing function" — the contract the user wants the rolling agent to
hew to — should be written down somewhere agents can find it.

## Resolution path

Two options:

1. **Write the doc.** A single page describing the contract: per-iteration
   deliverable shape (always-built APK, single version, gap-task lifecycle,
   commit message conventions, push protocol). Lands as
   `docs/orchestration-policy-podcast-forcing-function.md`.

2. **Re-route the task brief.** If the contract lives only in the
   orchestrator's prompts, update the T157 task brief to drop the
   reference and inline the relevant invariants.

Either is fine; option 1 is durable across agent restarts and lets future
T157-N agents start with full context.

## Cross-references

- Surfaced commit (T157 step 1 scaffold): `feat(android-podcast): scaffold
  podcast Compose module (T157)`
- Related but distinct: `docs/perf/pending-user-decisions.md` (autonomous
  decision log, has different scope)
