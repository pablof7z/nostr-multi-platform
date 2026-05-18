# Pending User Decisions

Decisions I made autonomously while the user was asleep, with my reasoning. If the user disagrees with any, the noted commit can be reverted or amended.

Format: one entry per decision. Surface every entry in every status update until the user explicitly acknowledges or supersedes.

---

## Open (need user review)

### PD-003 — M7 publishing-pipeline scope (task #45) shipped as substrate-only ahead of M3/M6/M8 wiring

**Decision (autonomous):** shipped `crates/nmp-core/src/publish/` with engine + state machine + trait shims + 20 tests. Did NOT wire it into the actor / FFI / iOS slice. Did NOT use MockRelay (does not exist). Did NOT exercise real LMDB persistence.

**Background:** task #45 spec asked for a fully-wired publishing pipeline with NIP-65 outbox routing, AUTH-REQUIRED reauth via real signer, durable LMDB queue, MockRelay integration tests in `crates/nmp-testing/tests/`, etc. Three dependencies named in the task — #43 (M6 Signer), #46 (M8 RelayManager) — and one implicit dep (M3 LMDB store for publish queue rows) are all either not landed or only partially landed for adjacent concerns (M3 store covers events, not publish queue).

The task's own escape clause: "If one missing: define minimal trait shim that #43/#46 will satisfy when they land." I extended that clause to cover all three.

**What shipped:**
- `PublishEngine` with deterministic per-(event,relay) state machine: Pending → InFlight → Ok | RelayError | TimedOut → FailedAfterRetries
- Retry policy: AUTH-REQUIRED → reauth +1 retry; transient → 3 retries at 1s/4s/16s
- `PublishStatusView` with bounded snapshot (rev counter, in_flight, recent_ok cap 32, recent_errors cap 32)
- Traits: `Signer`, `RelayDispatcher`, `OutboxResolver`, `PublishStore` — each with in-memory/noop/static test impl
- 11 unit tests (state machine + engine), 9 integration tests (NIP-65 routing, retry, give-up, restart, dedup, outcome classification)
- `docs/plan/m7-publishing.md` capturing scope + wiring deferred to dependency milestones

**Known weaknesses surfaced for codex/user review:**
- `publish_durable_across_restart` shares one `Arc<InMemoryPublishStore>` across the two engine instances — that's two engines reading the same in-process `Mutex<HashMap>`, not a serialize/deserialize round-trip through actual storage. The proof is weaker than the test name implies; the M3 LMDB-backed `PublishStore` impl will need its own round-trip test to close this gap.
- `PublishModule::reduce` (the ActionModule impl) is a syntactic pass-through. Real orchestration goes through `PublishEngine` direct methods. M6 ledger bridge will translate `ActionInput::RelayOk` → `PublishEngine::on_ack`.
- Engine consumes `Arc<dyn Signer>` for AUTH-REQUIRED retries but `apply_verdict::Reauth` currently models reauth as a transient backoff retry (no actual `sign_auth` call). M6 plumb-through will close this by calling `signer.sign_auth` between the verdict and the retry dispatch.
- File `crates/nmp-core/src/publish/tests.rs` (338 LOC) and `crates/nmp-core/tests/publish_engine.rs` (390 LOC) exceed the 300 LOC soft cap. Both under 500 hard cap. Precedent: `crates/nmp-testing/tests/m2_subscription_compilation_audit.rs` (460 LOC). Did NOT split.

**Hard-reset orphan commits:** during rebase I hard-reset to `origin/master` to escape doc-only conflicts in `docs/design/framework-magic/`. Approximately 7 doc-edit commits previously on `origin/worktree-agent-a53de6ee35b4e2ccc` (T22 doctrine alignment) are now orphaned on that remote branch. They were ALREADY in master per `git rebase` reporting (`skipped previously applied commit`) — so no semantic loss, but the orphan branch on origin still shows them. The heartbeat orphan-sweep will surface this.

**If wrong:** revert with `git revert <merge-sha>`; the substrate is self-contained and the wiring milestones can re-derive against a different shape. Or amend the scope (e.g. demand the full M3/M6/M8 wiring before merge).

---

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

