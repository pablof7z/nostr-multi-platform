# Task reconcile + next-steps synthesis — HB43 pause-and-review

> **Audience:** the user, returning to a session that has been running autonomously through HB1–HB43. **Sibling docs from the same pause-and-review wave:** `review-repo-state-2026-05-18.md` (master invariants + crate inventory) and `review-worktree-branches-2026-05-18.md` (52 worktrees + cleanup). This doc reconciles the orchestrator's T-task tracker against master and produces a dependency-ordered next-steps queue.
>
> **Master tip at write:** `e872c2a` (HB43 row); workspace **GREEN** (T110 landed; 119 test groups, 0 failed). No orphan branches.

---

## 1 — Task reconcile (T1 → T124)

Methodology: every Tnnn referenced in `orchestration-log.md` HB0–HB43 + commit subjects was classified by cross-checking against `git log origin/master`. Bulk-DONE ranges are collapsed; IN-FLIGHT / PENDING / STALE rows are itemised.

### 1.1 Bulk-DONE (verified via HB row + commit SHA on master)

| Range | Phase | Cite |
|---|---|---|
| T1–T7 | Wave-1 build + M1/M2/M3/M10.5/M11 designers | HB1 (`f1e374b/d660735/031fc07/9fead0e/0dfb975/fcf8b43`) |
| T8–T14 | Codex-fixer / FFI safety / clippy / M2+M3 design fixes | HB2–HB7a (`03d7a23/fb139ab/f68a479/c8c4f01`) |
| T15–T22 | Applesauce + NDK + framework-magic explorers / highlighter / plan-split / m1-hardening / m2-addresses / deferral-folder | HB3–HB7a (`edc17b0/8d633e8/ab632c1/701d0e5/b0ae439/c090952/6d7c46c`) |
| T23–T28 | M1 codex fix / M2 addresses / M3 codex round-2 / m2-impl / m3-impl / m105-impl | HB7a–HB16 (`a4ef834/8806f04/f765f60/92b8260/afc5475/7951cc9`) |
| T29–T34 | M2/M3 follow-up rounds 2-3 / M10.5 round 2-3 | HB17–HB22 (`7d16b3f/bc84cfe/0a10e42/942034d/ed241d2/8660961/7e8f607/f7fd6cd`) |
| T35–T44 | M10.5 round-3, surge wave (NIP-77, NIP-42, M6 signers, M11 step-0) | HB23–HB27 (`076173d/e69c3a4/4a7e0f3/a7faa74/d264d9d/e89ee13/f04c735/54b1c82/d3d6004`) |
| T45–T57 | M5–M8 wiring + framework-magic activate + nip29 integration + pulse builder L1 + brainstorm | HB28–HB32 (`df4e843/8fd2764/14d6924/741f5c0/eb7bed9/cc0e41b/93f57f4`) |
| T58–T65 | docs-planner / e2e-pulse / pulse-builder L1 / followlist trigger / nip19+nip21 / keychain T63a / supply-chain / kind-wrappers design | HB32–HB35 (`001ebf6/65e6812/11fa4d7/f76e46c/456a299/3f7dfac`) |
| T66a–T75 | Pulse L2-L4 / kind-wrappers / content-rendering / nostrdb-investigate / D1 placeholder / Layer-A content / nip23 / doctrine-lint | HB34–HB37 (`00c3bf6/7f4953d/ef0d15d/868d570/d3067a6/cd810b4/d264d9d/b3d7963`) |
| T76–T86 | nip42 fail-closed / nip42-types crate / nmp-content fixes / nip23 hardening / nmp-cli / OneshotApi / TimeCached / query_visit / nostrdb-rs reject | HB35–HB37 (`782096b/f52e394/fa4fa16/b717fa7/0554848/fbace99/2962893/b3d7963`) |
| T87–T98 | publish-outbox / PD-004 audit / content-gallery 3-stage / OpenUri / SubKey / ContentTree FFI / D1-avatar / KernelAction reducer / kernel KeyringCapability / util D6 | HB35–HB38 (`c5c6f5b/aa00a03/3685d84/fd31c69/fd002ce/88ff444/a94984c/068341e/1d664f7/b826150`) |
| T99–T104 | publish-outbox half / pulse-feed (HELD/absorbed) / softcap-split / @mention deferred / FFI dual-shape envelope / M11 oneshot-bound | HB37–HB38 (`ed2e88e/e61b283`); T100, T102, T104 still pending — see §1.3 |
| T105 keystone | outbox-on-wire (5 commits) | HB39–HB42 (`167d4bc/5c5d417/e74247c/0849fd2/fada22b/24cac7d`) |
| T106 | doctrine-lint workspace-red fix | HB38 (`863bea0`) |
| T107 | host UpdateEnvelope migration (absorbed into T103 + desktop session) | implicit (`e61b283/9840d5f`) |
| T108–T109 | (unfilled / absorbed) | n/a — number gap; not orphans |
| T110 | C13 contract fix (workspace GREEN) | HB43 (`726550b`) |
| T112 | nip77 status plumb | dispatched HB40; **status: silently complete OR still in-flight** — no clear landing commit yet, needs verification |
| T114 part 1 | bounded actor channel | HB43 (`44cbfd2`) |

