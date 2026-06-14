---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 10138-10233
agent_attribution: aa4f32b4bf798bb50
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Adversarial code review of PR #1417 (feed change-signal) with pre-registered rulings on determinism, flag divergence, epoch-reset coverage, and test/debt status; verdict REQUEST-CHANGES due to Reset-freeze bug and file-size CI failure"
captured_at: 2026-06-14T15:00:36Z
---

Adversarial code review of PR #1417 (feed change-signal) with pre-registered rulings on determinism, flag divergence, epoch-reset coverage, and test/debt status; verdict REQUEST-CHANGES due to Reset-freeze bug and file-size CI failure

---

User: [Agent task result: Agent "Opus review R6-S1 PR1417" completed]
The voice server is not running (404), so the notification could not be delivered. Proceeding with the written verdict.

---

# Adversarial Review — PR #1417 (ADR-0055 R6-S1, trap-proof feed change-signal)

## VERDICT: REQUEST-CHANGES

Two independent blockers: (1) a concrete, structural **frozen/blank-feed path on `nmp_app_reset` / `resetAndRestart()`** that the "trap-proof by construction" claim does not actually cover, and (2) **red CI** — the file-size gate fails. The make-or-break determinism (Ruling #1) and flag-divergence (Ruling #2) checks both PASS. The freeze is in Ruling #3.

---

## RULING #1 — Is the frozen feed structurally impossible? Is the encode deterministic? — PASS

The change-signal is **exact byte-equality** (`*last == payload`, memcmp of the retained `Vec<u8>` at `emission_state.rs:464-466`), not the `hash64` the #1415 design specified. This is strictly *safer* than the design (zero collision risk) — a correct, defensible deviation, documented at `emission_state.rs:55-60`. Because the signal IS the literal wire bytes, Group A test completeness is a regression guard, not the correctness floor — any rendered-byte change is caught even if untested. Confirmed.

**Determinism — fully established end-to-end (the make-or-break):**
- Roots: explicitly sorted by `(created_at, id)` with a total tiebreaker — `engine/mod.rs:318` (`rt.cmp(lt).then_with(|| rid.cmp(lid))`). Deterministic.
- `roots` / `attributions` are `BoundedMessageMap`, which is built on **`indexmap::IndexMap`** (`bounded.rs:65,77`) → iterates in **insertion order, not hash order**. `attributions...values()` (`engine/mod.rs:339`) is therefore stable across idle ticks for a fixed insertion sequence. No HashMap/HashSet iteration anywhere in the feed snapshot path.
- Render trees use `BTreeMap` (`timeline_projection.rs:204-205`) — sorted-key iteration. `mentioned_pubkeys` is a `Vec<String>`.
- FlatBuffers encode uses plain `create_string` (no `create_shared_string` dedup, no `force_defaults`) — `typed_wire.rs` + `encode.rs:292-295`. Builder layout is a pure function of the fixed call sequence.
- **No relative-time / wall-clock field at encode:** `created_at`, `reply_created_at` are raw Unix-second values copied from the events (`typed_wire.rs` `reply_created_at: attr.reply_created_at`), never an "x minutes ago" computed at encode. So idle ticks do NOT false-resend on a moving clock — the omit actually fires. Confirmed against the STEP-1 "40 idle ticks byte-identical" measurement; the code structure corroborates it.

A missed-bump on a *content/membership/order/profile* change inside the fixed default window is indeed structurally impossible. The "trap-proof" claim holds **on the content axis**. It does NOT hold on the lifecycle/session axis — see Ruling #3.

---

## RULING #2 — Two-flags-for-one-fact divergence — PASS (with an architectural nit)

The new `incremental_apply_handle: Arc<AtomicBool>` (`nmp-ffi/src/lib.rs:238`) is a mirror of `SnapshotRegistry::incremental_apply_enabled` (`snapshot_registry.rs:201`). **They cannot diverge:**
- Both are set inside `NmpApp::declare_incremental_apply` (`lib.rs:1583-1605` as patched): the registry flag via `registry.declare_incremental_apply()`, the Arc via `store(true)` **gated on `result.is_ok()`**.
- `SnapshotRegistry::declare_incremental_apply` returns `()` — it cannot fail (`incremental_apply.rs:28-36`). The only failure is the `.lock()` poison → `RegistryUnavailable`, in which case neither flag is set (both stay false). `AlreadyStarted` returns before touching either. Idempotent re-call: registry no-ops, Arc stores true again. All paths keep them equal.

**The deadlock justification is genuine.** `run_typed_projections` takes `slot.lock()` and calls `registry.run_typed()` *with the lock held* (`kernel_access.rs:64-67`), which invokes the producer closure. A re-lock of the same non-reentrant `Mutex` from inside the closure would deadlock. So the lock-free `Arc<AtomicBool>` mirror is a legitimate, necessary read path — not gratuitous fragmentation.

**Nit (architecture, not blocking):** the cleaner non-fragmented design is to make `SnapshotRegistry::incremental_apply_enabled` itself an `Arc<AtomicBool>` read from both `make_update` and the closure, eliminating the mirror and the "two fields, one fact" smell entirely. Given the flags provably cannot diverge today, this is a recommendation, not a change-request.

---

## RULING #3 — Epoch-reset coverage — **FREEZE FOUND** (REQUEST-CHANGES)

**Pagination / `load_older`: not a freeze.** The typed sidecar always snapshots the fixed default window — `op_feed_defaults.rs:271` calls `engine_for_typed.snapshot(&nmp_feed::FeedRequest::default())`, ignoring `load_older` growth (acknowledged as a follow-up at the diff's :103-105). So `load_older` never reaches the typed closure; test A.8 (`a8_load_older_window_growth_emits`) does not reflect a real production path for this sidecar (test-fidelity nit). No stale-rev edge here.

**Account-switch: correct.** The kernel bumps `bump_epoch()` on active-account change (`identity_state.rs:240,270`) → frame `snapshot_epoch` changes → host `ProjectionCache.merge` resets its cache (`ProjectionCache.generated.swift:162-167`). The producer independently bumps `emission_epoch` via `follow_set.on_change` (`op_feed_defaults.rs:294-...`, diff :181 `fetch_add`), resetting `FeedEmissionState` → fresh `rev=1` baseline. Host cache is empty (`cached.rev=0`) so `rev=1 > 0` is accepted. First-frame-after-advertise and first-frame-after-epoch-reset both emit (`last_emitted=None` ⇒ emit; `emission_state.rs:456-460`). These are correct.

**The freeze — `nmp_app_reset` / `resetAndRestart()`:**

The producer's omit-memory (`FeedEmissionState`) lives **outside the kernel** in the captured closure, and survives `ActorCommand::Reset`; the host resets its cache on a signal (`session_id`) that the producer's epoch does not track. Concrete sequence:

1. `resetAndRestart()` (`ios/.../KernelModel.swift:355`) → `kernel.reset()` → `nmp_app_reset` (`nmp-ffi/src/lib.rs:2885-2889`) → `ActorCommand::Reset`.
2. The Reset handler **rebuilds the kernel** (`actor/dispatch.rs:1315` `*ctx.kernel = Kernel::with_storage_path_and_account_slot(...)`) and calls `ctx.kernel.start()` at the arm's end (`dispatch.rs` ~:1465), which **re-stamps `started_unix_ms = now_ms()`** (`kernel/mod.rs:2443-2446`). Since `session_id = started_unix_ms` (`projection_rev/kernel_impl.rs:15-17`), the **`session_id` changes**.
3. Host receives the new `session_id` → `ProjectionCache.merge` does `cache.removeAll()` (`ProjectionCache.generated.swift:162-167`). **The feed cache entry is gone.**
4. The Reset handler **preserves** the snapshot-projection handle and observer handles (`dispatch.rs` ~:1387 `set_snapshot_projection_handle`). The **op_feed engine `Arc` is preserved and still holds the pre-Reset roots** — Reset rebuilt the *kernel struct*, not the externally-held engine. The **`FeedEmissionState` is likewise preserved**, still holding `last_emitted = Some(pre_reset_bytes)`, and its `emission_epoch` was **NOT** bumped (Reset clears the account slot to `None` directly at `dispatch.rs:1331`; it does not go through the `follow_set.on_change` account-switch path that bumps `emission_epoch`).
5. Next typed tick: producer snapshots the still-populated engine → encodes → **bytes identical to `last_emitted`** → `should_emit` returns `None` → **OMIT**.
6. Host cache empty + omit ⇒ **no feed entry ⇒ BLANK / FROZEN feed** until the next genuine content change arrives from the network.

**Why "trap-proof by construction" misses this:** the guarantee covers byte-changes for fixed lifecycle state. But the host resets its cache on **`session_id`** (D4), whereas the producer's only reset trigger is an **account-switch `emission_epoch`**. These are different signals. The `FeedEmissionState.should_emit(payload, epoch)` is **blind to `session_id`** — the diff itself admits this at `op_feed_defaults.rs:84-91` ("the typed closure signature has no parameters — it cannot receive the frame epoch"), then substitutes an account-switch-only internal epoch. That substitution does not cover the Reset/session axis.

**Contrast that proves the gap:** Tier-2 kernel built-ins are reset-safe *automatically* because their omit-memory (`ProjectionRevTracker.last_emitted`, `projection_rev/mod.rs:207`) lives **inside the kernel** and is destroyed when Reset rebuilds the kernel (`dispatch.rs:1315`) → fresh tracker → forced full baseline. The feed's omit-memory lives outside the kernel and survives. The feed is the one omitting projection whose state does NOT die with the kernel.

**Residual uncertainty (does not clear the blocker):** the freeze is averted *only if* `ActiveFollowSet::on_change` happens to re-fire during the Reset rebuild with the cleared account (`A != None`), which would bump `emission_epoch`. That is timing-dependent observer-refire coupling across two mechanisms — exactly the fragility M1 was chosen to eliminate. It must not be left to chance, and there is **no test** for it (the suite drives the `epoch` parameter directly; `HostCacheSim.apply` is fed the *same* epoch as `should_emit`, so it structurally cannot reproduce a host-resets-but-producer-doesn't divergence — strawman on this axis; `emission_state_tests.rs` has zero `session`/Reset coverage).

**Required fix (report only — not applied):** key the producer's reset on the **same** signal the host resets on. Either (a) feed the frame `session_id`/`snapshot_epoch` into `should_emit` instead of the account-switch-only internal epoch (close the staged-removal that hardcodes `FeedRequest::default()` and the no-param closure), or (b) reset `FeedEmissionState` from the `ActorCommand::Reset` path itself (a Reset-survival reset of the emission state, mirroring how the kernel tracker dies on Reset), or (c) bump `emission_epoch` unconditionally whenever the kernel epoch/session changes. Then add a Group C test: host-cache-reset-on-session-change while `FeedEmissionState` is preserved with `last_emitted=Some` ⇒ must emit a baseline.

---

## RULING #4 — Tests, build, debt — MIXED (one CI blocker)

**Test runs (PR worktree `/private/tmp/wt-r6s1-feed-signal` @ 99430457d):**
- `cargo test -p nmp-nip01`: **PASS** — 152 lib (incl. all 25 `emission_state` A/B/C tests) + 5+5+5 integration; 0 failed.
- `cargo test -p nmp-defaults`: **PASS** — all suites green (23/10/2/4/4/2/3/9/1/2/4; 0 failed).
- `cargo test -p nmp-testing --test doctrine_lint_smoke`: **PASS** — 60 passed, 0 failed.
- Production files (`op_feed_defaults.rs`, `emission_state.rs`) are warning-clean; the unused-import warnings are pre-existing test-helper noise unrelated to this PR.

**File-size gate: FAIL (CI blocker).** `./.githooks/check-file-size.sh --from-ref origin/master --to-ref HEAD --baseline-ref origin/master` → **real exit code 1** (the earlier `0` was the `tail` pipe's exit, not the script's). Cause: `crates/nmp-ffi/src/lib.rs` grew **2976 → 3004 LOC** (HARD-cap baseline expansion) from the added `incremental_apply_handle` field + method + docs. `gh pr checks 1417` confirms **"File-size check" = fail** on both runs. Per the repo's own rule (file-size-gate memory + AGENTS.md): **split the god-file, never bump the baseline.** `lib.rs` at ~3000 LOC should not absorb this; the new accessor + field can move to a sibling module (the same `incremental_apply.rs` extraction precedent the registry already uses).

**"AI architecture signoff: fail" is NOT a code finding** — it is an infra failure (OpenAI 401 invalid API key in CI), per its log. Disregard for this PR's correctness.

**`HostCacheSim` fidelity vs real `ProjectionCache.generated.swift`:** faithful on the axes it models — omit==retain (`merge` omitted-key no-op vs sim `None` branch), reorder guard (`incomingRev <= cached.rev` skip, line 184, vs sim `incoming_rev > cached_rev`), epoch reset (line 162-167 `cache.removeAll()` vs sim epoch branch). **But it does NOT model the `session_id` axis** (Swift resets on `sessionId != appliedSession` OR `snapshotEpoch != appliedEpoch`, line 162; the sim only takes one `epoch`), nor `sessionId == 0` pass-through (line 156). That omission is precisely what hides the Ruling #3 freeze — making it a strawman on the session axis.

**Group A subtle-input coverage:** A.7 (profile rename in a visible card) and A.5/A.6 (attribution add/remove) are present and pass — good regression guards. A.8 (`load_older`) tests a path the typed sidecar never exercises in production (fixed default window) — harmless but non-representative.

**Engine untouched / no debt:** `git diff` shows **zero** changes to `crates/nmp-feed/src/root_indexed/engine/mod.rs` — the riskiest code is untouched, as designed. No FlatBuffers schema change, no codegen drift (all drift gates pass). Capability-OFF is byte-identical to today (test C.4/C.7 + `emission_state.rs:474-482` always-emit branch).

---

## Summary of required changes before merge
1. **Blocker (freeze):** Reset/session-change can blank the feed because `FeedEmissionState` survives `ActorCommand::Reset` while the host resets its cache on `session_id`. Key the producer's reset on the host's actual reset signal (session/snapshot_epoch), or reset emission state on the Reset path. Add the missing Group-C test (host resets while emission state preserved ⇒ must baseline) and extend `HostCacheSim` to model the `session_id` axis.
2. **Blocker (CI):** file-size gate red — `nmp-ffi/src/lib.rs` 2976→3004. Split into a sibling module; do not bump the baseline.
3. **Nit (architecture):** collapse the two incremental-apply flags into one `Arc<AtomicBool>` source of truth (they cannot diverge today, but it removes the smell).
4. **Nit (test fidelity):** A.8 `load_older` tests a path the fixed-window typed sidecar does not take.

Key files: `crates/nmp-nip01/src/op_feed/emission_state.rs`, `crates/nmp-nip01/src/op_feed/emission_state_tests.rs`, `crates/nmp-defaults/src/op_feed_defaults.rs`, `crates/nmp-ffi/src/lib.rs`, `crates/nmp-core/src/actor/dispatch.rs` (Reset arm ~:1260-1465), `crates/nmp-core/src/kernel/identity_state.rs:240`, `ios/Chirp/Chirp/Bridge/Generated/ProjectionCache.generated.swift:162-167`, `ios/Chirp/Chirp/Bridge/KernelModel.swift:355`.

I did not merge and pushed no fixes.
