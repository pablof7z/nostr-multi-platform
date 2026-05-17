Reading additional input from stdin...
2026-05-17T22:30:02.745931Z ERROR codex_core::session: failed to load skill /Users/pablofernandez/.agents/skills/voice-capture-sheet/SKILL.md: invalid YAML: mapping values are not allowed in this context at line 2 column 116
OpenAI Codex v0.129.0 (research preview)
--------
workdir: /Users/pablofernandez/Work/nostr-multi-platform
model: gpt-5.5
provider: openai
approval: never
sandbox: workspace-write [workdir, /tmp, $TMPDIR, /Users/pablofernandez/.codex/memories]
reasoning effort: xhigh
reasoning summaries: none
session id: 019e380f-88fc-71a3-bae1-74d179107e17
--------
user
You are reviewing merge fcf8b431b8d86f1801fef2fad26e81fbf56506f4 on master in the nostr-multi-platform repo. Doctrine D0–D5 (kernel never grows app nouns; best-effort rendering; reactivity contract ≤60 Hz/view; errors never cross FFI; one writer per fact; capabilities report don't decide). File-size: 300 LOC soft, 500 hard.

Session goal: complete v1 with zero technical debt before rebuilding /Users/pablofernandez/src/podcast as M11.

Merge:

=== diff stat (HEAD~1..HEAD) ===
 docs/design/ffi-hardening.md                    |  314 ++++++
 docs/design/ffi-hardening/ci.md                 |  345 +++++++
 docs/design/ffi-hardening/gates.md              |  267 +++++
 docs/design/ffi-hardening/harness.md            |  410 ++++++++
 docs/design/ffi-hardening/scenarios.md          |  353 +++++++
 docs/perf/codex-reviews/2026-05-18-session-1.md | 1263 +++++++++++++++++++++++
 docs/perf/m10.5/debt-inventory.md               |   67 +-
 docs/plan.md                                    |    4 +-
 8 files changed, 2974 insertions(+), 49 deletions(-)

=== commit log ===
fcf8b43 docs: apply codex review fixes from session-1
Codex flagged 5 issues against the wave-1 merges:

1. debt-inventory.md prematurely claimed "production-ready" / "M10.5 Exit
   Criteria READY" from a read-only audit. Downgraded to "static-debt
   baseline established"; explicit list of remaining M10.5 empirical
   gates that must pass before exit.
2. Safety-comment additions to ffi.rs F1-F3 + relay_worker.rs F4 were
   marked "optional/recommended" — conflicts with the zero-shortcut
   posture. Promoted to must-fix M10.5 cleanup.
3. plan.md M10.5 zero-debt gate had an "ADR escape" carve-out that
   weakened the hard-zero. Removed: real escape is "it's a later
   milestone task, not a code marker".
4. plan.md M11 still referenced NIP-XX as a podcast NIP placeholder.
   Replaced with explicit "not a v1 deliverable unless a real published
   NIP is selected during M11 design; otherwise RSS + Podcast 2.0 + Nostr
   social overlay only".
5. (Tracked as follow-up tasks, not in this merge:) plan.md is 789 LOC >
   500 hard limit and debt-inventory.md is 434 > 300 soft. Splits queued.

Also records codex review output at docs/perf/codex-reviews/2026-05-18-session-1.md
(the "after every merge, ask codex" protocol established this session).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>

---

=== diff (truncated to first 8000 lines) ===
diff --git a/docs/design/ffi-hardening.md b/docs/design/ffi-hardening.md
new file mode 100644
index 0000000..914f352
--- /dev/null
+++ b/docs/design/ffi-hardening.md
@@ -0,0 +1,314 @@
+# Design: FFI hardening stress harness (M10.5)
+
+> **Status:** Draft for the M10.5 hard gate before M11.
+> **Authoring agent:** Architect (M10.5 design task #4).
+> **Companion audit:** [`docs/perf/m10.5/debt-inventory.md`](../perf/m10.5/debt-inventory.md).
+> **Reading order:** this file first (§1, §2, §7, §10), then the five sub-docs (§3, §4, §5, §6, §8, §9).
+
+This is the design spec for the M10.5 dedicated FFI-surface hardening pass. The
+goal is to prove — in measured numbers, not adjectives — that the current raw
+C FFI between `crates/nmp-core` and `ios/NmpStress` is rock-solid before a
+single line of the M11 podcast app is written.
+
+## 1. Goals and non-goals
+
+### 1.1 Goals
+
+1. **Empirical proof of FFI invariants.** Every claim made in the RMP bible
+   (`docs/aim.md` §2 commandments 1–10), every cardinal doctrine
+   (`docs/product-spec/overview-and-dx.md` §1.5 D0–D5), and every ownership
+   rule in the current FFI surface is verified by a concrete harness scenario
+   that produces a numeric pass/fail.
+2. **Exhaustive failure-mode coverage.** Ten named failure modes (§3 in
+   [`scenarios.md`](./ffi-hardening/scenarios.md)) cover the full shape of
+   FFI bugs a non-social-domain consumer would otherwise find the hard way.
+3. **Three independent runner paths.** A Rust-side stress binary, an XCUITest
+   target on iPhone 16 Pro simulator + iPhone 12 hardware, and a parallel
+   Sonnet-agent UI-script fleet. Each surfaces a different class of bug.
+4. **Zero pre-existing debt at gate close.** The `docs/perf/m10.5/debt-inventory.md`
+   audit must show 0 items in every "must-fix" row when M10.5 declares done.
+5. **Reproducible numeric exit gates.** No "fast enough", no "acceptable
+   memory growth". Every cell of the §5 table in
+   [`gates.md`](./ffi-hardening/gates.md) is a specific number.
+
+### 1.2 Non-goals
+
+- **Changing the FFI shape.** The current surface (13 raw `extern "C"`
+  functions in `crates/nmp-core/src/ffi.rs`, JSON-string update callback) is
+  what we harden. Migrating to UniFFI + per-app generated enums per
+  ADR-0010 is **M14**, not M10.5.
+- **New domain features.** No new view kinds, no new actions, no new
+  capabilities. M10.5 only fixes, instruments, measures.
+- **Cross-platform parity.** M10.5 covers iOS only. Android / desktop /
+  web FFI hardening is a separate milestone.
+- **Adversarial input fuzzing of relay traffic.** Malformed event payloads,
+  signature bombs, and kind:5 storms are out of scope for M10.5; the
+  harness here exercises the *FFI surface*, not the *protocol surface*.
+  Protocol hardening is M11/M12.
+
+## 2. FFI surface inventory
+
+The current FFI surface is **13 exported C symbols** in
+`crates/nmp-core/src/ffi.rs` (lines 44–268) plus **one callback type**
+(`UpdateCallback` at line 10). Every symbol below must have its
+ownership, lifetime, thread-safety, and null-handling rules documented
+and enforced by at least one harness scenario.
+
+| # | Symbol | Signature (C) | Ownership / lifetime / threading | Null handling |
+|---|---|---|---|---|
+| 1 | `nmp_app_new` | `void * nmp_app_new(void)` | Returns a `Box::into_raw(NmpApp)`. Caller owns. Spawns 2 OS threads (actor + listener) + N relay-worker threads on `start`. Pointer is `Send` because internal `Sender`/`Mutex` are `Send`; **callers must not share the same pointer across threads without external synchronization**. | n/a (returns) |
+| 2 | `nmp_app_free(*mut NmpApp)` | `void nmp_app_free(void *app)` | Reclaims the box, sends `Shutdown` to actor, joins actor + listener threads inside `Drop`. **Idempotent on null.** Caller must guarantee no other thread is mid-call into the same pointer when this is invoked. | Early-return if null (ffi.rs:74). |
+| 3 | `nmp_app_set_update_callback` | `void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback cb)` | Replaces the registered `(context, fn)` pair. The `*mut c_void` context is **stored as `usize`** (ffi.rs:13–16) and dereferenced inside the listener thread — caller owns the context lifetime and **must clear the callback to null before freeing the context**. Passing `cb=None` clears registration. | Returns silently if app null or mutex poisoned (ffi.rs:87–96). |
+| 4 | `nmp_app_start` | `void nmp_app_start(void *app, uint events_per_second, uint visible_limit, uint emit_hz)` | Fire-and-forget. `events_per_second` is currently ignored (`_events_per_second`); kept for ABI stability. `visible_limit` clamped to `[1, 500]`; 0 → default 80. `emit_hz` clamped to `[1, 12]`; 0 → default 4. Spawns relay workers on first call. | Silent no-op on null (ffi.rs:107–108). |
+| 5 | `nmp_app_configure` | same shape as `_start` | Same clamping. Re-tunes a running actor. | Silent no-op on null. |
+| 6 | `nmp_app_stop` | `void nmp_app_stop(void *app)` | Sends `Stop`; actor closes relay workers, keeps actor + listener threads alive. Caller may call `_start` again. | Silent no-op on null. |
+| 7 | `nmp_app_reset` | `void nmp_app_reset(void *app)` | Closes relays, replaces the in-actor `Kernel` instance, reopens relays if running. | Silent no-op on null. |
+| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
+| 9 | `nmp_app_open_thread(*mut, *const c_char)` | `void nmp_app_open_thread(void *app, const char *event_id)` | UTF-8, 64-char hex event id. `is_hex_id`-validated. Same silent-drop on bad input. | Silent no-op. |
+| 10 | `nmp_app_open_firehose_tag(*mut, *const c_char)` | `void nmp_app_open_firehose_tag(void *app, const char *tag)` | UTF-8 tag value. No hex validation. Empty → silent no-op. | Silent no-op. |
+| 11 | `nmp_app_claim_profile(*mut, *const c_char, *const c_char)` | `void nmp_app_claim_profile(void *app, const char *pubkey, const char *consumer_id)` | Hex-pubkey-validated. `consumer_id` is an opaque caller-chosen string (used by the kernel refcount table). Two strings, two lifetime contracts: both must be valid for the duration of the call only — the kernel `String`-copies them. | Silent no-op on any null or invalid. |
+| 12 | `nmp_app_release_profile(*mut, *const c_char, *const c_char)` | mirror of `_claim_profile` | Same. **Pair invariant:** every `claim_profile(pk, id)` must be matched by exactly one `release_profile(pk, id)`; an unmatched `release` is silently dropped by the kernel refcount logic. | Silent no-op. |
+| 13 | `nmp_app_close_author(*mut, *const c_char)` | `void nmp_app_close_author(void *app, const char *pubkey)` | Closes the named author view. Different from `release_profile` — closes the *view*, not a refcounted profile claim. | Silent no-op. |
+| 14 | `nmp_app_close_thread(*mut, *const c_char)` | `void nmp_app_close_thread(void *app, const char *event_id)` | Closes the named thread view. | Silent no-op. |
+
+### 2.1 Callback contract (`UpdateCallback`)
+
+```rust
+type UpdateCallback = extern "C" fn(*mut c_void, *const c_char);
+```
+
+- Invoked **from the dedicated listener OS thread** (ffi.rs:52–62), never
+  from the actor thread, never from a relay-worker thread.
+- The `*const c_char` payload is a JSON-serialized `KernelUpdate`. The
+  `CString` backing it is allocated in the listener thread and dropped
+  **at the end of the callback invocation** — the pointer dangles after
+  the callback returns. Callers must `String::from(cString)` (or
+  equivalent) inside the callback if they need to retain the data.
+- The `*mut c_void` context is whatever was passed to
+  `nmp_app_set_update_callback`. Per ffi.rs:13–16 it is round-tripped via
+  `as usize` to satisfy `Send` for the `Arc<Mutex<...>>` registration
+  slot. Callers must keep the underlying object alive for as long as the
+  callback is registered.
+- The callback **must not call back into any `nmp_app_*` function on the
+  listener thread synchronously** unless the caller has accepted the
+  reentrancy contract specified by scenario §3.5.
+- The callback **must not block the listener thread**; doing so will back
+  up the `update_rx` channel and stall every subsequent emit (which is
+  bounded only by available memory in the unbounded `mpsc::channel`).
+
+### 2.2 Threading topology (current)
+
+```
+                      ┌──────────────────────────┐
+caller thread ──────► │ nmp_app_*() FFI entry    │
+                      └────────┬─────────────────┘
+                               ▼   (mpsc::Sender<ActorCommand>)
+                      ┌──────────────────────────┐
+                      │ Actor OS thread          │  ◄─── (mpsc) ── RelayWorker N
+                      │ - owns Kernel state      │
+                      │ - bridges commands+relays│
+                      └────────┬─────────────────┘
+                               ▼   (mpsc::Sender<String> JSON)
+                      ┌──────────────────────────┐
+                      │ Listener OS thread       │
+                      │ - invokes UpdateCallback │
+                      └────────┬─────────────────┘
+                               ▼   (extern "C" fn, *const c_char)
+                      ┌──────────────────────────┐
+                      │ caller-supplied callback │
+                      │ (Swift hops to MainActor)│
+                      └──────────────────────────┘
+```
+
+Five thread classes total per `NmpApp` instance: caller, actor, listener,
+N×relay-worker, and tokio-pool (currently unused for the FFI surface; relay
+workers use `tungstenite` synchronously). Every harness scenario in §3
+explicitly names which thread it stresses.
+
+## 3. Failure modes
+
+Ten scenarios — full table with assertions and numeric thresholds is in
+[`ffi-hardening/scenarios.md`](./ffi-hardening/scenarios.md). Summary:
+
+| # | Scenario | Threading concern | Primary doctrine tested |
+|---|---|---|---|
+| S1 | Mount/unmount churn | actor recv + refcount | D5 (snapshot bounded), bible #5 |
+| S2 | Dispatch flood | mpsc backpressure | bible #3 (fire-and-forget) |
+| S3 | Snapshot pressure | listener serialization | bible #9 (≤60 Hz), D5 |
+| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
+| S5 | Reentrancy | callback re-enters FFI | bible #3, deadlock-free |
+| S6 | Capability lifecycle storms | start/stop/restart | bible #7 (idempotent) |
+| S7 | Error-shape exhaustion | every invalid input path | D3 (no errors cross FFI) |
+| S8 | Subscription planner DOS | OpenView/CloseView storm | D2 (≤60 Hz/view), D5 |
+| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
+| S10 | Long suspend simulation | background → foreground | bible #1 (monotonic rev) |
+
+## 4. Harness architecture
+
+Full sketches in [`ffi-hardening/harness.md`](./ffi-hardening/harness.md).
+Three runners, one shared report format:
+
+1. **`crates/nmp-testing/bin/ffi-stress/`** — Rust binary. Drives the FFI
+   surface directly via the same `nmp_app_*` C symbols Swift uses,
+   linking `nmp-core` as `staticlib`. Models the iOS main-thread loop
+   with a single-threaded executor that imposes a budget.
+2. **`ios/NmpStress/StressUITests/`** — XCUITest target. One test class
+   per scenario, drives `NmpStress.app` end-to-end, asserts UI invariants
+   and captures `XCTPerformanceMetric` (CPU, memory, hitches).
+3. **Sonnet-agent runner** — a shell wrapper at
+   `crates/nmp-testing/bin/ffi-stress/sonnet-runner.sh` that orchestrates
+   N parallel `claude` agent processes, each scripting the simulator via
+   the `mcp__xcode__*` tool family. Output: per-agent screenshot trail,
+   assertion log, timing.
+
+## 5. Numeric exit gates
+
+The complete (scenario × metric × threshold) table is in
+[`ffi-hardening/gates.md`](./ffi-hardening/gates.md). Sample row:
+
+> S1 `mount_unmount_churn` — leaked bytes after 10 min = **0**;
+> dispatch round-trip p99 = **≤ 1 ms** (Swift → Rust channel send);
+> `OpenView/CloseView` ratio = **≥ 0.95** (every claim has a release);
+> Instruments retained-by-cycle leaks = **0**.
+
+No row in §5 contains the word "acceptable" or the phrase "fast enough".
+Every cell is a number with units.
+
+## 6. Run + reporting protocol
+
+Full recipes in [`ffi-hardening/ci.md`](./ffi-hardening/ci.md). Headline:
+
+```
+just stress          # local simulator, fast scenarios (S1, S2, S3, S5, S7, S8)
+just stress-device   # iPhone 12 hardware, full ten scenarios
+just stress-sonnet   # parallel Sonnet-agent UI fleet
+```
+
+Output bundle layout:
+
+```
+docs/perf/m10.5/
+├── <scenario>/
+│   ├── report.md            # human-readable summary, per-metric pass/fail
+│   ├── metrics.json         # machine-readable, schema-versioned
+│   ├── instruments.trace/   # Instruments capture (Leaks, Allocations)
+│   └── screenshots/         # PNG trail from XCUITest + Sonnet-agent
+├── debt-inventory.md        # the companion audit (already lives here)
+├── doctrine-review.md       # D0–D5 sign-off (M10.5 exit-gate artifact)
+└── iphone12-baseline.md     # firehose-bench rerun on real hardware
+```
+
+## 7. Pre-existing debt
+
+A separate auditor agent has produced
+[`docs/perf/m10.5/debt-inventory.md`](../perf/m10.5/debt-inventory.md)
+(442 LOC) covering 19 Rust files + 9 Swift files (6,559 LOC scanned)
+with **0 critical, 0 high, 6 medium/low** findings. **Every item listed
+as "Must-Fix" in that file must be 0 at the end of M10.5.**
+
+As of 2026-05-18 the must-fix list is:
+
+| Ref | File:line | Action | Effort |
+|---|---|---|---|
+| F1 | `crates/nmp-core/src/ffi.rs:75` | Add `// safe: ...` doc on unsafe `Box::from_raw` | 5 min |
+| F2 | `crates/nmp-core/src/ffi.rs:275` | Same on unsafe `&*app` | 5 min |
+| F3 | `crates/nmp-core/src/ffi.rs:284` | Same on unsafe `CStr::from_ptr` | 5 min |
+| F4 | `crates/nmp-core/src/relay_worker.rs:242` | Comment `#[allow(unreachable_patterns)]` rationale | 2 min |
+| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
+
+### 7.1 Re-grep gate
+
+The M10.5 done declaration is **gated on a re-run of the audit grep
+yielding zero results**. The exact command (captured 2026-05-18, 0 hits):
+
+```bash
+grep -rEn '(TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit)' \
+  crates/nmp-core/src crates/nmp-testing/src \
+  ios/NmpStress/NmpStress \
+  | grep -v target | grep -v DerivedData
+```
+
+If the re-run at gate close yields any hits, the new items are
+triaged into either *resolve before close* or *deferred with ADR + GH
+issue*. Net unresolved must be 0.
+
+### 7.2 D3 structural gap (named, not hidden)
+
+The harness will exercise every typed FFI input path that fails
+validation (§3 scenario S7). The current FFI is **silently no-op** on
+bad input — `nmp_app_open_author(app, NULL)`, an empty string, or a
+non-hex string all early-return from ffi.rs without any signal to the
+caller and without setting any state field. This is **D3-compliant in
+the strict sense** (no error crosses FFI as a value) **but
+D3-incomplete in the user-visible sense** (no toast surfaces in
+`AppState`).
+
+The debt-inventory's D3 audit (lines 317–334) concludes the same:
+errors-as-data crosses FFI via `RelayStatus.last_error` and
+`RelayStatus.last_notice`, which is correct, but **app-action error
+paths (invalid input dropped) have no equivalent surface**. M10.5
+adds a `toast: Option<String>` field to the JSON update payload
+(placed alongside `logs: Vec<String>` as a sibling of `metrics` in the
+`KernelUpdate` serialization — see `crates/nmp-core/src/kernel/update.rs`)
+and populates it from S7's validation failure paths. The schema change
+is additive (older Swift readers ignore an unknown field) so this is not
+a breaking FFI change.
+
+This is intentionally surfaced in the design doc, not papered over.
+The M14 UniFFI migration moves the surface to typed `Result`-shaped
+returns; M10.5 ships the interim toast-field bridge.
+
+## 8. Doctrine review checklist
+
+Full D0–D5 line-item-to-scenario mapping in
+[`ffi-hardening/gates.md`](./ffi-hardening/gates.md) §8. Headline:
+
+| Doctrine | Proven by |
+|---|---|
+| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
+| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
+| **D2** ≤60Hz/view, working-set bound | S2, S3, S8 — emit-rate cap, planner dedup |
+| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
+| **D4** one writer per fact | S1, S5 — refcount only mutated on actor thread; reentrancy under same single-writer rule |
+| **D5** snapshots bounded by what's open | S1 (refcount drives eviction) + S3 (full-state size scales with open views, not store) |
+
+## 9. CI integration
+
+Three tiers, defined in [`ffi-hardening/ci.md`](./ffi-hardening/ci.md):
+
+| Tier | Frequency | Scenarios | Where |
+|---|---|---|---|
+| **Pre-merge** | every PR | S1 (short), S2, S3 (10k events), S5, S7, S8 | GH Actions `macos-14` runner, iPhone 16 Pro sim |
+| **Nightly** | daily | All ten, S1 full 10-min, S4 250 ms stalls × 60, S9 100 flap, S10 60s suspend | Mac mini self-hosted runner, iPhone 12 device |
+| **On-demand** | release candidates | S1 8-hour soak, S2 1 M dispatch, S9 24-hour flap | Lab device, manual sign-off |
+
+## 10. Open questions (for ADR after review)
+
+1. **Toast field schema.** Is `toast: Option<String>` enough, or do we
+   want `toast: Option<{ id: String, severity: Info|Warn|Error, message: String, source: String }>`? The latter is more useful but
+   collides with ADR-0010's plan to migrate this entire surface to
+   typed `Result` via UniFFI in M14. Recommend: keep it scalar for
+   M10.5, revisit in ADR-0011 when M14 lands.
+2. **Sonnet-agent determinism.** Parallel Sonnet agents producing
+   non-deterministic taps means flaky CI. Should the Sonnet runner be
+   nightly-only (no pre-merge gating), or do we record + replay agent
+   traces the way `firehose-bench` does for relay frames? Recommend:
+   record + replay, but defer the replay infrastructure to M11.5.
+3. **iPhone 12 vs iPhone 16 Pro baselines.** The exit gate quotes one
+   set of numbers; iPhone 12 is roughly 2× slower than 16 Pro on
+   single-thread workloads. Should gate values be device-tagged, or do
+   we publish two tables? Recommend: one table, scaled by a
+   per-device coefficient documented in
+   `docs/perf/m10.5/iphone12-baseline.md`.
+4. **Reentrancy from inside the reconciler callback.** Bible #3 says
+   `dispatch()` is fire-and-forget — but is dispatch-from-within-a-callback
+   allowed at all? Scenario S5 asserts ordering and deadlock-freedom
+   under the assumption that yes; should that be explicitly documented
+   as a supported pattern, or actively discouraged? Recommend: document
+   as supported (Swift will inevitably do it via Combine pipelines);
+   the actor's mpsc Sender is `Send + Sync` and the test proves it
+   works.
+
+---
+
+**Next:** open the five sub-docs in order: scenarios, harness, gates, ci.
diff --git a/docs/design/ffi-hardening/ci.md b/docs/design/ffi-hardening/ci.md
new file mode 100644
index 0000000..30c9825
--- /dev/null
+++ b/docs/design/ffi-hardening/ci.md
@@ -0,0 +1,345 @@
+# FFI hardening — run + reporting protocol (§6) and CI integration (§9)
+
+Two layers in this file:
+1. **§R** local run protocol — `just stress` recipes, output bundle layout.
+2. **§C** CI integration — pre-merge / nightly / on-demand tiers.
+
+---
+
+## §R. Run + reporting protocol
+
+### R.1 `justfile` recipes
+
+Append to `/Users/pablofernandez/Work/nostr-multi-platform/justfile`:
+
+```make
+# === FFI hardening (M10.5) ===
+
+# Pre-merge fast suite: simulator only, ~10 min wall-time
+stress:
+    cargo build --release -p nmp-testing --bin ffi-stress
+    just stress-rust-fast
+    just stress-xcui-fast
+
+stress-rust-fast:
+    cargo run --release -p nmp-testing --bin ffi-stress -- \
+        mount-unmount --duration 60s --fail-on-gate
+    cargo run --release -p nmp-testing --bin ffi-stress -- \
+        dispatch-flood --duration 30s --threads 4 --fail-on-gate
+    cargo run --release -p nmp-testing --bin ffi-stress -- \
+        snapshot-pressure --duration 30s --fail-on-gate
+    cargo run --release -p nmp-testing --bin ffi-stress -- \
+        reentrancy --duration 30s --fail-on-gate
+    cargo run --release -p nmp-testing --bin ffi-stress -- \
+        error-exhaustion --fail-on-gate
+    cargo run --release -p nmp-testing --bin ffi-stress -- \
+        planner-dos --duration 60s --fail-on-gate
+
+stress-xcui-fast: build-ios
+    xcodebuild test \
+        -project ios/NmpStress/NmpStress.xcodeproj \
+        -scheme StressUITests-Fast \
+        -destination 'platform=iOS Simulator,name=iPhone 16 Pro,OS=latest' \
+        -derivedDataPath ios/DerivedData \
+        -resultBundlePath docs/perf/m10.5/xcui-fast.xcresult
+
+# Nightly full suite: all scenarios at full duration, includes device
+stress-full:
+    just stress-rust-full
+    just stress-xcui-full
+    just stress-device
+
+stress-rust-full:
+    cargo run --release -p nmp-testing --bin ffi-stress -- \
+        all --duration 10m --instruments --fail-on-gate
+
+stress-xcui-full: build-ios
+    xcodebuild test \
+        -project ios/NmpStress/NmpStress.xcodeproj \
+        -scheme StressUITests-Full \
+        -destination 'platform=iOS Simulator,name=iPhone 16 Pro,OS=latest' \
+        -derivedDataPath ios/DerivedData \
+        -resultBundlePath docs/perf/m10.5/xcui-full.xcresult
+
+stress-device:
+    cargo build --release -p nmp-core --target aarch64-apple-ios
+    xcodebuild test \
+        -project ios/NmpStress/NmpStress.xcodeproj \
+        -scheme StressUITests-Full \
+        -destination 'platform=iOS,name=iPhone 12' \
+        -derivedDataPath ios/DerivedData \
+        -resultBundlePath docs/perf/m10.5/xcui-device.xcresult
+
+stress-sonnet:
+    just build-ios
+    crates/nmp-testing/bin/ffi-stress/sonnet-runner.sh \
+        default 4 5
+
+# Aggregate all scenario JSON into one markdown summary
+stress-report:
+    python3 scripts/stress-aggregate.py docs/perf/m10.5/ \
+        > docs/perf/m10.5/m10.5-summary.md
+
+# The doctrine-review.md gate: read every scenario report, assert PASS
+stress-gate:
+    python3 scripts/stress-gate.py docs/perf/m10.5/ \
+        --require-doctrines D0,D1,D2,D3,D4,D5 \
+        --require-debt-inventory-clean \
+        --require-grep-clean
+```
+
+### R.2 Output bundle layout
+
+Every run produces (or appends to) a tree rooted at
+`docs/perf/m10.5/`:
+
+```
+docs/perf/m10.5/
+├── README.md                       # top-level index, auto-generated
+├── debt-inventory.md               # produced by debt-auditor (exists 2026-05-18)
+├── doctrine-review.md              # M10.5 sign-off, produced at gate close
+├── iphone12-baseline.md            # firehose-bench rerun on real hardware
+├── m10.5-summary.md                # aggregate of all scenarios, auto-generated
+├── S1/
+│   ├── report.md                   # human-readable, per-metric pass/fail
+│   ├── metrics.json                # schema in harness.md §4
+│   ├── instruments.trace/          # only if --instruments was passed
+│   │   └── ... (xctrace bundle)
+│   ├── screenshots/                # XCUITest screenshots, time-ordered
+│   │   ├── 0001-start.png
+│   │   ├── 0002-mid-churn-1min.png
+│   │   └── ...
+│   └── raw/
+│       ├── ffi-stress.log          # stdout from Rust harness
+│       └── xctrace.log             # stdout from xctrace
+├── S2/ ...
+├── S3/ ...
+├── ... (one dir per scenario through S10)
+└── sonnet/
+    └── <scenario>-<unix-ts>/       # per-run, multiple permitted
+        ├── agent-1/
+        │   ├── transcript.json
+        │   ├── assertions.log
+        │   └── screenshots/
+        ├── agent-2/ ...
+        └── aggregate-report.md
+```
+
+### R.3 Report content (`<scenario>/report.md`)
+
+Generated by the harness; example for S1:
+
+```markdown
+# S1 — Mount/unmount churn — 2026-06-XX
+
+| Gate | Threshold | Measured | Result |
+|---|---|---|---|
+| Wall duration | == 600 s ± 5 s | 600.3 s | ✅ |
+| Cycles completed | >= 540,000 | 591,217 | ✅ |
+| Final per-pubkey refcount | == 0 | 0 | ✅ |
+| RSS growth | <= 5 MiB | 1.8 MiB | ✅ |
+| Instruments retained-cycle leaks | == 0 | 0 | ✅ |
+| Counting allocator slope (post-warmup) | <= 0 bytes/sec | -240 bytes/sec | ✅ |
+
+**Overall:** PASS (6/6 gates green)
+
+## Observations
+- First 30 s: allocator slope +3.2 KiB/sec during warmup; expected.
+- After cycle 100 000: slope drops to -240 bytes/sec (gradual reclamation of fragmented allocations).
+- Instruments-Leaks bundle: see ./instruments.trace.
+
+## Limitations
+- Sim only; iPhone 12 device rerun pending (S1 on device: see docs/perf/m10.5/S1-iphone12-2026-06-XX/report.md).
+```
+
+### R.4 The gate script
+
+`scripts/stress-gate.py` is the source of truth for "is M10.5
+done?". Pseudocode:
+
+```python
+def gate(perf_dir, require_doctrines, require_debt_clean, require_grep_clean):
+    fails = []
+    for scenario in ["S1", "S2", "S3", "S5", "S6", "S7", "S8", "S9"]:
+        # S4 is XCUITest-only; S10 is conditional on M3+M4
+        m = load(f"{perf_dir}/{scenario}/metrics.json")
+        if not m["passed"]:
+            fails.append(f"{scenario}: {failed_gates(m)}")
+
+    if require_doctrines:
+        d = load(f"{perf_dir}/doctrine-review.md")
+        for code in require_doctrines:
+            if not has_passing_row(d, code):
+                fails.append(f"doctrine {code} not signed off")
+
+    if require_debt_clean:
+        debt = load(f"{perf_dir}/debt-inventory.md")
+        if has_open_must_fix(debt):
+            fails.append("debt-inventory open must-fix items")
+
+    if require_grep_clean:
+        if grep_ffi_surface_for_debt_tokens() != 0:
+            fails.append("FFI grep yielded TODO/FIXME tokens; see §7.1")
+
+    return (len(fails) == 0, fails)
+```
+
+Exit 0 = M10.5 can close. Non-zero = work remaining; specific
+failures printed.
+
+---
+
+## §C. CI integration
+
+### C.1 Pre-merge tier (every PR)
+
+**Runner.** GitHub Actions `macos-14` (Apple Silicon, ~10 min budget).
+
+**Scenarios.** S1 (short — 60 s), S2 (30 s), S3 (30 s), S5 (30 s),
+S7 (full matrix), S8 (60 s). **Not S4** (iOS-main-thread, slow XCUITest
+boot) — runs nightly instead. **Not S6** (5 min) — runs nightly.
+**Not S9** (10 min) — nightly. **Not S10** (conditional).
+
+**Workflow.** `.github/workflows/stress-pre-merge.yml`:
+
+```yaml
+name: FFI stress (pre-merge)
+on:
+  pull_request:
+    paths:
+      - 'crates/nmp-core/**'
+      - 'crates/nmp-testing/**'
+      - 'ios/NmpStress/**'
+
+jobs:
+  stress-fast:
+    runs-on: macos-14
+    timeout-minutes: 15
+    steps:
+      - uses: actions/checkout@v4
+      - uses: dtolnay/rust-toolchain@stable
+      - run: just stress
+      - run: just stress-report
+      - run: just stress-gate
+      - uses: actions/upload-artifact@v4
+        if: always()
+        with:
+          name: stress-pre-merge-${{ github.run_id }}
+          path: docs/perf/m10.5/
+```
+
+**Gating.** The `just stress-gate` step exit code is the PR gate.
+
+### C.2 Nightly tier
+
+**Runner.** Mac mini self-hosted runner with an iPhone 12 wired
+in. ~90 min budget.
+
+**Scenarios.** All ten at full duration: S1 (10 min), S2 (60 s),
+S3 (10 emits × 100 k events), S4 (60 s × 12 stalls), S5 (30 s),
+S6 (1 000 cycles), S7 (full matrix), S8 (5 storms), S9 (10 min ×
+100/min), S10 (60 s suspend — *only if M3+M4 are complete; the
+harness skips with a noted "deferred" if not*).
+
+**Workflow.** `.github/workflows/stress-nightly.yml`:
+
+```yaml
+name: FFI stress (nightly)
+on:
+  schedule:
+    - cron: '0 7 * * *'  # 07:00 UTC daily
+  workflow_dispatch: {}
+
+jobs:
+  stress-full:
+    runs-on: [self-hosted, macos, iphone12-attached]
+    timeout-minutes: 90
+    steps:
+      - uses: actions/checkout@v4
+      - run: just stress-full
+      - run: just stress-sonnet
+      - run: just stress-report
+      - run: just stress-gate || echo "::warning::nightly gate failed"
+      - uses: actions/upload-artifact@v4
+        if: always()
+        with:
+          name: stress-nightly-${{ github.run_id }}
+          path: docs/perf/m10.5/
+```
+
+**Gating.** Nightly failures emit a GH warning + Slack notification
+but do not block merge. They block the M10.5 milestone-close
+declaration.
+
+### C.3 On-demand tier (release candidates)
+
+**Trigger.** Manual `workflow_dispatch`, or a git tag matching
+`v*-rc*`.
+
+**Scenarios.** Soak versions:
+- S1: 8-hour mount/unmount churn.
+- S2: 1 M dispatches at 10 k/sec (~ 100 s, repeated for an hour).
+- S9: 24-hour relay flap.
+- Sonnet-agent: 8 agents × 4-hour parallel run.
+
+**Runner.** Lab device (iPhone 12 + iPhone 16 Pro + dedicated Mac
+mini); manual sign-off required.
+
+**Reporting.** Output bundle goes to
+`docs/perf/m10.5/rc-<tag>/`. Sign-off recorded in the release notes.
+
+### C.4 Trace-based regression detection
+
+The Rust harness supports `--capture-trace` (records all FFI
+calls + timestamps + emit payload hashes) and `--replay <PATH>`
+(deterministic replay). One capture per scenario is checked into
+`crates/nmp-testing/bin/ffi-stress/traces/` (LFS-tracked). Nightly
+replay against the same trace must produce byte-identical metrics
+± 5 %; deviations flag a regression even if the gate passes.
+
+This is the same pattern firehose-bench uses (see
+`docs/design/firehose-bench.md` §5).
+
+### C.5 What does not block CI
+
+- **Sonnet-agent runs** are advisory. Flake by design; failures are
+  triaged manually. Two-or-more agents hitting the same failure in
+  one nightly = upgraded to a tracked bug.
+- **iPhone 12 hardware-only scenarios** (S9 device variant, S4 device
+  variant) skip if the device is detached/offline; the missing
+  results are noted in the report and the gate script
+  treats "device-absent" as a deferred-not-failed state.
+- **S10 if M3+M4 are not complete:** scenario reports as `skipped:
+  prereq` with a note in `metrics.json`; gate script treats this
+  as PASS-with-deferral.
+
+---
+
+## §C.6 CI artifact retention
+
+| Tier | Retention | Notes |
+|---|---|---|
+| Pre-merge | 14 days | Per-PR; bulk delete |
+| Nightly | 90 days | Per-run; archived to S3 quarterly |
+| On-demand (RC) | indefinite | Release-attached artifact |
+
+Instruments traces are large (50–500 MiB per scenario). Pre-merge
+runs omit `--instruments` to stay within 15 min; only nightly + RC
+capture traces.
+
+---
+
+## §C.7 The M10.5 close protocol
+
+1. Latest nightly run = PASS on every scenario (or PASS-with-deferral
+   for S10).
+2. `docs/perf/m10.5/debt-inventory.md` must-fix list = empty.
+3. §7.1 grep gate = 0 hits.
+4. Doctrine review (D0–D5) signed off in `doctrine-review.md`.
+5. iPhone 12 baseline = published in `iphone12-baseline.md` with no
+   p99 regression > 5 % vs M10 baseline (plan.md M10.5 exit-gate
+   row 2).
+6. M11 podcast app scoping begins.
+
+A single broken row in any of 1–5 means M10.5 stays open. There
+is no partial close.
diff --git a/docs/design/ffi-hardening/gates.md b/docs/design/ffi-hardening/gates.md
new file mode 100644
index 0000000..7e4a1bb
--- /dev/null
+++ b/docs/design/ffi-hardening/gates.md
@@ -0,0 +1,267 @@
+# FFI hardening — exit gates (§5) and doctrine review (§8)
+
+Two tables in this file:
+1. **§G** numeric exit gates per scenario per metric — the complete
+   contract; every cell is a specific number with units. No "fast
+   enough", no "acceptable".
+2. **§D** doctrine D0–D5 review checklist — each line item maps to
+   a scenario, an audit document, or an explicit code-comment task.
+
+The values here are the **simulator** baseline (iPhone 16 Pro
+simulator on Apple Silicon Mac). The iPhone 12 hardware baseline is
+quoted as a separate column where scaling matters; the per-device
+coefficient is documented in
+`docs/perf/m10.5/iphone12-baseline.md` (produced by the M1–M10
+firehose-bench rerun, not by this design doc).
+
+---
+
+## §G. Numeric exit gates
+
+### G-S1. Mount/unmount churn (10 min)
+
+| Metric | Op | Sim threshold | iPhone 12 threshold |
+|---|---|---|---|
+| Wall duration | `==` | 600 s ± 5 s | 600 s ± 5 s |
+| Cycles completed | `>=` | 540,000 (90 % of 600k nominal) | 360,000 (60 %) |
+| Final per-pubkey refcount | `==` | 0 for every key | 0 |
+| Unmatched claim/release | `==` | 0 | 0 |
+| Process RSS growth | `<=` | 5 MiB | 8 MiB |
+| Counting allocator slope (post-warmup) | `<=` | 0 bytes/sec | 0 bytes/sec |
+| Instruments retained-by-cycle leaks | `==` | 0 | 0 |
+| Listener thread CPU avg | `<=` | 5 % | 8 % |
+
+### G-S2. Dispatch flood (60 s, 10 000/s, 4 threads)
+
+| Metric | Op | Sim threshold | iPhone 12 threshold |
+|---|---|---|---|
+| Dispatches submitted | `>=` | 600,000 | 600,000 |
+| Per-dispatch Swift→Rust send latency p50 | `<=` | 100 µs | 200 µs |
+| Per-dispatch Swift→Rust send latency p99 | `<=` | 1 ms | 2 ms |
+| Actor mpsc backlog peak | `<=` | 10,000 | 10,000 |
+| Main thread hitches > 16 ms (XCTHitchMetric) | `==` | 0 | 0 |
+| Dropped messages (Sender::send failures) | `==` | 0 | 0 |
+| RSS growth | `<=` | 20 MiB | 30 MiB |
+
+### G-S3. Snapshot pressure (100 k events, 10 emits)
+
+| Metric | Op | Sim threshold | iPhone 12 threshold |
+|---|---|---|---|
+| Per-emit JSON serialization wall-time p99 (Rust) | `<=` | 20 ms | 40 ms |
+| Per-emit payload size | `<=` | 2 MiB | 2 MiB |
+| Swift `apply_us` p99 | `<=` | 16 ms (16 000 µs) | 32 ms |
+| End-to-end reconciler frequency | `<=` | 60 Hz | 60 Hz |
+| Allocations per emit | `<=` | `2 × payload_bytes` | `2 × payload_bytes` |
+| Counting allocator: temporary peak during emit | `<=` | `3 × payload_bytes` | `3 × payload_bytes` |
+| `rev` monotonicity across the 10 emits | `==` | strictly increasing | strictly increasing |
+
+### G-S4. Reconciler back-pressure (12 stalls × 250 ms, 60 s)
+
+| Metric | Op | Sim threshold | iPhone 12 threshold |
+|---|---|---|---|
+| Stalls injected | `==` | 12 | 12 |
+| Actor `actor_queue_depth` peak during any stall | `<=` | 50 | 100 |
+| Listener `update_rx` backlog after each stall | `<=` | `ceil(0.25 × emit_hz) + 1` | same |
+| Rev order on resume | `==` | strictly monotonic | strictly monotonic |
+| Stale-rev filter drops counted | `>=` | 1 per stall (12 total) | same |
+| Total emits dropped (listener-side) | `==` | 0 | 0 |
+| Apply-after-resume burst max | `<=` | 33 ms | 50 ms |
+
+### G-S5. Reentrancy (30 s, 50 events/s, callback dispatches)
+
+| Metric | Op | Sim threshold | iPhone 12 threshold |
+|---|---|---|---|
+| Deadlocks (5 s watchdog) | `==` | 0 | 0 |
+| Dispatch-from-callback emits processed | `>=` | 100 (over 30 s) | 100 |
+| Out-of-order callback→dispatch pairs | `==` | 0 | 0 |
+| Listener thread CPU per emit avg | `<=` | 2 ms | 4 ms |
+| Dispatch loss | `==` | 0 | 0 |
+
+### G-S6. Capability lifecycle storms (1 000 cycles, 5 min)
+
+| Metric | Op | Sim threshold | iPhone 12 threshold |
+|---|---|---|---|
+| Cycles completed | `==` | 1,000 | 1,000 |
+| Thread count delta from baseline | `<=` | 2 | 2 |
+| Active `RelayControl` entries after final Stop | `==` | 0 | 0 |
+| Relay-worker generation counter wrap | `==` | 0 (no wrap) | 0 |
+| RSS growth | `<=` | 2 MiB | 4 MiB |
+| Cycle wall-time p99 | `<=` | 250 ms | 500 ms |
+| Deadlocks (5 s per-cycle watchdog) | `==` | 0 | 0 |
+
+### G-S7. Error-shape exhaustion (full matrix)
+
+| Metric | Op | Sim threshold | iPhone 12 threshold |
+|---|---|---|---|
+| Crashes / signals across full input matrix | `==` | 0 | 0 |
+| Use-after-free probe (free → call) crashes | `==` | 0 | 0 |
+| Toast field populated on every silent-no-op path | `==` | 100 % of validation-fail paths | 100 % |
+| Toast strings non-empty + actionable (regex match) | `==` | 100 % | 100 % |
+| Instruments-Allocations delta over full matrix | `==` | 0 (no leak per error path) | 0 |
+| Symbols × invalid-input variants exercised | `>=` | 70 (14 symbols × 5 variants avg) | 70 |
+
+### G-S8. Subscription planner DOS (5 × 10 k storm, 30 s gap)
+
+| Metric | Op | Sim threshold | iPhone 12 threshold |
+|---|---|---|---|
+| Peak working-set RSS during storm | `<=` | 150 MiB | 200 MiB |
+| Wire-REQ frame count per 10 k OpenViews | `<=` | 2,500 (4× dedup floor) | 2,500 |
+| Planner state size after all closes | `<=` | baseline × 1.05 | baseline × 1.10 |
+| Actor stalls > 50 ms during storm | `==` | 0 | 0 |
+| Auto-close success rate (every release → close) | `==` | 100 % | 100 % |
+| Wire subscription leaks after 5 storms | `==` | 0 | 0 |
+
+### G-S9. Relay flap (100/min × 10 min = 1 000 flaps)
+
+| Metric | Op | Sim threshold | iPhone 12 threshold |
+|---|---|---|---|
+| Flaps injected | `==` | 1,000 | 1,000 |
+| Wire-subscription count after each reconnect | `==` | 8 (one per logical view) | 8 |
+| `reconnect_count` field matches injected count | `==` | 1,000 | 1,000 |
+| Reconnect→first-REQ-out p99 latency | `<=` | 500 ms | 1,000 ms |
+| Total bytes-RX over 10 min | `<=` | 2 × baseline bandwidth | 2 × baseline |
+| Deferred-outbound queue drain on reconnect | `==` | 100 % | 100 % |
+| Subscription leaks (count after final reconnect) | `==` | 0 | 0 |
+
+### G-S10. Long suspend (conditional on M3+M4)
+
+| Metric | Op | Sim threshold | iPhone 12 threshold |
+|---|---|---|---|
+| Watermark persisted across suspend | `==` | true | true |
+| Catch-up REQ uses `since` filter | `==` | true | true |
+| Catch-up window wall-time | `<=` | 5 s | 8 s |
+| Catch-up bandwidth vs steady-state | `<=` | 3× | 3× |
+| Post-resume state snapshot equals control | `==` | byte-equal (relevant payloads) | byte-equal |
+
+---
+
+## §G.1 Aggregate gates (apply across all scenarios)
+
+| Metric | Op | Threshold | Source |
+|---|---|---|---|
+| Total Instruments-Leaks across the suite | `==` | 0 | every scenario with `--instruments` |
+| Total crashes / panics across the suite | `==` | 0 | every scenario |
+| M1–M10 firehose-bench rerun regression (p99 ms, any scenario) | `<=` | +5 % vs baseline | plan.md §M10.5 exit-gate row 2 |
+| Pre-merge runtime budget per PR | `<=` | 15 min wall-time on `macos-14` | ci.md §1 |
+| Nightly runtime budget on Mac mini self-hosted | `<=` | 90 min | ci.md §2 |
+
+---
+
+## §D. Doctrine D0–D5 review checklist
+
+Cardinal doctrines per `docs/product-spec/overview-and-dx.md` §1.5.
+Each line item names the artifact that proves it.
+
+> **Note.** The task brief mentioned "D0–D5". The canonical list in
+> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
+> file follows that list. (The aim.md §6 list of 12 items is a
+> larger doctrine set the FFI hardening pass deliberately does
+> not redundantly re-prove — items beyond D0–D5 are covered by
+> earlier milestones' own gates.)
+
+### D0. Kernel never grows app nouns
+
+- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
+  no app-domain types in `nmp-core` substrate.
+- ✅ **Stress proof:** S6 (capability lifecycle storms) does
+  1 000 start/stop/restart cycles; the kernel's capability set is
+  unchanged across all cycles (no dynamic registration of new
+  noun types).
+- 📝 **Sign-off:** auditor signature line in
+  `docs/perf/m10.5/doctrine-review.md` § D0.
+
+### D1. Best-effort rendering — render now, refine in place
+
+- ✅ **Stress proof:** S3 (snapshot pressure) — every emit must
+  satisfy the placeholder-then-refine contract: missing display
+  names → shortened-npub placeholders are present (no `None`); when
+  kind:0 arrives, the next emit updates in place. Assertion: for
+  every author with placeholder text in emit N, emit M > N where
+  the kind:0 arrived must contain the resolved text and an
+  unchanged `id`.
+- ✅ **Stress proof (long path):** S10 (long suspend) — on resume
+  the catch-up rendering does not stall on missing profiles; the
+  timeline renders with placeholders immediately.
+- 📝 **Sign-off:** doctrine-review.md § D1.
+
+### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
+
+- ✅ **Stress proof:** S2 (dispatch flood), S3 (snapshot pressure),
+  S8 (planner DOS) all assert reconciler frequency stays bounded
+  (currently capped at 12 Hz; the doctrine says ≤ 60 Hz; the gate
+  is the 60 Hz ceiling, the configured cap is internal policy).
+- ✅ **Stress proof (working set):** S8 asserts planner state
+  returns to baseline ± 5 % after close storms.
+- ✅ **Composite reverse index:** S8 asserts wire-REQ dedup
+  (4× compaction floor); the reverse index drives the dedup.
+- 📝 **Sign-off:** doctrine-review.md § D2.
+
+### D3. Errors never cross FFI
+
+- ⚠️ **Current state:** debt-inventory §3 D3 audit concludes
+  errors-as-data crosses FFI correctly via `RelayStatus.last_error`
+  / `last_notice`. App-action error paths are
+  **silently no-op** (see parent doc §7.2).
+- ✅ **Stress proof + remediation:** S7 (error-shape exhaustion)
+  exercises every invalid-input path. The M10.5 deliverable
+  adds a `toast: Option<String>` field to the JSON update
+  payload and populates it from S7's failure paths. The
+  schema change is additive and non-breaking.
+- 📝 **Sign-off:** doctrine-review.md § D3, with the explicit
+  note that this milestone *closes* the D3-incomplete state
+  identified in the debt inventory.
+
+### D4. Single writer per fact — caches derive
+
+- ✅ **Proof:** debt-inventory §3 D4 audit — single-threaded actor
+  is the only writer; `KernelModel` on the iOS side is
+  `@MainActor`-isolated so derived state has a single writer per
+  layer.
+- ✅ **Stress proof:** S5 (reentrancy) — the reentrant
+  dispatch-from-callback path does not violate single-writer
+  because the dispatch enqueues a command for the actor; the
+  callback does not mutate kernel state directly.
+- ✅ **Stress proof:** S1 (mount/unmount) — refcount table is
+  only mutated by the actor; verified by harness asserting no
+  refcount-table mutation on any other thread (via
+  `loom`-style instrumentation in the harness binary only, not
+  prod).
+- 📝 **Sign-off:** doctrine-review.md § D4.
+
+### D5. Snapshots bounded by what's open
+
+- ✅ **Stress proof:** S1 (mount/unmount) — refcount drops to
+  zero ⇒ associated view payload evicted from snapshot.
+- ✅ **Stress proof:** S3 (snapshot pressure) — payload size
+  scales with `open_view_count`, not with `stored_events`
+  count (100 k events ⇒ payload < 2 MiB because only views are
+  open, not the full store).
+- ✅ **Stress proof:** S8 (planner DOS) — peak RSS bounded by
+  open-view count even under 10 k concurrent OpenViews.
+- 📝 **Sign-off:** doctrine-review.md § D5.
+
+---
+
+## §D.1 Doctrine sign-off artifact
+
+`docs/perf/m10.5/doctrine-review.md` is produced at the *end* of
+M10.5, not as part of this design. The structure is:
+
+```
+# M10.5 Doctrine Review
+
+| Doctrine | Status | Evidence | Reviewer | Date |
+|---|---|---|---|---|
+| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
+| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
+| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
+| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
+| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
+| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
+
+## Notes
+<any caveats, deferrals, follow-ups>
+```
+
+A PASS in every row + the §7.1 grep gate yielding 0 hits = M10.5
+ready to close. Anything less = the milestone is open.
diff --git a/docs/design/ffi-hardening/harness.md b/docs/design/ffi-hardening/harness.md
new file mode 100644
index 0000000..434d8c0
--- /dev/null
+++ b/docs/design/ffi-hardening/harness.md
@@ -0,0 +1,410 @@
+# FFI hardening — harness architecture (§4)
+
+Three runner paths share one report schema. The Rust harness drives the
+FFI symbols directly (fastest iteration). The XCUITest target exercises
+the real `NmpStress.app` on simulator + device (only path that catches
+iOS-main-thread bugs). The Sonnet-agent runner produces screenshots and
+unscripted UI exercises that catch what scripted tests miss.
+
+---
+
+## 1. Rust-side harness — `crates/nmp-testing/bin/ffi-stress/`
+
+### 1.1 Layout
+
+Modeled directly on `crates/nmp-testing/bin/firehose-bench/` (already
+exists, 4 files: `main.rs`, `config.rs`, `report.rs`, `scenarios.rs`):
+
+```
+crates/nmp-testing/bin/ffi-stress/
+├── main.rs                  # mode dispatch via subcommand
+├── config.rs                # CLI parsing, scenario selection
+├── report.rs                # JSON metrics + markdown report writer
+├── allocator.rs             # counting allocator (vendored from reactivity-bench)
+├── mock_relay.rs            # in-process flap-able WebSocket (S9)
+├── scenarios/
+│   ├── mod.rs               # scenario registry + dispatcher
+│   ├── mount_unmount.rs     # S1
+│   ├── dispatch_flood.rs    # S2
+│   ├── snapshot_pressure.rs # S3
+│   ├── reentrancy.rs        # S5
+│   ├── lifecycle_storm.rs   # S6
+│   ├── error_exhaustion.rs  # S7
+│   ├── planner_dos.rs       # S8
+│   ├── relay_flap.rs        # S9
+│   └── long_suspend.rs      # S10 (conditional on M3+M4)
+└── sonnet-runner.sh         # shell driver for the agent fleet (§3)
+```
+
+(S4 reconciler back-pressure is iOS-main-thread-only; lives in
+StressUITests, not here.)
+
+### 1.2 CLI shape
+
+```
+ffi-stress <scenario> [options]
+
+scenarios:
+  mount-unmount        S1
+  dispatch-flood       S2
+  snapshot-pressure    S3
+  reentrancy           S5
+  lifecycle-storm      S6
+  error-exhaustion     S7
+  planner-dos          S8
+  relay-flap           S9
+  long-suspend         S10 (skipped unless --experimental-suspend)
+  all                  run every gated scenario
+
+options:
+  --duration <D>       wall-clock duration (e.g. 10m, 60s); default per scenario
+  --threads <N>        caller-thread count (S2 default 4, others 1)
+  --rate <R>           operations per second (default per scenario)
+  --target <T>         sim | device | none — for trace/report tagging only
+  --report-dir <PATH>  default: docs/perf/m10.5/<scenario>/
+  --fail-on-gate       exit 2 if any gate fails
+  --capture-trace      record FFI call log + emit log for replay
+  --replay <PATH>      deterministic replay against a captured trace
+  --instruments        on macOS, spawn `xctrace record` for the duration
+```
+
+### 1.3 Scenario module shape
+
+Each scenario is a `fn run(cfg: &Config, report: &mut ScenarioReport)`
+with the same signature so the dispatcher in `scenarios/mod.rs` stays
+trivial. Sketch:
+
+```rust
+// crates/nmp-testing/bin/ffi-stress/scenarios/mount_unmount.rs
+use crate::report::{Gate, ScenarioReport};
+use nmp_testing::ffi_stress::CountingAllocator;
+use std::ffi::{c_void, CString};
+use std::time::{Duration, Instant};
+
+pub fn run(cfg: &super::Cfg, report: &mut ScenarioReport) {
+    let app = unsafe { nmp_core_ffi::nmp_app_new() };
+    let ctx = Box::into_raw(Box::new(SinkCtx::default())) as *mut c_void;
+    unsafe {
+        nmp_core_ffi::nmp_app_set_update_callback(app, ctx, Some(sink_cb));
+        nmp_core_ffi::nmp_app_start(app, 0, 80, 4);
+    }
+
+    let pubkeys = generate_test_pubkeys(100);
+    let start = Instant::now();
+    let mut cycles = 0u64;
+    let baseline_rss = process_rss_bytes();
+
+    while start.elapsed() < cfg.duration {
+        let pk = &pubkeys[cycles as usize % pubkeys.len()];
+        let consumer = format!("ffi-stress-{}", cycles);
+        let pk_c = CString::new(pk.as_str()).unwrap();
+        let cn_c = CString::new(consumer).unwrap();
+        unsafe {
+            nmp_core_ffi::nmp_app_claim_profile(app, pk_c.as_ptr(), cn_c.as_ptr());
+        }
+        std::thread::sleep(Duration::from_millis(1));
+        unsafe {
+            nmp_core_ffi::nmp_app_release_profile(app, pk_c.as_ptr(), cn_c.as_ptr());
+        }
+        cycles += 1;
+        if cycles % 1000 == 0 {
+            cfg.pacer.wait_for_next_second();
+        }
+    }
+
+    let final_rss = process_rss_bytes();
+    let rss_growth = final_rss.saturating_sub(baseline_rss);
+
+    report.gates.push(Gate::numeric(
+        "rss_growth_bytes",
+        rss_growth as f64,
+        op: "<=",
+        threshold: 5 * 1024 * 1024,
+    ));
+    report.gates.push(Gate::numeric(
+        "cycles_completed",
+        cycles as f64,
+        op: ">=",
+        threshold: cfg.duration.as_secs() * 1000 * 90 / 100, // 90% of nominal
+    ));
+
+    unsafe {
+        nmp_core_ffi::nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
+        nmp_core_ffi::nmp_app_free(app);
+        drop(Box::from_raw(ctx as *mut SinkCtx));
+    }
+}
+```
+
+The shape mirrors the existing firehose-bench scenarios — same `Gate`
+abstraction, same `ScenarioReport` builder, same JSON output.
+
+### 1.4 Counting allocator
+
+Vendored from `crates/nmp-testing/bin/reactivity-bench/allocator.rs`
+(ADR-0004). Installed as `#[global_allocator]` in the harness binary
+only (not in `nmp-core`). Used by S1, S2, S3, S6, S8 to detect heap
+growth without Instruments.
+
+### 1.5 Mock relay for S9 (relay flap)
+
+A minimal in-process WebSocket server that accepts a connection,
+serves canned events, and can be `kill()`-ed externally on a schedule.
+Lives in `mock_relay.rs`. Reuses `nostr-relay-builder` if available;
+otherwise a hand-rolled `tungstenite::accept` loop is enough for the
+flap test (we're testing reconnection logic, not protocol fidelity).
+
+### 1.6 Linking
+
+`nmp-core` already compiles as `cdylib + staticlib + rlib`. The
+harness binary depends on it as an rlib through a new
+`nmp-core-ffi-decls` crate that re-exports the `extern "C"` symbols
+as Rust function declarations:
+
+```rust
+// crates/nmp-core-ffi-decls/src/lib.rs
+extern "C" {
+    pub fn nmp_app_new() -> *mut std::ffi::c_void;
+    pub fn nmp_app_free(app: *mut std::ffi::c_void);
+    pub fn nmp_app_set_update_callback(
+        app: *mut std::ffi::c_void,
+        context: *mut std::ffi::c_void,
+        callback: Option<extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_char)>,
+    );
+    // ... (rest of the 14 declarations)
+}
+```
+
+(Alternative: use `nmp-core` directly as a crate dep and avoid the
+extern declarations. Either works; the extern-decl path matches what
+Swift sees more faithfully and surfaces ABI-mismatch bugs.)
+
+---
+
+## 2. iOS XCUITest target — `ios/NmpStress/StressUITests/`
+
+### 2.1 Layout
+
+New target alongside the existing `NmpStressUITests/`:
+
+```
+ios/NmpStress/StressUITests/
+├── StressUITests.swift             # target entry, shared helpers
+├── S1MountUnmountChurn.swift
+├── S2DispatchFlood.swift
+├── S3SnapshotPressure.swift
+├── S4ReconcilerBackpressure.swift  # iOS-only
+├── S5Reentrancy.swift
+├── S6LifecycleStorms.swift
+├── S7ErrorExhaustion.swift
+├── S8PlannerDOS.swift
+├── S9RelayFlap.swift               # nightly device only
+└── S10LongSuspend.swift            # conditional on M3+M4
+```
+
+The existing `NmpStressUITests/NmpStressUITests.swift` (102 LOC) is
+kept for the boot-render smoke test; the StressUITests target is for
+the harness.
+
+### 2.2 Test class skeleton
+
+```swift
+import XCTest
+
+@MainActor
+final class S1MountUnmountChurn: XCTestCase {
+    func testMountUnmountChurn_10Min() throws {
+        let app = XCUIApplication()
+        app.launchEnvironment["NMP_STRESS_SCENARIO"] = "S1"
+        app.launchEnvironment["NMP_STRESS_DURATION_SEC"] = "600"
+        app.launchEnvironment["NMP_VISIBLE_LIMIT"] = "80"
+        app.launchEnvironment["NMP_EMIT_HZ"] = "4"
+        app.launch()
+
+        // The app, when launched with NMP_STRESS_SCENARIO=S1, runs the
+        // mount/unmount churn from inside the Swift bridge, exercising
+        // the real KernelHandle pathway (not just the C ABI).
+        let metricsExporter = app.staticTexts["stress-metrics-exporter"]
+        XCTAssertTrue(metricsExporter.waitForExistence(timeout: 620))
+
+        let payload = metricsExporter.label  // JSON blob
+        let metrics = try JSONDecoder().decode(StressMetrics.self,
+                                                from: Data(payload.utf8))
+
+        XCTAssertEqual(metrics.unmatchedClaims, 0)
+        XCTAssertLessThanOrEqual(metrics.rssGrowthBytes, 5 * 1024 * 1024)
+        XCTAssertEqual(metrics.instrumentsLeakCount, 0,
+                       "Instruments-Leaks must be 0 — see Instruments.trace bundle")
+    }
+}
+```
+
+The pattern: the `NmpStress` app honors a `NMP_STRESS_SCENARIO`
+launch-env that puts it in a driven mode (no human interaction
+needed), runs the scenario, then exposes a JSON metrics blob as an
+accessibility label the XCUITest can read. This is the same pattern
+the existing `NmpStressUITests` already uses for its
+`relay-state-value` / `metric-events-value` accessibility-ID
+exposures (NmpStressUITests.swift:13–28).
+
+### 2.3 Performance metrics
+
+Use `XCTMetric` for what XCUITest measures natively:
+
+- `XCTHitchMetric` — main-thread hitches (S2, S3, S4).
+- `XCTClockMetric` — wall time (S1, S6).
+- `XCTMemoryMetric` — RSS sample (S1, S3, S8).
+- `XCTCPUMetric` — CPU usage (S2, S3).
+- `XCTApplicationLaunchMetric` — cold-start (used by the M1–M10
+  perf reruns, not by these scenarios directly).
+
+Instruments-Leaks integration is via `xcrun xctrace record --template
+'Leaks' --launch -- /path/to/NmpStress.app`, captured by the
+harness shell script that drives `xcodebuild test` (see ci.md §3).
+
+---
+
+## 3. Sonnet-agent runner — `sonnet-runner.sh`
+
+### 3.1 What it does
+
+Spawns **N parallel `claude` agent processes**, each given a system
+prompt that scopes them to a single user flow (e.g., "open the
+profile of pubkey X, scroll, tap a thread, return"). Each agent
+drives the simulator via the `mcp__xcode__*` tool family
+(`boot_sim`, `launch_app_sim`, `tap`, `swipe`, `screenshot`,
+`describe_ui`, `stop_app_sim`).
+
+The point is to catch what scripted UI tests miss: real human-shaped
+interleavings, unexpected tap targets, race conditions between gesture
+and emit, and visual regressions that XCTAssert can't see.
+
+### 3.2 Concrete invocation sketch
+
+```bash
+#!/usr/bin/env bash
+# crates/nmp-testing/bin/ffi-stress/sonnet-runner.sh
+# Usage: ./sonnet-runner.sh <scenario> <parallel-agent-count> <duration-min>
+
+set -euo pipefail
+
+SCENARIO="${1:-default}"
+N="${2:-4}"
+DURATION_MIN="${3:-5}"
+
+REPORT_DIR="docs/perf/m10.5/sonnet/${SCENARIO}-$(date +%s)"
+mkdir -p "$REPORT_DIR"
+
+# Boot a fresh simulator
+SIM_ID=$(xcrun simctl list devices available | grep "iPhone 16 Pro" \
+  | head -1 | grep -oE '[A-F0-9-]{36}')
+xcrun simctl boot "$SIM_ID" || true
+xcrun simctl install "$SIM_ID" ios/DerivedData/Build/Products/Debug-iphonesimulator/NmpStress.app
+
+# Spawn N agents in parallel
+for i in $(seq 1 "$N"); do
+  AGENT_DIR="$REPORT_DIR/agent-$i"
+  mkdir -p "$AGENT_DIR/screenshots"
+  (
+    claude --print --output-format=json \
+      --max-turns 200 \
+      --append-system-prompt "$(cat <<EOF
+You are a stress-testing agent for the NmpStress iOS app. You have $DURATION_MIN
+minutes to exercise the app via mcp__xcode__* tools on simulator $SIM_ID.
+Bundle ID: com.example.NmpStress. Goal: stress the FFI surface by mounting
+and unmounting profile views via aggressive tap/back navigation. After every
+5 actions, call mcp__xcode__screenshot and save the output. Append every
+assertion (UI element present? rev increased? no error toast?) to
+$AGENT_DIR/assertions.log. Stop after $DURATION_MIN minutes wall-clock.
+EOF
+)" \
+      "Begin stress run #$i for scenario $SCENARIO" \
+      > "$AGENT_DIR/transcript.json"
+  ) &
+done
+
+wait
+
+# Aggregate
+python3 scripts/sonnet-aggregate.py "$REPORT_DIR" \
+  > "$REPORT_DIR/aggregate-report.md"
+```
+
+### 3.3 Output bundle (per agent)
+
+```
+docs/perf/m10.5/sonnet/<scenario>-<unix-ts>/
+├── agent-1/
+│   ├── transcript.json       # full Claude conversation
+│   ├── assertions.log        # one line per assertion: PASS/FAIL <description>
+│   └── screenshots/
+│       ├── 0001.png
+│       ├── 0002.png
+│       └── ...
+├── agent-2/ ...
+└── aggregate-report.md       # union of assertions, screenshot grid
+```
+
+### 3.4 Why this is separate from XCUITest
+
+XCUITest assertions are scripted and deterministic. Sonnet agents
+make unscripted choices. They will find:
+- UI states the scripted test didn't think to navigate to;
+- gesture sequences that exercise FFI corners the scripted test
+  doesn't reach;
+- visual regressions (the screenshot trail is human-reviewable).
+
+Trade-off: non-determinism makes flaky CI. Mitigation: nightly only,
+not pre-merge; treated as advisory unless multiple agents in one
+run hit the same failure.
+
+### 3.5 Number of agents
+
+Default **N=4** (matches a single iPhone 16 Pro simulator's
+comfortable concurrency budget — multiple sims is overkill for
+M10.5). N up to 8 in nightly runs on the Mac mini self-hosted
+runner where multiple simulators can boot in parallel.
+
+---
+
+## 4. Shared report schema
+
+All three runners produce `metrics.json` with the same schema so the
+aggregation in `docs/perf/m10.5/` is uniform:
+
+```json
+{
+  "schema_version": "1",
+  "scenario": "S1",
+  "runner": "rust-harness" | "xcuitest" | "sonnet",
+  "device": "iPhone 16 Pro Simulator" | "iPhone 12" | "macOS-host",
+  "started_at_unix": 1779100000,
+  "duration_sec": 600,
+  "passed": true,
+  "gates": [
+    {"name": "rss_growth_bytes", "value": 3145728, "op": "<=", "threshold": 5242880, "passed": true},
+    {"name": "unmatched_claims", "value": 0, "op": "==", "threshold": 0, "passed": true}
+  ],
+  "metrics": { /* scenario-specific KV pairs */ },
+  "limitations": [],
+  "observations": []
+}
+```
+
+The schema matches the existing `firehose-bench` `FirehoseReport`
+shape (see `crates/nmp-testing/bin/firehose-bench/report.rs`) so the
+aggregator script reuses the same parser.
+
+---
+
+## 5. What's intentionally out of scope
+
+- **Network simulation.** No `tc netem`-style packet loss; the mock
+  relay's flap behavior is binary on/off. Realistic-loss scenarios
+  are deferred.
+- **iOS background-extension stress.** NSE decryption load (firehose
+  §3.7) is a separate harness, not part of M10.5.
+- **Multi-account.** S5/S6 use a single account; multi-account
+  concurrent stress is firehose §3.5.
+- **Cross-platform.** Android / desktop / web FFI surfaces are not
+  exercised; M10.5 is iOS-only.
diff --git a/docs/design/ffi-hardening/scenarios.md b/docs/design/ffi-hardening/scenarios.md
new file mode 100644
index 0000000..f239a76
--- /dev/null
+++ b/docs/design/ffi-hardening/scenarios.md
@@ -0,0 +1,353 @@
+# FFI hardening — scenarios (§3)
+
+Ten named failure modes. Each entry: setup, assertion, threading target,
+runner (Rust harness / XCUITest / Sonnet-agent), and numeric threshold.
+Numeric thresholds are quoted here for clarity; the canonical exit-gate
+table lives in [`gates.md`](./gates.md).
+
+Conventions:
+- "dispatch" = a single Swift→Rust call through any `nmp_app_*` symbol.
+- "emit" = one full update payload delivered to the registered callback.
+- "view-handle wrapper" = the iOS-side refcounted entity created when a
+  SwiftUI component mounts a `useProfile(pubkey)`-style observer (in the
+  current `NmpStress` codebase this maps to `KernelModel.claimProfile` +
+  `releaseProfile`; ADR-0005).
+
+---
+
+## S1. Mount/unmount churn — view-handle wrapper refcount
+
+**The bug shape.** Wrappers (`claim_profile` / `release_profile`) leak
+internal references; the kernel's per-pubkey refcount table grows
+without bound; eventually the platform shadow holds entries no
+component still needs.
+
+**Setup.** Drive the kernel with a closed-loop of 1,000
+claim/release pairs per second across 100 unique pubkeys (rotating
+LRU). Sustain for **10 minutes** (600,000 cycles). Each cycle:
+`claim_profile(pk_i, consumer_id_i) → 1ms wait → release_profile(pk_i, consumer_id_i)`.
+
+**Threading.** Driver thread = Swift main (XCUITest) **or** the
+single-threaded executor in the Rust harness. Actor thread receives
+the storm; listener emits batched updates.
+
+**Assertions.**
+1. Final refcount for every pubkey = 0.
+2. `Kernel::interest_refcount(pk).is_none()` for every pubkey after a
+   grace-period (configurable, default 30 s — ADR-0005).
+3. Process RSS after 10 min ≤ baseline + **5 MB**.
+4. Instruments → Leaks → 0 retained-by-cycle leaks.
+5. Allocator (counting allocator from `reactivity-bench`) shows
+   slope ≤ 0 bytes/sec post-warmup (first 30 s).
+
+**Runner.** Primary: Rust harness `ffi-stress mount-unmount-churn`.
+Secondary (nightly only): XCUITest `S1MountUnmountChurn.swift` driving
+real `NmpStress` with Instruments-Leaks attached.
+
+**Numeric gate.** See gates.md §G-S1.
+
+---
+
+## S2. Dispatch flood
+
+**The bug shape.** Swift dispatches faster than the actor can drain;
+the mpsc channel grows unbounded; OOM eventually. Or: the FFI symbol
+itself takes a lock that blocks the main thread.
+
+**Setup.** **10,000 dispatches per second** for 60 s across **N=4**
+caller threads (matching iOS's typical concurrency budget). Mix:
+30 % `open_author` (valid pubkey from a pool of 50), 30 %
+`close_author`, 20 % `claim_profile`, 20 % `release_profile`. All
+inputs valid (no validation-path flooding — that's S7).
+
+**Threading.** Four caller OS threads (in the Rust harness, four
+`std::thread`; in the XCUITest variant, four `DispatchQueue.global()`
+queues). Actor thread is the bottleneck under test.
+
+**Assertions.**
+1. No dispatch call takes > **1 ms** at p99 (Swift→Rust channel send
+   latency; this is the bible-#3 "fire-and-forget" guarantee
+   quantified).
+2. Main thread (XCUITest variant) is never blocked > **16 ms**
+   (measured via XCTest's `XCTHitchMetric`).
+3. Actor mpsc backlog never exceeds **10,000 messages** (configurable
+   harness-side check via `actor_queue_depth` field already exposed in
+   `KernelMetrics`).
+4. Zero dropped messages: count of `_ = tx.send(...)` failures = 0.
+5. RSS growth over 60 s ≤ 20 MB.
+
+**Runner.** Rust harness primary. XCUITest secondary because the
+real-world iOS main-thread budget is the load-bearing constraint.
+
+---
+
+## S3. Snapshot pressure — `AppUpdate::FullState` with 100k events
+
+**The bug shape.** Marshal cost for a full-state snapshot grows
+super-linearly; the listener thread spends > 1 emit-interval
+serializing JSON; the reconciler exceeds 60 Hz; the iOS main thread
+spends every frame applying snapshots.
+
+**Setup.** Pre-load kernel state via a captured firehose-bench trace
+(`docs/perf/firehose-bench/traces/m10.5-snapshot.cap`, captured
+expressly for this scenario) containing **100,000 stored events**
+across 3,000 authors. Force a full-state emit by calling
+`nmp_app_configure` (which currently triggers `emit_now`) ten times
+in a row to amortize the JSON cost over ten observations.
+
+**Threading.** Actor builds the update; listener serializes; Swift
+main applies. The bottleneck under test is the **listener
+serialization + main-thread apply** path.
+
+**Assertions.**
+1. Per-emit JSON serialization wall-time p99 ≤ **20 ms** (Rust side).
+2. Payload size ≤ **2 MiB** (if it exceeds, the harness fails and
+   asks for bible-#10 granular-update variants).
+3. Swift `apply_us` (already instrumented in `KernelModel.apply`) p99
+   ≤ **16 ms**.
+4. End-to-end reconciler frequency stays ≤ **60 Hz** (configured cap
+   is 12 Hz today; the harness verifies the cap is honored under
+   pressure rather than asserting the cap value itself).
+5. Allocations per emit ≤ **payload_bytes × 2** (no quadratic-copy
+   regression).
+
+**Runner.** Rust harness primary (measures Rust-side); XCUITest
+secondary (measures Swift-side).
+
+---
+
+## S4. Reconciler back-pressure — main thread stalled 250 ms
+
+**The bug shape.** When the iOS main thread blocks (e.g.,
+file-picker, modal sheet, large layout pass), the listener-thread
+emits accumulate. On resume, the app applies a flood and visibly
+hitches, or worse, the actor stalls because its `update_tx` channel
+fills.
+
+**Setup.** Start the kernel, open following + author + thread views,
+drive 100 events/sec for 60 s. Periodically inject a **250 ms
+synchronous sleep on the main thread** (XCUITest:
+`Thread.sleep(forTimeInterval: 0.25)` inside the reconciler closure).
+Repeat every 5 s for the 60-s window → **12 stalls total**.
+
+**Threading.** Main thread (artificially stalled); listener thread
+(accumulates emits); actor (must not block on `update_tx.send`).
+
+**Assertions.**
+1. Actor `actor_queue_depth` never grows during a stall (the listener
+   thread is the queue under back-pressure, not the actor's command
+   queue).
+2. Listener `update_rx` backlog after each 250 ms stall ≤ ⌈ 250 ms
+   × emit_hz ⌉ + 1 = **2** messages (at 4 Hz default) or **4** (at 12
+   Hz max).
+3. On stall release, the main thread applies all backlogged emits in
+   monotonic `rev` order (bible #1).
+4. Stale-rev filter in `KernelModel.apply` correctly drops any
+   intermediate revs (already implemented at KernelModel.swift:139;
+   harness validates).
+5. No emit is dropped — the listener delivers each one in order.
+
+**Runner.** XCUITest only (the bug shape is iOS-main-thread specific).
+
+---
+
+## S5. Reentrancy — dispatch from inside reconciler callback
+
+**The bug shape.** A SwiftUI `.onChange` handler observes a kernel
+update and immediately calls `kernel.openAuthor(...)`. If the FFI
+symbol takes a lock the listener thread also takes, deadlock. If it
+re-enters synchronously, message ordering can invert.
+
+**Setup.** Register a callback that, on every emit where
+`update.metrics.eventsSinceLastUpdate > 0`, immediately dispatches
+`open_author` for the first item in the timeline. Sustain for 30 s
+with a 50 events/sec inflow.
+
+**Threading.** Callback runs on **listener thread**; dispatch
+enqueues to the actor's command channel which the **actor thread**
+drains. The bug shape requires the actor to be processing a message
+when the callback fires (race window).
+
+**Assertions.**
+1. Zero deadlocks (harness times out after 5 s = fail).
+2. Message order preserved — for every emit-then-dispatch pair, the
+   resulting `OpenAuthor` action is processed strictly **after** the
+   emit that triggered it (verified via `rev` monotonicity in the
+   subsequent emit referencing that author view).
+3. No dispatch loss under reentrant pressure.
+4. Listener thread CPU time per emit ≤ **2 ms** even with the
+   reentrant dispatch path active.
+
+**Runner.** Rust harness primary, XCUITest secondary.
+
+---
+
+## S6. Capability lifecycle storms — start/stop/restart
+
+**The bug shape.** `start` → `stop` → `start` cycles leave thread
+handles dangling; relay-worker generations get confused; the second
+`start` brings up a phantom relay worker.
+
+**Setup.** For each capability handle (today: the relay role pair
+Content + Indexer, plus the actor itself), run **1,000
+start/stop/restart cycles** in 5 minutes. Cycle:
+`nmp_app_start → wait 100ms → nmp_app_stop → wait 100ms → nmp_app_start → wait 100ms`.
+
+**Threading.** Caller drives cycles; actor receives Start/Stop;
+relay worker threads spawn and join repeatedly.
+
+**Assertions.**
+1. Thread count after 1,000 cycles ≤ baseline + 2 (actor + listener
+   only; relay workers gone after every Stop).
+2. **Idempotency (bible #7):** two consecutive Starts produce one
+   set of relay workers (verified by counting `RelayControl` entries).
+3. Generation counter `next_relay_generation` strictly monotonic, no
+   wrap (`u64` is fine for 1,000 cycles; harness verifies anyway).
+4. RSS growth ≤ **2 MB** over 1,000 cycles.
+5. No deadlocks (harness 5-s timeout per cycle).
+
+**Runner.** Rust harness only — these are pure FFI symbol calls.
+
+---
+
+## S7. Error-shape exhaustion — every typed FFI error path
+
+**The bug shape.** A typed FFI error path produces an uncaught
+exception, a crash, or — per the §7.2 finding in the parent doc —
+*silent loss*: invalid input is dropped without any state field
+surfacing the problem.
+
+**Setup.** For every `nmp_app_*` symbol that takes a `*const c_char`
+input, exercise the full set of invalid inputs:
+
+| Symbol | Invalid inputs to test |
+|---|---|
+| `nmp_app_open_author` | NULL, "", " ", "not-hex", 63-char hex, 65-char hex, UTF-8 with non-hex chars |
+| `nmp_app_open_thread` | same shapes |
+| `nmp_app_open_firehose_tag` | NULL, "" (others valid; tag is unconstrained) |
+| `nmp_app_claim_profile` | NULL/empty/non-hex pubkey × {NULL, "", "valid"} consumer_id |
+| `nmp_app_release_profile` | same matrix; also: release without prior claim |
+| `nmp_app_close_author` / `_thread` | same |
+| any `_app` arg | NULL |
+
+Plus: dispatch each symbol with `*mut NmpApp` pointing to a
+**freed** allocation (use-after-free probe; must not crash —
+ideally hits the null check after `nmp_app_free` zeroes; harness
+documents observed behavior).
+
+**Threading.** Caller. Pure FFI exercise.
+
+**Assertions.**
+1. Zero crashes / SIGSEGV / SIGABRT across the full matrix.
+2. Every silent-no-op input produces a **toast field** in the next
+   emit (post §7.2 toast-bridge addition) — current behavior fails
+   this assertion; the harness publishes the failing diff and the
+   M10.5 fix adds the toast field.
+3. Every typed error path's toast string is non-empty and
+   actionable (regex match against the catalog in
+   `docs/perf/m10.5/error-catalog.md`, generated by this scenario).
+4. No error path leaks heap memory (Instruments-Allocations delta = 0
+   across the matrix).
+
+**Runner.** Rust harness primary, XCUITest secondary (UI assertion
+on toast banner rendering).
+
+---
+
+## S8. Subscription planner DOS
+
+**The bug shape.** A pathological app opens 10,000 distinct views
+in 1 s; the planner compiles 10,000 wire filters; the actor's
+working set explodes; relay workers can't send fast enough.
+
+**Setup.** Pre-generate 10,000 unique pubkeys. Mix of operations:
+5,000 `open_author` + 5,000 `claim_profile` in a 1-s burst, then
+5,000 `close_author` + 5,000 `release_profile` over the next 1 s.
+Repeat 5 × at 30-s intervals (5 storms total).
+
+**Threading.** Caller bursts; actor compiles; relay workers
+serialize REQ frames.
+
+**Assertions.**
+1. Peak working-set memory during storm ≤ **150 MB** (planner is the
+   dominant allocator).
+2. Wire-subscription dedup: subscriber-planner REQ frame count on
+   the wire ≤ **2,000** (4× compaction floor — adjust based on
+   measured baseline; the assertion is that compaction *happens*,
+   not zero).
+3. After all closes, planner state size returns to baseline ± **5 %**.
+4. No actor-thread stall > **50 ms** during storm.
+5. Auto-close mechanism (close on EOSE for one-shot subs, close on
+   refcount 0 for view subs) operates correctly under storm.
+
+**Runner.** Rust harness only — this exercises planner internals.
+
+---
+
+## S9. Relay flap — simulated WebSocket disconnect/reconnect storm
+
+**The bug shape.** Each disconnect-reconnect leaks a wire
+subscription; bandwidth balloons because every reconnect re-issues
+all subs without dedup; the planner doesn't reconcile state.
+
+**Setup.** Use the harness's mock relay (forked from
+`crates/nmp-testing` — see harness.md §3.3). Kill and restore the
+relay connection at **100 cycles per minute for 10 minutes** (1,000
+flaps total). Maintain a fixed open-view set: 1 timeline + 5 authors
++ 2 threads = 8 logical views throughout.
+
+**Threading.** Mock relay (test harness); relay-worker reconnect
+loop (`run_relay_worker` in `relay_worker.rs`); actor.
+
+**Assertions.**
+1. Wire-subscription count after each reconnect = **8** (one per
+   open logical view, no growth).
+2. Total bytes-RX over 10 min bounded by **2 × baseline bandwidth**
+   (some retransmit is unavoidable; 2× is the budget).
+3. Reconnect p99 latency from disconnect-detection to first
+   re-issued REQ ≤ **500 ms**.
+4. `reconnect_count` field in `RelayStatus` matches the harness's
+   injection count exactly.
+5. No `OutboundMessage` is silently lost — the kernel's
+   `defer_outbound` path captures any send during disconnect, and
+   the harness validates the deferred queue drains on reconnect.
+
+**Runner.** Rust harness primary (mock relay), XCUITest secondary
+(uses the real `NmpStress` against a control-plane that toggles
+network reachability — nightly only).
+
+---
+
+## S10. Long suspend simulation — 60-second background
+
+**Status: conditional on M3 (event store) + M4 (sync watermarks).**
+This scenario is specified now and **scheduled to land in M10.5
+only if M3+M4 are complete by then.** If not, S10 graduates to
+M11.5 and the M10.5 gate excludes it explicitly.
+
+**The bug shape.** iOS suspends the app for 60 s (background). On
+resume, the kernel actor's main loop has paused; relay sockets
+have timed out; sync watermarks need to drive the catch-up. If the
+watermark logic is wrong, the app over-fetches (bandwidth waste) or
+under-fetches (missed events).
+
+**Setup.** Open kernel, drive 20 events/sec for 30 s to establish
+baseline watermarks. Inject a 60-s synthetic main-loop pause via
+`SIGSTOP` on the actor thread (Rust harness only; XCUITest cannot
+inject this). On resume:
+1. Verify watermark-driven REQ uses `since = last_event_at_ms`.
+2. Verify replay completes within **5 s** of resume.
+3. Verify state reconciles to the same snapshot a never-suspended
+   run would produce (byte-equality on the relevant view payloads).
+
+**Threading.** Actor (suspended); relay workers (sockets time out
+during pause); listener (idle during pause).
+
+**Assertions.** (Conditional on M3+M4)
+1. Watermark `last_event_at_ms` persists across suspend.
+2. Catch-up REQ uses `since` correctly (no full re-fetch).
+3. Bandwidth over the 5-s catch-up window ≤ **3 × steady-state
+   bandwidth**.
+4. Final state snapshot identical to the non-suspended control run.
+
+**Runner.** Rust harness only (XCUITest cannot SIGSTOP the actor).
diff --git a/docs/perf/codex-reviews/2026-05-18-session-1.md b/docs/perf/codex-reviews/2026-05-18-session-1.md
new file mode 100644
index 0000000..c724bec
--- /dev/null
+++ b/docs/perf/codex-reviews/2026-05-18-session-1.md
@@ -0,0 +1,1263 @@
+Reading additional input from stdin...
+2026-05-17T22:25:40.547257Z ERROR codex_core::session: failed to load skill /Users/pablofernandez/.agents/skills/voice-capture-sheet/SKILL.md: invalid YAML: mapping values are not allowed in this context at line 2 column 116
+OpenAI Codex v0.129.0 (research preview)
+--------
+workdir: /Users/pablofernandez/Work/nostr-multi-platform
+model: gpt-5.5
+provider: openai
+approval: never
+sandbox: workspace-write [workdir, /tmp, $TMPDIR, /Users/pablofernandez/.codex/memories]
+reasoning effort: xhigh
+reasoning summaries: none
+session id: 019e380b-88c6-7481-a75b-c6b7cc0c162f
+--------
+user
+You are reviewing a session's worth of merges on master in the nostr-multi-platform repo. NMP is a Rust multiplatform framework for Nostr apps building toward v1 per docs/plan.md. Doctrine D0–D5 (docs/product-spec.md §1.5):
+- D0 kernel never grows app nouns
+- D1 best-effort rendering with placeholders
+- D2 reactivity contract (composite reverse index, ≤60 Hz/view, working-set bound)
+- D3 errors never cross FFI (become toast state fields)
+- D4 one writer per fact
+- D5 capabilities report, never decide
+
+File-size: 300 LOC soft, 500 LOC hard (AGENTS.md).
+
+Session goal: complete v1 with zero technical debt, no "for later" shortcuts, robust guardrails, empirical iOS proof before the M11 podcast-app rebuild of /Users/pablofernandez/src/podcast.
+
+These 4 commits landed:
+
+=== diff stat (e9cbafa..d660735) ===
+ docs/perf/m10.5/debt-inventory.md | 434 ++++++++++++++++++++++++++++++++++++++
+ docs/perf/orchestration-log.md    |   8 +
+ docs/plan.md                      | 167 ++++++++++++---
+ 3 files changed, 580 insertions(+), 29 deletions(-)
+
+=== commit log ===
+d660735 audit(m10.5): FFI + iOS bridge debt inventory
+Complete read-only audit of FFI boundary (ffi.rs) and iOS bridge code paths
+(KernelBridge.swift, KernelModel.swift, and 7 other Swift UI files) against
+technical debt patterns and cardinal doctrines.
+
+Summary:
+- 6,559 LOC audited (5,184 Rust, 1,375 Swift)
+- 0 critical debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!)
+- 20 findings classified: 0 bugs, 3 documentation gaps, 14 justified casts, 2 justified expects
+- All 5 cardinal doctrines (D0–D5) compliant
+- iOS code is clean (9 files, 0 findings)
+
+Findings:
+F1–F3: Unsafe blocks in ffi.rs lack safety comments (code correct, docs improve auditability)
+F4: allow(unreachable_patterns) in relay_worker.rs lacks clarification
+F5–F14: Integer casts (metrics, saturation, FFI bounds) all justified and bounded
+F15–F16: expect() calls in kernel/status.rs justified by construction invariants
+
+Doctrine compliance (exit gates for M10.5):
+✅ D0 (kernel never grows app nouns): kernel is domain-agnostic
+✅ D1 (best-effort rendering): ProfileCard.placeholder renders immediately
+✅ D2 (reactivity contract): all updates flow through composite reverse index
+✅ D3 (errors never cross FFI): errors as advisory JSON data, not FFI codes
+✅ D4 (one writer per fact): kernel actor is single-threaded
+✅ D5 (capabilities report): iOS bridge is pure relay, no policy decisions
+
+Recommendations:
+- (Optional) Add 3 safety comments to ffi.rs unsafe blocks (5 min improvement)
+- (Optional) Add 1 comment to relay_worker.rs for defensive intent (2 min)
+- No code changes required; no blocking debt
+
+M10.5 Exit Criteria: Ready for iOS empirical proof phase.
+
+Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
+
+---
+31fa612 perf(orchestration): advisor adjustments (push protocol, triage, gates)
+
+---
+18e4953 perf(orchestration): start log; wave 1 dispatched
+
+---
+55dd5f2 docs(plan): insert M10.5 FFI hardening gate + concretize M11 podcast rebuild
+M10.5 — FFI hardening + iOS empirical proof
+- Dedicated stress harness for mount/unmount churn, dispatch flood,
+  snapshot pressure, reconciler back-pressure, reentrancy, capability
+  lifecycle storms, error-shape exhaustion
+- Real iPhone 12 firehose-bench live across all 8 scenarios
+- Simulator-driven Sonnet-agent UI fleet
+- Instruments leaks + allocations + time profiler audit
+- Zero open TODO/FIXME/unimplemented in FFI/actor/relay/kernel/iOS bridge
+- Hard gate before M11
+
+M11 — Podcast app (../podcast rebuild on NMP)
+- Concrete rebuild of /Users/pablofernandez/src/podcast (Swift, 20 views,
+  ~8.8k LOC) on NMP, with UI literally identical (copy verbatim, then
+  rewire data sources only)
+- Reference podcast-rmp's RMP-ARCHITECTURE-BIBLE.md, FINAL_PLAN.md,
+  iphone-feature-parity-plan.md for lessons (not code)
+- View-by-view to ViewModule mapping table
+- Per-view Rust crates: podcast-core, podcast-llm (rig.rs), podcast-rag,
+  podcast-feeds
+- Pixel-parity screenshot gate against the Swift reference
+- Feature parity, perf gates, kernel-boundary gates all enumerated
+
+Plan + parallelization + matrix updates carry the new milestone through.
+
+Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
+
+---
+
+=== full diff ===
+diff --git a/docs/perf/m10.5/debt-inventory.md b/docs/perf/m10.5/debt-inventory.md
+new file mode 100644
+index 0000000..8545c27
+--- /dev/null
++++ b/docs/perf/m10.5/debt-inventory.md
+@@ -0,0 +1,434 @@
++# FFI & iOS Bridge Debt Inventory (M10.5 Audit)
++
++**Audit Date:** 2026-05-18  
++**Scope:** Rust FFI boundary + iOS bridge code paths  
++**Files Scanned:** 19 Rust modules + 9 Swift files (5,184 LOC Rust, 1,375 LOC Swift)  
++**Exit Criteria:** All findings classified; doctrine violations identified and severity-ranked
++
++---
++
++## 1. Summary Table
++
++| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
++|------|-------|--------|--------|---------|------------------|-------|------------------|---------|-------|
++| ffi.rs | 0 | 0 | 0 | 0 | 3 | 2 | 0 | 0 | FFI boundary; 3 unsafe blocks without safety comments |
++| actor.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
++| relay.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
++| relay_worker.rs | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 1 allow(unreachable_patterns) |
++| app.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
++| lib.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
++| kernel/mod.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
++| kernel/nostr.rs | 0 | 0 | 0 | 0 | 0 | 2 | 0 | 0 | 2 casts (f64 ratio); .unwrap_or_default used safely |
++| kernel/status.rs | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 2 | 1 cast; 2 expect() with clear messages |
++| kernel/requests.rs | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 1 cast (u64 saturation) |
++| kernel/ingest.rs | 0 | 0 | 0 | 0 | 0 | 2 | 0 | 0 | 2 casts (u64 saturation) |
++| kernel/update.rs | 0 | 0 | 0 | 0 | 0 | 5 | 0 | 0 | 5 casts (count→u64/u32) |
++| kernel/tests.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ tests only |
++| substrate/mod.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
++| substrate/view.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
++| substrate/action.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
++| substrate/capability.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
++| substrate/domain.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
++| substrate/identity.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
++| **iOS (Swift)** | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ all 9 Swift files clean |
++| **TOTALS** | **0** | **0** | **0** | **0** | **3** | **14** | **1** | **2** | **20 items; 0 bugs** |
++
++---
++
++## 2. Findings
++
++### Finding F1: Unsafe FFI Pointer Dereference — `ffi.rs:75`
++
++**File:** `crates/nmp-core/src/ffi.rs:73–79`  
++**Severity:** Medium (correct code, documentation gap)  
++**Type:** Unsafe without safety comment
++
++```rust
++73  pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
++74      if !app.is_null() {
++75          unsafe {
++76              drop(Box::from_raw(app));
++77          }
++78          }
++79  }
++```
++
++**Issue:** The unsafe block dereferences a raw C pointer returned from FFI without a safety comment explaining the invariant (the caller must guarantee the pointer is a valid NmpApp allocated by `nmp_app_new()`).
++
++**Classification:** **ACCEPTABLE**. Code is correct; this is a standard FFI ownership transfer pattern. Pointer validity is guaranteed by C FFI caller contract.
++
++**Recommendation:** Add inline safety comment for documentation and future maintainability:
++```rust
++// safe: caller guarantees app is a valid pointer allocated by nmp_app_new()
++unsafe { drop(Box::from_raw(app)); }
++```
++
++---
++
++### Finding F2: Unsafe Pointer Dereference — `ffi.rs:275`
++
++**File:** `crates/nmp-core/src/ffi.rs:271–277`  
++**Severity:** Medium (correct code, documentation gap)  
++**Type:** Unsafe without safety comment
++
++```rust
++271  fn app_ref<'a>(app: *mut NmpApp) -> Option<&'a NmpApp> {
++272      if app.is_null() {
++273          None
++274      } else {
++275          Some(unsafe { &*app })
++276      }
++277  }
++```
++
++**Issue:** Unsafe dereference of a raw C FFI pointer without a safety comment. The lifetime `'a` is sound because the reference is scoped to the caller, but the memory safety invariant (pointer validity) is not documented.
++
++**Classification:** **ACCEPTABLE**. The null check makes this safe in context. The pattern is correct.
++
++**Recommendation:** Add inline safety comment:
++```rust
++// safe: caller guarantees non-null app is a valid NmpApp pointer
++Some(unsafe { &*app })
++```
++
++---
++
++### Finding F3: Unsafe C String Conversion — `ffi.rs:284`
++
++**File:** `crates/nmp-core/src/ffi.rs:279–290`  
++**Severity:** Low (standard C FFI pattern, validated downstream)  
++**Type:** Unsafe without safety comment
++
++```rust
++279  fn c_string_argument(ptr: *const c_char) -> Option<String> {
++280      if ptr.is_null() {
++281          return None;
++282      }
++283
++284      unsafe { CStr::from_ptr(ptr) }
++285          .to_str()
++286          .ok()
++287          .map(str::trim)
++288          .filter(|value| !value.is_empty())
++289          .map(ToOwned::to_owned)
++290  }
++```
++
++**Issue:** Standard C FFI pattern but lacks a safety comment. The caller must guarantee the pointer is a valid, null-terminated UTF-8 C string. The unsafe is narrowly scoped and the result is validated via `to_str()`.
++
++**Classification:** **ACCEPTABLE**. Pattern is correct. The result chain (`to_str().ok()`) validates UTF-8 and null-termination.
++
++**Recommendation:** Add safety comment:
++```rust
++// safe: caller guarantees ptr is a valid null-terminated C string.
++// Validation: to_str() will reject invalid UTF-8.
++unsafe { CStr::from_ptr(ptr) }
++```
++
++---
++
++### Finding F4: Allow Dead Code Suppression — `relay_worker.rs:242`
++
++**File:** `crates/nmp-core/src/relay_worker.rs:240–245`  
++**Severity:** Low (defensive programming, clarity gap)  
++**Type:** Compiler attribute without comment
++
++```rust
++238        MaybeTlsStream::Rustls(stream) => {
++239            let tcp = stream.get_ref();
++240            let _ = tcp.set_read_timeout(Some(duration));
++241        }
++242        #[allow(unreachable_patterns)]
++243        _ => {}
++244    }
++```
++
++**Issue:** The `#[allow(unreachable_patterns)]` suppresses a warning for the final catch-all arm. This can occur if `tungstenite::MaybeTlsStream` enum variants change between versions. The intent is clear (defense against future enum variants), but the rationale is not documented in code.
++
++**Classification:** **ACCEPTABLE**. This is defensive programming for a third-party enum. The suppression is narrow and justified.
++
++**Recommendation:** Add comment explaining the defensive intent:
++```rust
++// Stream type may have additional TLS variants in future tungstenite versions
++#[allow(unreachable_patterns)]
++_ => {}
++```
++
++---
++
++### Finding F5: Saturation Casts — `kernel/status.rs:117`
++
++**File:** `crates/nmp-core/src/kernel/status.rs:117`  
++**Severity:** Low (intentional, bounded, explicit overflow handling)  
++**Type:** Integer cast with saturation
++
++```rust
++117  refcount: claim_count.min(u32::MAX as usize) as u32,
++```
++
++**Issue:** Casts a `usize` (which can be >u32 on 64-bit systems) to `u32` after explicit saturation at `u32::MAX`. This is intentional: if there are more than 2^32 profile claims (which will not occur in practice given working-set bounds), cap the refcount at u32::MAX. No silent overflow.
++
++**Classification:** **ACCEPTABLE**. The saturation is explicit and intentional. Pattern is correct and defensive.
++
++**Recommendation:** No change required. The code is correct.
++
++---
++
++### Finding F6–F10: Count-to-Metric Casts — `kernel/update.rs, kernel/nostr.rs`
++
++**Files:**
++- `kernel/update.rs:43–56` (5 casts: `count as u64`, `count as u32`)
++- `kernel/nostr.rs:85` (1 cast: `usize as f64`)
++
++**Severity:** Low (bounded by design, no overflow risk)  
++**Type:** Collection count casts to metric types
++
++**Examples:**
++```rust
++// kernel/update.rs:43
++note_events: self.events.values().filter(|event| event.kind == 1).count() as u64,
++
++// kernel/nostr.rs:85
++numerator as f64 / denominator as f64
++```
++
++**Issue:** Casting collection `.count()` and arithmetic results to metric types. No overflow risk because counts are bounded by working set size constraints (max 5,000 stored events per ADR-0001; max visible 500 per FFI clamp).
++
++**Classification:** **ACCEPTABLE**. Bounded by design. No overflow risk.
++
++**Recommendation:** No change required. Code is correct.
++
++---
++
++### Finding F11–F12: Relay Counter Saturation Casts — `kernel/ingest.rs:13,20` and `kernel/requests.rs:572`
++
++**Files:**
++- `kernel/ingest.rs:13,20` (2 casts: `.len() as u64`)
++- `kernel/requests.rs:572` (1 cast: `.len() as u64`)
++
++**Severity:** Low (safe saturation arithmetic, bounded message sizes)  
++**Type:** Safe saturation casts for telemetry counters
++
++```rust
++// kernel/ingest.rs:13
++relay.counters.bytes_rx = relay.counters.bytes_rx.saturating_add(text.len() as u64);
++```
++
++**Issue:** Casting message length to u64 for counter accumulation. The message length is bounded by relay protocol limits (WebSocket frames are ≤2^63 bytes in practice); saturation is intentional for overflow safety.
++
++**Classification:** **ACCEPTABLE**. Defensive overflow handling via saturation arithmetic.
++
++**Recommendation:** No change required. Pattern is correct.
++
++---
++
++### Finding F13–F14: C FFI Bounds Casts — `ffi.rs:94,296`
++
++**File:** `crates/nmp-core/src/ffi.rs`
++
++**Severity:** Low (caller contract guaranteed, clamped bounds)  
++**Type:** FFI argument coercion
++
++```rust
++// ffi.rs:94
++context: context as usize,  // void* → usize
++
++// ffi.rs:296
++visible_limit.clamp(1, 500) as usize  // c_uint → usize
++```
++
++**Issue:** The first is a FFI callback context pointer (passed back via callback); the second is a clamped c_uint. Both are small values in practice (context is application-controlled; visible limit is clamped to 1–500 by design).
++
++**Classification:** **ACCEPTABLE**. Pointer-to-usize conversion is standard FFI. The clamp bounds the second value.
++
++**Recommendation:** No change required.
++
++---
++
++### Finding F15–F16: Expect Calls — `kernel/status.rs:225,231`
++
++**File:** `crates/nmp-core/src/kernel/status.rs:222–232`  
++**Severity:** Low (invariant maintained by construction)  
++**Type:** Expect with clear message
++
++```rust
++222  pub(super) fn relay(&self, role: RelayRole) -> &RelayHealth {
++223      self.relays
++224          .get(&role)
++225          .expect("relay health initialized for every role")
++226  }
++```
++
++**Issue:** Two `expect()` calls that will panic if the relay HashMap doesn't have an entry for a given role. This can only happen if `Kernel::new()` fails to initialize relays for all `RelayRole` variants.
++
++**Classification:** **ACCEPTABLE**. The invariant is maintained by construction: `Kernel::new()` explicitly initializes relays for all roles via `RelayRole::all().into_iter().map(...)`. The expect message is self-documenting.
++
++**Recommendation:** No change required. Invariant is correct.
++
++---
++
++## 3. Doctrine Violations
++
++### D0 Audit: Kernel Never Grows App Nouns
++
++**Status:** ✅ **COMPLIANT**
++
++**Evidence:**
++- `nmp-core/src/kernel/` contains only Nostr-agnostic data structures (`StoredEvent`, `Profile`, `TimelineItem`), which are domain-neutral wrappers for events.
++- All Nostr-specific parsing (NIP-05, profile format, kind:0 interpretation) is isolated in `kernel/nostr.rs` as utility functions, not as kernel state types.
++- The substrate trait families (`DomainModule`, `ViewModule`, `ActionModule`) in `src/substrate/` define no concrete Nostr types; they are generic over module implementations.
++- The iOS bridge (`KernelBridge.swift`) decodes Nostr-shaped payloads into app structs without leaking type names into the kernel API.
++
++**Violation?** No. The kernel is domain-agnostic.
++
++---
++
++### D1 Audit: Best-Effort Rendering with Placeholders
++
++**Status:** ✅ **COMPLIANT**
++
++**Evidence:**
++- iOS ProfileDetailView (`ProfileViews.swift:51`) renders `ProfileCard.placeholder(pubkey:)` when profile is not yet loaded: `profile ?? ProfileCard.placeholder(pubkey: pubkey)`.
++- TimelineRow and all avatar components render initials + color fallback without spinners, allowing the UI to display immediately while kind:0 events arrive asynchronously.
++- The `KernelModel` projection cache (`projectionCacheTTL = 60s`) keeps previous author/thread views visible while new data is in-flight.
++
++**Violation?** No. Placeholders are rendered immediately; refinement happens in place.
++
++---
++
++### D2 Audit: Reactivity Contract (Composite Reverse Index)
++
++**Status:** ✅ **COMPLIANT**
++
++**Evidence:**
++- `kernel/mod.rs` maintains `wire_subs: HashMap<String, WireSub>` and `profile_claims: HashMap<String, BTreeSet<String>>` — the core reverse index mappings.
++- `kernel/requests.rs` and `kernel/ingest.rs` route all message handling and event arrival through kernel methods, which update reverse index state.
++- No projection bypass: views like `AuthorViewPayload`, `ThreadViewPayload` are computed via `kernel/update.rs::make_update()`, which serializes the index state.
++- iOS `KernelModel` applies updates atomically via `apply(result:)` on the main thread; SwiftUI reactivity is driven by @Published updates.
++
++**Violation?** No. All reactivity flows through the kernel's reverse index.
++
++---
++
++### D3 Audit: Errors Never Cross FFI
++
++**Status:** ✅ **COMPLIANT**
++
++**Evidence:**
++- FFI boundary (`ffi.rs`) exports only void-returning C functions: `nmp_app_start()`, `nmp_app_configure()`, etc.
++- No Result types or error enum variants are exposed through the FFI surface.
++- Error states are communicated via the update callback: `RelayStatus.last_error` and `RelayStatus.last_notice` fields in JSON payloads.
++- The iOS app reads error messages from the kernel model's `relayStatus?.lastError` field and renders them in the UI as advisory diagnostics, not as control flow.
++
++**Violation?** No. Error information crosses the FFI as JSON strings (side-band data), not as FFI error codes or control signals. All error handling decisions remain kernel-side.
++
++---
++
++### D4 Audit: One Writer Per Fact
++
++**Status:** ✅ **COMPLIANT**
++
++**Evidence:**
++- The kernel actor is single-threaded (runs on one OS thread spawned in `ffi.rs:51`).
++- All state mutations go through `run_actor()` in `actor.rs`, which receives commands via a single `mpsc::channel()`.
++- Relay worker threads (`relay_worker.rs`) only send *events* back to the actor; they never mutate kernel state directly.
++- The iOS bridge is @MainActor-annotated (`KernelModel`), ensuring all SwiftUI state updates are serialized.
++
++**Violation?** No. Single-writer-per-subsystem is enforced architecturally.
++
++---
++
++### D5 Audit: Capabilities Report, Never Decide
++
++**Status:** ✅ **COMPLIANT**
++
++**Evidence:**
++- The iOS FFI bridge exposes capability requests as simple commands (`openAuthor()`, `claimProfile()`, etc.) with no decision-making logic at the boundary.
++- The kernel processes these as `ActorCommand` variants, routing them to domain logic.
++- No policy decisions are made in `KernelBridge.swift` or the iOS-side FFI wrapper; all routing and business logic lives in the kernel.
++- The `CapabilityModule` trait (in `substrate/capability.rs`) defines how modules *report* capabilities, not how the bridge decides what to expose.
++
++**Violation?** No. The bridge is a pure relay; decisions are kernel-side.
++
++---
++
++## 4. Acceptable & Justified Findings (No Action Required)
++
++| Finding | Classification | Justification |
++|---------|---|---|
++| 3 unsafe blocks in ffi.rs (F1, F2, F3) | Documentation gap | Standard FFI pattern; pointers validated by caller contract. Safety comments recommended for future audits but code is correct. |
++| allow(unreachable_patterns) in relay_worker.rs (F4) | Documentation gap | Defensive programming for third-party enum evolution. Code is correct; add comment for clarity. |
++| 14 integer casts (count→metric types) (F6–F14) | Acceptable | All bounded by design constraints; no overflow risk. Casts are intentional and safe. |
++| 2 expect() calls in kernel/status.rs (F15, F16) | Acceptable | Invariant maintained by construction (relay HashMap initialized for all roles in `Kernel::new()`). |
++| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
++| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
++
++---
++
++## 5. Recommended Next Actions
++
++### Documentation-Only Improvements (Recommended for M10.5)
++
++These are **not bugs**; the code is correct. Adding safety comments improves auditability and prevents future misclassification:
++
++1. **Add safety comments to ffi.rs unsafe blocks** (F1, F2, F3)
++   - Files: `crates/nmp-core/src/ffi.rs` (3 locations: lines 75, 275, 284)
++   - Effort: 5 min
++   - Impact: Documents FFI contract; improves future audits
++   - Recommended text:
++     - Line 75: `// safe: caller guarantees app is valid, allocated by nmp_app_new()`
++     - Line 275: `// safe: caller guarantees non-null app is a valid NmpApp pointer`
++     - Line 284: `// safe: caller guarantees ptr is a valid null-terminated C string; to_str() validates UTF-8`
++
++2. **Add code comment to relay_worker.rs#242** (F4)
++   - Files: `crates/nmp-core/src/relay_worker.rs` (line 242)
++   - Effort: 2 min
++   - Impact: Clarifies defensive intent
++   - Recommended text: `// Stream type may have additional TLS variants in future tungstenite versions`
++
++### No Action Required
++
++- All integer casts (F6–F14) are bounded and intentional; no comments needed.
++- Both expect() calls (F15, F16) are justified by invariants; no action needed.
++- All Swift iOS code is clean (9 files audited; 0 findings).
++- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
++
++---
++
++## 6. Metrics
++
++| Category | Count | Status |
++|----------|-------|--------|
++| **Code Quality** | | |
++| Rust files audited | 19 | ✓ |
++| Swift files audited | 9 | ✓ |
++| Total LOC scanned | 6,559 | ✓ |
++| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
++| Code bugs found | 0 | ✓ No bugs |
++| Unsafe blocks without safety comments | 3 | ⚠️ Documentation gap |
++| Integer casts | 14 | ✓ Justified |
++| Cardinal doctrine violations | 0 | ✓ Compliant |
++| **Post-Audit Actions** | | |
++| Critical bugs | 0 | ✓ |
++| Code changes required | 0 | ✓ |
++| Documentation improvements (optional) | 3 | → Recommended |
++
++---
++
++## 7. Conclusion
++
++The FFI and iOS bridge code is **production-ready for M10.5**. 
++
++**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
++
++**Recommendation:** The 3 safety comments in ffi.rs are optional but recommended for auditability and preventing future misclassification.
++
++**M10.5 Exit Criteria:** ✅ **READY**
++
++- ✅ Zero blocking debt markers
++- ✅ Zero code bugs
++- ✅ All doctrines compliant
++- ✅ iOS bridge code clean (9 files, 0 findings)
++- ✅ Unsafe blocks are correct (documentation gap only)
++
++**Audit Sign-Off:** Ready for iOS empirical proof phase (M10.5).
+diff --git a/docs/perf/orchestration-log.md b/docs/perf/orchestration-log.md
+new file mode 100644
+index 0000000..30d5671
+--- /dev/null
++++ b/docs/perf/orchestration-log.md
+@@ -0,0 +1,8 @@
++# Orchestration Log
++
++Durable trail of the parallel-agent orchestration. One line per heartbeat or significant event.
++
++| When (local) | # | Event |
++|---|---|---|
++| 2026-05-18 01:24 | 0 | Session start. Pulled to e9cbafa. Plan revised (55dd5f2) inserting M10.5 (FFI hardening) and concretizing M11 (`../podcast` rebuild). 15-min cron heartbeat armed. First wave of 6 background agents dispatched: build-verifier (T7), debt-auditor (T6), m2-designer (T2), m3-designer (T3), m105-designer (T4), m11-designer (T5). T1 blocked on T7. |
++| 2026-05-18 01:30 | 0a | Advisor pass: broadcast safe-rebase-push protocol to all 6 running agents (avoid push race on master). T1 description updated to mandate worktree isolation + rebase-push protocol. Heartbeat cron rewritten (job 811003f1) with stronger triage rules (design→review→impl chain; M11 gated on M10.5 *empirical* pass, not just designed; debt triage uses both must-fix and ADR-defer lanes; DerivedData sprawl mitigation; orphan detection). Heartbeat runtime is session-only (durable flag ignored). 6 stale stashes from prior codex sessions dropped. |
+diff --git a/docs/plan.md b/docs/plan.md
+index 81e1702..32c8471 100644
+--- a/docs/plan.md
++++ b/docs/plan.md
+@@ -2,9 +2,11 @@
+ 
+ > Companion to `docs/product-spec.md` (what we ship) and the design docs in `docs/design/` (how each subsystem works). This document defines **the single ladder of milestones**, each one a runnable product that proves a specific architectural claim with real (not modeled) evidence.
+ 
+-> **Three arcs:** Kernel substrate + Nostr social stack (M0–M10) → kernel-boundary proof with a non-social-domain app (M11) → wallet/WoT + cross-platform + release (M12–M17).
++> **Four arcs:** Kernel substrate + Nostr social stack (M0–M10) → FFI hardening + iOS empirical proof (M10.5) → kernel-boundary proof with a non-social-domain app (M11, the **`../podcast` rebuild on NMP**) → wallet/WoT + cross-platform + release (M12–M17).
+ 
+-> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. No silent endings.
++> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.
++
++> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
+ 
+ ---
+ 
+@@ -48,8 +50,10 @@ Honest accounting before forecasting forward.
+ - Blossom and media-capability lifecycle (long-running, resumable, background) were one bullet under Phase 6.
+ - No milestone proved the kernel boundary for a fundamentally non-social product.
+ - The plan didn't reflect that M0 and M1 are largely done.
++- **No dedicated FFI hardening + iOS empirical proof gate before the kernel-boundary proof.** The prior M11 implicitly assumed the FFI surface was ready; this rewrite makes it a separate milestone (M10.5).
++- **M11 was generic.** This rewrite ties it concretely to `/Users/pablofernandez/src/podcast` (the fully-functional Swift app) as the rebuild target, with copy-first UI fidelity and an explicit view-by-view module mapping.
+ 
+-The plan below is a single ladder of seventeen milestones (M0–M17), each producing a runnable artifact, ordered so that each milestone strictly adds capabilities to the prior demoable product.
++The plan below is a single ladder of eighteen milestones (M0–M17, with M10.5 inserted as the FFI gate), each producing a runnable artifact, ordered so that each milestone strictly adds capabilities to the prior demoable product.
+ 
+ ---
+ 
+@@ -334,48 +338,146 @@ Each milestone has: **demo product**, **scope (what gets built)**, **subsystem d
+ 
+ ---
+ 
+-### M11 — Podcast app (the kernel-boundary proof in a non-social domain)
++### M10.5 — FFI hardening + iOS empirical proof *(hard gate before M11 starts)*
++
++**Demo product:** The iOS Twitter slice from M1–M10 subjected to a published, exhaustive stress harness on the iOS simulator and a real iPhone 12. The kernel↔FFI↔SwiftUI path is proven, in measured numbers, to be **rock-solid and demonstrably performant** before a single line of the podcast app is written.
++
++**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
++
++**Scope.**
++
++- **Stress harness** (`crates/nmp-testing/bin/ffi-stress` + `ios/NmpStress/StressUITests/`):
++  - Mount/unmount churn: 1000 view-handle wrappers cycled per second for 10 minutes; assert zero leaks (via Instruments leak instrument scripted run).
++  - Dispatch flood: 10k `dispatch(...)` calls per second from Swift across multiple threads; assert no dropped messages, no main-thread block > 16 ms.
++  - Snapshot pressure: `AppUpdate::FullState` with 100k events forced; measure marshal time, allocations, and that the reconciler stays ≤ 60 Hz via batching.
++  - Reconciler back-pressure: deliberately stall the Swift main thread for 250 ms; assert no actor stall, deltas accumulate and replay correctly when the main thread resumes.
++  - Reentrancy: dispatch from inside a reconciler callback (a known footgun); assert ordered, deadlock-free.
++  - Capability lifecycle storms: start/stop/restart each registered capability 1000 times; assert idempotency per RMP bible.
++  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
++- **Real-device measurement on iPhone 12** (one full battery of `firehose-bench live` against primal, all 8 scenarios from `docs/design/firehose-bench.md` §3); produces `docs/perf/m10.5/iphone12-baseline.md` with hardware-tagged numbers.
++- **Simulator-driven UI test fleet** (parallel Sonnet agents via the `mcp__xcode` and `BrowserAgent`/`QATester` skills) exercising the app from the outside — boot sim, launch app, tap, scroll, swipe, kill-relaunch — capturing screenshots and assertions per scripted scenario. Every M1–M10 user-visible feature gets a UI test; failures block the milestone.
++- **Memory + leak audit** with Xcode Instruments (Leaks, Allocations, Time Profiler) on canonical workflows; zero retained-by-cycle leaks; allocations after warmup linear-or-better in active-view count, never in cached-event count.
++- **Profile-Guided Optimization sweep** on the kernel hot paths surfaced by Time Profiler; document tradeoffs taken.
++- **All M1–M10 perf reports re-run** on the final FFI surface to confirm no regressions.
++- **FFI surface documentation audit** in `docs/ffi-surface.md` — every exported type, function, capability trait, and ownership/lifetime invariant called out; reviewed against `RMP-ARCHITECTURE-BIBLE.md` commandments and ADR-0010.
++- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
++
++**Subsystem deliverables.**
++
++- `crates/nmp-testing/bin/ffi-stress` — new bench binary.
++- `ios/NmpStress/StressUITests` — XCUITest target driven by both XCTest and (where relevant) a scripted Sonnet-agent runner.
++- `docs/design/ffi-hardening.md` — design doc enumerating every FFI failure mode and how the harness exercises it.
++- `docs/ffi-surface.md` — the canonical FFI surface reference.
++- `docs/perf/m10.5/` — measured numbers from simulator, M-series Mac, iPhone 12; plus screenshots from the Sonnet-driven UI runs.
++
++**Exit gate.**
++
++- All stress-harness scenarios pass on simulator and iPhone 12 with the numeric thresholds enumerated in `docs/design/ffi-hardening.md` §exit-gate.
++- All M1–M10 perf reports re-run cleanly on the post-M10.5 binaries; no regression > 5 % on any p99 number.
++- Instruments-recorded Leaks count = 0 over the 10-minute canonical workflow.
++- Every UI-scripted scenario (Sonnet-agent and XCUITest) passes on a fresh boot of the iPhone 16 Pro simulator and on iPhone 12 hardware.
++- `docs/ffi-surface.md` reviewed and tagged.
++- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
++
++**Runnable artifact.** Same iOS Twitter app, now load-bearing. Report bundle in `docs/perf/m10.5/`.
++
++---
++
++### M11 — Podcast app (the `../podcast` rebuild on NMP — the kernel-boundary proof)
+ 
+-**Demo product:** A podcast app built entirely as an extension-module set, sharing nothing app-specific with `nmp-core`. Subscribes to podcast feeds. Downloads episodes. Plays them with background audio. Resumes playback position across app launches. Pulls feed updates via Nostr where available, RSS where not.
++**Demo product:** A 1:1 rebuild of `/Users/pablofernandez/src/podcast` (the fully-functional Swift app, 20 SwiftUI views, ~8.8k LOC of Swift) running on NMP. **UI is pixel-identical** to the reference Swift app; **all business logic, LLM, audio orchestration, downloads, transcripts, RAG, recommendations** are in Rust extension modules driving the kernel.
+ 
+ **This is the load-bearing kernel-boundary check.** If the kernel needs even one podcast noun to make this work, the boundary is wrong and we go back to fix it.
+ 
++**Reference inputs** (read before scoping):
++
++- `/Users/pablofernandez/src/podcast/` — canonical Swift implementation. Source of truth for UI and feature behavior. **Every view in `PodcastApp/Views/` is copied verbatim into `ios/NmpPodcast/Views/`** as step 1; only the data source is rewired.
++- `/Users/pablofernandez/src/podcast-rmp/` — prior WIP RMP rewrite (incomplete). **Not a code source** but a lessons source: read its `RMP-ARCHITECTURE-BIBLE.md`, `FINAL_PLAN.md`, `docs/plans/iphone-feature-parity-plan.md`, and `docs/plans/iphone-feature-parity-checklist.md` before scoping. That repo's `AGENTS.md` is the working guide for any agent touching that tree.
++- `/Users/pablofernandez/src/podcast/docs/plans/` — original feature design docs (podcast-app-design, discovery-tab-redesign, insights-feature-design).
++
++**Reference inventory of the Swift app** (so the scope is explicit, not vibes):
++
++| Swift `Views/` group | Files | NMP target |
++|---|---|---|
++| `Ask/` | AskView.swift | `ask-core` ActionModule + ViewModule wrapping `rig.rs` LLM call |
++| `Components/` | CachedAsyncImage, DiscoveryCards | reusable Swift components, ported as-is; image cache backed by NMP Blossom-aware capability |
++| `Feed/` | FeedView, EpisodeRow | `podcast-core::FeedViewModule` + `EpisodeRowViewModule` |
++| `Insights/` | InsightsView | `insights-core` ViewModule + ActionModule (uses RAG via `rig.rs`) |
++| `Library/` | ActivityView, AddPodcastView, DiscoverView, EpisodeDetailView, LibraryView, PodcastDetailSheet, PodcastDetailView, QueueView | `podcast-core` ViewModules + ActionModules |
++| `Player/` | ChaptersPanel, GuestAgentSheet, MiniPlayer, PlayerSheet, TranscriptView | `player-core` ViewModule + `AudioPlaybackCapability` |
++| `Settings/` | SettingsView | `settings-core` ActionModule (mostly capability invocations) |
++
++Swift `Services/` (AIService, AudioService, DownloadService, GuestEnrichmentService, ImageCache, InsightService, PodcastIndexService, PodcastService, ProcessingQueue, RAGService, RecommendationService, ServiceContainer, TranscriptionService, VectorDatabase) **all move to Rust** as ActionModules + ProjectionCaches + capability bridges; Swift loses its Services/ directory entirely.
++
++Swift `Models/` (AITypes, Chapter, Episode, Guest, Insight, Podcast, Settings, Transcript) **all move to Rust** as DomainRecords inside `podcast-core` and sibling crates.
++
++Swift `ViewModels/` **disappear** — they become Rust ViewModules whose output crosses FFI as typed ViewBatch deltas.
++
+ **Scope.**
+ 
+-**Subsystem deliverables (extension modules — not in `nmp-core`):**
++**Step 0 — copy step (UI-fidelity invariant lock):**
+ 
+-- `podcast-core` app crate:
+-  - `DomainModule`s: `Podcast`, `Episode`, `Transcript`, `PlayerState`, `Subscription`.
+-  - `ViewModule`s: `PodcastLibrary`, `EpisodeDetail`, `NowPlaying`, `EpisodeQueue`.
+-  - `ActionModule`s: `SubscribePodcast`, `RefreshFeed`, `DownloadEpisode`, `Play`, `Pause`, `Seek`, `MarkPlayed`, `ImportRss`.
+-  - `IdentityModule::AppLocal` if anonymous subscription syncing across devices is wanted.
++- Copy every file in `/Users/pablofernandez/src/podcast/PodcastApp/Views/` into `ios/NmpPodcast/NmpPodcast/Views/` verbatim. Commit immediately. No edits except the minimum needed to compile against placeholder data sources (`// MARK: NMP-WIRE` markers).
++- Copy `Resources/Assets.xcassets` and `Info.plist` (sanitized) verbatim.
++- The result compiles and renders against stubbed data; UI is visually identical to `../podcast` per a side-by-side simulator screenshot diff (≤ 1 px tolerance, font-rendering exceptions documented).
+ 
+-**Subsystem deliverables (capabilities added to the kernel's reusable set):**
++**Step 1 — domain + view modules in Rust** (per the table above):
+ 
+-- `AudioPlaybackCapability`: kernel asks the platform to play a URL or local file; platform reports position events + state transitions back. iOS implementation via `AVPlayer` + background-audio entitlement.
+-- `BackgroundWorkCapability`: kernel registers periodic background tasks (feed refresh, scheduled downloads); platform implements via BGTask scheduler (iOS) / WorkManager (Android).
++- `apps/podcast/podcast-core/` — main app crate. `DomainModule`s: `Podcast`, `Episode`, `Transcript`, `Chapter`, `Guest`, `Insight`, `Subscription`, `PlayerState`, `QueueEntry`, `Activity`.
++- `apps/podcast/podcast-core/` — `ViewModule`s: `PodcastLibrary`, `EpisodeDetail`, `NowPlaying`, `EpisodeQueue`, `Discover`, `Insights`, `Activity`, `PodcastDetail`, `Feed`, `EpisodeRow`, `Chapters`, `Transcript`, `MiniPlayer`, `PlayerSheet`, `GuestAgent`, `Ask`, `Settings`.
++- `apps/podcast/podcast-core/` — `ActionModule`s: `SubscribePodcast`, `UnsubscribePodcast`, `RefreshFeed`, `DownloadEpisode`, `CancelDownload`, `Play`, `Pause`, `Seek`, `SkipForward`, `SkipBack`, `MarkPlayed`, `EnqueueEpisode`, `ReorderQueue`, `ImportRss`, `ImportOpml`, `AskQuestion`, `EnrichGuest`, `RunInsight`, `SearchPodcasts`.
++- `apps/podcast/podcast-llm/` — LLM-driven actions via `rig.rs`: `AskQuestion`, `EnrichGuest`, `RunInsight`. Uses the kernel's capability bridge for HTTP + key storage.
++- `apps/podcast/podcast-rag/` — RAG + vector DB store; uses a swappable `EmbeddingCapability` and a Rust-side vector store (sqlite-vss or qdrant-client).
++- `apps/podcast/podcast-feeds/` — RSS + Atom + JSON Feed + Podcast 2.0 namespaces parsing; transcripts; chapters; value-for-value. Pure Rust; pulls via `HttpCapability`.
++
++**Step 2 — capabilities added to the kernel's reusable set** (these are general, not podcast-specific):
++
++- `AudioPlaybackCapability`: play URL or local file; report position events + state transitions back; iOS impl via `AVPlayer` + background-audio entitlement + lock-screen `MPNowPlayingInfoCenter`/`MPRemoteCommandCenter`.
++- `BackgroundWorkCapability`: register periodic background tasks; iOS impl via `BGTaskScheduler`.
+ - `LocalNotificationCapability`: extended for episode-available alerts.
+-- `HttpCapability`: extended for podcast feed fetch (long-running streaming response).
++- `HttpCapability`: long-running streaming response support (RSS, transcripts).
++- `EmbeddingCapability`: callable embedding model; kernel-owned policy, platform-owned execution (CoreML on iOS, ONNX or remote API as fallback).
++- `KeyValueStoreCapability`: typed persistent KV (for saved playback position when persistence-by-store is overkill).
++
++**Step 3 — protocol module integration:**
++
++- `nmp-podcast` (Nostr podcast NIP integration where it exists — NIP-XX podcast feed events, value-for-value zaps, episode discussion threads). Where Nostr coverage is incomplete, the app uses RSS via `podcast-feeds` and Nostr for social overlay (zaps, discussions, recommendations from the WoT subsystem).
+ 
+-**Subsystem deliverables (protocol modules):**
++**Step 4 — wire each copied Swift view to its Rust view module:**
+ 
+-- `nmp-podcast` (or whatever the Nostr podcast NIP is called, e.g. NIP-XX for podcast feed events): parsed feed events. If no NIP, the app uses RSS via the action ledger to fetch + parse, storing entries as domain records.
++- Replace stubbed data with the generated wrapper hooks (`@PodcastLibrary`, `@NowPlaying`, etc. — produced by `nmp gen modules`).
++- The Swift file shape stays the same; only the data source changes.
++- After every Library/Feed/Player/Insights/Ask/Settings group is wired, run the side-by-side screenshot diff again.
+ 
+ **Exit gate (kernel boundary).**
+ 
+-- **`nmp-core` gains zero podcast nouns.** No `Podcast`, `Episode`, `Transcript`, `Player`, `Feed` types added to the kernel. Verified by grep + manual review at the commit.
+-- **The capability families added in M11 are general** (audio playback, background work, local notifications, HTTP). Their request/response shapes are not podcast-specific.
+-- **Reactivity behavior is identical** to the social demo — composite-key dependencies, delta coalescing, claim-based GC, ADR-0007 diagnostics all work for podcast view modules.
++- **`nmp-core` gains zero podcast nouns.** No `Podcast`, `Episode`, `Transcript`, `Chapter`, `Player`, `Feed`, `Insight`, `Guest` types added to the kernel. Verified by grep + manual review at the commit.
++- **The capability families added in M11 are general** (audio playback, background work, local notifications, HTTP, embedding, KV-store). Their request/response shapes are not podcast-specific.
++- **Reactivity behavior is identical** to the Twitter slice — composite-key dependencies, delta coalescing, claim-based GC, ADR-0007 diagnostics all work for podcast view modules.
++- **No app-state leaks across the boundary in either direction:** no Nostr type appears in `podcast-core`'s public surface; no podcast type appears in `nmp-core`'s public surface.
+ 
+-**Exit gate (product).**
++**Exit gate (product fidelity to `../podcast`).**
+ 
+-- Subscribe to 5 real podcasts (use any well-known Nostr-podcast feeds if available, plus RSS imports).
+-- Download an episode in the background while the app is suspended.
+-- Play it with background audio while the iPhone is locked.
+-- Resume playback at the correct position after a kill-relaunch.
+-- Push notification on a new episode arrival.
++- **UI parity:** side-by-side screenshot of every screen in `../podcast` vs `ios/NmpPodcast` matches at ≤ 1 px tolerance (font/rendering differences whitelisted explicitly in `docs/perf/m11/parity-screenshots.md`).
++- **Feature parity:** every user flow exercised in `/Users/pablofernandez/src/podcast/Tests/` (or its equivalent on the canonical Swift app) reproduced as a scripted Sonnet-agent run on `ios/NmpPodcast`. No "feature dropped" footnotes.
++- **Subscribe to 10 real podcasts** spanning RSS + (where available) Nostr feeds; library populates correctly.
++- **Download an episode in the background** while the app is suspended; resumable on relaunch.
++- **Play with background audio** while the iPhone is locked; lock-screen artwork, scrubber, skip/seek controls all functional.
++- **Resume playback at the correct position** after a kill-relaunch.
++- **Push notification on a new episode arrival.**
++- **Ask a question** about an episode; answer streams in via `rig.rs` LLM with the transcript as RAG context.
++- **Insights** view generates a structured episode summary on demand.
++- **Guest enrichment** populates guest cards via external lookup, identical to the Swift impl behavior.
+ 
+-**Runnable artifact.** A second iOS app (`ios/NmpPodcast`) — distinct binary, same Rust kernel, different module set. Report in `docs/perf/m11/podcast-app.md` documenting the kernel-boundary verification.
++**Stress + perf gates.**
++
++- Library of 100 podcasts × 50 episodes (5k episodes total) scrolls at 60 fps on iPhone 12.
++- Player UI updates every 250 ms during playback without visible jank.
++- Download queue with 20 concurrent downloads keeps the UI responsive.
++- LLM ask flow streams first token in ≤ 1500 ms over Wi-Fi; full answer in ≤ 8 s for an average-length episode (measured).
++- Battery drain during 1 hour of background playback ≤ Swift baseline + 10 %.
++
++**Runnable artifact.** `ios/NmpPodcast` — distinct binary, same Rust kernel, different module set, **same UI as `../podcast`**. Report in `docs/perf/m11/podcast-app.md` documenting kernel-boundary verification, parity screenshots, and the perf measurements above.
+ 
+ ---
+ 
+@@ -546,6 +648,8 @@ Cross-reference of which milestone delivers which user-specified concern.
+ | **NDK-style subscription aggregation** | M2 | Per `docs/design/ndk-applesauce-lessons.md` §7, the planner becomes a subscription compiler. Logical interests → per-relay plans → wire REQs, semantics-preserving merge/split. |
+ | **Reactivity as planned** | M0–M7 | Already validated by reactivity-bench run 002 against the model; M1 runs the same code path against real iOS; subsequent milestones add view modules that exercise the contract under varied loads. |
+ | **Non-Nostr data bridge** | M0 (substrate), M10 (long-running capabilities), M11 (podcast app proves it in production) | DomainModule trait + ADR-0007 bridge lanes; first proven by fixture-todo-core; production proof in podcast app. |
++| **FFI hardening + empirical iOS proof** | M10.5 | Dedicated stress harness, real-device measurement, simulator-driven Sonnet-agent UI suite; hard gate before M11. |
++| **UI parity to `../podcast`** | M11 (copy step) | Every Swift view copied verbatim, screenshot-diff gated. |
+ | **NIP-42 auth** | M5 | Per-relay auth state machine; integrates with diagnostics; works with both local-key and NIP-46 signers. |
+ | **Blossom** | M10 | Upload + download with resumable progress; long-running capability lifecycle. |
+ | **Multi-session clients** | M8 | Per-account view-spec scoping; account switcher; isolation tests. |
+@@ -593,10 +697,15 @@ The ladder above is the **dependency order** — what must precede what — not
+ - **M2 (outbox), M3 (LMDB), M4 (negentropy)** can pipeline tightly: M3 + M4 are almost mechanically pluggable once M2's compiled-plan abstraction exists.
+ - **M5 (NIP-42)** is independent of M3/M4 and can be done alongside.
+ - **M6 (signer + write path) is a serialization point** — most downstream milestones (M7, M8, M9, M10, M12) depend on it. Land this fast.
++- **M10.5 (FFI hardening)** is itself parallelizable: the stress harness, the iPhone-12 perf rerun, the UI-script Sonnet-agent fleet, and the FFI surface audit are four independent workstreams.
++- **M11 (podcast app)** starts only after M10.5 passes. Its own internal parallelism is wide: the copy step + each `*-core` Rust extension crate + each view-wiring batch can be split across agents (one per view group: Library, Feed, Player, Insights, Ask, Settings, Components, plus one agent per LLM/RAG/feeds module).
+ - **M15 (Android + Desktop + Web)** is three parallel tracks once M14 (UniFFI) lands.
+-- **M11 (podcast app)** can begin as soon as M10 (Blossom + long-running capabilities) is in good shape, even if M12/M13 haven't started.
+ 
+-A team of two could run M5 alongside the M2–M4 sequence with no integration risk.
++A team of two could run M5 alongside the M2–M4 sequence with no integration risk. With parallel-agent execution (this session's mode), the practical limit is conflict surface: independent crates, independent docs, and independent platform shells fan out cleanly; shared mutable files (e.g. `nmp.toml`, the codegen output, `Cargo.toml`) serialize.
++
++### Worktree hygiene
++
++Every parallel worker that mutates source operates in its own git worktree under `.claude/worktrees/`. **On merge, the worktree is removed** (`git worktree remove --force` + branch cleanup) by the worker before the parent acknowledges done — otherwise DerivedData and `target/` clones blow out the disk fast. The known precedent is podcast-rmp's `~/Library/Developer/Xcode/DerivedData/Podcastr-*` sprawl; we share `CARGO_TARGET_DIR` and `-derivedDataPath` across worktrees from the start to avoid it.
+ 
+ ---
+ 
+
+Review the cumulative change. Look for:
+1. Doctrine compliance — any rule weakening?
+2. TODO / FIXME / unimplemented / "for later" snuck in
+3. Test coverage for what changed (note: 3 of 4 commits are docs)
+4. File-size budget — any new file > 300 LOC?
+5. Plan coherence — does M10.5 + M11 expansion + orchestration log fit together cleanly?
+6. Anything that looks like a shortcut that should be flagged
+
+Be terse. If fine, say so. If a real concern: name the file/section/line and what to fix. No filler.
+codex
+I’ll verify the merged snapshot, then check the new docs against the stated doctrine, LOC ceiling, and shortcut language with exact line refs.
+exec
+/bin/zsh -lc 'wc -l docs/perf/m10.5/debt-inventory.md docs/perf/orchestration-log.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     434 docs/perf/m10.5/debt-inventory.md
+       8 docs/perf/orchestration-log.md
+     789 docs/plan.md
+    1231 total
+
+exec
+/bin/zsh -lc 'git rev-parse --short HEAD' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+f1e374b
+
+exec
+/bin/zsh -lc 'git status -sb' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+## master...origin/master [ahead 1]
+?? .claude/
+?? docs/design/ffi-hardening.md
+?? docs/design/lmdb-schema.md
+?? docs/design/lmdb/
+?? docs/design/subscription-compilation.md
+?? docs/design/subscription-compilation/
+?? docs/perf/codex-reviews/
+?? docs/perf/m10.5/debt-inventory-revised.md
+
+exec
+/bin/zsh -lc 'rg -n "TODO|FIXME|XXX|unimplemented!|todo!|for later|later|Optional|optional|recommended|Recommended|No action required|defer|deferral|future|worktree remove --force|dropped" docs/perf/m10.5/debt-inventory.md docs/perf/orchestration-log.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+docs/perf/m10.5/debt-inventory.md:60:**Recommendation:** Add inline safety comment for documentation and future maintainability:
+docs/perf/m10.5/debt-inventory.md:146:**Issue:** The `#[allow(unreachable_patterns)]` suppresses a warning for the final catch-all arm. This can occur if `tungstenite::MaybeTlsStream` enum variants change between versions. The intent is clear (defense against future enum variants), but the rationale is not documented in code.
+docs/perf/m10.5/debt-inventory.md:152:// Stream type may have additional TLS variants in future tungstenite versions
+docs/perf/m10.5/debt-inventory.md:359:| 3 unsafe blocks in ffi.rs (F1, F2, F3) | Documentation gap | Standard FFI pattern; pointers validated by caller contract. Safety comments recommended for future audits but code is correct. |
+docs/perf/m10.5/debt-inventory.md:368:## 5. Recommended Next Actions
+docs/perf/m10.5/debt-inventory.md:370:### Documentation-Only Improvements (Recommended for M10.5)
+docs/perf/m10.5/debt-inventory.md:372:These are **not bugs**; the code is correct. Adding safety comments improves auditability and prevents future misclassification:
+docs/perf/m10.5/debt-inventory.md:377:   - Impact: Documents FFI contract; improves future audits
+docs/perf/m10.5/debt-inventory.md:378:   - Recommended text:
+docs/perf/m10.5/debt-inventory.md:387:   - Recommended text: `// Stream type may have additional TLS variants in future tungstenite versions`
+docs/perf/m10.5/debt-inventory.md:406:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+docs/perf/m10.5/debt-inventory.md:414:| Documentation improvements (optional) | 3 | → Recommended |
+docs/perf/m10.5/debt-inventory.md:424:**Recommendation:** The 3 safety comments in ffi.rs are optional but recommended for auditability and preventing future misclassification.
+docs/plan.md:7:> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.
+docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
+docs/plan.md:64:3. **Capability layering is strict.** Each milestone adds exactly one new architectural ingredient on top of the previous demo. No "we'll wire it up later" — wiring is the milestone.
+docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
+docs/plan.md:231:- Login UX (single nsec field for now; multi-step onboarding deferred to M16).
+docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+docs/plan.md:351:  - Dispatch flood: 10k `dispatch(...)` calls per second from Swift across multiple threads; assert no dropped messages, no main-thread block > 16 ms.
+docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/plan.md:462:- **Feature parity:** every user flow exercised in `/Users/pablofernandez/src/podcast/Tests/` (or its equivalent on the canonical Swift app) reproduced as a scripted Sonnet-agent run on `ios/NmpPodcast`. No "feature dropped" footnotes.
+docs/plan.md:708:Every parallel worker that mutates source operates in its own git worktree under `.claude/worktrees/`. **On merge, the worktree is removed** (`git worktree remove --force` + branch cleanup) by the worker before the parent acknowledges done — otherwise DerivedData and `target/` clones blow out the disk fast. The known precedent is podcast-rmp's `~/Library/Developer/Xcode/DerivedData/Podcastr-*` sprawl; we share `CARGO_TARGET_DIR` and `-derivedDataPath` across worktrees from the start to avoid it.
+docs/perf/orchestration-log.md:8:| 2026-05-18 01:30 | 0a | Advisor pass: broadcast safe-rebase-push protocol to all 6 running agents (avoid push race on master). T1 description updated to mandate worktree isolation + rebase-push protocol. Heartbeat cron rewritten (job 811003f1) with stronger triage rules (design→review→impl chain; M11 gated on M10.5 *empirical* pass, not just designed; debt triage uses both must-fix and ADR-defer lanes; DerivedData sprawl mitigation; orphan detection). Heartbeat runtime is session-only (durable flag ignored). 6 stale stashes from prior codex sessions dropped. |
+
+codex
+The first pass found two concrete review axes already: a new docs file over the 300-line soft budget, and the debt inventory treating known documentation gaps as optional despite the new “no debt / no for-later carve-outs” language. I’m checking the exact sections now so the final review can be line-specific.
+exec
+/bin/zsh -lc "nl -ba docs/perf/m10.5/debt-inventory.md | sed -n '360,435p'" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc "nl -ba docs/plan.md | sed -n '338,385p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+   360	| allow(unreachable_patterns) in relay_worker.rs (F4) | Documentation gap | Defensive programming for third-party enum evolution. Code is correct; add comment for clarity. |
+   361	| 14 integer casts (count→metric types) (F6–F14) | Acceptable | All bounded by design constraints; no overflow risk. Casts are intentional and safe. |
+   362	| 2 expect() calls in kernel/status.rs (F15, F16) | Acceptable | Invariant maintained by construction (relay HashMap initialized for all roles in `Kernel::new()`). |
+   363	| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
+   364	| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
+   365	
+   366	---
+   367	
+   368	## 5. Recommended Next Actions
+   369	
+   370	### Documentation-Only Improvements (Recommended for M10.5)
+   371	
+   372	These are **not bugs**; the code is correct. Adding safety comments improves auditability and prevents future misclassification:
+   373	
+   374	1. **Add safety comments to ffi.rs unsafe blocks** (F1, F2, F3)
+   375	   - Files: `crates/nmp-core/src/ffi.rs` (3 locations: lines 75, 275, 284)
+   376	   - Effort: 5 min
+   377	   - Impact: Documents FFI contract; improves future audits
+   378	   - Recommended text:
+   379	     - Line 75: `// safe: caller guarantees app is valid, allocated by nmp_app_new()`
+   380	     - Line 275: `// safe: caller guarantees non-null app is a valid NmpApp pointer`
+   381	     - Line 284: `// safe: caller guarantees ptr is a valid null-terminated C string; to_str() validates UTF-8`
+   382	
+   383	2. **Add code comment to relay_worker.rs#242** (F4)
+   384	   - Files: `crates/nmp-core/src/relay_worker.rs` (line 242)
+   385	   - Effort: 2 min
+   386	   - Impact: Clarifies defensive intent
+   387	   - Recommended text: `// Stream type may have additional TLS variants in future tungstenite versions`
+   388	
+   389	### No Action Required
+   390	
+   391	- All integer casts (F6–F14) are bounded and intentional; no comments needed.
+   392	- Both expect() calls (F15, F16) are justified by invariants; no action needed.
+   393	- All Swift iOS code is clean (9 files audited; 0 findings).
+   394	- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
+   395	
+   396	---
+   397	
+   398	## 6. Metrics
+   399	
+   400	| Category | Count | Status |
+   401	|----------|-------|--------|
+   402	| **Code Quality** | | |
+   403	| Rust files audited | 19 | ✓ |
+   404	| Swift files audited | 9 | ✓ |
+   405	| Total LOC scanned | 6,559 | ✓ |
+   406	| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+   407	| Code bugs found | 0 | ✓ No bugs |
+   408	| Unsafe blocks without safety comments | 3 | ⚠️ Documentation gap |
+   409	| Integer casts | 14 | ✓ Justified |
+   410	| Cardinal doctrine violations | 0 | ✓ Compliant |
+   411	| **Post-Audit Actions** | | |
+   412	| Critical bugs | 0 | ✓ |
+   413	| Code changes required | 0 | ✓ |
+   414	| Documentation improvements (optional) | 3 | → Recommended |
+   415	
+   416	---
+   417	
+   418	## 7. Conclusion
+   419	
+   420	The FFI and iOS bridge code is **production-ready for M10.5**. 
+   421	
+   422	**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
+   423	
+   424	**Recommendation:** The 3 safety comments in ffi.rs are optional but recommended for auditability and preventing future misclassification.
+   425	
+   426	**M10.5 Exit Criteria:** ✅ **READY**
+   427	
+   428	- ✅ Zero blocking debt markers
+   429	- ✅ Zero code bugs
+   430	- ✅ All doctrines compliant
+   431	- ✅ iOS bridge code clean (9 files, 0 findings)
+   432	- ✅ Unsafe blocks are correct (documentation gap only)
+   433	
+   434	**Audit Sign-Off:** Ready for iOS empirical proof phase (M10.5).
+
+ succeeded in 0ms:
+   338	
+   339	---
+   340	
+   341	### M10.5 — FFI hardening + iOS empirical proof *(hard gate before M11 starts)*
+   342	
+   343	**Demo product:** The iOS Twitter slice from M1–M10 subjected to a published, exhaustive stress harness on the iOS simulator and a real iPhone 12. The kernel↔FFI↔SwiftUI path is proven, in measured numbers, to be **rock-solid and demonstrably performant** before a single line of the podcast app is written.
+   344	
+   345	**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+   346	
+   347	**Scope.**
+   348	
+   349	- **Stress harness** (`crates/nmp-testing/bin/ffi-stress` + `ios/NmpStress/StressUITests/`):
+   350	  - Mount/unmount churn: 1000 view-handle wrappers cycled per second for 10 minutes; assert zero leaks (via Instruments leak instrument scripted run).
+   351	  - Dispatch flood: 10k `dispatch(...)` calls per second from Swift across multiple threads; assert no dropped messages, no main-thread block > 16 ms.
+   352	  - Snapshot pressure: `AppUpdate::FullState` with 100k events forced; measure marshal time, allocations, and that the reconciler stays ≤ 60 Hz via batching.
+   353	  - Reconciler back-pressure: deliberately stall the Swift main thread for 250 ms; assert no actor stall, deltas accumulate and replay correctly when the main thread resumes.
+   354	  - Reentrancy: dispatch from inside a reconciler callback (a known footgun); assert ordered, deadlock-free.
+   355	  - Capability lifecycle storms: start/stop/restart each registered capability 1000 times; assert idempotency per RMP bible.
+   356	  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
+   357	- **Real-device measurement on iPhone 12** (one full battery of `firehose-bench live` against primal, all 8 scenarios from `docs/design/firehose-bench.md` §3); produces `docs/perf/m10.5/iphone12-baseline.md` with hardware-tagged numbers.
+   358	- **Simulator-driven UI test fleet** (parallel Sonnet agents via the `mcp__xcode` and `BrowserAgent`/`QATester` skills) exercising the app from the outside — boot sim, launch app, tap, scroll, swipe, kill-relaunch — capturing screenshots and assertions per scripted scenario. Every M1–M10 user-visible feature gets a UI test; failures block the milestone.
+   359	- **Memory + leak audit** with Xcode Instruments (Leaks, Allocations, Time Profiler) on canonical workflows; zero retained-by-cycle leaks; allocations after warmup linear-or-better in active-view count, never in cached-event count.
+   360	- **Profile-Guided Optimization sweep** on the kernel hot paths surfaced by Time Profiler; document tradeoffs taken.
+   361	- **All M1–M10 perf reports re-run** on the final FFI surface to confirm no regressions.
+   362	- **FFI surface documentation audit** in `docs/ffi-surface.md` — every exported type, function, capability trait, and ownership/lifetime invariant called out; reviewed against `RMP-ARCHITECTURE-BIBLE.md` commandments and ADR-0010.
+   363	- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+   364	
+   365	**Subsystem deliverables.**
+   366	
+   367	- `crates/nmp-testing/bin/ffi-stress` — new bench binary.
+   368	- `ios/NmpStress/StressUITests` — XCUITest target driven by both XCTest and (where relevant) a scripted Sonnet-agent runner.
+   369	- `docs/design/ffi-hardening.md` — design doc enumerating every FFI failure mode and how the harness exercises it.
+   370	- `docs/ffi-surface.md` — the canonical FFI surface reference.
+   371	- `docs/perf/m10.5/` — measured numbers from simulator, M-series Mac, iPhone 12; plus screenshots from the Sonnet-driven UI runs.
+   372	
+   373	**Exit gate.**
+   374	
+   375	- All stress-harness scenarios pass on simulator and iPhone 12 with the numeric thresholds enumerated in `docs/design/ffi-hardening.md` §exit-gate.
+   376	- All M1–M10 perf reports re-run cleanly on the post-M10.5 binaries; no regression > 5 % on any p99 number.
+   377	- Instruments-recorded Leaks count = 0 over the 10-minute canonical workflow.
+   378	- Every UI-scripted scenario (Sonnet-agent and XCUITest) passes on a fresh boot of the iPhone 16 Pro simulator and on iPhone 12 hardware.
+   379	- `docs/ffi-surface.md` reviewed and tagged.
+   380	- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
+   381	
+   382	**Runnable artifact.** Same iOS Twitter app, now load-bearing. Report bundle in `docs/perf/m10.5/`.
+   383	
+   384	---
+   385	
+
+exec
+/bin/zsh -lc 'nl -ba docs/perf/orchestration-log.md' in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc "nl -ba docs/plan.md | sed -n '386,475p'" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     1	# Orchestration Log
+     2	
+     3	Durable trail of the parallel-agent orchestration. One line per heartbeat or significant event.
+     4	
+     5	| When (local) | # | Event |
+     6	|---|---|---|
+     7	| 2026-05-18 01:24 | 0 | Session start. Pulled to e9cbafa. Plan revised (55dd5f2) inserting M10.5 (FFI hardening) and concretizing M11 (`../podcast` rebuild). 15-min cron heartbeat armed. First wave of 6 background agents dispatched: build-verifier (T7), debt-auditor (T6), m2-designer (T2), m3-designer (T3), m105-designer (T4), m11-designer (T5). T1 blocked on T7. |
+     8	| 2026-05-18 01:30 | 0a | Advisor pass: broadcast safe-rebase-push protocol to all 6 running agents (avoid push race on master). T1 description updated to mandate worktree isolation + rebase-push protocol. Heartbeat cron rewritten (job 811003f1) with stronger triage rules (design→review→impl chain; M11 gated on M10.5 *empirical* pass, not just designed; debt triage uses both must-fix and ADR-defer lanes; DerivedData sprawl mitigation; orphan detection). Heartbeat runtime is session-only (durable flag ignored). 6 stale stashes from prior codex sessions dropped. |
+
+ succeeded in 0ms:
+   386	### M11 — Podcast app (the `../podcast` rebuild on NMP — the kernel-boundary proof)
+   387	
+   388	**Demo product:** A 1:1 rebuild of `/Users/pablofernandez/src/podcast` (the fully-functional Swift app, 20 SwiftUI views, ~8.8k LOC of Swift) running on NMP. **UI is pixel-identical** to the reference Swift app; **all business logic, LLM, audio orchestration, downloads, transcripts, RAG, recommendations** are in Rust extension modules driving the kernel.
+   389	
+   390	**This is the load-bearing kernel-boundary check.** If the kernel needs even one podcast noun to make this work, the boundary is wrong and we go back to fix it.
+   391	
+   392	**Reference inputs** (read before scoping):
+   393	
+   394	- `/Users/pablofernandez/src/podcast/` — canonical Swift implementation. Source of truth for UI and feature behavior. **Every view in `PodcastApp/Views/` is copied verbatim into `ios/NmpPodcast/Views/`** as step 1; only the data source is rewired.
+   395	- `/Users/pablofernandez/src/podcast-rmp/` — prior WIP RMP rewrite (incomplete). **Not a code source** but a lessons source: read its `RMP-ARCHITECTURE-BIBLE.md`, `FINAL_PLAN.md`, `docs/plans/iphone-feature-parity-plan.md`, and `docs/plans/iphone-feature-parity-checklist.md` before scoping. That repo's `AGENTS.md` is the working guide for any agent touching that tree.
+   396	- `/Users/pablofernandez/src/podcast/docs/plans/` — original feature design docs (podcast-app-design, discovery-tab-redesign, insights-feature-design).
+   397	
+   398	**Reference inventory of the Swift app** (so the scope is explicit, not vibes):
+   399	
+   400	| Swift `Views/` group | Files | NMP target |
+   401	|---|---|---|
+   402	| `Ask/` | AskView.swift | `ask-core` ActionModule + ViewModule wrapping `rig.rs` LLM call |
+   403	| `Components/` | CachedAsyncImage, DiscoveryCards | reusable Swift components, ported as-is; image cache backed by NMP Blossom-aware capability |
+   404	| `Feed/` | FeedView, EpisodeRow | `podcast-core::FeedViewModule` + `EpisodeRowViewModule` |
+   405	| `Insights/` | InsightsView | `insights-core` ViewModule + ActionModule (uses RAG via `rig.rs`) |
+   406	| `Library/` | ActivityView, AddPodcastView, DiscoverView, EpisodeDetailView, LibraryView, PodcastDetailSheet, PodcastDetailView, QueueView | `podcast-core` ViewModules + ActionModules |
+   407	| `Player/` | ChaptersPanel, GuestAgentSheet, MiniPlayer, PlayerSheet, TranscriptView | `player-core` ViewModule + `AudioPlaybackCapability` |
+   408	| `Settings/` | SettingsView | `settings-core` ActionModule (mostly capability invocations) |
+   409	
+   410	Swift `Services/` (AIService, AudioService, DownloadService, GuestEnrichmentService, ImageCache, InsightService, PodcastIndexService, PodcastService, ProcessingQueue, RAGService, RecommendationService, ServiceContainer, TranscriptionService, VectorDatabase) **all move to Rust** as ActionModules + ProjectionCaches + capability bridges; Swift loses its Services/ directory entirely.
+   411	
+   412	Swift `Models/` (AITypes, Chapter, Episode, Guest, Insight, Podcast, Settings, Transcript) **all move to Rust** as DomainRecords inside `podcast-core` and sibling crates.
+   413	
+   414	Swift `ViewModels/` **disappear** — they become Rust ViewModules whose output crosses FFI as typed ViewBatch deltas.
+   415	
+   416	**Scope.**
+   417	
+   418	**Step 0 — copy step (UI-fidelity invariant lock):**
+   419	
+   420	- Copy every file in `/Users/pablofernandez/src/podcast/PodcastApp/Views/` into `ios/NmpPodcast/NmpPodcast/Views/` verbatim. Commit immediately. No edits except the minimum needed to compile against placeholder data sources (`// MARK: NMP-WIRE` markers).
+   421	- Copy `Resources/Assets.xcassets` and `Info.plist` (sanitized) verbatim.
+   422	- The result compiles and renders against stubbed data; UI is visually identical to `../podcast` per a side-by-side simulator screenshot diff (≤ 1 px tolerance, font-rendering exceptions documented).
+   423	
+   424	**Step 1 — domain + view modules in Rust** (per the table above):
+   425	
+   426	- `apps/podcast/podcast-core/` — main app crate. `DomainModule`s: `Podcast`, `Episode`, `Transcript`, `Chapter`, `Guest`, `Insight`, `Subscription`, `PlayerState`, `QueueEntry`, `Activity`.
+   427	- `apps/podcast/podcast-core/` — `ViewModule`s: `PodcastLibrary`, `EpisodeDetail`, `NowPlaying`, `EpisodeQueue`, `Discover`, `Insights`, `Activity`, `PodcastDetail`, `Feed`, `EpisodeRow`, `Chapters`, `Transcript`, `MiniPlayer`, `PlayerSheet`, `GuestAgent`, `Ask`, `Settings`.
+   428	- `apps/podcast/podcast-core/` — `ActionModule`s: `SubscribePodcast`, `UnsubscribePodcast`, `RefreshFeed`, `DownloadEpisode`, `CancelDownload`, `Play`, `Pause`, `Seek`, `SkipForward`, `SkipBack`, `MarkPlayed`, `EnqueueEpisode`, `ReorderQueue`, `ImportRss`, `ImportOpml`, `AskQuestion`, `EnrichGuest`, `RunInsight`, `SearchPodcasts`.
+   429	- `apps/podcast/podcast-llm/` — LLM-driven actions via `rig.rs`: `AskQuestion`, `EnrichGuest`, `RunInsight`. Uses the kernel's capability bridge for HTTP + key storage.
+   430	- `apps/podcast/podcast-rag/` — RAG + vector DB store; uses a swappable `EmbeddingCapability` and a Rust-side vector store (sqlite-vss or qdrant-client).
+   431	- `apps/podcast/podcast-feeds/` — RSS + Atom + JSON Feed + Podcast 2.0 namespaces parsing; transcripts; chapters; value-for-value. Pure Rust; pulls via `HttpCapability`.
+   432	
+   433	**Step 2 — capabilities added to the kernel's reusable set** (these are general, not podcast-specific):
+   434	
+   435	- `AudioPlaybackCapability`: play URL or local file; report position events + state transitions back; iOS impl via `AVPlayer` + background-audio entitlement + lock-screen `MPNowPlayingInfoCenter`/`MPRemoteCommandCenter`.
+   436	- `BackgroundWorkCapability`: register periodic background tasks; iOS impl via `BGTaskScheduler`.
+   437	- `LocalNotificationCapability`: extended for episode-available alerts.
+   438	- `HttpCapability`: long-running streaming response support (RSS, transcripts).
+   439	- `EmbeddingCapability`: callable embedding model; kernel-owned policy, platform-owned execution (CoreML on iOS, ONNX or remote API as fallback).
+   440	- `KeyValueStoreCapability`: typed persistent KV (for saved playback position when persistence-by-store is overkill).
+   441	
+   442	**Step 3 — protocol module integration:**
+   443	
+   444	- `nmp-podcast` (Nostr podcast NIP integration where it exists — NIP-XX podcast feed events, value-for-value zaps, episode discussion threads). Where Nostr coverage is incomplete, the app uses RSS via `podcast-feeds` and Nostr for social overlay (zaps, discussions, recommendations from the WoT subsystem).
+   445	
+   446	**Step 4 — wire each copied Swift view to its Rust view module:**
+   447	
+   448	- Replace stubbed data with the generated wrapper hooks (`@PodcastLibrary`, `@NowPlaying`, etc. — produced by `nmp gen modules`).
+   449	- The Swift file shape stays the same; only the data source changes.
+   450	- After every Library/Feed/Player/Insights/Ask/Settings group is wired, run the side-by-side screenshot diff again.
+   451	
+   452	**Exit gate (kernel boundary).**
+   453	
+   454	- **`nmp-core` gains zero podcast nouns.** No `Podcast`, `Episode`, `Transcript`, `Chapter`, `Player`, `Feed`, `Insight`, `Guest` types added to the kernel. Verified by grep + manual review at the commit.
+   455	- **The capability families added in M11 are general** (audio playback, background work, local notifications, HTTP, embedding, KV-store). Their request/response shapes are not podcast-specific.
+   456	- **Reactivity behavior is identical** to the Twitter slice — composite-key dependencies, delta coalescing, claim-based GC, ADR-0007 diagnostics all work for podcast view modules.
+   457	- **No app-state leaks across the boundary in either direction:** no Nostr type appears in `podcast-core`'s public surface; no podcast type appears in `nmp-core`'s public surface.
+   458	
+   459	**Exit gate (product fidelity to `../podcast`).**
+   460	
+   461	- **UI parity:** side-by-side screenshot of every screen in `../podcast` vs `ios/NmpPodcast` matches at ≤ 1 px tolerance (font/rendering differences whitelisted explicitly in `docs/perf/m11/parity-screenshots.md`).
+   462	- **Feature parity:** every user flow exercised in `/Users/pablofernandez/src/podcast/Tests/` (or its equivalent on the canonical Swift app) reproduced as a scripted Sonnet-agent run on `ios/NmpPodcast`. No "feature dropped" footnotes.
+   463	- **Subscribe to 10 real podcasts** spanning RSS + (where available) Nostr feeds; library populates correctly.
+   464	- **Download an episode in the background** while the app is suspended; resumable on relaunch.
+   465	- **Play with background audio** while the iPhone is locked; lock-screen artwork, scrubber, skip/seek controls all functional.
+   466	- **Resume playback at the correct position** after a kill-relaunch.
+   467	- **Push notification on a new episode arrival.**
+   468	- **Ask a question** about an episode; answer streams in via `rig.rs` LLM with the transcript as RAG context.
+   469	- **Insights** view generates a structured episode summary on demand.
+   470	- **Guest enrichment** populates guest cards via external lookup, identical to the Swift impl behavior.
+   471	
+   472	**Stress + perf gates.**
+   473	
+   474	- Library of 100 podcasts × 50 episodes (5k episodes total) scrolls at 60 fps on iPhone 12.
+   475	- Player UI updates every 250 ms during playback without visible jank.
+
+exec
+/bin/zsh -lc 'git log --oneline --decorate -8' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+f1e374b (HEAD -> master, origin/master) perf(m1): clean-build verification baseline
+d660735 audit(m10.5): FFI + iOS bridge debt inventory
+31fa612 perf(orchestration): advisor adjustments (push protocol, triage, gates)
+18e4953 perf(orchestration): start log; wave 1 dispatched
+55dd5f2 docs(plan): insert M10.5 FFI hardening gate + concretize M11 podcast rebuild
+e9cbafa docs(plan): consolidate into single milestone-driven plan with product checks
+2cd423a ios: wire ProfileInterestAvatar to kernel claim/release system
+3210bb7 actor/ffi: expose ClaimProfile and ReleaseProfile commands
+
+exec
+/bin/zsh -lc 'git show e9cbafa:docs/plan.md | wc -l' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+     680
+
+exec
+/bin/zsh -lc 'git diff --name-only e9cbafa..d660735' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+docs/perf/m10.5/debt-inventory.md
+docs/perf/orchestration-log.md
+docs/plan.md
+
+exec
+/bin/zsh -lc 'git diff --numstat e9cbafa..d660735 -- docs/plan.md docs/perf/m10.5/debt-inventory.md docs/perf/orchestration-log.md' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+434	0	docs/perf/m10.5/debt-inventory.md
+8	0	docs/perf/orchestration-log.md
+138	29	docs/plan.md
+
+exec
+/bin/zsh -lc "rg -n \"NIP-XX|optional|deferral|deferred|READY|production-ready|Exit Criteria|M10\\.5 Exit\" docs/plan.md docs/perf/m10.5/debt-inventory.md" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+docs/plan.md:231:- Login UX (single nsec field for now; multi-step onboarding deferred to M16).
+docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+docs/plan.md:444:- `nmp-podcast` (Nostr podcast NIP integration where it exists — NIP-XX podcast feed events, value-for-value zaps, episode discussion threads). Where Nostr coverage is incomplete, the app uses RSS via `podcast-feeds` and Nostr for social overlay (zaps, discussions, recommendations from the WoT subsystem).
+docs/perf/m10.5/debt-inventory.md:6:**Exit Criteria:** All findings classified; doctrine violations identified and severity-ranked
+docs/perf/m10.5/debt-inventory.md:414:| Documentation improvements (optional) | 3 | → Recommended |
+docs/perf/m10.5/debt-inventory.md:420:The FFI and iOS bridge code is **production-ready for M10.5**. 
+docs/perf/m10.5/debt-inventory.md:424:**Recommendation:** The 3 safety comments in ffi.rs are optional but recommended for auditability and preventing future misclassification.
+docs/perf/m10.5/debt-inventory.md:426:**M10.5 Exit Criteria:** ✅ **READY**
+
+exec
+/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/plan.md | rg -n "deferred|deferral|NIP-XX|optional|for later|wire it up later|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/perf/m10.5/debt-inventory.md | rg -n "optional|recommended|Recommended|READY|production-ready|No Action Required|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+18:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
+361:+## 4. Acceptable & Justified Findings (No Action Required)
+365:+| 3 unsafe blocks in ffi.rs (F1, F2, F3) | Documentation gap | Standard FFI pattern; pointers validated by caller contract. Safety comments recommended for future audits but code is correct. |
+374:+## 5. Recommended Next Actions
+376:+### Documentation-Only Improvements (Recommended for M10.5)
+384:+   - Recommended text:
+393:+   - Recommended text: `// Stream type may have additional TLS variants in future tungstenite versions`
+395:+### No Action Required
+412:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
+420:+| Documentation improvements (optional) | 3 | → Recommended |
+426:+The FFI and iOS bridge code is **production-ready for M10.5**. 
+430:+**Recommendation:** The 3 safety comments in ffi.rs are optional but recommended for auditability and preventing future misclassification.
+432:+**M10.5 Exit Criteria:** ✅ **READY**
+
+ succeeded in 0ms:
+13:+> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.
+40:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
+58:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+150:+- `nmp-podcast` (Nostr podcast NIP integration where it exists — NIP-XX podcast feed events, value-for-value zaps, episode discussion threads). Where Nostr coverage is incomplete, the app uses RSS via `podcast-feeds` and Nostr for social overlay (zaps, discussions, recommendations from the WoT subsystem).
+155:-- `nmp-podcast` (or whatever the Nostr podcast NIP is called, e.g. NIP-XX for podcast feed events): parsed feed events. If no NIP, the app uses RSS via the action ledger to fetch + parse, storing entries as domain records.
+
+exec
+/bin/zsh -lc "nl -ba crates/nmp-core/src/ffi.rs | sed -n '68,84p;268,290p'" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc "nl -ba crates/nmp-core/src/relay_worker.rs | sed -n '236,246p'" in /Users/pablofernandez/Work/nostr-multi-platform
+exec
+/bin/zsh -lc "rg -n \"unsafe|allow\\(unreachable_patterns\\)\" crates/nmp-core/src/ffi.rs crates/nmp-core/src/relay_worker.rs" in /Users/pablofernandez/Work/nostr-multi-platform
+ succeeded in 0ms:
+    68	        update_listener: Mutex::new(Some(update_listener)),
+    69	    }))
+    70	}
+    71	
+    72	#[no_mangle]
+    73	pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
+    74	    if !app.is_null() {
+    75	        unsafe {
+    76	            drop(Box::from_raw(app));
+    77	        }
+    78	    }
+    79	}
+    80	
+    81	#[no_mangle]
+    82	pub extern "C" fn nmp_app_set_update_callback(
+    83	    app: *mut NmpApp,
+    84	    context: *mut c_void,
+   268	    let _ = app.tx.send(ActorCommand::CloseThread { event_id });
+   269	}
+   270	
+   271	fn app_ref<'a>(app: *mut NmpApp) -> Option<&'a NmpApp> {
+   272	    if app.is_null() {
+   273	        None
+   274	    } else {
+   275	        Some(unsafe { &*app })
+   276	    }
+   277	}
+   278	
+   279	fn c_string_argument(ptr: *const c_char) -> Option<String> {
+   280	    if ptr.is_null() {
+   281	        return None;
+   282	    }
+   283	
+   284	    unsafe { CStr::from_ptr(ptr) }
+   285	        .to_str()
+   286	        .ok()
+   287	        .map(str::trim)
+   288	        .filter(|value| !value.is_empty())
+   289	        .map(ToOwned::to_owned)
+   290	}
+
+ succeeded in 0ms:
+   236	            let _ = stream.set_read_timeout(Some(duration));
+   237	        }
+   238	        MaybeTlsStream::Rustls(stream) => {
+   239	            let tcp = stream.get_ref();
+   240	            let _ = tcp.set_read_timeout(Some(duration));
+   241	        }
+   242	        #[allow(unreachable_patterns)]
+   243	        _ => {}
+   244	    }
+   245	}
+
+ succeeded in 0ms:
+crates/nmp-core/src/relay_worker.rs:242:        #[allow(unreachable_patterns)]
+crates/nmp-core/src/ffi.rs:75:        unsafe {
+crates/nmp-core/src/ffi.rs:275:        Some(unsafe { &*app })
+crates/nmp-core/src/ffi.rs:284:    unsafe { CStr::from_ptr(ptr) }
+
+codex
+Not fine.
+
+**Findings**
+- [docs/perf/m10.5/debt-inventory.md:420](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:420) and [line 426](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:426): this signs off “production-ready” / “M10.5 Exit Criteria READY” from a read-only audit, but [docs/plan.md:373](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:373) requires stress harness, iPhone 12, Instruments, UI fleet, perf reruns, and doctrine review. Fix: downgrade this to “audit ready for empirical proof” and remove exit-ready language.
+
+- [docs/perf/m10.5/debt-inventory.md:414](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:414) and [line 424](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:424): known FFI safety-comment gaps are marked optional. That conflicts with zero-debt / no-shortcut posture. Fix: make F1-F4 required M10.5 cleanup or explicitly move them outside exit evidence.
+
+- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
+
+- File-size budget: `docs/perf/m10.5/debt-inventory.md` is 434 LOC, over the 300-line soft limit. `docs/plan.md` is 789 LOC, over the 500-line hard ceiling and this change added net +109 lines to it. Fix: split plan milestone detail into linked milestone docs and split the inventory into summary + findings appendix.
+
+- [docs/plan.md:444](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:444): `NIP-XX` is still placeholder protocol language. Fix: name the actual NIP/spec, or state “no known NIP; RSS plus Nostr social overlay only until a real spec is selected.”
+
+**Checks**
+No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
+tokens used
+50,249
+Not fine.
+
+**Findings**
+- [docs/perf/m10.5/debt-inventory.md:420](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:420) and [line 426](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:426): this signs off “production-ready” / “M10.5 Exit Criteria READY” from a read-only audit, but [docs/plan.md:373](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:373) requires stress harness, iPhone 12, Instruments, UI fleet, perf reruns, and doctrine review. Fix: downgrade this to “audit ready for empirical proof” and remove exit-ready language.
+
+- [docs/perf/m10.5/debt-inventory.md:414](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:414) and [line 424](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:424): known FFI safety-comment gaps are marked optional. That conflicts with zero-debt / no-shortcut posture. Fix: make F1-F4 required M10.5 cleanup or explicitly move them outside exit evidence.
+
+- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
+
+- File-size budget: `docs/perf/m10.5/debt-inventory.md` is 434 LOC, over the 300-line soft limit. `docs/plan.md` is 789 LOC, over the 500-line hard ceiling and this change added net +109 lines to it. Fix: split plan milestone detail into linked milestone docs and split the inventory into summary + findings appendix.
+
+- [docs/plan.md:444](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:444): `NIP-XX` is still placeholder protocol language. Fix: name the actual NIP/spec, or state “no known NIP; RSS plus Nostr social overlay only until a real spec is selected.”
+
+**Checks**
+No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
diff --git a/docs/perf/m10.5/debt-inventory.md b/docs/perf/m10.5/debt-inventory.md
index 8545c27..d9af970 100644
--- a/docs/perf/m10.5/debt-inventory.md
+++ b/docs/perf/m10.5/debt-inventory.md
@@ -352,48 +352,25 @@ visible_limit.clamp(1, 500) as usize  // c_uint → usize
 
 ---
 
-## 4. Acceptable & Justified Findings (No Action Required)
+## 4. Justified Findings (No code change, no auditability gap)
 
 | Finding | Classification | Justification |
 |---------|---|---|
-| 3 unsafe blocks in ffi.rs (F1, F2, F3) | Documentation gap | Standard FFI pattern; pointers validated by caller contract. Safety comments recommended for future audits but code is correct. |
-| allow(unreachable_patterns) in relay_worker.rs (F4) | Documentation gap | Defensive programming for third-party enum evolution. Code is correct; add comment for clarity. |
-| 14 integer casts (count→metric types) (F6–F14) | Acceptable | All bounded by design constraints; no overflow risk. Casts are intentional and safe. |
-| 2 expect() calls in kernel/status.rs (F15, F16) | Acceptable | Invariant maintained by construction (relay HashMap initialized for all roles in `Kernel::new()`). |
+| 14 integer casts (count→metric types) (F6–F14) | Justified | All bounded by design constraints (metrics counters, working-set bounds); no overflow risk. Casts are intentional and safe. |
+| 2 expect() calls in kernel/status.rs (F15, F16) | Justified | Invariant maintained by construction (relay HashMap initialized for all roles in `Kernel::new()`). |
 | ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
 | Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
 
 ---
 
-## 5. Recommended Next Actions
+## 5. Must-Fix Items for M10.5 (mandatory)
 
-### Documentation-Only Improvements (Recommended for M10.5)
+Per the M10.5 zero-shortcut posture (no "for later", no "ADR escape" for files in scope), the following are **required cleanup** before the M10.5 empirical exit gate:
 
-These are **not bugs**; the code is correct. Adding safety comments improves auditability and prevents future misclassification:
+1. **Add SAFETY comments to ffi.rs unsafe blocks** (F1, F2, F3) — `crates/nmp-core/src/ffi.rs` lines 75, 275, 284. The unsafe code is correct; missing safety comments are themselves auditability debt and violate the M10.5 zero-debt posture for the FFI surface. Suggested text in the findings section above.
+2. **Add clarifying comment to relay_worker.rs#242** (F4) — explain why `#[allow(unreachable_patterns)]` is defensive against future tungstenite enum evolution.
 
-1. **Add safety comments to ffi.rs unsafe blocks** (F1, F2, F3)
-   - Files: `crates/nmp-core/src/ffi.rs` (3 locations: lines 75, 275, 284)
-   - Effort: 5 min
-   - Impact: Documents FFI contract; improves future audits
-   - Recommended text:
-     - Line 75: `// safe: caller guarantees app is valid, allocated by nmp_app_new()`
-     - Line 275: `// safe: caller guarantees non-null app is a valid NmpApp pointer`
-     - Line 284: `// safe: caller guarantees ptr is a valid null-terminated C string; to_str() validates UTF-8`
-
-2. **Add code comment to relay_worker.rs#242** (F4)
-   - Files: `crates/nmp-core/src/relay_worker.rs` (line 242)
-   - Effort: 2 min
-   - Impact: Clarifies defensive intent
-   - Recommended text: `// Stream type may have additional TLS variants in future tungstenite versions`
-
-### No Action Required
-
-- All integer casts (F6–F14) are bounded and intentional; no comments needed.
-- Both expect() calls (F15, F16) are justified by invariants; no action needed.
-- All Swift iOS code is clean (9 files audited; 0 findings).
-- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
-
----
+These must land in a single PR titled `m10.5(ffi): add SAFETY comments + clarify defensive pattern`. Tracked as a TaskList task.
 
 ## 6. Metrics
 
@@ -405,30 +382,26 @@ These are **not bugs**; the code is correct. Adding safety comments improves aud
 | Total LOC scanned | 6,559 | ✓ |
 | Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
 | Code bugs found | 0 | ✓ No bugs |
-| Unsafe blocks without safety comments | 3 | ⚠️ Documentation gap |
+| Unsafe blocks without safety comments | 3 | ⚠️ **Must-fix for M10.5** |
 | Integer casts | 14 | ✓ Justified |
 | Cardinal doctrine violations | 0 | ✓ Compliant |
 | **Post-Audit Actions** | | |
 | Critical bugs | 0 | ✓ |
-| Code changes required | 0 | ✓ |
-| Documentation improvements (optional) | 3 | → Recommended |
+| Required code changes (safety comments) | 4 | **Must-fix for M10.5** |
 
 ---
 
 ## 7. Conclusion
 
-The FFI and iOS bridge code is **production-ready for M10.5**. 
-
-**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
-
-**Recommendation:** The 3 safety comments in ffi.rs are optional but recommended for auditability and preventing future misclassification.
-
-**M10.5 Exit Criteria:** ✅ **READY**
+The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
 
-- ✅ Zero blocking debt markers
-- ✅ Zero code bugs
-- ✅ All doctrines compliant
-- ✅ iOS bridge code clean (9 files, 0 findings)
-- ✅ Unsafe blocks are correct (documentation gap only)
+**This audit is necessary but not sufficient for M10.5 exit.** M10.5 exit requires (per `docs/plan.md` §M10.5):
+- Stress harness scenarios passing on simulator and iPhone 12
+- All M1–M10 perf reports re-run with no > 5 % regression
+- Instruments-recorded zero leaks over 10-min canonical workflow
+- UI-scripted scenarios passing (Sonnet-agent + XCUITest)
+- `docs/ffi-surface.md` reviewed and tagged
+- Doctrine review signed in writing in `docs/perf/m10.5/doctrine-review.md`
 
-**Audit Sign-Off:** Ready for iOS empirical proof phase (M10.5).
+**Audit status:** ✅ static-debt baseline established. The 4 must-fix items above are tracked.
+**M10.5 exit status:** ❌ pending empirical proof + must-fix cleanup.
diff --git a/docs/plan.md b/docs/plan.md
index 32c8471..04f4608 100644
--- a/docs/plan.md
+++ b/docs/plan.md
@@ -360,7 +360,7 @@ Each milestone has: **demo product**, **scope (what gets built)**, **subsystem d
 - **Profile-Guided Optimization sweep** on the kernel hot paths surfaced by Time Profiler; document tradeoffs taken.
 - **All M1–M10 perf reports re-run** on the final FFI surface to confirm no regressions.
 - **FFI surface documentation audit** in `docs/ffi-surface.md` — every exported type, function, capability trait, and ownership/lifetime invariant called out; reviewed against `RMP-ARCHITECTURE-BIBLE.md` commandments and ADR-0010.
-- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Hard zero — no deferral escape, no "tracking issue" carve-out. Every pre-existing one is resolved in M10.5. If something genuinely cannot be done in M10.5 because it belongs to a later milestone (e.g. NIP-65 outbox work), then it is not a TODO/FIXME in the scoped files — it lives as a milestone task in `docs/plan.md`, not as a code marker.
 
 **Subsystem deliverables.**
 
@@ -441,7 +441,7 @@ Swift `ViewModels/` **disappear** — they become Rust ViewModules whose output
 
 **Step 3 — protocol module integration:**
 
-- `nmp-podcast` (Nostr podcast NIP integration where it exists — NIP-XX podcast feed events, value-for-value zaps, episode discussion threads). Where Nostr coverage is incomplete, the app uses RSS via `podcast-feeds` and Nostr for social overlay (zaps, discussions, recommendations from the WoT subsystem).
+- `nmp-podcast` is **not a v1 deliverable** unless a real published NIP is selected during M11 design (e.g. NIP-54 or a successor) and the choice is recorded in `docs/design/podcast-app-rebuild.md`. Until then, the podcast app uses RSS + Podcast 2.0 namespaces (chapters, transcripts, value-for-value) via `podcast-feeds`, and Nostr is the **social overlay only** — kind:1 discussion threads referencing the episode URL/GUID, kind:7 reactions, NIP-57 zaps. The decision is locked in M11 step-0; no `NIP-XX` placeholders allowed in code or in plans past that point.
 
 **Step 4 — wire each copied Swift view to its Rust view module:**
 

Review for: (1) doctrine compliance, (2) any TODO/FIXME/unimplemented sneaking in, (3) test coverage where tests should exist, (4) file-size compliance (any new file > 300 LOC?), (5) docs internal consistency (M10.5 gate, M11 pod-NIP language, etc), (6) hidden shortcuts. Be terse. If fine, say so. If concern, name file:line + fix.
codex
I’ll verify the merged snapshot and then review the changed docs against the doctrine, LOC ceiling, and shortcut language with exact line refs.
exec
/bin/zsh -lc 'git status -sb' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
## master...origin/master
?? .claude/
?? docs/design/podcast-app-rebuild.md
?? docs/design/podcast/
?? docs/perf/codex-reviews/fcf8b43.md

exec
/bin/zsh -lc 'git rev-parse HEAD && git log --oneline --decorate -5' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
fcf8b431b8d86f1801fef2fad26e81fbf56506f4
fcf8b43 (HEAD -> master, origin/master) docs: apply codex review fixes from session-1
9fead0e design(m3): LMDB schema + EventStore trait + GC policy
031fc07 design(m2): subscription compilation + outbox routing spec
f1e374b perf(m1): clean-build verification baseline
d660735 audit(m10.5): FFI + iOS bridge debt inventory

exec
/bin/zsh -lc 'wc -l docs/design/ffi-hardening.md docs/design/ffi-hardening/ci.md docs/design/ffi-hardening/gates.md docs/design/ffi-hardening/harness.md docs/design/ffi-hardening/scenarios.md docs/perf/codex-reviews/2026-05-18-session-1.md docs/perf/m10.5/debt-inventory.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     314 docs/design/ffi-hardening.md
     345 docs/design/ffi-hardening/ci.md
     267 docs/design/ffi-hardening/gates.md
     410 docs/design/ffi-hardening/harness.md
     353 docs/design/ffi-hardening/scenarios.md
    1263 docs/perf/codex-reviews/2026-05-18-session-1.md
     407 docs/perf/m10.5/debt-inventory.md
     789 docs/plan.md
    4148 total

exec
/bin/zsh -lc 'git diff --stat HEAD~1..HEAD && git diff --name-only HEAD~1..HEAD' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
 docs/design/ffi-hardening.md                    |  314 ++++++
 docs/design/ffi-hardening/ci.md                 |  345 +++++++
 docs/design/ffi-hardening/gates.md              |  267 +++++
 docs/design/ffi-hardening/harness.md            |  410 ++++++++
 docs/design/ffi-hardening/scenarios.md          |  353 +++++++
 docs/perf/codex-reviews/2026-05-18-session-1.md | 1263 +++++++++++++++++++++++
 docs/perf/m10.5/debt-inventory.md               |   67 +-
 docs/plan.md                                    |    4 +-
 8 files changed, 2974 insertions(+), 49 deletions(-)
docs/design/ffi-hardening.md
docs/design/ffi-hardening/ci.md
docs/design/ffi-hardening/gates.md
docs/design/ffi-hardening/harness.md
docs/design/ffi-hardening/scenarios.md
docs/perf/codex-reviews/2026-05-18-session-1.md
docs/perf/m10.5/debt-inventory.md
docs/plan.md

codex
The quick snapshot confirms HEAD is `fcf8b43` and the merge added several large hand-authored docs. I’m checking the doc claims against the actual FFI surface now, because count/schema mismatches are where design debt tends to hide.
exec
/bin/zsh -lc "rg -n \"pub extern \\\"C\\\" fn|type UpdateCallback|struct KernelUpdate|toast|logs|metrics|RelayStatus|last_error|last_notice\" crates/nmp-core/src/ffi.rs crates/nmp-core/src/kernel/update.rs crates/nmp-core/src/kernel/status.rs docs/design/ffi-hardening.md docs/design/ffi-hardening/*.md docs/perf/m10.5/debt-inventory.md docs/plan.md" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
crates/nmp-core/src/ffi.rs:10:type UpdateCallback = extern "C" fn(*mut c_void, *const c_char);
crates/nmp-core/src/ffi.rs:45:pub extern "C" fn nmp_app_new() -> *mut NmpApp {
crates/nmp-core/src/ffi.rs:73:pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
crates/nmp-core/src/ffi.rs:82:pub extern "C" fn nmp_app_set_update_callback(
crates/nmp-core/src/ffi.rs:100:pub extern "C" fn nmp_app_start(
crates/nmp-core/src/ffi.rs:117:pub extern "C" fn nmp_app_configure(
crates/nmp-core/src/ffi.rs:134:pub extern "C" fn nmp_app_stop(app: *mut NmpApp) {
crates/nmp-core/src/ffi.rs:142:pub extern "C" fn nmp_app_reset(app: *mut NmpApp) {
crates/nmp-core/src/ffi.rs:150:pub extern "C" fn nmp_app_open_author(app: *mut NmpApp, pubkey: *const c_char) {
crates/nmp-core/src/ffi.rs:165:pub extern "C" fn nmp_app_open_thread(app: *mut NmpApp, event_id: *const c_char) {
crates/nmp-core/src/ffi.rs:180:pub extern "C" fn nmp_app_open_firehose_tag(app: *mut NmpApp, tag: *const c_char) {
crates/nmp-core/src/ffi.rs:192:pub extern "C" fn nmp_app_claim_profile(
crates/nmp-core/src/ffi.rs:217:pub extern "C" fn nmp_app_release_profile(
crates/nmp-core/src/ffi.rs:242:pub extern "C" fn nmp_app_close_author(app: *mut NmpApp, pubkey: *const c_char) {
crates/nmp-core/src/ffi.rs:257:pub extern "C" fn nmp_app_close_thread(app: *mut NmpApp, event_id: *const c_char) {
docs/design/ffi-hardening.md:77:type UpdateCallback = extern "C" fn(*mut c_void, *const c_char);
docs/design/ffi-hardening.md:193:│   ├── metrics.json         # machine-readable, schema-versioned
docs/design/ffi-hardening.md:217:| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
docs/design/ffi-hardening.md:243:D3-incomplete in the user-visible sense** (no toast surfaces in
docs/design/ffi-hardening.md:247:errors-as-data crosses FFI via `RelayStatus.last_error` and
docs/design/ffi-hardening.md:248:`RelayStatus.last_notice`, which is correct, but **app-action error
docs/design/ffi-hardening.md:250:adds a `toast: Option<String>` field to the JSON update payload
docs/design/ffi-hardening.md:251:(placed alongside `logs: Vec<String>` as a sibling of `metrics` in the
docs/design/ffi-hardening.md:259:returns; M10.5 ships the interim toast-field bridge.
docs/design/ffi-hardening.md:271:| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
docs/design/ffi-hardening.md:287:1. **Toast field schema.** Is `toast: Option<String>` enough, or do we
docs/design/ffi-hardening.md:288:   want `toast: Option<{ id: String, severity: Info|Warn|Error, message: String, source: String }>`? The latter is more useful but
docs/design/ffi-hardening/scenarios.md:162:`update.metrics.eventsSinceLastUpdate > 0`, immediately dispatches
docs/design/ffi-hardening/scenarios.md:242:2. Every silent-no-op input produces a **toast field** in the next
docs/design/ffi-hardening/scenarios.md:243:   emit (post §7.2 toast-bridge addition) — current behavior fails
docs/design/ffi-hardening/scenarios.md:245:   M10.5 fix adds the toast field.
docs/design/ffi-hardening/scenarios.md:246:3. Every typed error path's toast string is non-empty and
docs/design/ffi-hardening/scenarios.md:253:on toast banner rendering).
docs/design/ffi-hardening/scenarios.md:309:4. `reconnect_count` field in `RelayStatus` matches the harness's
docs/plan.md:210:- Auth failure (wrong signer) produces a visible diagnostic state and a toast in the app; subscriptions stay paused until resolved.
docs/plan.md:236:- Bug-extinction #9 (NIP-46 lost on suspend): simulate suspend mid-publish; resume retries or surfaces failure as toast.
docs/plan.md:356:  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
docs/plan.md:724:| Manual exploratory | Humans on reference devices | What metrics can't catch | per-milestone manual checklist |
docs/design/ffi-hardening/harness.md:22:├── report.rs                # JSON metrics + markdown report writer
docs/design/ffi-hardening/harness.md:227:        let metricsExporter = app.staticTexts["stress-metrics-exporter"]
docs/design/ffi-hardening/harness.md:228:        XCTAssertTrue(metricsExporter.waitForExistence(timeout: 620))
docs/design/ffi-hardening/harness.md:230:        let payload = metricsExporter.label  // JSON blob
docs/design/ffi-hardening/harness.md:231:        let metrics = try JSONDecoder().decode(StressMetrics.self,
docs/design/ffi-hardening/harness.md:234:        XCTAssertEqual(metrics.unmatchedClaims, 0)
docs/design/ffi-hardening/harness.md:235:        XCTAssertLessThanOrEqual(metrics.rssGrowthBytes, 5 * 1024 * 1024)
docs/design/ffi-hardening/harness.md:236:        XCTAssertEqual(metrics.instrumentsLeakCount, 0,
docs/design/ffi-hardening/harness.md:244:needed), runs the scenario, then exposes a JSON metrics blob as an
docs/design/ffi-hardening/harness.md:250:### 2.3 Performance metrics
docs/design/ffi-hardening/harness.md:317:assertion (UI element present? rev increased? no error toast?) to
docs/design/ffi-hardening/harness.md:372:All three runners produce `metrics.json` with the same schema so the
docs/design/ffi-hardening/harness.md:388:  "metrics": { /* scenario-specific KV pairs */ },
docs/design/ffi-hardening/ci.md:105:│   ├── metrics.json                # schema in harness.md §4
docs/design/ffi-hardening/ci.md:165:        m = load(f"{perf_dir}/{scenario}/metrics.json")
docs/design/ffi-hardening/ci.md:297:replay against the same trace must produce byte-identical metrics
docs/design/ffi-hardening/ci.md:313:  prereq` with a note in `metrics.json`; gate script treats this
crates/nmp-core/src/kernel/status.rs:4:    pub(super) fn relay_status(&self) -> RelayStatus {
crates/nmp-core/src/kernel/status.rs:8:    pub(super) fn relay_statuses(&self) -> Vec<RelayStatus> {
crates/nmp-core/src/kernel/status.rs:15:    pub(super) fn relay_status_for(&self, role: RelayRole) -> RelayStatus {
crates/nmp-core/src/kernel/status.rs:17:        RelayStatus {
crates/nmp-core/src/kernel/status.rs:33:            last_notice: relay.last_notice.clone(),
crates/nmp-core/src/kernel/status.rs:34:            last_error: relay.last_error.clone(),
crates/nmp-core/src/kernel/status.rs:313:        self.logs.push_back(line);
crates/nmp-core/src/kernel/status.rs:314:        while self.logs.len() > 80 {
crates/nmp-core/src/kernel/status.rs:315:            self.logs.pop_front();
docs/design/ffi-hardening/gates.md:202:  errors-as-data crosses FFI correctly via `RelayStatus.last_error`
docs/design/ffi-hardening/gates.md:203:  / `last_notice`. App-action error paths are
docs/design/ffi-hardening/gates.md:207:  adds a `toast: Option<String>` field to the JSON update
docs/design/ffi-hardening/gates.md:255:| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
docs/design/ffi-hardening/gates.md:256:| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
docs/design/ffi-hardening/gates.md:257:| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
docs/design/ffi-hardening/gates.md:258:| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
docs/design/ffi-hardening/gates.md:259:| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
docs/design/ffi-hardening/gates.md:260:| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
docs/perf/m10.5/debt-inventory.md:47:73  pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
docs/perf/m10.5/debt-inventory.md:320:- Error states are communicated via the update callback: `RelayStatus.last_error` and `RelayStatus.last_notice` fields in JSON payloads.
docs/perf/m10.5/debt-inventory.md:359:| 14 integer casts (count→metric types) (F6–F14) | Justified | All bounded by design constraints (metrics counters, working-set bounds); no overflow risk. Casts are intentional and safe. |
crates/nmp-core/src/kernel/update.rs:41:            metrics: Metrics {
crates/nmp-core/src/kernel/update.rs:91:            logs: self.logs.iter().cloned().collect(),
crates/nmp-core/src/kernel/update.rs:95:        update.metrics.payload_bytes = first.len();
crates/nmp-core/src/kernel/update.rs:96:        update.metrics.store_to_payload_ratio = ratio(
crates/nmp-core/src/kernel/update.rs:97:            update.metrics.estimated_store_bytes,
crates/nmp-core/src/kernel/update.rs:98:            update.metrics.payload_bytes,
crates/nmp-core/src/kernel/update.rs:109:                update.metrics.payload_bytes,

exec
/bin/zsh -lc 'rg -n "13 exported|14 declarations|14 symbols|S10|conditional|PASS-with-deferral|device-absent|does not block|advisory|nightly only|not pre-merge|gate script|stress-gate|stress-pre-merge|S4|S6|S9|all ten|All ten|Every cell|specific number" docs/design/ffi-hardening.md docs/design/ffi-hardening/*.md' in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc 'rg -n "TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit|optional|Optional|recommended|Recommended|defer|deferred|deferral|NIP-XX|production-ready|Exit Criteria|READY|acceptable|fast enough" docs/design/ffi-hardening.md docs/design/ffi-hardening/*.md docs/perf/codex-reviews/2026-05-18-session-1.md docs/perf/m10.5/debt-inventory.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
docs/design/ffi-hardening/scenarios.md:44:Secondary (nightly only): XCUITest `S1MountUnmountChurn.swift` driving
docs/design/ffi-hardening/scenarios.md:119:## S4. Reconciler back-pressure — main thread stalled 250 ms
docs/design/ffi-hardening/scenarios.md:185:## S6. Capability lifecycle storms — start/stop/restart
docs/design/ffi-hardening/scenarios.md:287:## S9. Relay flap — simulated WebSocket disconnect/reconnect storm
docs/design/ffi-hardening/scenarios.md:317:network reachability — nightly only).
docs/design/ffi-hardening/scenarios.md:321:## S10. Long suspend simulation — 60-second background
docs/design/ffi-hardening/scenarios.md:323:**Status: conditional on M3 (event store) + M4 (sync watermarks).**
docs/design/ffi-hardening/scenarios.md:325:only if M3+M4 are complete by then.** If not, S10 graduates to
docs/design/ffi-hardening/harness.md:24:├── mock_relay.rs            # in-process flap-able WebSocket (S9)
docs/design/ffi-hardening/harness.md:31:│   ├── lifecycle_storm.rs   # S6
docs/design/ffi-hardening/harness.md:34:│   ├── relay_flap.rs        # S9
docs/design/ffi-hardening/harness.md:35:│   └── long_suspend.rs      # S10 (conditional on M3+M4)
docs/design/ffi-hardening/harness.md:39:(S4 reconciler back-pressure is iOS-main-thread-only; lives in
docs/design/ffi-hardening/harness.md:52:  lifecycle-storm      S6
docs/design/ffi-hardening/harness.md:55:  relay-flap           S9
docs/design/ffi-hardening/harness.md:56:  long-suspend         S10 (skipped unless --experimental-suspend)
docs/design/ffi-hardening/harness.md:146:only (not in `nmp-core`). Used by S1, S2, S3, S6, S8 to detect heap
docs/design/ffi-hardening/harness.md:149:### 1.5 Mock relay for S9 (relay flap)
docs/design/ffi-hardening/harness.md:174:    // ... (rest of the 14 declarations)
docs/design/ffi-hardening/harness.md:196:├── S4ReconcilerBackpressure.swift  # iOS-only
docs/design/ffi-hardening/harness.md:198:├── S6LifecycleStorms.swift
docs/design/ffi-hardening/harness.md:201:├── S9RelayFlap.swift               # nightly device only
docs/design/ffi-hardening/harness.md:202:└── S10LongSuspend.swift            # conditional on M3+M4
docs/design/ffi-hardening/harness.md:254:- `XCTHitchMetric` — main-thread hitches (S2, S3, S4).
docs/design/ffi-hardening/harness.md:255:- `XCTClockMetric` — wall time (S1, S6).
docs/design/ffi-hardening/harness.md:357:Trade-off: non-determinism makes flaky CI. Mitigation: nightly only,
docs/design/ffi-hardening/harness.md:358:not pre-merge; treated as advisory unless multiple agents in one
docs/design/ffi-hardening/harness.md:407:- **Multi-account.** S5/S6 use a single account; multi-account
docs/design/ffi-hardening/ci.md:84:stress-gate:
docs/design/ffi-hardening/ci.md:85:    python3 scripts/stress-gate.py docs/perf/m10.5/ \
docs/design/ffi-hardening/ci.md:117:├── ... (one dir per scenario through S10)
docs/design/ffi-hardening/ci.md:155:### R.4 The gate script
docs/design/ffi-hardening/ci.md:157:`scripts/stress-gate.py` is the source of truth for "is M10.5
docs/design/ffi-hardening/ci.md:163:    for scenario in ["S1", "S2", "S3", "S5", "S6", "S7", "S8", "S9"]:
docs/design/ffi-hardening/ci.md:164:        # S4 is XCUITest-only; S10 is conditional on M3+M4
docs/design/ffi-hardening/ci.md:199:S7 (full matrix), S8 (60 s). **Not S4** (iOS-main-thread, slow XCUITest
docs/design/ffi-hardening/ci.md:200:boot) — runs nightly instead. **Not S6** (5 min) — runs nightly.
docs/design/ffi-hardening/ci.md:201:**Not S9** (10 min) — nightly. **Not S10** (conditional).
docs/design/ffi-hardening/ci.md:203:**Workflow.** `.github/workflows/stress-pre-merge.yml`:
docs/design/ffi-hardening/ci.md:223:      - run: just stress-gate
docs/design/ffi-hardening/ci.md:227:          name: stress-pre-merge-${{ github.run_id }}
docs/design/ffi-hardening/ci.md:231:**Gating.** The `just stress-gate` step exit code is the PR gate.
docs/design/ffi-hardening/ci.md:238:**Scenarios.** All ten at full duration: S1 (10 min), S2 (60 s),
docs/design/ffi-hardening/ci.md:239:S3 (10 emits × 100 k events), S4 (60 s × 12 stalls), S5 (30 s),
docs/design/ffi-hardening/ci.md:240:S6 (1 000 cycles), S7 (full matrix), S8 (5 storms), S9 (10 min ×
docs/design/ffi-hardening/ci.md:241:100/min), S10 (60 s suspend — *only if M3+M4 are complete; the
docs/design/ffi-hardening/ci.md:262:      - run: just stress-gate || echo "::warning::nightly gate failed"
docs/design/ffi-hardening/ci.md:282:- S9: 24-hour relay flap.
docs/design/ffi-hardening/ci.md:303:### C.5 What does not block CI
docs/design/ffi-hardening/ci.md:305:- **Sonnet-agent runs** are advisory. Flake by design; failures are
docs/design/ffi-hardening/ci.md:308:- **iPhone 12 hardware-only scenarios** (S9 device variant, S4 device
docs/design/ffi-hardening/ci.md:310:  results are noted in the report and the gate script
docs/design/ffi-hardening/ci.md:311:  treats "device-absent" as a deferred-not-failed state.
docs/design/ffi-hardening/ci.md:312:- **S10 if M3+M4 are not complete:** scenario reports as `skipped:
docs/design/ffi-hardening/ci.md:313:  prereq` with a note in `metrics.json`; gate script treats this
docs/design/ffi-hardening/ci.md:314:  as PASS-with-deferral.
docs/design/ffi-hardening/ci.md:334:1. Latest nightly run = PASS on every scenario (or PASS-with-deferral
docs/design/ffi-hardening/ci.md:335:   for S10).
docs/design/ffi-hardening/gates.md:5:   contract; every cell is a specific number with units. No "fast
docs/design/ffi-hardening/gates.md:58:### G-S4. Reconciler back-pressure (12 stalls × 250 ms, 60 s)
docs/design/ffi-hardening/gates.md:80:### G-S6. Capability lifecycle storms (1 000 cycles, 5 min)
docs/design/ffi-hardening/gates.md:101:| Symbols × invalid-input variants exercised | `>=` | 70 (14 symbols × 5 variants avg) | 70 |
docs/design/ffi-hardening/gates.md:114:### G-S9. Relay flap (100/min × 10 min = 1 000 flaps)
docs/design/ffi-hardening/gates.md:126:### G-S10. Long suspend (conditional on M3+M4)
docs/design/ffi-hardening/gates.md:166:- ✅ **Stress proof:** S6 (capability lifecycle storms) does
docs/design/ffi-hardening/gates.md:182:- ✅ **Stress proof (long path):** S10 (long suspend) — on resume
docs/design/ffi-hardening/gates.md:255:| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
docs/design/ffi-hardening/gates.md:256:| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
docs/design/ffi-hardening.md:31:   memory growth". Every cell of the §5 table in
docs/design/ffi-hardening.md:32:   [`gates.md`](./ffi-hardening/gates.md) is a specific number.
docs/design/ffi-hardening.md:51:The current FFI surface is **13 exported C symbols** in
docs/design/ffi-hardening.md:138:| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
docs/design/ffi-hardening.md:140:| S6 | Capability lifecycle storms | start/stop/restart | bible #7 (idempotent) |
docs/design/ffi-hardening.md:143:| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
docs/design/ffi-hardening.md:144:| S10 | Long suspend simulation | background → foreground | bible #1 (monotonic rev) |
docs/design/ffi-hardening.md:175:Every cell is a number with units.
docs/design/ffi-hardening.md:217:| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
docs/design/ffi-hardening.md:268:| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
docs/design/ffi-hardening.md:269:| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
docs/design/ffi-hardening.md:282:| **Nightly** | daily | All ten, S1 full 10-min, S4 250 ms stalls × 60, S9 100 flap, S10 60s suspend | Mac mini self-hosted runner, iPhone 12 device |
docs/design/ffi-hardening.md:283:| **On-demand** | release candidates | S1 8-hour soak, S2 1 M dispatch, S9 24-hour flap | Lab device, manual sign-off |

exec
/bin/zsh -lc 'find docs/design/ffi-hardening docs/design/ffi-hardening.md docs/perf/codex-reviews/2026-05-18-session-1.md docs/perf/m10.5/debt-inventory.md docs/plan.md -type f -maxdepth 3 -print0 | xargs -0 wc -l | sort -nr' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
docs/design/ffi-hardening/scenarios.md:261:working set explodes; relay workers can't send fast enough.
docs/design/ffi-hardening/scenarios.md:312:   `defer_outbound` path captures any send during disconnect, and
docs/design/ffi-hardening/scenarios.md:313:   the harness validates the deferred queue drains on reconnect.
docs/perf/m10.5/debt-inventory.md:6:**Exit Criteria:** All findings classified; doctrine violations identified and severity-ranked
docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/m10.5/debt-inventory.md:368:Per the M10.5 zero-shortcut posture (no "for later", no "ADR escape" for files in scope), the following are **required cleanup** before the M10.5 empirical exit gate:
docs/perf/m10.5/debt-inventory.md:383:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/m10.5/debt-inventory.md:396:The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
docs/design/ffi-hardening/harness.md:404:  are deferred.
docs/design/ffi-hardening/gates.md:6:   enough", no "acceptable".
docs/design/ffi-hardening/gates.md:263:<any caveats, deferrals, follow-ups>
docs/plan.md:7:> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.
docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
docs/plan.md:231:- Login UX (single nsec field for now; multi-step onboarding deferred to M16).
docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Hard zero — no deferral escape, no "tracking issue" carve-out. Every pre-existing one is resolved in M10.5. If something genuinely cannot be done in M10.5 because it belongs to a later milestone (e.g. NIP-65 outbox work), then it is not a TODO/FIXME in the scoped files — it lives as a milestone task in `docs/plan.md`, not as a code marker.
docs/plan.md:444:- `nmp-podcast` is **not a v1 deliverable** unless a real published NIP is selected during M11 design (e.g. NIP-54 or a successor) and the choice is recorded in `docs/design/podcast-app-rebuild.md`. Until then, the podcast app uses RSS + Podcast 2.0 namespaces (chapters, transcripts, value-for-value) via `podcast-feeds`, and Nostr is the **social overlay only** — kind:1 discussion threads referencing the episode URL/GUID, kind:7 reactions, NIP-57 zaps. The decision is locked in M11 step-0; no `NIP-XX` placeholders allowed in code or in plans past that point.
docs/design/ffi-hardening/ci.md:182:            fails.append("FFI grep yielded TODO/FIXME tokens; see §7.1")
docs/design/ffi-hardening/ci.md:242:harness skips with a noted "deferred" if not*).
docs/design/ffi-hardening/ci.md:311:  treats "device-absent" as a deferred-not-failed state.
docs/design/ffi-hardening/ci.md:314:  as PASS-with-deferral.
docs/design/ffi-hardening/ci.md:334:1. Latest nightly run = PASS on every scenario (or PASS-with-deferral
docs/design/ffi-hardening.md:30:5. **Reproducible numeric exit gates.** No "fast enough", no "acceptable
docs/design/ffi-hardening.md:174:No row in §5 contains the word "acceptable" or the phrase "fast enough".
docs/design/ffi-hardening.md:225:grep -rEn '(TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit)' \
docs/design/ffi-hardening.md:232:triaged into either *resolve before close* or *deferred with ADR + GH
docs/design/ffi-hardening.md:291:   M10.5, revisit in ADR-0011 when M14 lands.
docs/design/ffi-hardening.md:296:   record + replay, but defer the replay infrastructure to M11.5.
docs/perf/codex-reviews/2026-05-18-session-1.md:25:Session goal: complete v1 with zero technical debt, no "for later" shortcuts, robust guardrails, empirical iOS proof before the M11 podcast-app rebuild of /Users/pablofernandez/src/podcast.
docs/perf/codex-reviews/2026-05-18-session-1.md:43:- 0 critical debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!)
docs/perf/codex-reviews/2026-05-18-session-1.md:63:- (Optional) Add 3 safety comments to ffi.rs unsafe blocks (5 min improvement)
docs/perf/codex-reviews/2026-05-18-session-1.md:64:- (Optional) Add 1 comment to relay_worker.rs for defensive intent (2 min)
docs/perf/codex-reviews/2026-05-18-session-1.md:67:M10.5 Exit Criteria: Ready for iOS empirical proof phase.
docs/perf/codex-reviews/2026-05-18-session-1.md:86:- Zero open TODO/FIXME/unimplemented in FFI/actor/relay/kernel/iOS bridge
docs/perf/codex-reviews/2026-05-18-session-1.md:119:+**Exit Criteria:** All findings classified; doctrine violations identified and severity-ranked
docs/perf/codex-reviews/2026-05-18-session-1.md:125:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/2026-05-18-session-1.md:472:+| 3 unsafe blocks in ffi.rs (F1, F2, F3) | Documentation gap | Standard FFI pattern; pointers validated by caller contract. Safety comments recommended for future audits but code is correct. |
docs/perf/codex-reviews/2026-05-18-session-1.md:481:+## 5. Recommended Next Actions
docs/perf/codex-reviews/2026-05-18-session-1.md:483:+### Documentation-Only Improvements (Recommended for M10.5)
docs/perf/codex-reviews/2026-05-18-session-1.md:491:+   - Recommended text:
docs/perf/codex-reviews/2026-05-18-session-1.md:500:+   - Recommended text: `// Stream type may have additional TLS variants in future tungstenite versions`
docs/perf/codex-reviews/2026-05-18-session-1.md:519:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/2026-05-18-session-1.md:527:+| Documentation improvements (optional) | 3 | → Recommended |
docs/perf/codex-reviews/2026-05-18-session-1.md:533:+The FFI and iOS bridge code is **production-ready for M10.5**. 
docs/perf/codex-reviews/2026-05-18-session-1.md:535:+**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/2026-05-18-session-1.md:537:+**Recommendation:** The 3 safety comments in ffi.rs are optional but recommended for auditability and preventing future misclassification.
docs/perf/codex-reviews/2026-05-18-session-1.md:539:+**M10.5 Exit Criteria:** ✅ **READY**
docs/perf/codex-reviews/2026-05-18-session-1.md:561:+| 2026-05-18 01:30 | 0a | Advisor pass: broadcast safe-rebase-push protocol to all 6 running agents (avoid push race on master). T1 description updated to mandate worktree isolation + rebase-push protocol. Heartbeat cron rewritten (job 811003f1) with stronger triage rules (design→review→impl chain; M11 gated on M10.5 *empirical* pass, not just designed; debt triage uses both must-fix and ADR-defer lanes; DerivedData sprawl mitigation; orphan detection). Heartbeat runtime is session-only (durable flag ignored). 6 stale stashes from prior codex sessions dropped. |
docs/perf/codex-reviews/2026-05-18-session-1.md:574:+> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.
docs/perf/codex-reviews/2026-05-18-session-1.md:601:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/2026-05-18-session-1.md:619:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/2026-05-18-session-1.md:711:+- `nmp-podcast` (Nostr podcast NIP integration where it exists — NIP-XX podcast feed events, value-for-value zaps, episode discussion threads). Where Nostr coverage is incomplete, the app uses RSS via `podcast-feeds` and Nostr for social overlay (zaps, discussions, recommendations from the WoT subsystem).
docs/perf/codex-reviews/2026-05-18-session-1.md:716:-- `nmp-podcast` (or whatever the Nostr podcast NIP is called, e.g. NIP-XX for podcast feed events): parsed feed events. If no NIP, the app uses RSS via the action ledger to fetch + parse, storing entries as domain records.
docs/perf/codex-reviews/2026-05-18-session-1.md:793:2. TODO / FIXME / unimplemented / "for later" snuck in
docs/perf/codex-reviews/2026-05-18-session-1.md:829:/bin/zsh -lc 'rg -n "TODO|FIXME|XXX|unimplemented!|todo!|for later|later|Optional|optional|recommended|Recommended|No action required|defer|deferral|future|worktree remove --force|dropped" docs/perf/m10.5/debt-inventory.md docs/perf/orchestration-log.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/2026-05-18-session-1.md:831:docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/2026-05-18-session-1.md:835:docs/perf/m10.5/debt-inventory.md:359:| 3 unsafe blocks in ffi.rs (F1, F2, F3) | Documentation gap | Standard FFI pattern; pointers validated by caller contract. Safety comments recommended for future audits but code is correct. |
docs/perf/codex-reviews/2026-05-18-session-1.md:836:docs/perf/m10.5/debt-inventory.md:368:## 5. Recommended Next Actions
docs/perf/codex-reviews/2026-05-18-session-1.md:837:docs/perf/m10.5/debt-inventory.md:370:### Documentation-Only Improvements (Recommended for M10.5)
docs/perf/codex-reviews/2026-05-18-session-1.md:840:docs/perf/m10.5/debt-inventory.md:378:   - Recommended text:
docs/perf/codex-reviews/2026-05-18-session-1.md:841:docs/perf/m10.5/debt-inventory.md:387:   - Recommended text: `// Stream type may have additional TLS variants in future tungstenite versions`
docs/perf/codex-reviews/2026-05-18-session-1.md:842:docs/perf/m10.5/debt-inventory.md:406:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/2026-05-18-session-1.md:843:docs/perf/m10.5/debt-inventory.md:414:| Documentation improvements (optional) | 3 | → Recommended |
docs/perf/codex-reviews/2026-05-18-session-1.md:844:docs/perf/m10.5/debt-inventory.md:424:**Recommendation:** The 3 safety comments in ffi.rs are optional but recommended for auditability and preventing future misclassification.
docs/perf/codex-reviews/2026-05-18-session-1.md:845:docs/plan.md:7:> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.
docs/perf/codex-reviews/2026-05-18-session-1.md:846:docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
docs/perf/codex-reviews/2026-05-18-session-1.md:848:docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
docs/perf/codex-reviews/2026-05-18-session-1.md:849:docs/plan.md:231:- Login UX (single nsec field for now; multi-step onboarding deferred to M16).
docs/perf/codex-reviews/2026-05-18-session-1.md:850:docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/2026-05-18-session-1.md:852:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/2026-05-18-session-1.md:855:docs/perf/orchestration-log.md:8:| 2026-05-18 01:30 | 0a | Advisor pass: broadcast safe-rebase-push protocol to all 6 running agents (avoid push race on master). T1 description updated to mandate worktree isolation + rebase-push protocol. Heartbeat cron rewritten (job 811003f1) with stronger triage rules (design→review→impl chain; M11 gated on M10.5 *empirical* pass, not just designed; debt triage uses both must-fix and ADR-defer lanes; DerivedData sprawl mitigation; orphan detection). Heartbeat runtime is session-only (durable flag ignored). 6 stale stashes from prior codex sessions dropped. |
docs/perf/codex-reviews/2026-05-18-session-1.md:858:The first pass found two concrete review axes already: a new docs file over the 300-line soft budget, and the debt inventory treating known documentation gaps as optional despite the new “no debt / no for-later carve-outs” language. I’m checking the exact sections now so the final review can be line-specific.
docs/perf/codex-reviews/2026-05-18-session-1.md:872:   368	## 5. Recommended Next Actions
docs/perf/codex-reviews/2026-05-18-session-1.md:874:   370	### Documentation-Only Improvements (Recommended for M10.5)
docs/perf/codex-reviews/2026-05-18-session-1.md:882:   378	   - Recommended text:
docs/perf/codex-reviews/2026-05-18-session-1.md:891:   387	   - Recommended text: `// Stream type may have additional TLS variants in future tungstenite versions`
docs/perf/codex-reviews/2026-05-18-session-1.md:910:   406	| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/2026-05-18-session-1.md:918:   414	| Documentation improvements (optional) | 3 | → Recommended |
docs/perf/codex-reviews/2026-05-18-session-1.md:924:   420	The FFI and iOS bridge code is **production-ready for M10.5**. 
docs/perf/codex-reviews/2026-05-18-session-1.md:926:   422	**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/2026-05-18-session-1.md:928:   424	**Recommendation:** The 3 safety comments in ffi.rs are optional but recommended for auditability and preventing future misclassification.
docs/perf/codex-reviews/2026-05-18-session-1.md:930:   426	**M10.5 Exit Criteria:** ✅ **READY**
docs/perf/codex-reviews/2026-05-18-session-1.md:948:   345	**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/2026-05-18-session-1.md:966:   363	- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/2026-05-18-session-1.md:1002:     8	| 2026-05-18 01:30 | 0a | Advisor pass: broadcast safe-rebase-push protocol to all 6 running agents (avoid push race on master). T1 description updated to mandate worktree isolation + rebase-push protocol. Heartbeat cron rewritten (job 811003f1) with stronger triage rules (design→review→impl chain; M11 gated on M10.5 *empirical* pass, not just designed; debt triage uses both must-fix and ADR-defer lanes; DerivedData sprawl mitigation; orphan detection). Heartbeat runtime is session-only (durable flag ignored). 6 stale stashes from prior codex sessions dropped. |
docs/perf/codex-reviews/2026-05-18-session-1.md:1063:   444	- `nmp-podcast` (Nostr podcast NIP integration where it exists — NIP-XX podcast feed events, value-for-value zaps, episode discussion threads). Where Nostr coverage is incomplete, the app uses RSS via `podcast-feeds` and Nostr for social overlay (zaps, discussions, recommendations from the WoT subsystem).
docs/perf/codex-reviews/2026-05-18-session-1.md:1128:/bin/zsh -lc "rg -n \"NIP-XX|optional|deferral|deferred|READY|production-ready|Exit Criteria|M10\\.5 Exit\" docs/plan.md docs/perf/m10.5/debt-inventory.md" in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/2026-05-18-session-1.md:1130:docs/plan.md:231:- Login UX (single nsec field for now; multi-step onboarding deferred to M16).
docs/perf/codex-reviews/2026-05-18-session-1.md:1131:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/2026-05-18-session-1.md:1132:docs/plan.md:444:- `nmp-podcast` (Nostr podcast NIP integration where it exists — NIP-XX podcast feed events, value-for-value zaps, episode discussion threads). Where Nostr coverage is incomplete, the app uses RSS via `podcast-feeds` and Nostr for social overlay (zaps, discussions, recommendations from the WoT subsystem).
docs/perf/codex-reviews/2026-05-18-session-1.md:1133:docs/perf/m10.5/debt-inventory.md:6:**Exit Criteria:** All findings classified; doctrine violations identified and severity-ranked
docs/perf/codex-reviews/2026-05-18-session-1.md:1134:docs/perf/m10.5/debt-inventory.md:414:| Documentation improvements (optional) | 3 | → Recommended |
docs/perf/codex-reviews/2026-05-18-session-1.md:1135:docs/perf/m10.5/debt-inventory.md:420:The FFI and iOS bridge code is **production-ready for M10.5**. 
docs/perf/codex-reviews/2026-05-18-session-1.md:1136:docs/perf/m10.5/debt-inventory.md:424:**Recommendation:** The 3 safety comments in ffi.rs are optional but recommended for auditability and preventing future misclassification.
docs/perf/codex-reviews/2026-05-18-session-1.md:1137:docs/perf/m10.5/debt-inventory.md:426:**M10.5 Exit Criteria:** ✅ **READY**
docs/perf/codex-reviews/2026-05-18-session-1.md:1140:/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/plan.md | rg -n "deferred|deferral|NIP-XX|optional|for later|wire it up later|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/2026-05-18-session-1.md:1142:/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/perf/m10.5/debt-inventory.md | rg -n "optional|recommended|Recommended|READY|production-ready|No Action Required|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/2026-05-18-session-1.md:1144:18:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/2026-05-18-session-1.md:1146:365:+| 3 unsafe blocks in ffi.rs (F1, F2, F3) | Documentation gap | Standard FFI pattern; pointers validated by caller contract. Safety comments recommended for future audits but code is correct. |
docs/perf/codex-reviews/2026-05-18-session-1.md:1147:374:+## 5. Recommended Next Actions
docs/perf/codex-reviews/2026-05-18-session-1.md:1148:376:+### Documentation-Only Improvements (Recommended for M10.5)
docs/perf/codex-reviews/2026-05-18-session-1.md:1149:384:+   - Recommended text:
docs/perf/codex-reviews/2026-05-18-session-1.md:1150:393:+   - Recommended text: `// Stream type may have additional TLS variants in future tungstenite versions`
docs/perf/codex-reviews/2026-05-18-session-1.md:1152:412:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/2026-05-18-session-1.md:1153:420:+| Documentation improvements (optional) | 3 | → Recommended |
docs/perf/codex-reviews/2026-05-18-session-1.md:1154:426:+The FFI and iOS bridge code is **production-ready for M10.5**. 
docs/perf/codex-reviews/2026-05-18-session-1.md:1155:430:+**Recommendation:** The 3 safety comments in ffi.rs are optional but recommended for auditability and preventing future misclassification.
docs/perf/codex-reviews/2026-05-18-session-1.md:1156:432:+**M10.5 Exit Criteria:** ✅ **READY**
docs/perf/codex-reviews/2026-05-18-session-1.md:1159:13:+> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.
docs/perf/codex-reviews/2026-05-18-session-1.md:1160:40:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/2026-05-18-session-1.md:1161:58:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/2026-05-18-session-1.md:1162:150:+- `nmp-podcast` (Nostr podcast NIP integration where it exists — NIP-XX podcast feed events, value-for-value zaps, episode discussion threads). Where Nostr coverage is incomplete, the app uses RSS via `podcast-feeds` and Nostr for social overlay (zaps, discussions, recommendations from the WoT subsystem).
docs/perf/codex-reviews/2026-05-18-session-1.md:1163:155:-- `nmp-podcast` (or whatever the Nostr podcast NIP is called, e.g. NIP-XX for podcast feed events): parsed feed events. If no NIP, the app uses RSS via the action ledger to fetch + parse, storing entries as domain records.
docs/perf/codex-reviews/2026-05-18-session-1.md:1235:- [docs/perf/m10.5/debt-inventory.md:420](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:420) and [line 426](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:426): this signs off “production-ready” / “M10.5 Exit Criteria READY” from a read-only audit, but [docs/plan.md:373](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:373) requires stress harness, iPhone 12, Instruments, UI fleet, perf reruns, and doctrine review. Fix: downgrade this to “audit ready for empirical proof” and remove exit-ready language.
docs/perf/codex-reviews/2026-05-18-session-1.md:1237:- [docs/perf/m10.5/debt-inventory.md:414](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:414) and [line 424](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:424): known FFI safety-comment gaps are marked optional. That conflicts with zero-debt / no-shortcut posture. Fix: make F1-F4 required M10.5 cleanup or explicitly move them outside exit evidence.
docs/perf/codex-reviews/2026-05-18-session-1.md:1239:- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
docs/perf/codex-reviews/2026-05-18-session-1.md:1243:- [docs/plan.md:444](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:444): `NIP-XX` is still placeholder protocol language. Fix: name the actual NIP/spec, or state “no known NIP; RSS plus Nostr social overlay only until a real spec is selected.”
docs/perf/codex-reviews/2026-05-18-session-1.md:1246:No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
docs/perf/codex-reviews/2026-05-18-session-1.md:1252:- [docs/perf/m10.5/debt-inventory.md:420](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:420) and [line 426](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:426): this signs off “production-ready” / “M10.5 Exit Criteria READY” from a read-only audit, but [docs/plan.md:373](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:373) requires stress harness, iPhone 12, Instruments, UI fleet, perf reruns, and doctrine review. Fix: downgrade this to “audit ready for empirical proof” and remove exit-ready language.
docs/perf/codex-reviews/2026-05-18-session-1.md:1254:- [docs/perf/m10.5/debt-inventory.md:414](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:414) and [line 424](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:424): known FFI safety-comment gaps are marked optional. That conflicts with zero-debt / no-shortcut posture. Fix: make F1-F4 required M10.5 cleanup or explicitly move them outside exit evidence.
docs/perf/codex-reviews/2026-05-18-session-1.md:1256:- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
docs/perf/codex-reviews/2026-05-18-session-1.md:1260:- [docs/plan.md:444](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:444): `NIP-XX` is still placeholder protocol language. Fix: name the actual NIP/spec, or state “no known NIP; RSS plus Nostr social overlay only until a real spec is selected.”
docs/perf/codex-reviews/2026-05-18-session-1.md:1263:No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.

 succeeded in 0ms:
    4148 total
    1263 docs/perf/codex-reviews/2026-05-18-session-1.md
     789 docs/plan.md
     410 docs/design/ffi-hardening/harness.md
     407 docs/perf/m10.5/debt-inventory.md
     353 docs/design/ffi-hardening/scenarios.md
     345 docs/design/ffi-hardening/ci.md
     314 docs/design/ffi-hardening.md
     267 docs/design/ffi-hardening/gates.md

exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening/gates.md | sed -n '88,108p;248,262p'" in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening.md | sed -n '45,72p;210,260p;278,300p'" in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening/scenarios.md | sed -n '224,252p;318,340p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
    88	| RSS growth | `<=` | 2 MiB | 4 MiB |
    89	| Cycle wall-time p99 | `<=` | 250 ms | 500 ms |
    90	| Deadlocks (5 s per-cycle watchdog) | `==` | 0 | 0 |
    91	
    92	### G-S7. Error-shape exhaustion (full matrix)
    93	
    94	| Metric | Op | Sim threshold | iPhone 12 threshold |
    95	|---|---|---|---|
    96	| Crashes / signals across full input matrix | `==` | 0 | 0 |
    97	| Use-after-free probe (free → call) crashes | `==` | 0 | 0 |
    98	| Toast field populated on every silent-no-op path | `==` | 100 % of validation-fail paths | 100 % |
    99	| Toast strings non-empty + actionable (regex match) | `==` | 100 % | 100 % |
   100	| Instruments-Allocations delta over full matrix | `==` | 0 (no leak per error path) | 0 |
   101	| Symbols × invalid-input variants exercised | `>=` | 70 (14 symbols × 5 variants avg) | 70 |
   102	
   103	### G-S8. Subscription planner DOS (5 × 10 k storm, 30 s gap)
   104	
   105	| Metric | Op | Sim threshold | iPhone 12 threshold |
   106	|---|---|---|---|
   107	| Peak working-set RSS during storm | `<=` | 150 MiB | 200 MiB |
   108	| Wire-REQ frame count per 10 k OpenViews | `<=` | 2,500 (4× dedup floor) | 2,500 |
   248	M10.5, not as part of this design. The structure is:
   249	
   250	```
   251	# M10.5 Doctrine Review
   252	
   253	| Doctrine | Status | Evidence | Reviewer | Date |
   254	|---|---|---|---|---|
   255	| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
   256	| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
   257	| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
   258	| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
   259	| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
   260	| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
   261	
   262	## Notes

 succeeded in 0ms:
    45	  signature bombs, and kind:5 storms are out of scope for M10.5; the
    46	  harness here exercises the *FFI surface*, not the *protocol surface*.
    47	  Protocol hardening is M11/M12.
    48	
    49	## 2. FFI surface inventory
    50	
    51	The current FFI surface is **13 exported C symbols** in
    52	`crates/nmp-core/src/ffi.rs` (lines 44–268) plus **one callback type**
    53	(`UpdateCallback` at line 10). Every symbol below must have its
    54	ownership, lifetime, thread-safety, and null-handling rules documented
    55	and enforced by at least one harness scenario.
    56	
    57	| # | Symbol | Signature (C) | Ownership / lifetime / threading | Null handling |
    58	|---|---|---|---|---|
    59	| 1 | `nmp_app_new` | `void * nmp_app_new(void)` | Returns a `Box::into_raw(NmpApp)`. Caller owns. Spawns 2 OS threads (actor + listener) + N relay-worker threads on `start`. Pointer is `Send` because internal `Sender`/`Mutex` are `Send`; **callers must not share the same pointer across threads without external synchronization**. | n/a (returns) |
    60	| 2 | `nmp_app_free(*mut NmpApp)` | `void nmp_app_free(void *app)` | Reclaims the box, sends `Shutdown` to actor, joins actor + listener threads inside `Drop`. **Idempotent on null.** Caller must guarantee no other thread is mid-call into the same pointer when this is invoked. | Early-return if null (ffi.rs:74). |
    61	| 3 | `nmp_app_set_update_callback` | `void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback cb)` | Replaces the registered `(context, fn)` pair. The `*mut c_void` context is **stored as `usize`** (ffi.rs:13–16) and dereferenced inside the listener thread — caller owns the context lifetime and **must clear the callback to null before freeing the context**. Passing `cb=None` clears registration. | Returns silently if app null or mutex poisoned (ffi.rs:87–96). |
    62	| 4 | `nmp_app_start` | `void nmp_app_start(void *app, uint events_per_second, uint visible_limit, uint emit_hz)` | Fire-and-forget. `events_per_second` is currently ignored (`_events_per_second`); kept for ABI stability. `visible_limit` clamped to `[1, 500]`; 0 → default 80. `emit_hz` clamped to `[1, 12]`; 0 → default 4. Spawns relay workers on first call. | Silent no-op on null (ffi.rs:107–108). |
    63	| 5 | `nmp_app_configure` | same shape as `_start` | Same clamping. Re-tunes a running actor. | Silent no-op on null. |
    64	| 6 | `nmp_app_stop` | `void nmp_app_stop(void *app)` | Sends `Stop`; actor closes relay workers, keeps actor + listener threads alive. Caller may call `_start` again. | Silent no-op on null. |
    65	| 7 | `nmp_app_reset` | `void nmp_app_reset(void *app)` | Closes relays, replaces the in-actor `Kernel` instance, reopens relays if running. | Silent no-op on null. |
    66	| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
    67	| 9 | `nmp_app_open_thread(*mut, *const c_char)` | `void nmp_app_open_thread(void *app, const char *event_id)` | UTF-8, 64-char hex event id. `is_hex_id`-validated. Same silent-drop on bad input. | Silent no-op. |
    68	| 10 | `nmp_app_open_firehose_tag(*mut, *const c_char)` | `void nmp_app_open_firehose_tag(void *app, const char *tag)` | UTF-8 tag value. No hex validation. Empty → silent no-op. | Silent no-op. |
    69	| 11 | `nmp_app_claim_profile(*mut, *const c_char, *const c_char)` | `void nmp_app_claim_profile(void *app, const char *pubkey, const char *consumer_id)` | Hex-pubkey-validated. `consumer_id` is an opaque caller-chosen string (used by the kernel refcount table). Two strings, two lifetime contracts: both must be valid for the duration of the call only — the kernel `String`-copies them. | Silent no-op on any null or invalid. |
    70	| 12 | `nmp_app_release_profile(*mut, *const c_char, *const c_char)` | mirror of `_claim_profile` | Same. **Pair invariant:** every `claim_profile(pk, id)` must be matched by exactly one `release_profile(pk, id)`; an unmatched `release` is silently dropped by the kernel refcount logic. | Silent no-op. |
    71	| 13 | `nmp_app_close_author(*mut, *const c_char)` | `void nmp_app_close_author(void *app, const char *pubkey)` | Closes the named author view. Different from `release_profile` — closes the *view*, not a refcounted profile claim. | Silent no-op. |
    72	| 14 | `nmp_app_close_thread(*mut, *const c_char)` | `void nmp_app_close_thread(void *app, const char *event_id)` | Closes the named thread view. | Silent no-op. |
   210	
   211	| Ref | File:line | Action | Effort |
   212	|---|---|---|---|
   213	| F1 | `crates/nmp-core/src/ffi.rs:75` | Add `// safe: ...` doc on unsafe `Box::from_raw` | 5 min |
   214	| F2 | `crates/nmp-core/src/ffi.rs:275` | Same on unsafe `&*app` | 5 min |
   215	| F3 | `crates/nmp-core/src/ffi.rs:284` | Same on unsafe `CStr::from_ptr` | 5 min |
   216	| F4 | `crates/nmp-core/src/relay_worker.rs:242` | Comment `#[allow(unreachable_patterns)]` rationale | 2 min |
   217	| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
   218	
   219	### 7.1 Re-grep gate
   220	
   221	The M10.5 done declaration is **gated on a re-run of the audit grep
   222	yielding zero results**. The exact command (captured 2026-05-18, 0 hits):
   223	
   224	```bash
   225	grep -rEn '(TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit)' \
   226	  crates/nmp-core/src crates/nmp-testing/src \
   227	  ios/NmpStress/NmpStress \
   228	  | grep -v target | grep -v DerivedData
   229	```
   230	
   231	If the re-run at gate close yields any hits, the new items are
   232	triaged into either *resolve before close* or *deferred with ADR + GH
   233	issue*. Net unresolved must be 0.
   234	
   235	### 7.2 D3 structural gap (named, not hidden)
   236	
   237	The harness will exercise every typed FFI input path that fails
   238	validation (§3 scenario S7). The current FFI is **silently no-op** on
   239	bad input — `nmp_app_open_author(app, NULL)`, an empty string, or a
   240	non-hex string all early-return from ffi.rs without any signal to the
   241	caller and without setting any state field. This is **D3-compliant in
   242	the strict sense** (no error crosses FFI as a value) **but
   243	D3-incomplete in the user-visible sense** (no toast surfaces in
   244	`AppState`).
   245	
   246	The debt-inventory's D3 audit (lines 317–334) concludes the same:
   247	errors-as-data crosses FFI via `RelayStatus.last_error` and
   248	`RelayStatus.last_notice`, which is correct, but **app-action error
   249	paths (invalid input dropped) have no equivalent surface**. M10.5
   250	adds a `toast: Option<String>` field to the JSON update payload
   251	(placed alongside `logs: Vec<String>` as a sibling of `metrics` in the
   252	`KernelUpdate` serialization — see `crates/nmp-core/src/kernel/update.rs`)
   253	and populates it from S7's validation failure paths. The schema change
   254	is additive (older Swift readers ignore an unknown field) so this is not
   255	a breaking FFI change.
   256	
   257	This is intentionally surfaced in the design doc, not papered over.
   258	The M14 UniFFI migration moves the surface to typed `Result`-shaped
   259	returns; M10.5 ships the interim toast-field bridge.
   260	
   278	
   279	| Tier | Frequency | Scenarios | Where |
   280	|---|---|---|---|
   281	| **Pre-merge** | every PR | S1 (short), S2, S3 (10k events), S5, S7, S8 | GH Actions `macos-14` runner, iPhone 16 Pro sim |
   282	| **Nightly** | daily | All ten, S1 full 10-min, S4 250 ms stalls × 60, S9 100 flap, S10 60s suspend | Mac mini self-hosted runner, iPhone 12 device |
   283	| **On-demand** | release candidates | S1 8-hour soak, S2 1 M dispatch, S9 24-hour flap | Lab device, manual sign-off |
   284	
   285	## 10. Open questions (for ADR after review)
   286	
   287	1. **Toast field schema.** Is `toast: Option<String>` enough, or do we
   288	   want `toast: Option<{ id: String, severity: Info|Warn|Error, message: String, source: String }>`? The latter is more useful but
   289	   collides with ADR-0010's plan to migrate this entire surface to
   290	   typed `Result` via UniFFI in M14. Recommend: keep it scalar for
   291	   M10.5, revisit in ADR-0011 when M14 lands.
   292	2. **Sonnet-agent determinism.** Parallel Sonnet agents producing
   293	   non-deterministic taps means flaky CI. Should the Sonnet runner be
   294	   nightly-only (no pre-merge gating), or do we record + replay agent
   295	   traces the way `firehose-bench` does for relay frames? Recommend:
   296	   record + replay, but defer the replay infrastructure to M11.5.
   297	3. **iPhone 12 vs iPhone 16 Pro baselines.** The exit gate quotes one
   298	   set of numbers; iPhone 12 is roughly 2× slower than 16 Pro on
   299	   single-thread workloads. Should gate values be device-tagged, or do
   300	   we publish two tables? Recommend: one table, scaled by a

 succeeded in 0ms:
   224	|---|---|
   225	| `nmp_app_open_author` | NULL, "", " ", "not-hex", 63-char hex, 65-char hex, UTF-8 with non-hex chars |
   226	| `nmp_app_open_thread` | same shapes |
   227	| `nmp_app_open_firehose_tag` | NULL, "" (others valid; tag is unconstrained) |
   228	| `nmp_app_claim_profile` | NULL/empty/non-hex pubkey × {NULL, "", "valid"} consumer_id |
   229	| `nmp_app_release_profile` | same matrix; also: release without prior claim |
   230	| `nmp_app_close_author` / `_thread` | same |
   231	| any `_app` arg | NULL |
   232	
   233	Plus: dispatch each symbol with `*mut NmpApp` pointing to a
   234	**freed** allocation (use-after-free probe; must not crash —
   235	ideally hits the null check after `nmp_app_free` zeroes; harness
   236	documents observed behavior).
   237	
   238	**Threading.** Caller. Pure FFI exercise.
   239	
   240	**Assertions.**
   241	1. Zero crashes / SIGSEGV / SIGABRT across the full matrix.
   242	2. Every silent-no-op input produces a **toast field** in the next
   243	   emit (post §7.2 toast-bridge addition) — current behavior fails
   244	   this assertion; the harness publishes the failing diff and the
   245	   M10.5 fix adds the toast field.
   246	3. Every typed error path's toast string is non-empty and
   247	   actionable (regex match against the catalog in
   248	   `docs/perf/m10.5/error-catalog.md`, generated by this scenario).
   249	4. No error path leaks heap memory (Instruments-Allocations delta = 0
   250	   across the matrix).
   251	
   252	**Runner.** Rust harness primary, XCUITest secondary (UI assertion
   318	
   319	---
   320	
   321	## S10. Long suspend simulation — 60-second background
   322	
   323	**Status: conditional on M3 (event store) + M4 (sync watermarks).**
   324	This scenario is specified now and **scheduled to land in M10.5
   325	only if M3+M4 are complete by then.** If not, S10 graduates to
   326	M11.5 and the M10.5 gate excludes it explicitly.
   327	
   328	**The bug shape.** iOS suspends the app for 60 s (background). On
   329	resume, the kernel actor's main loop has paused; relay sockets
   330	have timed out; sync watermarks need to drive the catch-up. If the
   331	watermark logic is wrong, the app over-fetches (bandwidth waste) or
   332	under-fetches (missed events).
   333	
   334	**Setup.** Open kernel, drive 20 events/sec for 30 s to establish
   335	baseline watermarks. Inject a 60-s synthetic main-loop pause via
   336	`SIGSTOP` on the actor thread (Rust harness only; XCUITest cannot
   337	inject this). On resume:
   338	1. Verify watermark-driven REQ uses `since = last_event_at_ms`.
   339	2. Verify replay completes within **5 s** of resume.
   340	3. Verify state reconciles to the same snapshot a never-suspended

exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening/ci.md | sed -n '155,184p;303,336p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   155	### R.4 The gate script
   156	
   157	`scripts/stress-gate.py` is the source of truth for "is M10.5
   158	done?". Pseudocode:
   159	
   160	```python
   161	def gate(perf_dir, require_doctrines, require_debt_clean, require_grep_clean):
   162	    fails = []
   163	    for scenario in ["S1", "S2", "S3", "S5", "S6", "S7", "S8", "S9"]:
   164	        # S4 is XCUITest-only; S10 is conditional on M3+M4
   165	        m = load(f"{perf_dir}/{scenario}/metrics.json")
   166	        if not m["passed"]:
   167	            fails.append(f"{scenario}: {failed_gates(m)}")
   168	
   169	    if require_doctrines:
   170	        d = load(f"{perf_dir}/doctrine-review.md")
   171	        for code in require_doctrines:
   172	            if not has_passing_row(d, code):
   173	                fails.append(f"doctrine {code} not signed off")
   174	
   175	    if require_debt_clean:
   176	        debt = load(f"{perf_dir}/debt-inventory.md")
   177	        if has_open_must_fix(debt):
   178	            fails.append("debt-inventory open must-fix items")
   179	
   180	    if require_grep_clean:
   181	        if grep_ffi_surface_for_debt_tokens() != 0:
   182	            fails.append("FFI grep yielded TODO/FIXME tokens; see §7.1")
   183	
   184	    return (len(fails) == 0, fails)
   303	### C.5 What does not block CI
   304	
   305	- **Sonnet-agent runs** are advisory. Flake by design; failures are
   306	  triaged manually. Two-or-more agents hitting the same failure in
   307	  one nightly = upgraded to a tracked bug.
   308	- **iPhone 12 hardware-only scenarios** (S9 device variant, S4 device
   309	  variant) skip if the device is detached/offline; the missing
   310	  results are noted in the report and the gate script
   311	  treats "device-absent" as a deferred-not-failed state.
   312	- **S10 if M3+M4 are not complete:** scenario reports as `skipped:
   313	  prereq` with a note in `metrics.json`; gate script treats this
   314	  as PASS-with-deferral.
   315	
   316	---
   317	
   318	## §C.6 CI artifact retention
   319	
   320	| Tier | Retention | Notes |
   321	|---|---|---|
   322	| Pre-merge | 14 days | Per-PR; bulk delete |
   323	| Nightly | 90 days | Per-run; archived to S3 quarterly |
   324	| On-demand (RC) | indefinite | Release-attached artifact |
   325	
   326	Instruments traces are large (50–500 MiB per scenario). Pre-merge
   327	runs omit `--instruments` to stay within 15 min; only nightly + RC
   328	capture traces.
   329	
   330	---
   331	
   332	## §C.7 The M10.5 close protocol
   333	
   334	1. Latest nightly run = PASS on every scenario (or PASS-with-deferral
   335	   for S10).
   336	2. `docs/perf/m10.5/debt-inventory.md` must-fix list = empty.

codex
I found one clear doctrine mismatch candidate: the new hardening docs appear to redefine D5 as “snapshots bounded by what’s open,” while the session brief and existing rubric define D5 as “capabilities report, never decide.” I’m verifying that against the repo’s canonical docs before calling it.
exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening/gates.md | sed -n '150,235p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   150	## §D. Doctrine D0–D5 review checklist
   151	
   152	Cardinal doctrines per `docs/product-spec/overview-and-dx.md` §1.5.
   153	Each line item names the artifact that proves it.
   154	
   155	> **Note.** The task brief mentioned "D0–D5". The canonical list in
   156	> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
   157	> file follows that list. (The aim.md §6 list of 12 items is a
   158	> larger doctrine set the FFI hardening pass deliberately does
   159	> not redundantly re-prove — items beyond D0–D5 are covered by
   160	> earlier milestones' own gates.)
   161	
   162	### D0. Kernel never grows app nouns
   163	
   164	- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
   165	  no app-domain types in `nmp-core` substrate.
   166	- ✅ **Stress proof:** S6 (capability lifecycle storms) does
   167	  1 000 start/stop/restart cycles; the kernel's capability set is
   168	  unchanged across all cycles (no dynamic registration of new
   169	  noun types).
   170	- 📝 **Sign-off:** auditor signature line in
   171	  `docs/perf/m10.5/doctrine-review.md` § D0.
   172	
   173	### D1. Best-effort rendering — render now, refine in place
   174	
   175	- ✅ **Stress proof:** S3 (snapshot pressure) — every emit must
   176	  satisfy the placeholder-then-refine contract: missing display
   177	  names → shortened-npub placeholders are present (no `None`); when
   178	  kind:0 arrives, the next emit updates in place. Assertion: for
   179	  every author with placeholder text in emit N, emit M > N where
   180	  the kind:0 arrived must contain the resolved text and an
   181	  unchanged `id`.
   182	- ✅ **Stress proof (long path):** S10 (long suspend) — on resume
   183	  the catch-up rendering does not stall on missing profiles; the
   184	  timeline renders with placeholders immediately.
   185	- 📝 **Sign-off:** doctrine-review.md § D1.
   186	
   187	### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
   188	
   189	- ✅ **Stress proof:** S2 (dispatch flood), S3 (snapshot pressure),
   190	  S8 (planner DOS) all assert reconciler frequency stays bounded
   191	  (currently capped at 12 Hz; the doctrine says ≤ 60 Hz; the gate
   192	  is the 60 Hz ceiling, the configured cap is internal policy).
   193	- ✅ **Stress proof (working set):** S8 asserts planner state
   194	  returns to baseline ± 5 % after close storms.
   195	- ✅ **Composite reverse index:** S8 asserts wire-REQ dedup
   196	  (4× compaction floor); the reverse index drives the dedup.
   197	- 📝 **Sign-off:** doctrine-review.md § D2.
   198	
   199	### D3. Errors never cross FFI
   200	
   201	- ⚠️ **Current state:** debt-inventory §3 D3 audit concludes
   202	  errors-as-data crosses FFI correctly via `RelayStatus.last_error`
   203	  / `last_notice`. App-action error paths are
   204	  **silently no-op** (see parent doc §7.2).
   205	- ✅ **Stress proof + remediation:** S7 (error-shape exhaustion)
   206	  exercises every invalid-input path. The M10.5 deliverable
   207	  adds a `toast: Option<String>` field to the JSON update
   208	  payload and populates it from S7's failure paths. The
   209	  schema change is additive and non-breaking.
   210	- 📝 **Sign-off:** doctrine-review.md § D3, with the explicit
   211	  note that this milestone *closes* the D3-incomplete state
   212	  identified in the debt inventory.
   213	
   214	### D4. Single writer per fact — caches derive
   215	
   216	- ✅ **Proof:** debt-inventory §3 D4 audit — single-threaded actor
   217	  is the only writer; `KernelModel` on the iOS side is
   218	  `@MainActor`-isolated so derived state has a single writer per
   219	  layer.
   220	- ✅ **Stress proof:** S5 (reentrancy) — the reentrant
   221	  dispatch-from-callback path does not violate single-writer
   222	  because the dispatch enqueues a command for the actor; the
   223	  callback does not mutate kernel state directly.
   224	- ✅ **Stress proof:** S1 (mount/unmount) — refcount table is
   225	  only mutated by the actor; verified by harness asserting no
   226	  refcount-table mutation on any other thread (via
   227	  `loom`-style instrumentation in the harness binary only, not
   228	  prod).
   229	- 📝 **Sign-off:** doctrine-review.md § D4.
   230	
   231	### D5. Snapshots bounded by what's open
   232	
   233	- ✅ **Stress proof:** S1 (mount/unmount) — refcount drops to
   234	  zero ⇒ associated view payload evicted from snapshot.
   235	- ✅ **Stress proof:** S3 (snapshot pressure) — payload size

exec
/bin/zsh -lc "rg -n \"D0|D1|D2|D3|D4|D5|capabilities report|never decide|snapshots bounded|bounded by what's open|Capabilities\" docs/product-spec* docs/aim.md docs/plan.md docs/design/ffi-hardening.md docs/design/ffi-hardening/gates.md docs/perf/m10.5/debt-inventory.md" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
docs/plan.md:9:> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
docs/plan.md:20:- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
docs/plan.md:65:4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
docs/plan.md:66:5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
docs/plan.md:100:- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
docs/plan.md:271:**Scope.** Per spec doctrine D4 (single writer per fact) extended to account scope:
docs/plan.md:335:- Profile picture update through compose → kind:0 republish with new Blossom URL → in-place refinement across all open Profile / Timeline payloads (per doctrine D1).
docs/plan.md:356:  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
docs/plan.md:380:- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
docs/design/ffi-hardening.md:19:   (`docs/product-spec/overview-and-dx.md` §1.5 D0–D5), and every ownership
docs/design/ffi-hardening.md:66:| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
docs/design/ffi-hardening.md:135:| S1 | Mount/unmount churn | actor recv + refcount | D5 (snapshot bounded), bible #5 |
docs/design/ffi-hardening.md:137:| S3 | Snapshot pressure | listener serialization | bible #9 (≤60 Hz), D5 |
docs/design/ffi-hardening.md:138:| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
docs/design/ffi-hardening.md:141:| S7 | Error-shape exhaustion | every invalid input path | D3 (no errors cross FFI) |
docs/design/ffi-hardening.md:142:| S8 | Subscription planner DOS | OpenView/CloseView storm | D2 (≤60 Hz/view), D5 |
docs/design/ffi-hardening.md:143:| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
docs/design/ffi-hardening.md:197:├── doctrine-review.md       # D0–D5 sign-off (M10.5 exit-gate artifact)
docs/design/ffi-hardening.md:217:| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
docs/design/ffi-hardening.md:235:### 7.2 D3 structural gap (named, not hidden)
docs/design/ffi-hardening.md:241:caller and without setting any state field. This is **D3-compliant in
docs/design/ffi-hardening.md:243:D3-incomplete in the user-visible sense** (no toast surfaces in
docs/design/ffi-hardening.md:246:The debt-inventory's D3 audit (lines 317–334) concludes the same:
docs/design/ffi-hardening.md:263:Full D0–D5 line-item-to-scenario mapping in
docs/design/ffi-hardening.md:268:| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
docs/design/ffi-hardening.md:269:| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
docs/design/ffi-hardening.md:270:| **D2** ≤60Hz/view, working-set bound | S2, S3, S8 — emit-rate cap, planner dedup |
docs/design/ffi-hardening.md:271:| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
docs/design/ffi-hardening.md:272:| **D4** one writer per fact | S1, S5 — refcount only mutated on actor thread; reentrancy under same single-writer rule |
docs/design/ffi-hardening.md:273:| **D5** snapshots bounded by what's open | S1 (refcount drives eviction) + S3 (full-state size scales with open views, not store) |
docs/aim.md:54:6. **Capability bridge pattern.** When Rust needs an OS API (keychain, push, location, external signer app), it requests the capability via a typed callback interface. Native executes and reports raw data. Rust decides policy. Native never decides "should we retry?" or "is this recoverable?"
docs/aim.md:233:11. **Capabilities, not callbacks.** Native↔Rust interactions go through bounded, idempotent capability bridges modeled exactly on the RMP bible's pattern.
docs/perf/m10.5/debt-inventory.md:272:### D0 Audit: Kernel Never Grows App Nouns
docs/perf/m10.5/debt-inventory.md:286:### D1 Audit: Best-Effort Rendering with Placeholders
docs/perf/m10.5/debt-inventory.md:299:### D2 Audit: Reactivity Contract (Composite Reverse Index)
docs/perf/m10.5/debt-inventory.md:313:### D3 Audit: Errors Never Cross FFI
docs/perf/m10.5/debt-inventory.md:327:### D4 Audit: One Writer Per Fact
docs/perf/m10.5/debt-inventory.md:341:### D5 Audit: Capabilities Report, Never Decide
docs/perf/m10.5/debt-inventory.md:361:| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
docs/perf/m10.5/debt-inventory.md:362:| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
docs/perf/m10.5/debt-inventory.md:396:The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
docs/design/ffi-hardening/gates.md:7:2. **§D** doctrine D0–D5 review checklist — each line item maps to
docs/design/ffi-hardening/gates.md:150:## §D. Doctrine D0–D5 review checklist
docs/design/ffi-hardening/gates.md:155:> **Note.** The task brief mentioned "D0–D5". The canonical list in
docs/design/ffi-hardening/gates.md:156:> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
docs/design/ffi-hardening/gates.md:159:> not redundantly re-prove — items beyond D0–D5 are covered by
docs/design/ffi-hardening/gates.md:162:### D0. Kernel never grows app nouns
docs/design/ffi-hardening/gates.md:164:- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
docs/design/ffi-hardening/gates.md:171:  `docs/perf/m10.5/doctrine-review.md` § D0.
docs/design/ffi-hardening/gates.md:173:### D1. Best-effort rendering — render now, refine in place
docs/design/ffi-hardening/gates.md:185:- 📝 **Sign-off:** doctrine-review.md § D1.
docs/design/ffi-hardening/gates.md:187:### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
docs/design/ffi-hardening/gates.md:197:- 📝 **Sign-off:** doctrine-review.md § D2.
docs/design/ffi-hardening/gates.md:199:### D3. Errors never cross FFI
docs/design/ffi-hardening/gates.md:201:- ⚠️ **Current state:** debt-inventory §3 D3 audit concludes
docs/design/ffi-hardening/gates.md:210:- 📝 **Sign-off:** doctrine-review.md § D3, with the explicit
docs/design/ffi-hardening/gates.md:211:  note that this milestone *closes* the D3-incomplete state
docs/design/ffi-hardening/gates.md:214:### D4. Single writer per fact — caches derive
docs/design/ffi-hardening/gates.md:216:- ✅ **Proof:** debt-inventory §3 D4 audit — single-threaded actor
docs/design/ffi-hardening/gates.md:229:- 📝 **Sign-off:** doctrine-review.md § D4.
docs/design/ffi-hardening/gates.md:231:### D5. Snapshots bounded by what's open
docs/design/ffi-hardening/gates.md:241:- 📝 **Sign-off:** doctrine-review.md § D5.
docs/design/ffi-hardening/gates.md:255:| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
docs/design/ffi-hardening/gates.md:256:| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
docs/design/ffi-hardening/gates.md:257:| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
docs/design/ffi-hardening/gates.md:258:| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
docs/design/ffi-hardening/gates.md:259:| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
docs/design/ffi-hardening/gates.md:260:| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
docs/product-spec/appendices.md:11:**`AppState` is bounded by what's open.** It does not contain the event store, the gossip cache, the working set, or anything proportional to the local cache size. It contains:
docs/product-spec/cli-toolchain-phasing.md:181:- **Best-effort rendering.** Doctrine D1: render what's available, refine in place; never withhold cached data; never block on fetches.
docs/product-spec/overview-and-dx.md:31:### D0. Kernel + extension modules — no app nouns in `nmp-core`
docs/product-spec/overview-and-dx.md:41:### D1. Best-effort rendering — render now, refine in place
docs/product-spec/overview-and-dx.md:54:### D2. Negentropy first, REQ second
docs/product-spec/overview-and-dx.md:60:### D3. Outbox routing is automatic; manual relay selection is the opt-out
docs/product-spec/overview-and-dx.md:73:### D4. Single writer per fact; caches derive
docs/product-spec/overview-and-dx.md:77:### D5. Snapshots bounded by what's open
docs/product-spec/api-surface.md:192:### 6.5 Capabilities
docs/product-spec/api-surface.md:229:Each capability is **idempotent** (`start` after `start` is a no-op) and **bounded** (the trait surface is minimal; no native code decides policy). Capabilities can be added in additional minor versions; doing so does not break existing apps because all setters are optional.
docs/product-spec/subsystems.md:74:Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
docs/product-spec/subsystems.md:181:**Best-effort field contract (per doctrine D1).** Every display-bearing field in every view payload is **non-optional** and has a defined placeholder when the underlying data is missing:
docs/product-spec/subsystems.md:242:Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.

exec
/bin/zsh -lc "nl -ba docs/plan.md | sed -n '1,12p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# Build & Validation Plan
     2	
     3	> Companion to `docs/product-spec.md` (what we ship) and the design docs in `docs/design/` (how each subsystem works). This document defines **the single ladder of milestones**, each one a runnable product that proves a specific architectural claim with real (not modeled) evidence.
     4	
     5	> **Four arcs:** Kernel substrate + Nostr social stack (M0–M10) → FFI hardening + iOS empirical proof (M10.5) → kernel-boundary proof with a non-social-domain app (M11, the **`../podcast` rebuild on NMP**) → wallet/WoT + cross-platform + release (M12–M17).
     6	
     7	> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.
     8	
     9	> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
    10	
    11	---
    12	

exec
/bin/zsh -lc "ls docs | sed -n '1,80p' && find docs -maxdepth 2 -name '*product*' -o -name '*overview*'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
aim.md
decisions
design
perf
plan.md
product-spec
product-spec.md
docs/product-spec.md
docs/product-spec
docs/product-spec/overview-and-dx.md

exec
/bin/zsh -lc "nl -ba docs/product-spec.md | sed -n '1,90p'" in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc "nl -ba docs/product-spec/overview-and-dx.md | sed -n '25,85p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# Product Specification - Nostr Multi-Platform Framework
     2	
     3	This document is split into focused sections to keep each hand-authored file under the repository LOC ceiling.
     4	
     5	- [Product Spec: Overview And Developer Experience](product-spec/overview-and-dx.md)
     6	- [Product Spec: API Surface](product-spec/api-surface.md)
     7	- [Product Spec: Subsystems](product-spec/subsystems.md)
     8	- [Product Spec: CLI, Toolchain, Phasing](product-spec/cli-toolchain-phasing.md)
     9	- [Product Spec: Appendices](product-spec/appendices.md)
    10	
    11	## Section Map
    12	
    13	- Sections 1-5: [overview and developer experience](product-spec/overview-and-dx.md)
    14	- Section 6: [API surface](product-spec/api-surface.md)
    15	- Section 7: [subsystems](product-spec/subsystems.md)
    16	- Sections 8-14: [CLI, toolchain, phasing, open questions, glossary](product-spec/cli-toolchain-phasing.md)
    17	- Appendices: [FFI details and NIP glossary](product-spec/appendices.md)

 succeeded in 0ms:
    25	---
    26	
    27	## 1.5 Cardinal doctrines
    28	
    29	Six named principles that subsume the rest of this spec. Every API decision answers to at least one of these; conflicts between them resolve in the order listed.
    30	
    31	### D0. Kernel + extension modules — no app nouns in `nmp-core`
    32	
    33	Per ADR-0009, NMP is a Nostr-native app kernel plus extension modules. The kernel provides substrate; protocol modules and app modules contribute typed variants via `ViewModule`, `ActionModule`, `DomainModule`, `CapabilityModule`, and `IdentityModule`. If implementing a real app requires adding domain nouns to `nmp-core`, the kernel boundary is wrong and must change.
    34	
    35	This rules out:
    36	
    37	- `nmp-core` becoming a junk drawer of every consumer's domain concepts.
    38	- App-specific business logic in Swift, Kotlin, or TypeScript shells.
    39	- Closed FFI enums that prevent modules from contributing typed views, actions, updates, capabilities, or identity scopes.
    40	
    41	### D1. Best-effort rendering — render now, refine in place
    42	
    43	Apps built with this framework **never withhold cached data and never block on fetches**. Every view payload field carries a value, not a "loading" status. Missing display names default to a shortened npub; missing pictures default to a deterministic identicon URI; missing timestamps default to "now". When a more authoritative value (e.g., the author's kind:0) arrives later, the view payload updates in place and the affected cell re-renders. The UI never sees a spinner gating already-renderable content.
    44	
    45	The doctrine is enforced by the view payload **types**: display fields are non-`Option`, placeholders are part of the type contract, and freshness is exposed (when relevant) as an optional badge hint, not a render gate. There is no `if has_profile { render } else { spinner }` pattern available in the API — the framework does not provide one.
    46	
    47	This rules out, by construction, the most common Nostr-client failure modes:
    48	
    49	- Hiding a post because the author's profile hasn't loaded yet.
    50	- Replacing cached profile metadata with a spinner because "we might have something newer."
    51	- Refusing to render threads because the root event isn't in cache.
    52	- Profile-picture flicker between cached and placeholder.
    53	
    54	### D2. Negentropy first, REQ second
    55	
    56	NIP-77 negentropy reconciliation is the default backfill mechanism. Every `(filter, relay)` pair the app touches is treated as a tracked sync target with a watermark. Live REQ remains the tailing path, but historical gaps consult coverage first and prefer sync over REQ scans when relays support it.
    57	
    58	This is not a product feature you opt into later; it is a subscription policy built on explicit coverage metadata. See §7.8.
    59	
    60	### D3. Outbox routing is automatic; manual relay selection is the opt-out
    61	
    62	Per NIP-65, reads and writes are routed to the relevant relays by framework policy without normal app code specifying them. Subscriptions with `authors` filters route to those authors' write relays; publishes go to the author's write relays plus tagged recipients' inbox relays; discovery falls back to a configurable indexer set.
    63	
    64	The safe public path does not ask the developer to pick relays per operation. Explicit override and diagnostic/test paths exist, but they are named, observable, and excluded from the default app-building flow.
    65	
    66	This rules out, by construction:
    67	
    68	- Posts to relays the author hasn't declared as write relays.
    69	- DMs leaked to public relays.
    70	- Silent reads against a default relay set that miss an author's actual relays; unknown relay lists surface as coverage/diagnostic state and use a bounded fallback policy.
    71	- Hand-rolled fan-out logic in app code.
    72	
    73	### D4. Single writer per fact; caches derive
    74	
    75	The "single source of truth" doctrine does not mean one cache — there are five layers (durable event store, in-memory working set, view payloads, gossip cache, platform reactive shadow). It means **one writer per fact**, and every downstream cache derives from the writer mechanically. Cache invalidation is not a concept in the public API. Recomputation happens in the actor; the platform receives new derived state.
    76	
    77	### D5. Snapshots bounded by what's open
    78	
    79	What crosses FFI is the projection through currently-open views, not the underlying event store. `AppState` carries small screen-shaped data plus a map of `ViewId → ViewPayload` for views currently in use. Closing a view evicts its payload from the snapshot. The event store itself never crosses FFI. See §6.2 and the FFI architecture appendix (§A1).
    80	
    81	---
    82	
    83	## 2. Audience and use cases
    84	
    85	**Primary audience.** Application developers building Nostr clients for production distribution on iOS, Android, desktop, and web — including LLM-driven and inexperienced developers who lack the protocol literacy to navigate Nostr's footguns unaided.

exec
/bin/zsh -lc "nl -ba docs/perf/m10.5/debt-inventory.md | sed -n '327,350p;396,407p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   327	### D4 Audit: One Writer Per Fact
   328	
   329	**Status:** ✅ **COMPLIANT**
   330	
   331	**Evidence:**
   332	- The kernel actor is single-threaded (runs on one OS thread spawned in `ffi.rs:51`).
   333	- All state mutations go through `run_actor()` in `actor.rs`, which receives commands via a single `mpsc::channel()`.
   334	- Relay worker threads (`relay_worker.rs`) only send *events* back to the actor; they never mutate kernel state directly.
   335	- The iOS bridge is @MainActor-annotated (`KernelModel`), ensuring all SwiftUI state updates are serialized.
   336	
   337	**Violation?** No. Single-writer-per-subsystem is enforced architecturally.
   338	
   339	---
   340	
   341	### D5 Audit: Capabilities Report, Never Decide
   342	
   343	**Status:** ✅ **COMPLIANT**
   344	
   345	**Evidence:**
   346	- The iOS FFI bridge exposes capability requests as simple commands (`openAuthor()`, `claimProfile()`, etc.) with no decision-making logic at the boundary.
   347	- The kernel processes these as `ActorCommand` variants, routing them to domain logic.
   348	- No policy decisions are made in `KernelBridge.swift` or the iOS-side FFI wrapper; all routing and business logic lives in the kernel.
   349	- The `CapabilityModule` trait (in `substrate/capability.rs`) defines how modules *report* capabilities, not how the bridge decides what to expose.
   350	
   396	The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
   397	
   398	**This audit is necessary but not sufficient for M10.5 exit.** M10.5 exit requires (per `docs/plan.md` §M10.5):
   399	- Stress harness scenarios passing on simulator and iPhone 12
   400	- All M1–M10 perf reports re-run with no > 5 % regression
   401	- Instruments-recorded zero leaks over 10-min canonical workflow
   402	- UI-scripted scenarios passing (Sonnet-agent + XCUITest)
   403	- `docs/ffi-surface.md` reviewed and tagged
   404	- Doctrine review signed in writing in `docs/perf/m10.5/doctrine-review.md`
   405	
   406	**Audit status:** ✅ static-debt baseline established. The 4 must-fix items above are tracked.
   407	**M10.5 exit status:** ❌ pending empirical proof + must-fix cleanup.

exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening/gates.md | sed -n '231,241p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   231	### D5. Snapshots bounded by what's open
   232	
   233	- ✅ **Stress proof:** S1 (mount/unmount) — refcount drops to
   234	  zero ⇒ associated view payload evicted from snapshot.
   235	- ✅ **Stress proof:** S3 (snapshot pressure) — payload size
   236	  scales with `open_view_count`, not with `stored_events`
   237	  count (100 k events ⇒ payload < 2 MiB because only views are
   238	  open, not the full store).
   239	- ✅ **Stress proof:** S8 (planner DOS) — peak RSS bounded by
   240	  open-view count even under 10 k concurrent OpenViews.
   241	- 📝 **Sign-off:** doctrine-review.md § D5.

exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening/harness.md | sed -n '155,178p'" in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc "nl -ba docs/design/ffi-hardening/ci.md | sed -n '10,42p;52,92p;190,225p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   155	flap test (we're testing reconnection logic, not protocol fidelity).
   156	
   157	### 1.6 Linking
   158	
   159	`nmp-core` already compiles as `cdylib + staticlib + rlib`. The
   160	harness binary depends on it as an rlib through a new
   161	`nmp-core-ffi-decls` crate that re-exports the `extern "C"` symbols
   162	as Rust function declarations:
   163	
   164	```rust
   165	// crates/nmp-core-ffi-decls/src/lib.rs
   166	extern "C" {
   167	    pub fn nmp_app_new() -> *mut std::ffi::c_void;
   168	    pub fn nmp_app_free(app: *mut std::ffi::c_void);
   169	    pub fn nmp_app_set_update_callback(
   170	        app: *mut std::ffi::c_void,
   171	        context: *mut std::ffi::c_void,
   172	        callback: Option<extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_char)>,
   173	    );
   174	    // ... (rest of the 14 declarations)
   175	}
   176	```
   177	
   178	(Alternative: use `nmp-core` directly as a crate dep and avoid the

 succeeded in 0ms:
    10	
    11	### R.1 `justfile` recipes
    12	
    13	Append to `/Users/pablofernandez/Work/nostr-multi-platform/justfile`:
    14	
    15	```make
    16	# === FFI hardening (M10.5) ===
    17	
    18	# Pre-merge fast suite: simulator only, ~10 min wall-time
    19	stress:
    20	    cargo build --release -p nmp-testing --bin ffi-stress
    21	    just stress-rust-fast
    22	    just stress-xcui-fast
    23	
    24	stress-rust-fast:
    25	    cargo run --release -p nmp-testing --bin ffi-stress -- \
    26	        mount-unmount --duration 60s --fail-on-gate
    27	    cargo run --release -p nmp-testing --bin ffi-stress -- \
    28	        dispatch-flood --duration 30s --threads 4 --fail-on-gate
    29	    cargo run --release -p nmp-testing --bin ffi-stress -- \
    30	        snapshot-pressure --duration 30s --fail-on-gate
    31	    cargo run --release -p nmp-testing --bin ffi-stress -- \
    32	        reentrancy --duration 30s --fail-on-gate
    33	    cargo run --release -p nmp-testing --bin ffi-stress -- \
    34	        error-exhaustion --fail-on-gate
    35	    cargo run --release -p nmp-testing --bin ffi-stress -- \
    36	        planner-dos --duration 60s --fail-on-gate
    37	
    38	stress-xcui-fast: build-ios
    39	    xcodebuild test \
    40	        -project ios/NmpStress/NmpStress.xcodeproj \
    41	        -scheme StressUITests-Fast \
    42	        -destination 'platform=iOS Simulator,name=iPhone 16 Pro,OS=latest' \
    52	stress-rust-full:
    53	    cargo run --release -p nmp-testing --bin ffi-stress -- \
    54	        all --duration 10m --instruments --fail-on-gate
    55	
    56	stress-xcui-full: build-ios
    57	    xcodebuild test \
    58	        -project ios/NmpStress/NmpStress.xcodeproj \
    59	        -scheme StressUITests-Full \
    60	        -destination 'platform=iOS Simulator,name=iPhone 16 Pro,OS=latest' \
    61	        -derivedDataPath ios/DerivedData \
    62	        -resultBundlePath docs/perf/m10.5/xcui-full.xcresult
    63	
    64	stress-device:
    65	    cargo build --release -p nmp-core --target aarch64-apple-ios
    66	    xcodebuild test \
    67	        -project ios/NmpStress/NmpStress.xcodeproj \
    68	        -scheme StressUITests-Full \
    69	        -destination 'platform=iOS,name=iPhone 12' \
    70	        -derivedDataPath ios/DerivedData \
    71	        -resultBundlePath docs/perf/m10.5/xcui-device.xcresult
    72	
    73	stress-sonnet:
    74	    just build-ios
    75	    crates/nmp-testing/bin/ffi-stress/sonnet-runner.sh \
    76	        default 4 5
    77	
    78	# Aggregate all scenario JSON into one markdown summary
    79	stress-report:
    80	    python3 scripts/stress-aggregate.py docs/perf/m10.5/ \
    81	        > docs/perf/m10.5/m10.5-summary.md
    82	
    83	# The doctrine-review.md gate: read every scenario report, assert PASS
    84	stress-gate:
    85	    python3 scripts/stress-gate.py docs/perf/m10.5/ \
    86	        --require-doctrines D0,D1,D2,D3,D4,D5 \
    87	        --require-debt-inventory-clean \
    88	        --require-grep-clean
    89	```
    90	
    91	### R.2 Output bundle layout
    92	
   190	---
   191	
   192	## §C. CI integration
   193	
   194	### C.1 Pre-merge tier (every PR)
   195	
   196	**Runner.** GitHub Actions `macos-14` (Apple Silicon, ~10 min budget).
   197	
   198	**Scenarios.** S1 (short — 60 s), S2 (30 s), S3 (30 s), S5 (30 s),
   199	S7 (full matrix), S8 (60 s). **Not S4** (iOS-main-thread, slow XCUITest
   200	boot) — runs nightly instead. **Not S6** (5 min) — runs nightly.
   201	**Not S9** (10 min) — nightly. **Not S10** (conditional).
   202	
   203	**Workflow.** `.github/workflows/stress-pre-merge.yml`:
   204	
   205	```yaml
   206	name: FFI stress (pre-merge)
   207	on:
   208	  pull_request:
   209	    paths:
   210	      - 'crates/nmp-core/**'
   211	      - 'crates/nmp-testing/**'
   212	      - 'ios/NmpStress/**'
   213	
   214	jobs:
   215	  stress-fast:
   216	    runs-on: macos-14
   217	    timeout-minutes: 15
   218	    steps:
   219	      - uses: actions/checkout@v4
   220	      - uses: dtolnay/rust-toolchain@stable
   221	      - run: just stress
   222	      - run: just stress-report
   223	      - run: just stress-gate
   224	      - uses: actions/upload-artifact@v4
   225	        if: always()

exec
/bin/zsh -lc 'rg -n "stress:|stress-rust-fast|stress-gate|for scenario|S6|S9|just stress" docs/design/ffi-hardening/ci.md' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
4:1. **§R** local run protocol — `just stress` recipes, output bundle layout.
19:stress:
21:    just stress-rust-fast
22:    just stress-xcui-fast
24:stress-rust-fast:
48:    just stress-rust-full
49:    just stress-xcui-full
50:    just stress-device
84:stress-gate:
85:    python3 scripts/stress-gate.py docs/perf/m10.5/ \
157:`scripts/stress-gate.py` is the source of truth for "is M10.5
163:    for scenario in ["S1", "S2", "S3", "S5", "S6", "S7", "S8", "S9"]:
200:boot) — runs nightly instead. **Not S6** (5 min) — runs nightly.
201:**Not S9** (10 min) — nightly. **Not S10** (conditional).
221:      - run: just stress
222:      - run: just stress-report
223:      - run: just stress-gate
231:**Gating.** The `just stress-gate` step exit code is the PR gate.
240:S6 (1 000 cycles), S7 (full matrix), S8 (5 storms), S9 (10 min ×
259:      - run: just stress-full
260:      - run: just stress-sonnet
261:      - run: just stress-report
262:      - run: just stress-gate || echo "::warning::nightly gate failed"
282:- S9: 24-hour relay flap.
308:- **iPhone 12 hardware-only scenarios** (S9 device variant, S4 device

exec
/bin/zsh -lc "nl -ba docs/perf/m10.5/debt-inventory.md | sed -n '360,407p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   360	| 2 expect() calls in kernel/status.rs (F15, F16) | Justified | Invariant maintained by construction (relay HashMap initialized for all roles in `Kernel::new()`). |
   361	| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
   362	| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
   363	
   364	---
   365	
   366	## 5. Must-Fix Items for M10.5 (mandatory)
   367	
   368	Per the M10.5 zero-shortcut posture (no "for later", no "ADR escape" for files in scope), the following are **required cleanup** before the M10.5 empirical exit gate:
   369	
   370	1. **Add SAFETY comments to ffi.rs unsafe blocks** (F1, F2, F3) — `crates/nmp-core/src/ffi.rs` lines 75, 275, 284. The unsafe code is correct; missing safety comments are themselves auditability debt and violate the M10.5 zero-debt posture for the FFI surface. Suggested text in the findings section above.
   371	2. **Add clarifying comment to relay_worker.rs#242** (F4) — explain why `#[allow(unreachable_patterns)]` is defensive against future tungstenite enum evolution.
   372	
   373	These must land in a single PR titled `m10.5(ffi): add SAFETY comments + clarify defensive pattern`. Tracked as a TaskList task.
   374	
   375	## 6. Metrics
   376	
   377	| Category | Count | Status |
   378	|----------|-------|--------|
   379	| **Code Quality** | | |
   380	| Rust files audited | 19 | ✓ |
   381	| Swift files audited | 9 | ✓ |
   382	| Total LOC scanned | 6,559 | ✓ |
   383	| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
   384	| Code bugs found | 0 | ✓ No bugs |
   385	| Unsafe blocks without safety comments | 3 | ⚠️ **Must-fix for M10.5** |
   386	| Integer casts | 14 | ✓ Justified |
   387	| Cardinal doctrine violations | 0 | ✓ Compliant |
   388	| **Post-Audit Actions** | | |
   389	| Critical bugs | 0 | ✓ |
   390	| Required code changes (safety comments) | 4 | **Must-fix for M10.5** |
   391	
   392	---
   393	
   394	## 7. Conclusion
   395	
   396	The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
   397	
   398	**This audit is necessary but not sufficient for M10.5 exit.** M10.5 exit requires (per `docs/plan.md` §M10.5):
   399	- Stress harness scenarios passing on simulator and iPhone 12
   400	- All M1–M10 perf reports re-run with no > 5 % regression
   401	- Instruments-recorded zero leaks over 10-min canonical workflow
   402	- UI-scripted scenarios passing (Sonnet-agent + XCUITest)
   403	- `docs/ffi-surface.md` reviewed and tagged
   404	- Doctrine review signed in writing in `docs/perf/m10.5/doctrine-review.md`
   405	
   406	**Audit status:** ✅ static-debt baseline established. The 4 must-fix items above are tracked.
   407	**M10.5 exit status:** ❌ pending empirical proof + must-fix cleanup.

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/kernel/update.rs | sed -n '1,120p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	use super::*;
     2	
     3	impl Kernel {
     4	    pub(crate) fn make_update(&mut self, running: bool) -> String {
     5	        let emit_started = Instant::now();
     6	        self.rev = self.rev.saturating_add(1);
     7	        self.update_sequence = self.update_sequence.saturating_add(1);
     8	
     9	        let batch_events = self.events_since_last_update;
    10	        self.max_events_per_update = self.max_events_per_update.max(batch_events);
    11	        let last_event_to_emit_ms = self
    12	            .last_event_at
    13	            .map(|last_event_at| emit_started.duration_since(last_event_at).as_millis());
    14	        if let Some(value) = last_event_to_emit_ms {
    15	            self.max_event_to_emit_ms = self.max_event_to_emit_ms.max(value);
    16	        }
    17	
    18	        let items = self.visible_items();
    19	        let (inserted, updated, removed) = diff_items(&self.last_emitted_items, &items);
    20	        self.last_emitted_items = items.clone();
    21	
    22	        let visible_profiled_items = items
    23	            .iter()
    24	            .filter(|item| item.author_avatar_source == "kind0")
    25	            .count();
    26	        let visible_placeholder_avatar_items = items.len().saturating_sub(visible_profiled_items);
    27	        let counters = self.total_counters();
    28	        let mut update = KernelUpdate {
    29	            rev: self.rev,
    30	            update_kind: "ViewBatch",
    31	            running,
    32	            relay_url: CONTENT_RELAY_URL,
    33	            test_npub: TEST_NPUB,
    34	            profile: self.profile_card(),
    35	            items,
    36	            author_view: self.author_view(),
    37	            thread_view: self.thread_view(),
    38	            inserted: inserted.clone(),
    39	            updated: updated.clone(),
    40	            removed: removed.clone(),
    41	            metrics: Metrics {
    42	                generated_events: counters.events_rx,
    43	                note_events: self.events.values().filter(|event| event.kind == 1).count() as u64,
    44	                profile_events: self.profiles.len() as u64,
    45	                duplicate_events: self
    46	                    .events
    47	                    .values()
    48	                    .filter(|event| event.relay_count > 1)
    49	                    .count() as u64,
    50	                delete_events: 0,
    51	                stored_events: self.events.len() + self.profiles.len() + self.seed_contacts.len(),
    52	                tombstones: 0,
    53	                visible_items: self.last_emitted_items.len(),
    54	                visible_profiled_items,
    55	                visible_placeholder_avatar_items,
    56	                open_views: self.logical_interests().len() as u32,
    57	                events_since_last_update: self.events_since_last_update,
    58	                diagnostic_firehose_events: self.diagnostic_firehose_events,
    59	                inserted_count: inserted.len(),
    60	                updated_count: updated.len(),
    61	                removed_count: removed.len(),
    62	                events_per_second_configured: 0,
    63	                emit_hz_configured: DEFAULT_EMIT_HZ,
    64	                update_sequence: self.update_sequence,
    65	                estimated_store_bytes: self.estimated_store_bytes(),
    66	                payload_bytes: 0,
    67	                store_to_payload_ratio: 0.0,
    68	                actor_queue_depth: 0,
    69	                frames_rx: counters.frames_rx,
    70	                events_rx: counters.events_rx,
    71	                eose_rx: counters.eose_rx,
    72	                notices_rx: counters.notices_rx,
    73	                closed_rx: counters.closed_rx,
    74	                bytes_rx: counters.bytes_rx,
    75	                bytes_tx: counters.bytes_tx,
    76	                contacts_authors: self.seed_contacts.values().map(Vec::len).sum(),
    77	                timeline_authors: self.timeline_authors.len(),
    78	                first_event_ms: self.elapsed_ms(self.first_event_at),
    79	                target_profile_loaded_ms: self.elapsed_ms(self.target_profile_loaded_at),
    80	                timeline_opened_ms: self.elapsed_ms(self.timeline_opened_at),
    81	                timeline_first_item_ms: self.elapsed_ms(self.timeline_first_item_at),
    82	                update_emitted_ms: self.elapsed_ms(Some(emit_started)),
    83	                last_event_to_emit_ms,
    84	                max_event_to_emit_ms: self.max_event_to_emit_ms,
    85	                max_events_per_update: self.max_events_per_update,
    86	            },
    87	            relay_status: self.relay_status(),
    88	            relay_statuses: self.relay_statuses(),
    89	            logical_interests: self.logical_interests(),
    90	            wire_subscriptions: self.wire_subscriptions(),
    91	            logs: self.logs.iter().cloned().collect(),
    92	        };
    93	
    94	        let first = serde_json::to_string(&update).unwrap_or_else(|_| "{}".to_string());
    95	        update.metrics.payload_bytes = first.len();
    96	        update.metrics.store_to_payload_ratio = ratio(
    97	            update.metrics.estimated_store_bytes,
    98	            update.metrics.payload_bytes,
    99	        );
   100	        if batch_events > 0 || !inserted.is_empty() || !updated.is_empty() || !removed.is_empty() {
   101	            self.log(format!(
   102	                "NMP_PERF rust_update rev={} batch_events={} inserted={} updated={} removed={} visible={} payload_bytes={} event_to_emit_ms={} max_event_to_emit_ms={}",
   103	                self.rev,
   104	                batch_events,
   105	                inserted.len(),
   106	                updated.len(),
   107	                removed.len(),
   108	                self.last_emitted_items.len(),
   109	                update.metrics.payload_bytes,
   110	                last_event_to_emit_ms
   111	                    .map(|value| value.to_string())
   112	                    .unwrap_or_else(|| "none".to_string()),
   113	                self.max_event_to_emit_ms
   114	            ));
   115	        }
   116	        self.events_since_last_update = 0;
   117	        self.changed_since_emit = false;
   118	        serde_json::to_string(&update).unwrap_or(first)
   119	    }
   120	

exec
/bin/zsh -lc 'rg -n "struct AppUpdate|struct KernelUpdate|AppUpdate|KernelUpdate|logs:|metrics:" crates/nmp-core/src ios/NmpStress/NmpStress' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
ios/NmpStress/NmpStress/KernelBridge.swift:5:    private var updateSink: KernelUpdateSink?
ios/NmpStress/NmpStress/KernelBridge.swift:16:    func listen(_ handler: @escaping (KernelUpdateResult) -> Void) {
ios/NmpStress/NmpStress/KernelBridge.swift:17:        let sink = KernelUpdateSink(handler: handler)
ios/NmpStress/NmpStress/KernelBridge.swift:74:    fileprivate static func decode(pointer: UnsafePointer<CChar>) -> KernelUpdateResult? {
ios/NmpStress/NmpStress/KernelBridge.swift:80:        guard let update = try? decoder.decode(KernelUpdate.self, from: data) else {
ios/NmpStress/NmpStress/KernelBridge.swift:84:        return KernelUpdateResult(
ios/NmpStress/NmpStress/KernelBridge.swift:93:private final class KernelUpdateSink {
ios/NmpStress/NmpStress/KernelBridge.swift:94:    let handler: (KernelUpdateResult) -> Void
ios/NmpStress/NmpStress/KernelBridge.swift:96:    init(handler: @escaping (KernelUpdateResult) -> Void) {
ios/NmpStress/NmpStress/KernelBridge.swift:108:    let sink = Unmanaged<KernelUpdateSink>.fromOpaque(context).takeUnretainedValue()
ios/NmpStress/NmpStress/KernelBridge.swift:112:struct KernelUpdateResult {
ios/NmpStress/NmpStress/KernelBridge.swift:113:    let update: KernelUpdate
ios/NmpStress/NmpStress/KernelBridge.swift:119:struct KernelUpdate: Decodable {
ios/NmpStress/NmpStress/KernelBridge.swift:132:    let metrics: KernelMetrics
ios/NmpStress/NmpStress/KernelBridge.swift:137:    let logs: [String]
crates/nmp-core/src/kernel/mod.rs:201:struct KernelUpdate {
crates/nmp-core/src/kernel/mod.rs:214:    metrics: Metrics,
crates/nmp-core/src/kernel/mod.rs:219:    logs: Vec<String>,
crates/nmp-core/src/kernel/mod.rs:328:    logs: VecDeque<String>,
crates/nmp-core/src/kernel/mod.rs:381:            logs: VecDeque::new(),
crates/nmp-core/src/lib.rs:9:pub use app::{AppState, KernelAction, KernelUpdate, KernelViewSpec};
crates/nmp-core/src/kernel/update.rs:28:        let mut update = KernelUpdate {
crates/nmp-core/src/kernel/update.rs:41:            metrics: Metrics {
crates/nmp-core/src/kernel/update.rs:91:            logs: self.logs.iter().cloned().collect(),
ios/NmpStress/NmpStress/ContentView.swift:127:    private var metrics: some View {
crates/nmp-core/src/app.rs:13:pub enum KernelUpdate {
ios/NmpStress/NmpStress/KernelModel.swift:14:    @Published private(set) var metrics: KernelMetrics?
ios/NmpStress/NmpStress/KernelModel.swift:19:    @Published private(set) var logs: [String] = []
ios/NmpStress/NmpStress/KernelModel.swift:138:    private func apply(result: KernelUpdateResult) {

exec
/bin/zsh -lc "rg -n \"ActorCommand|enum ActorCommand|OpenAuthor|OpenThread|ClaimProfile|ReleaseProfile|CloseAuthor|CloseThread|Toast|toast|log\\(\" crates/nmp-core/src" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
crates/nmp-core/src/kernel/requests.rs:8:        self.log(format!("connecting {} relay {}", role.key(), role.url()));
crates/nmp-core/src/kernel/requests.rs:17:        self.log(format!("{} relay connected", role.key()));
crates/nmp-core/src/kernel/requests.rs:28:        self.log(format!(
crates/nmp-core/src/kernel/requests.rs:57:            self.log(format!(
crates/nmp-core/src/kernel/requests.rs:132:        self.log(format!("open author view {}", short_hex(&pubkey)));
crates/nmp-core/src/kernel/requests.rs:137:            self.log("author view request queued until relay connects");
crates/nmp-core/src/kernel/requests.rs:160:        self.log(format!("open thread view {}", short_hex(&event_id)));
crates/nmp-core/src/kernel/requests.rs:165:            self.log("thread request queued until relay connects");
crates/nmp-core/src/kernel/requests.rs:192:        self.log(format!("open diagnostic firehose #{tag}"));
crates/nmp-core/src/kernel/requests.rs:197:            self.log("diagnostic firehose queued until relay connects");
crates/nmp-core/src/kernel/requests.rs:214:            self.log(format!(
crates/nmp-core/src/kernel/requests.rs:234:            self.log("profile claim queued until indexer connects");
crates/nmp-core/src/kernel/requests.rs:256:        self.log(format!(
crates/nmp-core/src/kernel/requests.rs:281:        self.log(format!("close author view {}", short_hex(pubkey)));
crates/nmp-core/src/kernel/requests.rs:309:        self.log(format!("close thread view {}", short_hex(event_id)));
crates/nmp-core/src/kernel/requests.rs:537:        self.log(format!("REQ {sub_id}@{}: {summary}", role.key()));
crates/nmp-core/src/kernel/requests.rs:559:        self.log(format!(
crates/nmp-core/src/kernel/status.rs:309:    pub(super) fn log(&mut self, message: impl Into<String>) {
crates/nmp-core/src/kernel/mod.rs:390:        self.log("starting role-aware nmp demo slice");
crates/nmp-core/src/kernel/ingest.rs:37:            self.log(format!("unparseable relay frame: {}", truncate(text, 120)));
crates/nmp-core/src/kernel/ingest.rs:85:                self.log(format!("EOSE {sub_id}"));
crates/nmp-core/src/kernel/ingest.rs:97:                self.log(format!("NOTICE {} {notice}", role.key()));
crates/nmp-core/src/kernel/ingest.rs:120:                self.log(format!(
crates/nmp-core/src/kernel/ingest.rs:126:            _ => self.log(format!("relay frame {kind}")),
crates/nmp-core/src/kernel/ingest.rs:136:            self.log(format!("bad EVENT payload on {sub_id}"));
crates/nmp-core/src/kernel/ingest.rs:201:        self.log(format!(
crates/nmp-core/src/kernel/ingest.rs:224:            self.log(format!(
crates/nmp-core/src/kernel/ingest.rs:351:            self.log(format!(
crates/nmp-core/src/kernel/update.rs:101:            self.log(format!(
crates/nmp-core/src/actor.rs:11:pub(crate) enum ActorCommand {
crates/nmp-core/src/actor.rs:14:    OpenAuthor { pubkey: String },
crates/nmp-core/src/actor.rs:15:    OpenThread { event_id: String },
crates/nmp-core/src/actor.rs:17:    ClaimProfile { pubkey: String, consumer_id: String },
crates/nmp-core/src/actor.rs:18:    ReleaseProfile { pubkey: String, consumer_id: String },
crates/nmp-core/src/actor.rs:19:    CloseAuthor { pubkey: String },
crates/nmp-core/src/actor.rs:20:    CloseThread { event_id: String },
crates/nmp-core/src/actor.rs:27:    Command(ActorCommand),
crates/nmp-core/src/actor.rs:36:pub(crate) fn run_actor(command_rx: Receiver<ActorCommand>, update_tx: Sender<String>) {
crates/nmp-core/src/actor.rs:68:                    ActorCommand::Start {
crates/nmp-core/src/actor.rs:86:                    ActorCommand::Configure {
crates/nmp-core/src/actor.rs:95:                    ActorCommand::OpenAuthor { pubkey } => {
crates/nmp-core/src/actor.rs:100:                    ActorCommand::OpenThread { event_id } => {
crates/nmp-core/src/actor.rs:105:                    ActorCommand::OpenFirehoseTag { tag } => {
crates/nmp-core/src/actor.rs:110:                    ActorCommand::ClaimProfile {
crates/nmp-core/src/actor.rs:118:                    ActorCommand::ReleaseProfile {
crates/nmp-core/src/actor.rs:126:                    ActorCommand::CloseAuthor { pubkey } => {
crates/nmp-core/src/actor.rs:131:                    ActorCommand::CloseThread { event_id } => {
crates/nmp-core/src/actor.rs:136:                    ActorCommand::Stop => {
crates/nmp-core/src/actor.rs:143:                    ActorCommand::Reset => {
crates/nmp-core/src/actor.rs:159:                    ActorCommand::Shutdown => {
crates/nmp-core/src/actor.rs:232:fn bridge_commands(command_rx: Receiver<ActorCommand>, actor_tx: Sender<ActorMsg>) {
crates/nmp-core/src/ffi.rs:1:use crate::actor::{run_actor, ActorCommand};
crates/nmp-core/src/ffi.rs:19:    tx: Sender<ActorCommand>,
crates/nmp-core/src/ffi.rs:30:        let _ = self.tx.send(ActorCommand::Shutdown);
crates/nmp-core/src/ffi.rs:110:    let _ = app.tx.send(ActorCommand::Start {
crates/nmp-core/src/ffi.rs:127:    let _ = app.tx.send(ActorCommand::Configure {
crates/nmp-core/src/ffi.rs:138:    let _ = app.tx.send(ActorCommand::Stop);
crates/nmp-core/src/ffi.rs:146:    let _ = app.tx.send(ActorCommand::Reset);
crates/nmp-core/src/ffi.rs:161:    let _ = app.tx.send(ActorCommand::OpenAuthor { pubkey });
crates/nmp-core/src/ffi.rs:176:    let _ = app.tx.send(ActorCommand::OpenThread { event_id });
crates/nmp-core/src/ffi.rs:188:    let _ = app.tx.send(ActorCommand::OpenFirehoseTag { tag });
crates/nmp-core/src/ffi.rs:210:    let _ = app.tx.send(ActorCommand::ClaimProfile {
crates/nmp-core/src/ffi.rs:235:    let _ = app.tx.send(ActorCommand::ReleaseProfile {
crates/nmp-core/src/ffi.rs:253:    let _ = app.tx.send(ActorCommand::CloseAuthor { pubkey });
crates/nmp-core/src/ffi.rs:268:    let _ = app.tx.send(ActorCommand::CloseThread { event_id });

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/ffi.rs | sed -n '145,268p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   145	    };
   146	    let _ = app.tx.send(ActorCommand::Reset);
   147	}
   148	
   149	#[no_mangle]
   150	pub extern "C" fn nmp_app_open_author(app: *mut NmpApp, pubkey: *const c_char) {
   151	    let Some(app) = app_ref(app) else {
   152	        return;
   153	    };
   154	    let Some(pubkey) = c_string_argument(pubkey) else {
   155	        return;
   156	    };
   157	    if !is_hex_pubkey(&pubkey) {
   158	        return;
   159	    }
   160	
   161	    let _ = app.tx.send(ActorCommand::OpenAuthor { pubkey });
   162	}
   163	
   164	#[no_mangle]
   165	pub extern "C" fn nmp_app_open_thread(app: *mut NmpApp, event_id: *const c_char) {
   166	    let Some(app) = app_ref(app) else {
   167	        return;
   168	    };
   169	    let Some(event_id) = c_string_argument(event_id) else {
   170	        return;
   171	    };
   172	    if !is_hex_id(&event_id) {
   173	        return;
   174	    }
   175	
   176	    let _ = app.tx.send(ActorCommand::OpenThread { event_id });
   177	}
   178	
   179	#[no_mangle]
   180	pub extern "C" fn nmp_app_open_firehose_tag(app: *mut NmpApp, tag: *const c_char) {
   181	    let Some(app) = app_ref(app) else {
   182	        return;
   183	    };
   184	    let Some(tag) = c_string_argument(tag) else {
   185	        return;
   186	    };
   187	
   188	    let _ = app.tx.send(ActorCommand::OpenFirehoseTag { tag });
   189	}
   190	
   191	#[no_mangle]
   192	pub extern "C" fn nmp_app_claim_profile(
   193	    app: *mut NmpApp,
   194	    pubkey: *const c_char,
   195	    consumer_id: *const c_char,
   196	) {
   197	    let Some(app) = app_ref(app) else {
   198	        return;
   199	    };
   200	    let Some(pubkey) = c_string_argument(pubkey) else {
   201	        return;
   202	    };
   203	    let Some(consumer_id) = c_string_argument(consumer_id) else {
   204	        return;
   205	    };
   206	    if !is_hex_pubkey(&pubkey) {
   207	        return;
   208	    }
   209	
   210	    let _ = app.tx.send(ActorCommand::ClaimProfile {
   211	        pubkey,
   212	        consumer_id,
   213	    });
   214	}
   215	
   216	#[no_mangle]
   217	pub extern "C" fn nmp_app_release_profile(
   218	    app: *mut NmpApp,
   219	    pubkey: *const c_char,
   220	    consumer_id: *const c_char,
   221	) {
   222	    let Some(app) = app_ref(app) else {
   223	        return;
   224	    };
   225	    let Some(pubkey) = c_string_argument(pubkey) else {
   226	        return;
   227	    };
   228	    let Some(consumer_id) = c_string_argument(consumer_id) else {
   229	        return;
   230	    };
   231	    if !is_hex_pubkey(&pubkey) {
   232	        return;
   233	    }
   234	
   235	    let _ = app.tx.send(ActorCommand::ReleaseProfile {
   236	        pubkey,
   237	        consumer_id,
   238	    });
   239	}
   240	
   241	#[no_mangle]
   242	pub extern "C" fn nmp_app_close_author(app: *mut NmpApp, pubkey: *const c_char) {
   243	    let Some(app) = app_ref(app) else {
   244	        return;
   245	    };
   246	    let Some(pubkey) = c_string_argument(pubkey) else {
   247	        return;
   248	    };
   249	    if !is_hex_pubkey(&pubkey) {
   250	        return;
   251	    }
   252	
   253	    let _ = app.tx.send(ActorCommand::CloseAuthor { pubkey });
   254	}
   255	
   256	#[no_mangle]
   257	pub extern "C" fn nmp_app_close_thread(app: *mut NmpApp, event_id: *const c_char) {
   258	    let Some(app) = app_ref(app) else {
   259	        return;
   260	    };
   261	    let Some(event_id) = c_string_argument(event_id) else {
   262	        return;
   263	    };
   264	    if !is_hex_id(&event_id) {
   265	        return;
   266	    }
   267	
   268	    let _ = app.tx.send(ActorCommand::CloseThread { event_id });

codex
Not fine.

**Findings**
- [docs/design/ffi-hardening/gates.md:231](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:231) redefines D5 as “snapshots bounded by what’s open,” but [docs/plan.md:9](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:9) and [debt-inventory.md:341](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:341) define D5 as “capabilities report, never decide.” Fix the doctrine mapping before sign-off; add capability-bridge evidence instead of snapshot evidence, or explicitly reconcile the canonical doctrine docs.

- [docs/design/ffi-hardening/ci.md:221](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:221) runs `just stress-gate` in pre-merge, but [ci.md:163](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:163) requires S6/S9 metrics that pre-merge explicitly does not run at [ci.md:198](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:198). It also requires doctrine review via [ci.md:86](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:86), which is only produced at milestone close. Fix: split `stress-gate-fast` from `stress-close-gate`.

- [docs/design/ffi-hardening/scenarios.md:231](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/scenarios.md:231) includes null app pointers, and [scenarios.md:233](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/scenarios.md:233) requires freed-pointer calls to “must not crash.” That is not a valid raw-pointer FFI contract; after `nmp_app_free`, calls are undefined behavior, and null app has no state channel for the required toast at [scenarios.md:242](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/scenarios.md:242). Fix: require no-crash only for null app, no toast; remove freed-pointer probe or introduce a handle registry.

- [docs/design/ffi-hardening/ci.md:334](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:334) allows M10.5 close with S10 PASS-with-deferral, while [gates.md:256](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:256) uses S10 as D1 sign-off evidence. Fix: either make S10 required for M10.5 or remove it from doctrine evidence and replace with a non-conditional proof.

- File size: new hand-authored docs exceed the 300 LOC soft limit: `ffi-hardening.md` 314, `ci.md` 345, `harness.md` 410, `scenarios.md` 353, plus `debt-inventory.md` 407. `docs/plan.md` remains 789 over the 500 hard ceiling. Fix by splitting before claiming zero-debt posture. The 1263-line Codex transcript is only acceptable if treated as generated archival output.

- [docs/design/ffi-hardening.md:51](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening.md:51) says 13 exported symbols, but the table and actual FFI include 14, ending at [ffi.rs:257](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/ffi.rs:257). Fix the count.

**Checks**
No executable `TODO/FIXME/unimplemented` debt was added in source; hits are docs/audit text and the archived Codex transcript.
tokens used
119,419
Not fine.

**Findings**
- [docs/design/ffi-hardening/gates.md:231](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:231) redefines D5 as “snapshots bounded by what’s open,” but [docs/plan.md:9](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:9) and [debt-inventory.md:341](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:341) define D5 as “capabilities report, never decide.” Fix the doctrine mapping before sign-off; add capability-bridge evidence instead of snapshot evidence, or explicitly reconcile the canonical doctrine docs.

- [docs/design/ffi-hardening/ci.md:221](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:221) runs `just stress-gate` in pre-merge, but [ci.md:163](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:163) requires S6/S9 metrics that pre-merge explicitly does not run at [ci.md:198](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:198). It also requires doctrine review via [ci.md:86](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:86), which is only produced at milestone close. Fix: split `stress-gate-fast` from `stress-close-gate`.

- [docs/design/ffi-hardening/scenarios.md:231](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/scenarios.md:231) includes null app pointers, and [scenarios.md:233](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/scenarios.md:233) requires freed-pointer calls to “must not crash.” That is not a valid raw-pointer FFI contract; after `nmp_app_free`, calls are undefined behavior, and null app has no state channel for the required toast at [scenarios.md:242](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/scenarios.md:242). Fix: require no-crash only for null app, no toast; remove freed-pointer probe or introduce a handle registry.

- [docs/design/ffi-hardening/ci.md:334](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:334) allows M10.5 close with S10 PASS-with-deferral, while [gates.md:256](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:256) uses S10 as D1 sign-off evidence. Fix: either make S10 required for M10.5 or remove it from doctrine evidence and replace with a non-conditional proof.

- File size: new hand-authored docs exceed the 300 LOC soft limit: `ffi-hardening.md` 314, `ci.md` 345, `harness.md` 410, `scenarios.md` 353, plus `debt-inventory.md` 407. `docs/plan.md` remains 789 over the 500 hard ceiling. Fix by splitting before claiming zero-debt posture. The 1263-line Codex transcript is only acceptable if treated as generated archival output.

- [docs/design/ffi-hardening.md:51](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening.md:51) says 13 exported symbols, but the table and actual FFI include 14, ending at [ffi.rs:257](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/ffi.rs:257). Fix the count.

**Checks**
No executable `TODO/FIXME/unimplemented` debt was added in source; hits are docs/audit text and the archived Codex transcript.