### 1.2 IN-FLIGHT (running at HB43)

| Tnnn | Title | Worktree mtime hint | Notes |
|---|---|---|---|
| **T117** | publish-engine-wire (G5: wire `PublishEngine` FSM into `kernel/publish_cmd.rs`) | dispatched HB41; HB43 reports "0 ahead, 8 dirty" | Opus-sized, deep FSM work. **No publish-side commits on master since `167d4bc`.** Verified via `git log -- crates/nmp-core/src/publish/ crates/nmp-core/src/kernel/publish_cmd.rs` — last touch was T105. |
| T112 | nip77-status-plumb (`nip77_negentropy:"unknown"` → real probe state) | likely complete | Small task; either silently landed under un-grep-able subject or stuck. **Action when user resumes:** verify with `git log --since="HB40-time" --grep="nip77\|negentropy"`. |
| (this doc) | review-tasks-next-steps | — | one of three HB43 review agents |
| review-repo-state | sibling | — | produces master invariants snapshot |
| review-worktree-branches | sibling | — | 52-worktree audit + cleanup script |

### 1.3 PENDING (blocked or queued)

| Tnnn | Title | Blocked by | Severity | Queue position |
|---|---|---|---|---|
| **T114b** | retention audit + M10.5 §G-S2 close (PD-021 line-11 closer) | nothing — independent of T117's publish surface | HIGH | **dispatch immediately on user resume** — closes the last open M10.5 gate, was deferred from T114 narrowing after two prior timeouts on open-ended scope |
| **T116** | wire reconnect-replay (`SubscriptionLifecycle::handle_reconnect` called by actor) — G1 | T117 landing (overlapping actor surface) | HIGH | dispatch AFTER T117 lands |
| **T118** | app-lifecycle FFI (`nmp_app_set_foreground(bool)` + `TriggerEvent::Foreground` fan) — G3 | T117 + T116 (actor surface contention) | HIGH | after T116 |
| **T119** | NIP-46 transport wire (`Nip46Transport` impl + bunker-relay isolation invariant test) — G6 | T117 (publish path), partially independent | HIGH-product | after T117 |
| **T120** | composite: reconnect-backoff-policy + keepalive ping + CLOSED classifier + ConnectionPool prod impl (G4+G7+G8+G11+G12) | T116 (replays need new pool semantics) | MED | after T116, can split |
| **T121** | thread-hydration outbox routing (codex T105 review R1) | T105 done; no kernel-surface collision | MED | parallel-safe — dispatch alongside T114b |
| **T122** | firehose hashtag → inbox routing (codex T105 review R2) | T105 done; no kernel-surface collision | LOW | parallel-safe |
| **T123** | `requests/mod.rs` 353-LOC soft-cap split (codex T105 review R5) | none | LOW | cosmetic; parallel-safe |
| T100 | pulse-feed (consume CompiledPlan in UI) | T105 done — **now unblocked**; pulse author owns | MED | hand off to a Pulse-session dispatch when convenient |
| T102 | @mention autocomplete substrate (user-deferred) | nothing | LOW | deferred per HB37 |
| T104 | M11 podcast oneshot_subs bounded | M11 not on critical path | LOW | defer |
| T59 | iOS NIP-42 signer FFI binding (PD-005 residual) | M14 UniFFI scaffolding (preferred) OR hand-rolled C shim | MED | track inside PD-005 — not blocking v1 |
| T88 | PD-004 audit (one-account-per-pubkey) — kernel/audit landed `88ff444` | DONE (HB37) — should move to §1.1 | DONE | tracker mis-attributes; ignore |
| T103 | FFI dual-shape envelope | DONE (`e61b283`) | DONE | also DONE-rather-than-pending |
| T113 | likely-absorbed-by-T105 per HB41 | — | STALE | mark absorbed |

