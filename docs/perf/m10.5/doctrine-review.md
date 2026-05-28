# M10.5 Doctrine Review — FFI Surface Signoff (D0–D8)

- **Date:** 2026-05-18
- **Scope:** the raw `nmp_app_*` C FFI surface, pinned to `origin/master`
  @ `221feb6` (`docs/ffi-surface.md`) + the S1–S5 host baseline
  (`docs/perf/m10.5/sim-baseline.md` @ `158b744`).
- **Canonical doctrine:** `docs/product-spec/overview-and-dx.md` §1.5 →
  `docs/product-spec/doctrine.md` (D0–D8). **Nine** doctrines: D0–D5 policy,
  D6–D8 substrate invariants. The plan's "Doctrine review (D0–D5)" wording is
  stale (PD-001); this review covers all nine, per the scoped decision record
  in `docs/perf/pending-user-decisions.md` and the measured evidence in this
  directory.
- **Verdict legend:** PASS · PASS (noted) · **EXCEPTION** (logged, not waived).

This is a substrate-review of the FFI surface, not a full-framework audit.
Policy doctrines (D0–D5) are reviewed only for how the FFI boundary upholds or
violates them; D6–D8 are reviewed directly.

---

## Summary table

| # | Doctrine | Verdict | Primary evidence |
|---|----------|---------|------------------|
| D0 | No app nouns in `nmp-core`; test surface gated | PASS | `ffi-surface.md` §2 — `inject_*` cfg-gated |
| D1 | Best-effort rendering | PASS (noted) | FFI carries projections only; not decided at boundary |
| D2 | Negentropy first | PASS (noted) | Not an FFI-surface concern; actor-side |
| D3 | Outbox routing automatic | PASS (noted) | `add_relay`/`publish_note` carry no relay choice across FFI |
| D4 | Single writer per fact | PASS | S4: `configure()` p99 = 22 µs *during* a 250 ms callback stall |
| D5 | Snapshots bounded by what's open | PASS | S3: max payload 0.47 MiB; `claim/release_profile` scope the projection |
| D6 | Errors never cross FFI | PASS | All 28 production symbols early-return silently; 0 `Result`/panic across boundary |
| D7 | Capabilities report; never decide | PASS (noted) | Callback bridge stores opaque ctx only; capability socket in-flight |
| D8 | Reactivity ≤60 Hz · working-set bounded | **EXCEPTION** | S3 emit 6.43 Hz ✓, 22 B/emit ✓; **S2 RSS 45.9 MiB > 20 MiB ✗** |

---

## D0 — Kernel + extension modules; test surface gated — **PASS**

The two test-support symbols (`nmp_app_inject_pre_verified_events`,
`nmp_app_inject_signed_events`) are `#[cfg(any(test, feature = "test-support"))]`
(`ffi/mod.rs:298`, `:369`; `ffi-surface.md` §2) and are therefore excluded from
the production ABI — shipping Swift/C never sees them. No app-domain noun
(`twitter`, `podcast`, …) appears in any of the 28 symbols; the surface is
generic kernel verbs (`open_author`, `claim_profile`, `publish_note`, …). The
capability boundary is the `VerifiedEvent` type: production builds can only
construct one via `try_from_raw` (full Schnorr); `from_raw_unchecked` is
reachable only through the cfg-gated symbols. **PASS.**

## D1 — Best-effort rendering — **PASS (noted)**

D1 is a policy doctrine enforced by view-payload *types*, not by the FFI
signature. The FFI surface neither blocks nor gates: every read verb
(`open_author`, `open_thread`, `open_timeline`, `open_firehose_tag`) is a
fire-and-forget `ActorCommand` send (`let _ = app.tx.send(...)`) that returns
`()` immediately — no synchronous fetch, no error, no "loading" return value
crosses the boundary. The FFI cannot introduce a spinner-gate because it has no
return channel for one. Note: full D1 enforcement (non-`Option` display fields)
lives in the view-payload layer, out of FFI-review scope. **PASS (noted).**

## D2 — Negentropy first — **PASS (noted)**

