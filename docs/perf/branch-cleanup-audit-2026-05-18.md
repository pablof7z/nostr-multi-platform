# Branch + worktree cleanup audit — HB43 pause-and-review

Date: 2026-05-18
Auditor: agent-a39013f3742a30812 (worktree)
Source repo: `/Users/pablofernandez/Work/nostr-multi-platform`

## TL;DR

| Surface | Count | Disk |
|---|---|---|
| Total worktrees (including primary) | 65 | ~24 GB under `.claude/worktrees/` |
| Agent worktrees under `.claude/worktrees/agent-*/` | 55 | 22.25 GB |
| MERGED-SAFE-DELETE (dead pid + on master + clean) | 12 | 0.00 GB (all small) |
| IDLE-SYNCED-PAUSE-FIRST (alive pid + on master, biggest disk hogs) | 40 | 14.82 GB |
| ACTIVE-WITH-WORK (alive pid + substantive dirty) | 1 | 3.93 GB |
| HAS-ORPHAN-COMMITS (commit not on master) | 2 | 3.50 GB |
| Remote branches on origin (non-master) | 4 | n/a |

**Auto-delete reclaim (no risk):** ~0 GB (dead-pid worktrees are all near-empty).
**Pause-then-delete reclaim:** ~14.8 GB (idle agents holding build artifacts).
**Triage required (do NOT auto-delete):** 3 agent worktrees + 1 detached worktree + 2 remote branches.

## Classification model

- **MERGED-SAFE-DELETE** — owning agent pid is DEAD, HEAD is ancestor of `origin/master`, no substantive dirty files (Cargo.lock-only is routine). Safe `git worktree remove --force` + `git branch -D` now.
- **IDLE-SYNCED-PAUSE-FIRST** — owning agent pid is ALIVE, HEAD is ancestor of master, 0 commits ahead, 0 (or routine) dirty. Build-artifact bloat. Safe to GC **only after orchestrator pauses the pid**.
- **ACTIVE-WITH-WORK** — owning pid alive AND (commits ahead OR substantive dirty). Leave alone.
- **HAS-ORPHAN-COMMITS** — HEAD not ancestor of master AND ≥1 commit on branch not on master. Surface commit list; orchestrator decides cherry-pick vs abandon.
- **DIRTY-WIP-NO-COMMITS** — dead pid, on master, substantive dirty diff that might be lost work. (Zero such cases here.)

Alive pids (observed): 22412, 47244, 57397, 91662 (all `claude`).
Dead pids (orphan worktrees): 59628, 72011, 75334.

## TRIAGE (orchestrator decides — do NOT include in auto-delete block)

### T1 — agent-a455e47384e6c9a43 (HAS-ORPHAN-COMMITS, alive pid 47244, 2817 MB)
- branch `worktree-agent-a455e47384e6c9a43`, HEAD `6eb4110a0b10`, 1 ahead / 43 behind master, 9 dirty files
- orphan commit: `6eb4110 feat(outbox): wire CompiledPlan to live REQ + publish path (T105)` — body says "WIP checkpoint before rebase onto origin/master"
- master already has `167d4bc` with the same subject (landed via rebase); `origin/t105-salvage-reference` also retains a copy as `2a66306`
- Dirty diff spans `crates/nmp-core/src/{kernel/*, actor/relay_mgmt.rs}` — could be live in-flight work or stale rebase residue
- **Recommendation:** confirm agent 47244 is idle on this worktree; if abandoned, `git worktree remove --force` + `git branch -D`. The unique commit is redundant with master + salvage branch.

### T2 — agent-a8cd1017bb7070361 (HAS-ORPHAN-COMMITS, alive pid 47244, 766 MB)
- branch `worktree-agent-a8cd1017bb7070361`, HEAD `60246b9ae8f6`, 1 ahead / 97 behind master, 0 dirty
- orphan commit: `60246b9 feat(ios-keychain): Keychain-backed KeyringCapability for NmpPulse`
- master already has `11fa4d7` with the same subject (rebased landing)
- **Recommendation:** subsumed-by-master; safe to remove once pid 47244 is paused. Confirm pid not actively writing to this worktree.