### 1.4 STALE / SUBSUMED

| Tnnn | Why stale |
|---|---|
| T11 codex-fixer-2 (round of fix-its) | superseded by T23 + T25 + T29 + T30 — all incremental codex-driven cycles per design |
| T87 publish-outbox (kernel half) | absorbed into T105's 5-commit chain (`5c5d417` resolver + `e74247c` consumers) |
| T108, T109 | number gaps — no commit/log evidence, treat as never-allocated |
| T111, T115 | not referenced in any HB row; gaps in T-number allocation |
| T113 | per HB41 "likely absorbed by T105"; outbox-on-wire publish path is its content |
| PD-019 inflight tracker note about iOS keychain | RESOLVED HB37 by `fd002ce` + `11fa4d7` — already DONE in tracker |

### 1.5 Tracker entries flagged as WRONG (verify before re-using)

1. **T88, T103, T106, T110, T114-part-1** still appear in some HB rows as "pending / in-flight" but **commits are on master**. Tracker rows must be marked DONE. Verifier should grep `git log --grep="T88\|T103\|T106\|T110\|T114"` (output above).
2. **T119, T120 severity in HB41 note** — "all HIGH"; this doc downgrades T120 to MED per the relay-lifecycle G-list (G4/G7/G8/G11/G12 are all MED with G12 latent-until-T105-lands; T105 has landed, so G4 is now elevated — recommended to split T120 with the G4 (backoff-policy) slice promoted to HIGH).
3. **T112 attribution** — no commit on master since HB40 dispatch; either landed silently with an unsearchable subject or actually stalled. Verify before re-dispatching.

---

## 2 — Honest critical path (top 10, dependency-ordered)

Framing: relay-lifecycle §3 G1-G12 + §27 discrepancies + open PDs. Rule of dispatch: **don't pile collisions** — any two tasks that touch the same kernel surface (`actor/`, `kernel/publish_cmd.rs`, `subs/`) serialize; doc / store / protocol-crate tasks parallelise.

| # | Task | Sev | Blocked-by | Blocks | Why now |
|---|---|---|---|---|---|
| 1 | **Verify T117 status** (advisor + log check before any new dispatch) | meta | — | T116, T118, T119, T120 | Three HIGH items are gated on T117. If T117 silently landed: cascade unlocks. If stalled: salvage and re-narrow like T105/T110. |
| 2 | **T114b** retention audit + M10.5 §G-S2 close | HIGH | — | PD-021 (M10.5 line-11) closure | Independent kernel-test surface; closes the last open M10.5 gate; user-directed PD per HB37. **Most likely immediate-resume dispatch.** |
| 3 | **T116** wire-reconnect-replay (G1) | HIGH | T117 settle | T118, T120-G7 (keepalive depends on reconnect-replay contract) | The G1 substrate (`SubscriptionLifecycle::handle_reconnect`) is shipped but never called from the actor; every reconnect today silently loses live REQs. Pre-T105 this was masked by the bootstrap-only routing; **post-T105 (URL-keyed pool, on-demand sockets) it is now actively user-visible** — disconnected per-author sockets do not re-emit their REQs on reconnect. |
| 4 | **T117** publish-engine-wire (G5) — if in-flight, wait; if stalled, re-narrow + salvage | HIGH | (in flight) | T119 (NIP-46 needs publish path), T100 (pulse-feed publish), G4 backoff (publish retry uses backoff) | Engine FSM (`publish/state.rs:213-329`) is dead code on the live path; one-shot EVENT to RelayRole::Content (with T105 fan-out) is the entire publish guarantee today. |
| 5 | **T120-G4-slice** reconnect-backoff-policy (full-jitter exponential, per-relay token bucket) | HIGH (post-T105) | T116 lands | T118 foreground kick, T120-G7 keepalive | Flat 3s + no jitter + no cap was a non-issue with 2 fixed relays. T105 made the relay set dynamic; reconnect storms are now possible. **Promoted from MED to HIGH** by the T105 landing. |
| 6 | **T118** app-lifecycle FFI (G3) | HIGH (product) | T116 (foreground triggers reconnect-kick which needs replay) | iOS UX (foreground sync) | Kernel exposes no `nmp_app_set_foreground`; the nip77 `TriggerEvent::Foreground` engine has nowhere to fire from. Pulse `scenePhase` → kernel contract is undocumented. |
| 7 | **T121** thread-hydration outbox routing (codex T105 R1) | MED | none — parallel-safe | — | Reply threads currently route via bootstrap `RelayRole::Content`; should resolve `#e`/id → original-event-author → `author_write_relays`. Now that T105 is on the wire, this is a small follow-on touching only `kernel/requests/thread.rs:133,154`. |
| 8 | **T119** NIP-46 transport wire (G6) | HIGH-product | T117 settle + (spec NIP-46 isolation invariant first) | bunker:// sign-in product | `bunker://` URI validation lands but the transport pump never starts. Pair with a "no NIP-46 frame leaks to non-NIP-46 relay" regression test. |
| 9 | **T120-rest** keepalive (G7) + CLOSED classifier (G11) + ConnectionPool prod impl (G8) | MED | T116 lands | — | Bundle: per-app-layer ping (NDK 120s / applesauce REQ-EOSE), parse CLOSED-reason at actor, replace per-`RelayRole` channels with the shipped `subs::ConnectionPool` trait. |
| 10 | **T122 + T123** doc/firehose-inbox + `requests/mod.rs` soft-cap split | LOW | none | — | Parallel-safe housekeeping after the kernel surface settles. |