Not an FFI-surface property. Subscription/coverage policy is actor-internal;
no FFI symbol selects REQ-vs-negentropy or exposes a watermark. The FFI cannot
violate D2 because it does not carry sync policy. Out of substrate-review scope
for the boundary; deferred to subscription-layer review. **PASS (noted).**

## D3 — Outbox routing automatic — **PASS (noted)**

`nmp_app_publish_note` (`ffi/identity.rs:67`) takes only `content` +
`reply_to_id`; `nmp_app_add_relay`/`remove_relay` (`:136`/`:152`) edit the
declared relay set but carry **no per-operation relay choice** across FFI. The
boundary offers no "publish to relay X" verb — routing is the actor's, exactly
as D3 requires (manual selection is the named opt-out, absent here). **PASS
(noted).**

## D4 — Single writer per fact — **PASS**

Directly evidenced by S4 (`sim-baseline.md` §S4): during twelve 250 ms
main-thread callback stalls, `configure()` p99 latency = **22 µs** — the actor
is never blocked by a sleeping listener; `rev` stays strictly monotonic;
`stale_rev_pairs = 0`; 0 listener drops. The single writer (actor thread)
continues mutating state and emitting monotonic revisions while the FFI callback
consumer is stalled. The FFI handle exposes exactly one command `Sender`
(`ffi/mod.rs:25`); all mutation funnels through it. **PASS.**

## D5 — Snapshots bounded by what's open — **PASS**

`nmp_app_claim_profile`/`release_profile` and `open_/close_author`/`thread`
(`ffi/mod.rs:204`–`:280`) are the open/close verbs that scope the projection;
closing evicts. S3 (`sim-baseline.md` §S3) injected **100,000** events yet the
max FFI payload was **490,038 B (0.47 MiB)** — the snapshot tracked open views,
not the 100k-event store. The event store never crosses the boundary (no FFI
symbol returns events). **PASS.**

## D6 — Errors never cross FFI as exceptions — **PASS**

The strongest result. Every one of the **26 production symbols** (`ffi/mod.rs`
14 + `ffi/identity.rs` 12) early-returns silently on null/invalid input via
`app_ref` (`ffi/mod.rs:409`), `c_string_argument` (`:418`),
`c_optional_string_argument` (`:438`), `is_hex_pubkey`/`is_hex_id`
(`crate::kernel`). No production symbol returns `Result`/`Option<Error>`,
panics across the boundary, or throws — verified per-symbol in
`ffi-surface.md` §1/§1b ("D6 silent-no-op" column). The actor send is
fire-and-forget (`let _ = app.tx.send(...)`), so even a dead channel is a
silent no-op, never an error. Empirically corroborated: across S1–S5
(>460k FFI cycles + 300k dispatches + 100k injects) **zero** errors crossed the
boundary; `failed_sends = 0` (S2), `dispatch_loss = 0` (S5). Mirrors
`docs/aim.md` §2 bible invariant "errors do not cross FFI". **PASS.**

## D7 — Capabilities report; never decide policy — **PASS (noted)**