### T3 — agent-a9de3142cc7b1b20c (ACTIVE-WITH-WORK, alive pid 47244, 4025 MB)
- branch `worktree-agent-a9de3142cc7b1b20c`, HEAD `726550b4f733` (ancestor of master), 0 ahead, 9 dirty
- last commit on master tip `726550b4f733`: `fix(c13): reconcile placeholder/envelope contract with ADR-0017 + T103`
- dirty (non-routine):
  ```
   M crates/nmp-core/src/actor/commands/tests.rs
   M crates/nmp-core/src/kernel/ingest/mod.rs
   M crates/nmp-core/src/kernel/mod.rs
   M crates/nmp-core/src/kernel/publish_cmd.rs
  AM crates/nmp-core/src/kernel/publish_engine.rs
  ```
- **Recommendation:** looks like live agent work. LEAVE; ask pid 47244 to commit/push or explicitly abandon.

### T4 — `/private/tmp/nmp-clean-check-a3bf036` (detached, on master, 12 dirty iOS files, 1.8 GB)
- 12 iOS Swift files with substantial deletions (e.g. `NostrRichText.swift` -372 lines, `PodcastPlayerStore.swift` -474 lines). Net `12 files changed, 86 insertions(+), 4176 deletions(-)`.
- Looks like a substantive refactor (probably file splitting); not safe to discard without owner sign-off.
- `/private/tmp` is ephemeral on macOS (cleared on reboot). **Recommendation:** owner must reconcile this diff before next restart; otherwise abandon by removing the worktree.

### T5 — Other detached `/private/tmp/nmp-*` worktrees with orphan commits
Three worktrees have 1 commit each not on master AND not found in last 300 master log entries by subject or patch-id:
- `/private/tmp/nmp-codex-push.zqZ55G` — `7505077 fix(codex): clarify framework magic ignore owners` (1.4 GB)
- `/private/tmp/nmp-review-fix-d0b7df6` — `f6ec6d2 fix(codex): trim dead e2e test scaffolding` (1.9 GB)
- `/private/tmp/nmp-signers-review` — `3378160 fix(codex): align m8 subscription lifecycle docs` (2.0 GB)

These are 1-commit codex-review tweaks. **Recommendation:** orchestrator should `git format-patch -1 <sha>` from each before discarding, in case useful.

## Remote branches on `origin`