**HELD until prereqs land (don't dispatch yet):**
- T116/T118/T119/T120 all held until T117 settles (HB41 anchor: "they all touch overlapping kernel surface").
- T100 pulse-feed: unblocked by T105 but owned by the Pulse-builder session, not this orchestrator.
- T59 iOS NIP-42 signer FFI (PD-005): waits for M14 UniFFI OR explicit user direction to hand-roll the C shim.
- T120-G7 keepalive: held until T116 lands (the reconnect-replay contract must be stable before keepalive triggers reconnects).

---

## 3 — Open PD ledger (every PD-* still in "Open" status)

Format: `PD-NNN — title` · current status (1 sentence) · closeable? (boolean + condition)

- **PD-005** — *iOS signer binding for NIP-42* · kernel side hardened across T76 + T77 (fail-closed + types crate); iOS FFI binding (`nmp_app_bind_auth_signer` + `ActorCommand::BindAuthSigner`) still deferred to T59. **Not closeable yet** — T117 does not address the FFI signer surface; closeable only when M14 UniFFI scaffolding lands OR a hand-rolled C shim is dispatched.
- **PD-018** — *doctrine-lint D8 dormant on production code* · ships scoped to functions with explicit `// hot path` marker; zero production functions carry the marker today; D0/D6/D7 are live and enforcing. **Awaiting user acknowledgement** (autonomous decision); not actionable.
- **PD-020** — *T81 SubKey/triple: `iter_active` dedups by `(scope,key)` not `InterestId`* · matches the notedeck §3.2 "many owners share one live sub" model; legacy `push(LogicalInterest)` surface preserved with synthetic owner. **Awaiting user acknowledgement**; not actionable.
- **PD-021 line-11** — *M10.5 exit gate re-scoped to simulator-provable subset* · S2 drain analysis confirmed RETAINED (38 MiB net, 0.13% reclaimed) → bounded actor channel mandatory; **T114 part 1 landed `44cbfd2`**. **CLOSEABLE when T114b lands** (retention audit half + full §G-S2 close).
- **PD-021 line-348** — *T82 OneshotApi poll-based completion + UnknownIds reference scope* · poll-based completion (no async runtime in kernel) + raw NIP-01 tag scope (`p`/`e`/`q`); `nevent`/`naddr` decode deferred. **Awaiting user acknowledgement**; not actionable. **NOTE: PD-number collision** with the M10.5 entry above — flagged in HB37 row; user should re-number one of them.

**Closeable on next user contact (no agent action needed first):**
- PD-005 narrows further when T117 lands (a clean publish-engine path makes the signer-binding implementation more obvious) — but does not close.

**Awaiting user acknowledgement (autonomous decisions that just need a thumbs-up):**
- PD-007 through PD-017 are already in the "Resolved" section of `pending-user-decisions.md`; they are autonomous accepts/rejects of designer recommendations. Bulk-ack would be cheap; no harm in leaving them.

**Already CLOSED (per HB rows; verify the doc reflects this):**
- PD-001 (doctrine vocabulary), PD-002 (remote branch divergence), PD-003 (M7 substrate-only), PD-004 (IdentityId=pubkey), PD-006 (framework-magic C1-C13), PD-019 (keychain both halves).

---

## 4 — Velocity + risk read

### 4.1 Commits per heartbeat (last 10 HBs)

| HB | Commits net to master | Notes |
|---|---:|---|
| HB34 | ~9 | Reaped Layer-A/nip23/doctrine-lint/D1-placeholder; 9-agent wave dispatched |
| HB35 | ~7 | OpenUri/SubKey/keychain/codex/TimeCached/query_visit/nostrdb-reject |
| HB36 | 0 | Zero-completion HB; deliberate working-tree-no-touch to protect 7 in-flight agents |
| HB37 | ~11 | Reap T82/T93/T95/T96/T97/T66a wave (close PD-019/PD-003); dispatched 6-agent unlock wave |
| HB38 | ~7 | T101+gallery; T105 keystone + T106 workspace-red in flight |
| HB39 | ~6 | T105 salvaged to ref branch; 89 GB freed; 3 keystone retries |
| HB40 | 1 | Quiet; T112 dispatched |
| HB41 | ~3 | **T105 landed (keystone)**; relay-lifecycle research landed; T114/T117/codex dispatched |
| HB42 | ~3 | T105 extending (e74247c/0849fd2/fada22b); 0 of HB41 dispatches notified — HOLD |
| HB43 | ~7 | T110/T114-pt1/codex review landed (workspace GREEN); 3 review agents dispatched |

**Read:** sustained ~5-7 commits/HB through the impl phase, with HB36 a deliberate zero (one of the highest-discipline calls of the session — proves "don't churn" works). Total master commits this session: ~384.

### 4.2 Infra-attrition pattern

The following dispatches failed first-try (server-side rate-limits or stream-timeouts) and were salvaged on retry:
- **T105 outbox-on-wire** — first attempt timed out at ~26min / 135 tool_uses; commit `6eb4110` salvaged to `origin/t105-salvage-reference` (per HB39); re-dispatch succeeded on attempt 2 with reference-branch in brief.
- **T112 nip77-status-plumb** — possibly silently stalled (no landing commit traceable since HB40); status unknown.
- **T114 (original)** — open-ended scope timed out twice; narrowed to T114-pt1 (channel-bound only) which landed in HB43; T114b (retention audit) deferred.
- **T105-first** (different earlier attempt) — sub-agent contention noted in HB39.
- **content-gallery first-try** — stream-timeouts before HB38; resolved via staged-resilience (3-stage checkpoint-push) which then landed cleanly.
- **relay-lifecycle-review first-try** — barely started before rate-limit; re-dispatched as relay-lifecycle-review-2; landed `b428020`.

**Playbook adjustments to record in `memory/`:**
1. **Default to scope-narrowing for any dispatch >50 LOC of impl across >2 crates.** The salvage cost of an open-ended task is higher than the time saved by bundling. T114 → T114-pt1 + T114b is the canonical example.
2. **Salvage-branch + reference-in-brief pattern works.** `origin/t105-salvage-reference` let the retry study the prior approach without re-deriving. Make this the default for any keystone-class re-dispatch.
3. **Staged-resilience (3-stage checkpoint-push) is now the default for any iOS-app or fixture-heavy dispatch.** Single-commit dispatches lose work to stream-timeouts at >10 min runtime.
4. **One-dispatch-per-HB during high-pressure waves** (HB17, HB23, HB42). The temptation to fan out when a wave is mid-flight has produced collisions every time it was tried.
5. **Codex post-merge review must be re-runnable** post-keystone-completion (codex T105 review wrote its FIX patches against a snapshot, which were correctly discarded once `e74247c/0849fd2/fada22b` landed). Future protocol: run codex (b) post-landing for fresh state, not (a) mid-keystone.

### 4.3 Risk read

- **Highest current risk:** T117 silent stall. The publish engine FSM is the next-most-load-bearing keystone after T105; if it's stuck, three HIGH downstream tasks (T116/T118/T119) stay blocked. **First action on user resume:** verify T117 with a single bash check; salvage if needed.
- **Second-highest:** the post-T105 dynamic-relay-set now exposes the flat-3s-no-jitter backoff (G4 promoted MED→HIGH). Reconnect storms become possible the moment a real user follows >10 authors across >5 different write-relays. **Mitigation:** T120-G4-slice can dispatch in parallel with T117 wait (different file surface — `relay_worker.rs:64,197-217` vs `kernel/publish_cmd.rs`).
- **Worktree pressure:** 12 active worktrees + 52 historical (per sibling review-worktree-branches doc). The 52-worktree purge in HB34 freed 89 GB; further accumulation risks disk exhaustion. **Lean on the sibling cleanup script.**
- **PD number collision (PD-021 ×2)** — minor but should be renumbered for unambiguous reference. Suggest: keep PD-021 = M10.5 (canonical; line-11), renumber the OneshotApi entry to PD-022.

---

## 5 — What to dispatch IMMEDIATELY when the user resumes (top 3)

### #1 — T114b — retention audit + M10.5 §G-S2 close

**Why first:** independent of T117 (different file surface — `actor/state.rs` retention budgets vs `publish/engine.rs` FSM); HIGH severity; closes the last open M10.5 gate (PD-021 line-11); user-directed PD; T114-pt1 already landed the bounded channel half. The retention audit is the half that timed out twice on open scope; **dispatch with explicit scope-narrowing**.

**Prompt-prep notes:**
- Reference `docs/perf/m10.5/s2-drain-analysis.md` (`d6d5400`) for the "retained 38 MiB net" target.
- Hard scope ceiling: ONLY the actor-side retention audit; no FFI changes; no relay-worker changes.
- Required artefact: `docs/perf/m10.5/s2-retention-audit.md` with before/after RSS + `retained_heap_after_drain_bytes ≤ 1 MiB` gate green.
- Reference T114-pt1 commit `44cbfd2` for the bounded-channel half already in place.
- Push protocol: `git push origin HEAD:master`.

### #2 — Verify T117 status, then either RE-DISPATCH or HOLD

**Why second:** T117 is the gating dependency for three HIGH downstream tasks (T116/T118/T119). The single bash check `git log --since="2026-05-18 12:00" --grep="T117\|publish-engine"` answers the question in 5 seconds; without it the orchestrator can't safely fan out.

**Prompt-prep notes (if re-dispatch required):**
- Pattern after T105 salvage: extract any work-in-progress to a reference branch (`origin/t117-salvage-reference`), reference in re-dispatch brief.
- Narrowed scope: replace the one-shot EVENT emission at `kernel/publish_cmd.rs:89-100` with a call into `publish::PublishEngine::on_publish_request` + retain `accepted_locally` semantics; DON'T also wire pending_retries durability (already shipped per T54 `f04c735`).
- Required artefact: integration test in `nmp-testing` proving a published EVENT replays after a relay-worker reconnect.
- Hard-cap: 500 LOC; if it doesn't fit, split into T117a (kernel wire) + T117b (retry verification harness).

### #3 — T121 thread-hydration outbox routing (codex T105 review R1)

**Why third:** PARALLEL-SAFE with the above (touches only `kernel/requests/thread.rs:133,154` — does not collide with T114b's actor surface or T117's publish surface). MED severity but the test is well-specified by the codex review (cite the R1 follow-up text). Lands a visible doc/code win on a quiet HB while T117 is being verified.

**Prompt-prep notes:**
- Cite `docs/perf/codex-reviews/t105-167d4bc-5c5d417.md` §R1 for the resolution recipe ("resolve `#e` ids → original-event authors → `author_write_relays`").
- Reference `kernel/outbox.rs::author_write_relays` (lands `5c5d417`) as the reuse target.
- Required artefact: integration test in `nmp-testing` proving a thread hydration query for a known-author reply fans out to that author's NIP-65 write relays, not bootstrap.
- Hard-cap: 300 LOC.

---

## 6 — One-line resume summary

**Open tasks: 12** (T114b, T116, T117 in-flight, T118, T119, T120, T121, T122, T123, T100 unblocked, T112 unknown, T59 deferred).
**Top-5 critical-path order:** (1) verify T117 → (2) T114b → (3) T117-resolve → (4) T116 reconnect-replay → (5) T120-G4-slice backoff-policy.
**Open PDs: 4** (PD-005 narrowed, PD-018 dormant ack, PD-020 ack, PD-021 line-11 closer-by-T114b + line-348 ack).
**Most urgent dispatch:** **T114b** (M10.5 §G-S2 closer; PD-021 line-11 closeable on landing).