`nmp_app_set_update_callback` (`ffi/mod.rs:94`) stores only an opaque
`context: *mut c_void` (as `usize`, never deref'd by Rust) + an
`extern "C" fn` pointer; the bridge decides nothing — it relays. `nmp_app_free`
is idempotent on null (`:85`) but not on double-free (documented in
`ffi-surface.md` §1, a caller contract, not a policy decision by the bridge).
**Noted / re-verify hook:** the keyring capability FFI socket
(`nmp_app_set_capability_callback` / `dispatch_capability` / `free_string`,
`ffi/capability.rs`) is **in-flight in a concurrent session and not yet on
`origin/master`** at this review's pin (`ffi-surface.md` §2b; PD-019 update).
Its design intent (route `CapabilityRequest` JSON to the native handler and
return envelope-data only, D6-clean) is doctrine-conformant on paper, but the
D7 "reports, never decides" + idempotent-lifecycle review of those three
symbols is **explicitly deferred until the file lands on master** — tracked in
`ffi-surface.md` §2b. Committed surface: **PASS**; capability socket:
**deferred re-review (not yet reviewable)**.

## D8 — Reactivity contract — **EXCEPTION (logged, not waived)**

D8 has three sub-clauses; the FFI/host baseline splits on them:

- **≤60 Hz per view — PASS.** S3 (`sim-baseline.md` §S3): end-to-end
  reconciler `emit_hz = 6.43` against a 100k-event burst, gate ≤ 60 Hz, with
  large margin. `clamp_emit_hz` (`ffi/mod.rs:459`) hard-caps the FFI-settable
  rate at 12.
- **Allocations linear in active views, not cached events — PASS.** S3 net
  heap = **22 B/emit** (budget 980,076 B; ~0.002%). S1: over **463,207**
  mount/unmount cycles, `net_heap_slope = 0 bytes/sec`, `unmatched_claims = 0`,
  RSS growth 1.30 MiB ≤ 5 MiB — the FFI refcount/working-set path retains
  nothing per cycle.
- **Working-set bounded — EXCEPTION.** S2 (`sim-baseline.md` §S2): under a
  10,000-dispatch/s × 4-thread flood, `rss_growth_bytes` = **48,119,808 B
  (45.89 MiB)** vs the **20 MiB** §G-S2 gate — **2.29× over budget**. Send
  latency is excellent (p50 3.4 µs, p99 30 µs) and lossless (`failed_sends = 0`),
  so the *FFI send path* is clean; the overrun is the actor's **unbounded mpsc
  backlog** accumulating working set under sustained flood. This is a genuine
  D8 "working-set bounded" violation at flood rate, surfaced — exactly the
  bug-class M10.5 exists to find.

**D8 verdict: EXCEPTION — logged, not waived.** The reactivity-rate and
allocation-linearity halves of D8 PASS with wide margin; the working-set-bounded
half FAILS under the S2 10k/s flood. M10.5 cannot honestly claim a clean D8
signoff. **Update 2026-05-18:** the leak-vs-transient tiebreaker was run
(`s2-drain-analysis.md`) — verdict **RETAINED**: of ~38 MiB net heap allocated
during the flood, only 0.13 % is reclaimed after the backlog drains. The
threshold-revision escape is **foreclosed by evidence**; a **bounded actor
channel + bounded actor-side state is mandatory** (the kernel fix, owned by the
`nmp-core` session — out of this workstream's scope). The
`retained_heap_after_drain_bytes` gate (≤ 1 MiB) added to the harness is the
regression check. Tracked as the headline M10.5 finding in `sim-baseline.md`
§S2 / §Conclusion. The S1 `cycles_completed` FAIL is a separate, documented
**macOS-host timer artifact** (not a kernel/D8 regression — net-heap slope is
0); it is unobservable on the Rust host harness and re-routed to the Pulse/iOS
track, recorded FAIL not waived.

---

## Signoff

| Doctrine band | Result |
|---|---|
| D0–D5 (policy, at the FFI boundary) | **PASS** (D1/D2/D3 noted as out-of-boundary-scope) |
| D6 (errors never cross FFI) | **PASS** — strongest result; per-symbol + empirical |
| D7 (capabilities report) | **PASS** on committed surface; capability socket re-review **deferred** (not yet on master) |
| D8 (reactivity contract) | **EXCEPTION** — ≤60 Hz ✓ + alloc-linear ✓, **working-set-bounded ✗ (S2, 2.29× budget)** |

**Overall: M10.5 FFI surface is doctrine-conformant on D0–D7, with one logged
D8 exception (S2 working-set) and one deferred D7 re-review (in-flight
capability socket).** Per the M10.5 honesty mandate, this is **not** a clean
green signoff: D8 has a real, unwaived finding that must be fixed or
threshold-revised before the §G-S2 gate closes. Recorded honestly; not papered
over.

*Depends on D1 (`docs/ffi-surface.md` @ 221feb6) + D2
(`docs/perf/m10.5/sim-baseline.md` @ 158b744). Substrate review per
`docs/product-spec/doctrine.md`.*