| Branch | Ahead | Behind | Classification | Recommendation |
|---|---|---|---|---|
| `origin/master` | — | — | primary | keep |
| `origin/claude/nostrdb-notedeck-lessons` | 0 | 118 | subsumed-by-master (merged via PR #1, see `d43e862`) | safe-delete |
| `origin/t105-salvage-reference` | 1 | 24 | intentional-reference (T105 WIP salvage record; unique commit `2a66306` has same subject as landed `167d4bc`) | **KEEP** as salvage record (matches HB39 protocol) |
| `origin/wip-snapshot-hb42` | 3 | 22 | intentional-reference (the unique `4c564cc snapshot(wip): main-checkout WIP from concurrent sessions` is a multi-feature blob; other 2 commits landed via rebase) | **KEEP** as snapshot record |

Also visible: `codex-wt/master` (separate `codex-wt` remote, not `origin`). 1 commit ahead (`9acefe4 fix(codex): split nmp highlighter hard-cap files`) which already landed on origin/master as `4d7a1e6`. Out of scope for origin deletion; can be pruned by deleting the remote if no longer used.

No `origin/worktree-agent-*` branches exist (confirmed via `git branch -r | grep worktree-agent` → zero matches). Local-only branches must be removed via `git branch -D` after worktree removal.

## Agent worktree inventory (55 rows)

Columns: branch suffix | HEAD | pid | alive | ahead | dirty | size | class

| suffix | HEAD | pid | alive | ahead | dirty | size | class |
|---|---|---|---|---|---|---|---|
| a455e47384e6c9a43 | 6eb4110a0b10 | 47244 | Y | 1 | 9 | 2817MB | HAS-ORPHAN-COMMITS |
| a8cd1017bb7070361 | 60246b9ae8f6 | 47244 | Y | 1 | 0 | 766MB | HAS-ORPHAN-COMMITS |
| a9de3142cc7b1b20c | 726550b4f733 | 47244 | Y | 0 | 9 | 4025MB | ACTIVE-WITH-WORK |
| a1d6e4852eece73c9 | 7afbe9c6ce59 | 75334 | N | 0 | 0 | 0MB | MERGED-SAFE-DELETE |
| a20807065da8c480e | c8d6f3d51dbe | 72011 | N | 0 | 0 | 0MB | MERGED-SAFE-DELETE |
| a3210d15ce7a679c5 | 6543651d1470 | 75334 | N | 0 | 0 | 0MB | MERGED-SAFE-DELETE |
| a3c87fa9bba5f3170 | 801ecf865468 | 59628 | N | 0 | 0 | 0MB | MERGED-SAFE-DELETE |
| a3d3bdc0efd2c7d5e | bd2cc80f35b1 | 72011 | N | 0 | 0 | 0MB | MERGED-SAFE-DELETE |
| a59a3fcba9b16385c | bee04c741faa | 59628 | N | 0 | 0 | 0MB | MERGED-SAFE-DELETE |
| a5fa7167edc339656 | acb53cc2c298 | 59628 | N | 0 | 0 | 0MB | MERGED-SAFE-DELETE |
| a754684faa264412b | 40f67e75788e | 75334 | N | 0 | 0 | 0MB | MERGED-SAFE-DELETE |
| a7804c1debbabb2be | e7d96bef3e53 | 75334 | N | 0 | 0 | 0MB | MERGED-SAFE-DELETE |
| a7c43ab7c564a4943 | 6e68b98e495e | 75334 | N | 0 | 0 | 0MB | MERGED-SAFE-DELETE |
| aa0fd41ba37d8ff13 | 59b01e1a1c8c | 59628 | N | 0 | 0 | 0MB | MERGED-SAFE-DELETE |
| aedb2a05dc14668a1 | 81aceec76eb7 | 59628 | N | 0 | 0 | 0MB | MERGED-SAFE-DELETE |
| a0ab9ab93ffdd4336 | 726550b4f733 | 47244 | Y | 0 | 0 | 4906MB | IDLE-SYNCED-PAUSE-FIRST |
| ac60122791b4f17d4 | 24cac7dae1f6 | 47244 | Y | 0 | 0 | 4663MB | IDLE-SYNCED-PAUSE-FIRST |
| a78e129fa5808595d | 23ac82cf805f | 47244 | Y | 0 | 1 (Cargo.lock) | 2899MB | IDLE-SYNCED-PAUSE-FIRST |
| a71fb1286ffb293b2 | 44cbfd2ad041 | 47244 | Y | 0 | 0 | 2514MB | IDLE-SYNCED-PAUSE-FIRST |
| a39013f3742a30812 | 23ac82cf805f | 47244 | Y | 0 | 0 | 50MB | IDLE-SYNCED-PAUSE-FIRST (THIS audit's worktree) |
| a77f04b1b0d0cb059 | 23ac82cf805f | 47244 | Y | 0 | 0 | 50MB | IDLE-SYNCED-PAUSE-FIRST |
| af4cfcd0dee29118e | 56a707fe7e85 | 47244 | Y | 0 | 0 | 49MB | IDLE-SYNCED-PAUSE-FIRST |
| ab7741d316ec07947 | b428020858f9 | 47244 | Y | 0 | 0 | 49MB | IDLE-SYNCED-PAUSE-FIRST |
| a092d30fa140739c3 | 612752551996 | 57397 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a0bdae71ba0c897d4 | fbace99ebbf0 | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a112e32e34ed09b16 | 00c3bf6fe566 | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a132b65c76e95f8c9 | 4fc0225832bc | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a13d53404009a223e | 11fa4d780e85 | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a23f4a7a03738dd67 | 1b2c123f37b8 | 22412 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a2eea0ba8c8429774 | e61b28370e97 | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a2fdbcba63fbf48df | 9ecaec49e1c4 | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a3323dcab949bbd86 | 43a172e7d0eb | 22412 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a3bf906ff39522122 | c8d6f3d51dbe | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a3d21a731d76977b9 | d6d5400761a3 | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a409321d91cb29c7a | 88ff444515d2 | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a60468a568bda0fa8 | 4299bfcc9a3e | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a740dc807902a36d0 | fd31c691290d | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a764e76902826362f | faf82fcfc293 | 22412 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a79193fcee5ba0bd9 | 863bea0f1a3d | 57397 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a804bf7830c1c0d03 | 863bea0f1a3d | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a884a82984f01f731 | 863bea0f1a3d | 57397 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a8b72af0fc91842c4 | 863bea0f1a3d | 57397 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| aa1b6347206afb8b6 | a94984c1a3ab | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST (anomalous nested path — see note) |
| a967b0e850603a49d | 2e6933b515b1 | 91662 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a9c0c3e3071c95477 | 158b74458517 | 91662 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| a9ec3b22d18119a6d | 3685d840e60d | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| ab0cc7d1f99158019 | b3d7963072e3 | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| ab0fe3c820b99db43 | 612752551996 | 57397 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| ab1b5da447b0e52ac | ed2e88e1f67e | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| ab5ad5d8a49756646 | 9cab9355c637 | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| ac1a7c1ffc7a2c984 | 2962893404f3 | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| ae78c9ae4dfc30792 | eff1692593e9 | 22412 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| ae9d581b9734edc15 | aa00a037e75b | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| aed90a821b9543b3c | f52e3949cef5 | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |
| af27f8a97c1e37116 | 612752551996 | 47244 | Y | 0 | 0 | 0MB | IDLE-SYNCED-PAUSE-FIRST |

**Anomaly note:** `aa1b6347206afb8b6` is registered at path `.../agent-a9523a829a8fa8474/.claude/worktrees/agent-aa1b6347206afb8b6`, nested inside a parent directory whose `a9523a829a8fa8474` agent is NOT itself a registered worktree. Likely a pre-existing path leak; safe to clean up via `git worktree remove` like the others.

**Note on near-zero MB rows:** `du` returns ~0 because those worktrees have minimal checkouts; the shared Cargo `target/` lives via per-worktree paths and gets large only after a build. Agent processes that never compiled stay small.

## Non-agent worktree inventory (10 rows)

These are out of the primary cleanup scope per the audit definition (`agent-*/` only), but listed for completeness. `/private/tmp/*` directories are ephemeral on macOS.

| Path | Branch | HEAD on master? | Ahead | Dirty | Size | Recommendation |
|---|---|---|---|---|---|---|
| `/Users/.../nostr-multi-platform` | master | Y (HEAD) | 0 | 0 | (primary) | keep |
| `/private/tmp/nmp-clean-check-a3bf036` | detached | Y | 0 | 12 (substantive iOS) | 1.8 GB | **TRIAGE T4** |
| `/private/tmp/nmp-codex-push.zqZ55G` | detached | N | 1 | 2 (Cargo.lock + 1) | 1.4 GB | **TRIAGE T5** |
| `/private/tmp/nmp-desktop-run` | detached | Y | 0 | 1 (Cargo.lock) | 1.5 GB | safe-remove |
| `/private/tmp/nmp-nip23-review` | codex-review-nip23 | Y | 0 | 1 (Cargo.lock) | 1.5 GB | safe-remove (branch is review-only) |
| `/private/tmp/nmp-review-6e0feab` | detached | Y | 0 | 3 (firehose-bench files) | 29 MB | likely safe-remove |
| `/private/tmp/nmp-review-fix-d0b7df6` | detached | N | 1 | 0 | 2.0 GB | **TRIAGE T5** |
| `/private/tmp/nmp-signers-review` | detached | N | 1 | 0 | 2.0 GB | **TRIAGE T5** |
| `/private/tmp/nmp-t46-review` | detached | Y | 0 | 0 | 29 MB | safe-remove |
| `/Users/.../.codex-review-m6-signers` | detached | Y | 0 | 0 | 1.8 GB | safe-remove |

## CLEANUP — copy-pasteable bash

> Run from `/Users/pablofernandez/Work/nostr-multi-platform` (primary repo, NOT a worktree).
> All commands are idempotent. None of the agent-worktree branches exist on `origin` (confirmed), so `git push origin --delete` is only needed for the special remote branches.

### Block A — auto-delete: MERGED-SAFE-DELETE agent worktrees (dead pids, 0 risk)

```bash
cd /Users/pablofernandez/Work/nostr-multi-platform

# 12 worktrees from dead pids 59628 / 72011 / 75334. All HEAD ∈ master, no dirty work.
for id in \
  a1d6e4852eece73c9 a20807065da8c480e a3210d15ce7a679c5 a3c87fa9bba5f3170 \
  a3d3bdc0efd2c7d5e a59a3fcba9b16385c a5fa7167edc339656 a754684faa264412b \
  a7804c1debbabb2be a7c43ab7c564a4943 aa0fd41ba37d8ff13 aedb2a05dc14668a1 ; do
  git worktree remove --force ".claude/worktrees/agent-${id}" || true
  git branch -D "worktree-agent-${id}" || true
done

# Reclaim: ~0 GB (these are mostly empty checkouts — main value is cleanup of ref clutter)
git worktree prune
```

### Block B — remote branches safely deletable

```bash
cd /Users/pablofernandez/Work/nostr-multi-platform

# claude/nostrdb-notedeck-lessons: 0 ahead, merged via PR #1 → subsumed-by-master
git push origin --delete claude/nostrdb-notedeck-lessons

# KEEP origin/t105-salvage-reference  (intentional salvage record, HB39 protocol)
# KEEP origin/wip-snapshot-hb42       (intentional WIP-blob salvage record)
```

### Block C — pause-then-delete: IDLE-SYNCED-PAUSE-FIRST agent worktrees (~14.8 GB)

> **Prerequisite:** orchestrator must first stop or confirm-idle pids 22412, 47244, 57397, 91662. Killing pids while they hold worktree locks is fine — `git worktree remove --force` then succeeds.

```bash
cd /Users/pablofernandez/Work/nostr-multi-platform

# After pids paused: top reclaim targets (>1GB each)
for id in \
  a0ab9ab93ffdd4336 ac60122791b4f17d4 a78e129fa5808595d a71fb1286ffb293b2 ; do
  git worktree remove --force ".claude/worktrees/agent-${id}" || true
  git branch -D "worktree-agent-${id}" || true
done
# Subtotal so far: ~15.0 GB reclaimed.

# Remaining 36 idle-synced (≤50 MB each, mostly zero) — bulk pass:
for id in \
  a39013f3742a30812 a77f04b1b0d0cb059 af4cfcd0dee29118e ab7741d316ec07947 \
  a092d30fa140739c3 a0bdae71ba0c897d4 a112e32e34ed09b16 a132b65c76e95f8c9 \
  a13d53404009a223e a23f4a7a03738dd67 a2eea0ba8c8429774 a2fdbcba63fbf48df \
  a3323dcab949bbd86 a3bf906ff39522122 a3d21a731d76977b9 a409321d91cb29c7a \
  a60468a568bda0fa8 a740dc807902a36d0 a764e76902826362f a79193fcee5ba0bd9 \
  a804bf7830c1c0d03 a884a82984f01f731 a8b72af0fc91842c4 aa1b6347206afb8b6 \
  a967b0e850603a49d a9c0c3e3071c95477 a9ec3b22d18119a6d ab0cc7d1f99158019 \
  ab0fe3c820b99db43 ab1b5da447b0e52ac ab5ad5d8a49756646 ac1a7c1ffc7a2c984 \
  ae78c9ae4dfc30792 ae9d581b9734edc15 aed90a821b9543b3c af27f8a97c1e37116 ; do
  git worktree remove --force ".claude/worktrees/agent-${id}" || true
  git branch -D "worktree-agent-${id}" || true
done

# Stray empty parent dir from the aa1b6347 nesting anomaly:
rm -rf .claude/worktrees/agent-a9523a829a8fa8474 2>/dev/null

git worktree prune
```

**NOTE:** this audit itself runs from `agent-a39013f3742a30812`. If you want to remove that worktree too, do it last AFTER the audit doc is committed + pushed (otherwise this commit context goes away).

### Block D — out-of-scope but useful (detached /private/tmp + .codex-review)

```bash
cd /Users/pablofernandez/Work/nostr-multi-platform

# Confirmed safe-to-remove (HEAD ∈ master, no orphan commits, dirty = routine/none)
git worktree remove --force /private/tmp/nmp-desktop-run         || true
git worktree remove --force /private/tmp/nmp-nip23-review        || true
git worktree remove --force /private/tmp/nmp-review-6e0feab      || true
git worktree remove --force /private/tmp/nmp-t46-review          || true
git worktree remove --force /Users/pablofernandez/Work/nostr-multi-platform/.codex-review-m6-signers || true
git branch -D codex-review-nip23 || true

# Reclaim: ~6.8 GB extra
git worktree prune
```

## Disk-reclaim summary

| Action | Worktrees removed | Disk reclaimed |
|---|---|---|
| Block A (auto, dead pids) | 12 | ~0 GB |
| Block B (remote branches) | n/a | n/a |
| Block C (after pid pause) | 40 | ~14.8 GB |
| Block D (detached /tmp + codex-review) | 5 | ~6.8 GB |
| **Total after full execution** | **57** | **~21.6 GB** |

Remaining after cleanup: primary repo + 3 triage agent worktrees (T1/T2/T3 ~7.4 GB) + 3 triage `/private/tmp` worktrees (T5 ~5.3 GB) + T4 substantive iOS WIP (1.8 GB) until owner reconciles.

## Final sanity checklist for orchestrator

1. Confirm pids 22412 / 47244 / 57397 / 91662 idle (or kill) before Block C.
2. Resolve TRIAGE items T1–T5 (cherry-pick or abandon) before deleting their worktrees.
3. Keep `origin/t105-salvage-reference` and `origin/wip-snapshot-hb42`.
4. After Block C, re-run heartbeat sweep — no `worktree-agent-*` local branches should remain except active ones.
