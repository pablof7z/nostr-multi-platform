# Design: FFI hardening stress harness (M10.5)

> **Status:** Draft for the M10.5 hard gate before M11.
> **Authoring agent:** Architect (M10.5 design task #4).
> **Companion audit:** [`docs/perf/m10.5/debt-inventory.md`](../perf/m10.5/debt-inventory.md).
> **Reading order:** this file first (§1, §2, §7, §10), then the five sub-docs (§3, §4, §5, §6, §8, §9).

This is the design spec for the M10.5 dedicated FFI-surface hardening pass. The
goal is to prove — in measured numbers, not adjectives — that the current raw
C FFI between `crates/nmp-core` and `ios/NmpStress` is rock-solid before a
single line of the M11 podcast app is written.

## 1. Goals and non-goals

### 1.1 Goals

1. **Empirical proof of FFI invariants.** Every claim made in the RMP bible
   (`docs/aim.md` §2 commandments 1–10), every cardinal doctrine
   (`docs/product-spec/overview-and-dx.md` §1.5 D0–D5), and every ownership
   rule in the current FFI surface is verified by a concrete harness scenario
   that produces a numeric pass/fail.
2. **Exhaustive failure-mode coverage.** Ten named failure modes (§3 in
   [`scenarios.md`](./ffi-hardening/scenarios.md)) cover the full shape of
   FFI bugs a non-social-domain consumer would otherwise find the hard way.
3. **Three independent runner paths.** A Rust-side stress binary, an XCUITest
   target on iPhone 16 Pro simulator + iPhone 12 hardware, and a parallel
   Sonnet-agent UI-script fleet. Each surfaces a different class of bug.
4. **Zero pre-existing debt at gate close.** The `docs/perf/m10.5/debt-inventory.md`
   audit must show 0 items in every "must-fix" row when M10.5 declares done.
5. **Reproducible numeric exit gates.** No "fast enough", no "acceptable
   memory growth". Every cell of the §5 table in
   [`gates.md`](./ffi-hardening/gates.md) is a specific number.

### 1.2 Non-goals

- **Changing the FFI shape.** The current surface (14 raw `extern "C"`
  functions in `crates/nmp-core/src/ffi.rs`, JSON-string update callback) is
  what we harden. Migrating to UniFFI + per-app generated enums per
  ADR-0010 is **M14**, not M10.5.
- **New domain features.** No new view kinds, no new actions, no new
  capabilities. M10.5 only fixes, instruments, measures.
- **Cross-platform parity.** M10.5 covers iOS only. Android / desktop /
  web FFI hardening is a separate milestone.
- **Adversarial input fuzzing of relay traffic.** Malformed event payloads,
  signature bombs, and kind:5 storms are out of scope for M10.5; the
  harness here exercises the *FFI surface*, not the *protocol surface*.
  Protocol hardening is M11/M12.

## 2. FFI surface inventory

The current FFI surface is **14 exported C symbols** in
`crates/nmp-core/src/ffi.rs` (lines 44–268) plus **one callback type**
(`UpdateCallback` at line 10). Every symbol below must have its
ownership, lifetime, thread-safety, and null-handling rules documented
and enforced by at least one harness scenario.

| # | Symbol | Signature (C) | Ownership / lifetime / threading | Null handling |
|---|---|---|---|---|
| 1 | `nmp_app_new` | `void * nmp_app_new(void)` | Returns a `Box::into_raw(NmpApp)`. Caller owns. Spawns 2 OS threads (actor + listener) + N relay-worker threads on `start`. Pointer is `Send` because internal `Sender`/`Mutex` are `Send`; **callers must not share the same pointer across threads without external synchronization**. | n/a (returns) |
| 2 | `nmp_app_free(*mut NmpApp)` | `void nmp_app_free(void *app)` | Reclaims the box, sends `Shutdown` to actor, joins actor + listener threads inside `Drop`. **Idempotent on null.** Caller must guarantee no other thread is mid-call into the same pointer when this is invoked. | Early-return if null (ffi.rs:74). |
| 3 | `nmp_app_set_update_callback` | `void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback cb)` | Replaces the registered `(context, fn)` pair. The `*mut c_void` context is **stored as `usize`** (ffi.rs:13–16) and dereferenced inside the listener thread — caller owns the context lifetime and **must clear the callback to null before freeing the context**. Passing `cb=None` clears registration. | Returns silently if app null or mutex poisoned (ffi.rs:87–96). |
| 4 | `nmp_app_start` | `void nmp_app_start(void *app, uint events_per_second, uint visible_limit, uint emit_hz)` | Fire-and-forget. `events_per_second` is currently ignored (`_events_per_second`); kept for ABI stability. `visible_limit` clamped to `[1, 500]`; 0 → default 80. `emit_hz` clamped to `[1, 12]`; 0 → default 4. Spawns relay workers on first call. | Silent no-op on null (ffi.rs:107–108). |
| 5 | `nmp_app_configure` | same shape as `_start` | Same clamping. Re-tunes a running actor. | Silent no-op on null. |
| 6 | `nmp_app_stop` | `void nmp_app_stop(void *app)` | Sends `Stop`; actor closes relay workers, keeps actor + listener threads alive. Caller may call `_start` again. | Silent no-op on null. |
| 7 | `nmp_app_reset` | `void nmp_app_reset(void *app)` | Closes relays, replaces the in-actor `Kernel` instance, reopens relays if running. | Silent no-op on null. |
| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
| 9 | `nmp_app_open_thread(*mut, *const c_char)` | `void nmp_app_open_thread(void *app, const char *event_id)` | UTF-8, 64-char hex event id. `is_hex_id`-validated. Same silent-drop on bad input. | Silent no-op. |
| 10 | `nmp_app_open_firehose_tag(*mut, *const c_char)` | `void nmp_app_open_firehose_tag(void *app, const char *tag)` | UTF-8 tag value. No hex validation. Empty → silent no-op. | Silent no-op. |
| 11 | `nmp_app_claim_profile(*mut, *const c_char, *const c_char)` | `void nmp_app_claim_profile(void *app, const char *pubkey, const char *consumer_id)` | Hex-pubkey-validated. `consumer_id` is an opaque caller-chosen string (used by the kernel refcount table). Two strings, two lifetime contracts: both must be valid for the duration of the call only — the kernel `String`-copies them. | Silent no-op on any null or invalid. |
| 12 | `nmp_app_release_profile(*mut, *const c_char, *const c_char)` | mirror of `_claim_profile` | Same. **Pair invariant:** every `claim_profile(pk, id)` must be matched by exactly one `release_profile(pk, id)`; an unmatched `release` is silently dropped by the kernel refcount logic. | Silent no-op. |
| 13 | `nmp_app_close_author(*mut, *const c_char)` | `void nmp_app_close_author(void *app, const char *pubkey)` | Closes the named author view. Different from `release_profile` — closes the *view*, not a refcounted profile claim. | Silent no-op. |
| 14 | `nmp_app_close_thread(*mut, *const c_char)` | `void nmp_app_close_thread(void *app, const char *event_id)` | Closes the named thread view. | Silent no-op. |

### 2.1 Callback contract (`UpdateCallback`)

```rust
type UpdateCallback = extern "C" fn(*mut c_void, *const c_char);
```

- Invoked **from the dedicated listener OS thread** (ffi.rs:52–62), never
  from the actor thread, never from a relay-worker thread.
- The `*const c_char` payload is a JSON-serialized `KernelUpdate`. The
  `CString` backing it is allocated in the listener thread and dropped
  **at the end of the callback invocation** — the pointer dangles after
  the callback returns. Callers must `String::from(cString)` (or
  equivalent) inside the callback if they need to retain the data.
- The `*mut c_void` context is whatever was passed to
  `nmp_app_set_update_callback`. Per ffi.rs:13–16 it is round-tripped via
  `as usize` to satisfy `Send` for the `Arc<Mutex<...>>` registration
  slot. Callers must keep the underlying object alive for as long as the
  callback is registered.
- The callback **must not call back into any `nmp_app_*` function on the
  listener thread synchronously** unless the caller has accepted the
  reentrancy contract specified by scenario §3.5.
- The callback **must not block the listener thread**; doing so will back
  up the `update_rx` channel and stall every subsequent emit (which is
  bounded only by available memory in the unbounded `mpsc::channel`).

### 2.2 Threading topology (current)

See [`ffi-hardening/harness.md`](./ffi-hardening/harness.md) §0 for
the full threading topology diagram. Summary: five thread classes per
`NmpApp` instance — caller, actor, listener, N×relay-worker, and a
tokio-pool (currently unused). Every scenario in §3 names which thread
it stresses.

## 3. Failure modes

Ten scenarios — full table with assertions and numeric thresholds is in
[`ffi-hardening/scenarios.md`](./ffi-hardening/scenarios.md). Summary:

| # | Scenario | Threading concern | Primary doctrine tested |
|---|---|---|---|
| S1 | Mount/unmount churn | actor recv + refcount | D5 (snapshot bounded), bible #5 |
| S2 | Dispatch flood | mpsc backpressure | bible #3 (fire-and-forget) |
| S3 | Snapshot pressure | listener serialization | bible #9 (≤60 Hz), D5 |
| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
| S5 | Reentrancy | callback re-enters FFI | bible #3, deadlock-free |
| S6 | Capability lifecycle storms | start/stop/restart | bible #7 (idempotent) |
| S7 | Error-shape exhaustion | every invalid input path | D3 (no errors cross FFI) |
| S8 | Subscription planner DOS | OpenView/CloseView storm | D2 (≤60 Hz/view), D5 |
| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
| S10 | Long suspend simulation | background → foreground | bible #1 (monotonic rev) |

## 4. Harness architecture

Full sketches in [`ffi-hardening/harness.md`](./ffi-hardening/harness.md).
Three runners, one shared report format:

1. **`crates/nmp-testing/bin/ffi-stress/`** — Rust binary. Drives the FFI
   surface directly via the same `nmp_app_*` C symbols Swift uses,
   linking `nmp-core` as `staticlib`. Models the iOS main-thread loop
   with a single-threaded executor that imposes a budget.
2. **`ios/NmpStress/StressUITests/`** — XCUITest target. One test class
   per scenario, drives `NmpStress.app` end-to-end, asserts UI invariants
   and captures `XCTPerformanceMetric` (CPU, memory, hitches).
3. **Sonnet-agent runner** — a shell wrapper at
   `crates/nmp-testing/bin/ffi-stress/sonnet-runner.sh` that orchestrates
   N parallel `claude` agent processes, each scripting the simulator via
   the `mcp__xcode__*` tool family. Output: per-agent screenshot trail,
   assertion log, timing.

## 5. Numeric exit gates

The complete (scenario × metric × threshold) table is in
[`ffi-hardening/gates.md`](./ffi-hardening/gates.md). Sample row:

> S1 `mount_unmount_churn` — leaked bytes after 10 min = **0**;
> dispatch round-trip p99 = **≤ 1 ms** (Swift → Rust channel send);
> `OpenView/CloseView` ratio = **≥ 0.95** (every claim has a release);
> Instruments retained-by-cycle leaks = **0**.

No row in §5 contains the word "acceptable" or the phrase "fast enough".
Every cell is a number with units.

## 6. Run + reporting protocol

Full recipes in [`ffi-hardening/ci.md`](./ffi-hardening/ci.md). Headline:

```
just stress          # local simulator, fast scenarios (S1, S2, S3, S5, S7, S8)
just stress-device   # iPhone 12 hardware, full ten scenarios
just stress-sonnet   # parallel Sonnet-agent UI fleet
```

Output bundle layout:

```
docs/perf/m10.5/
├── <scenario>/
│   ├── report.md            # human-readable summary, per-metric pass/fail
│   ├── metrics.json         # machine-readable, schema-versioned
│   ├── instruments.trace/   # Instruments capture (Leaks, Allocations)
│   └── screenshots/         # PNG trail from XCUITest + Sonnet-agent
├── debt-inventory.md        # the companion audit (already lives here)
├── doctrine-review.md       # D0–D5 sign-off (M10.5 exit-gate artifact)
└── iphone12-baseline.md     # firehose-bench rerun on real hardware
```

## 7. Pre-existing debt

A separate auditor agent has produced
[`docs/perf/m10.5/debt-inventory.md`](../perf/m10.5/debt-inventory.md)
(442 LOC) covering 19 Rust files + 9 Swift files (6,559 LOC scanned)
with **0 critical, 0 high, 6 medium/low** findings. **Every item listed
as "Must-Fix" in that file must be 0 at the end of M10.5.**

As of 2026-05-18 the must-fix list is:

| Ref | File:line | Action | Effort |
|---|---|---|---|
| F1 | `crates/nmp-core/src/ffi.rs:75` | Add `// safe: ...` doc on unsafe `Box::from_raw` | 5 min |
| F2 | `crates/nmp-core/src/ffi.rs:275` | Same on unsafe `&*app` | 5 min |
| F3 | `crates/nmp-core/src/ffi.rs:284` | Same on unsafe `CStr::from_ptr` | 5 min |
| F4 | `crates/nmp-core/src/relay_worker.rs:242` | Comment `#[allow(unreachable_patterns)]` rationale | 2 min |
| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |

### 7.1 Re-grep gate

The M10.5 done declaration is **gated on a re-run of the audit grep
yielding zero results**. The exact command (captured 2026-05-18, 0 hits):

```bash
grep -rEn '(TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit)' \
  crates/nmp-core/src crates/nmp-testing/src \
  ios/NmpStress/NmpStress \
  | grep -v target | grep -v DerivedData
```

If the re-run at gate close yields any hits, the new items are
triaged into either *resolve before close* or *deferred with ADR + GH
issue*. Net unresolved must be 0.

### 7.2 D3 structural gap (named, not hidden)

The harness will exercise every typed FFI input path that fails
validation (§3 scenario S7). The current FFI is **silently no-op** on
bad input — `nmp_app_open_author(app, NULL)`, an empty string, or a
non-hex string all early-return from ffi.rs without any signal to the
caller and without setting any state field. This is **D3-compliant in
the strict sense** (no error crosses FFI as a value) **but
D3-incomplete in the user-visible sense** (no toast surfaces in
`AppState`).

The debt-inventory's D3 audit (lines 317–334) concludes the same:
errors-as-data crosses FFI via `RelayStatus.last_error` and
`RelayStatus.last_notice`, which is correct, but **app-action error
paths (invalid input dropped) have no equivalent surface**. M10.5
adds a `toast: Option<String>` field to the JSON update payload
(placed alongside `logs: Vec<String>` as a sibling of `metrics` in the
`KernelUpdate` serialization — see `crates/nmp-core/src/kernel/update.rs`)
and populates it from S7's validation failure paths. The schema change
is additive (older Swift readers ignore an unknown field) so this is not
a breaking FFI change.

This is intentionally surfaced in the design doc, not papered over.
The M14 UniFFI migration moves the surface to typed `Result`-shaped
returns; M10.5 ships the interim toast-field bridge.

## 8. Doctrine review checklist

Full D0–D5 line-item-to-scenario mapping in
[`ffi-hardening/gates.md`](./ffi-hardening/gates.md) §8. Headline:

| Doctrine | Proven by |
|---|---|
| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S4 (reconciler back-pressure) + code proof (ProfileCard.placeholder) |
| **D2** ≤60Hz/view, working-set bound | S2, S3, S8 — emit-rate cap, planner dedup |
| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
| **D4** one writer per fact | S1, S5 — refcount only mutated on actor thread; reentrancy under same single-writer rule |
| **D5** capabilities report, never decide | CapabilityModule code proof + S6 (relay capability lifecycle storms) |

## 9. CI integration

Three tiers, defined in [`ffi-hardening/ci.md`](./ffi-hardening/ci.md):

| Tier | Frequency | Scenarios | Where |
|---|---|---|---|
| **Pre-merge** | every PR | S1 (short), S2, S3 (10k events), S5, S7, S8 | GH Actions `macos-14` runner, iPhone 16 Pro sim |
| **Nightly** | daily | All ten, S1 full 10-min, S4 250 ms stalls × 60, S9 100 flap, S10 60s suspend | Mac mini self-hosted runner, iPhone 12 device |
| **On-demand** | release candidates | S1 8-hour soak, S2 1 M dispatch, S9 24-hour flap | Lab device, manual sign-off |

## 10. Open questions (for ADR after review)

1. **Toast field schema.** Is `toast: Option<String>` enough, or do we
   want `toast: Option<{ id: String, severity: Info|Warn|Error, message: String, source: String }>`? The latter is more useful but
   collides with ADR-0010's plan to migrate this entire surface to
   typed `Result` via UniFFI in M14. Recommend: keep it scalar for
   M10.5, revisit in ADR-0011 when M14 lands.
2. **Sonnet-agent determinism.** Parallel Sonnet agents producing
   non-deterministic taps means flaky CI. Should the Sonnet runner be
   nightly-only (no pre-merge gating), or do we record + replay agent
   traces the way `firehose-bench` does for relay frames? Recommend:
   record + replay, but defer the replay infrastructure to M11.5.
3. **iPhone 12 vs iPhone 16 Pro baselines.** The exit gate quotes one
   set of numbers; iPhone 12 is roughly 2× slower than 16 Pro on
   single-thread workloads. Should gate values be device-tagged, or do
   we publish two tables? Recommend: one table, scaled by a
   per-device coefficient documented in
   `docs/perf/m10.5/iphone12-baseline.md`.
4. **Reentrancy from inside the reconciler callback.** Bible #3 says
   `dispatch()` is fire-and-forget — but is dispatch-from-within-a-callback
   allowed at all? Scenario S5 asserts ordering and deadlock-freedom
   under the assumption that yes; should that be explicitly documented
   as a supported pattern, or actively discouraged? Recommend: document
   as supported (Swift will inevitably do it via Combine pipelines);
   the actor's mpsc Sender is `Send + Sync` and the test proves it
   works.
5. **Handle registry for freed-pointer safety (M14).** Calling `nmp_app_*`
   after `nmp_app_free` is undefined behavior; no handle registry exists
   to make it recoverable. S7 excludes freed-pointer probes for this reason.
   M14 UniFFI migration replaces raw pointers with typed handles, eliminating
   the UB class. Until then, caller must not call after free.

---

**Next:** open the sub-docs in order: scenarios, harness, gates, ci.
