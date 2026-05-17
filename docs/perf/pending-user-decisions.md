# Pending User Decisions

Decisions I made autonomously while the user was asleep, with my reasoning. If the user disagrees with any, the noted commit can be reverted or amended.

Format: one entry per decision. Surface every entry in every status update until the user explicitly acknowledges or supersedes.

---

## Open (need user review)

### PD-002 — Remote branch divergence: `origin/claude/review-rmp-spec-8a7VX` vs `origin/master`

**Decision (autonomous):** continuing all work on `master`. Will not touch `claude/review-rmp-spec-8a7VX` without your direction.

**Background:** at session start, `git status` reported:
> Current branch: master
> Main branch (you will usually use this for PRs): claude/review-rmp-spec-8a7VX

The remote HEAD is `origin/claude/review-rmp-spec-8a7VX` (GitHub default). All orchestrator + agent work this session has gone to `master`. The two branches diverged: T19 framework-magic-reconciler accidentally pushed its commits (`c53ed1e`, `76769d9`, `175632b`, `209dee8`) to `claude/review-rmp-spec-8a7VX` (because the worktree was created from that branch). I detected this on T19's completion notification and cherry-picked those commits onto `master` (`1a897e8`, `7f5944e`, `a52acfc`).

**The orphan branch is now stale.** It contains a parallel history with semantically-equivalent commits up through the doctrine expansion, but lacks everything master has past `ea3d40e` (M1 PASS, meta-subscribe research, M2 fixes, README updates, T19 cherry-picks themselves, etc.). Approximately 20+ commits.

**Options:**
- **(a)** Merge `master` into `claude/review-rmp-spec-8a7VX` (fast-forward-able if I rebase the orphan onto master first). Keeps the remote-default branch name with all-the-things.
- **(b)** Set GitHub default to `master` and delete `claude/review-rmp-spec-8a7VX`. Cleaner; breaks any URLs / external references to that branch name.
- **(c)** Leave both — `claude/review-rmp-spec-8a7VX` stays as a historical snapshot of pre-session state; master is the active branch.

**Recommendation:** (a). Preserves all branch references, no rename impact, keeps the historical name. If you prefer (b) it's a one-liner.

**While you decide:** all agents have been instructed (and the heartbeat reinforces) to push to `master`. Future T19-style accidents will be caught faster — I added a `git branch --show-current` check to spot drift earlier.

---



---

## Resolved (user acked or superseded)

### PD-001 (resolved 2026-05-18) — Doctrine vocabulary collision

**User picked option (b):** expand `docs/product-spec/overview-and-dx.md` §1.5 to formally absorb the three additional load-bearing rules (errors-never-FFI / capabilities-report / reactivity-≤60Hz) as named doctrines D6, D7, D8.

Product-spec now has D0–D8 with an explicit "two kinds" distinction:
- **D0–D5: policy doctrines** — user-facing semantics (kernel-boundary, best-effort rendering, negentropy-first, outbox-automatic, single-writer-per-fact, snapshots-bounded).
- **D6–D8: substrate invariants** — runtime / FFI / hot-path constraints (errors-never-FFI, capabilities-report, reactivity-contract).

Conflicts still resolve in listed order (D0 wins over D8). README aligned. T19 framework-magic-reconciler in flight will absorb D0–D8 into the framework-magic docs (sending them an updated brief alongside this commit).

