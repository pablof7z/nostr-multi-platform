# ADR-0054 — Web persistence (OPFS-SQLite sync VFS) + offline-queue durability

- **Status:** Accepted for Stage #5 (store-injection seam); Stages #6–#9
  Proposed with open implementation gates (see "Risks and implementation
  gates"). Stage #6 must not start until the gates marked *blocking #6* are
  resolved.
- **Date:** 2026-06-13
- **Blocked-by:** ADR-0067 / #2045 (browser-runtime ownership split) — OPFS store is injected via the browser builder storage decision, not constructed in nmp-wasm.
- **Relates to:** ADR-0045 (store→projection replay), ADR-0047 (browser worker
  runtime contract), ADR-0040 (capability-worker seam), ADR-0044 (Tier-3
  snapshot envelope typing)
- **Reference:** `docs/wasm-surface.md` (living wire contract); the EventStore
  invariant doc `docs/design/lmdb/trait.md`
- **Numbering note:** The originating mandate referred to this work as
  "ADR-0048". That number is already occupied by
  `0048-nip55-external-signer-capability.md`. This decision was originally
  filed as `0053` (the next free number after `0052`), but `0053` was
  simultaneously occupied by `0053-host-declared-projection-subscriptions.md`.
  To resolve the collision it is now filed as **ADR-0054**. References to
  "ADR-0053 web persistence" or "ADR-0048 persistence" in upstream planning
  text should be retargeted to ADR-0054.

## Context

NMP's browser runtime (`crates/nmp-browser-runtime`, with `crates/nmp-wasm` as
the ABI shell per ADR-0067) runs the `KernelReducer` on a
dedicated Worker event loop (ADR-0047). The kernel holds the single
authoritative event store as `store: Arc<dyn EventStore>`
(`crates/nmp-core/src/kernel/mod.rs:587`). Today the wasm build always
constructs an in-memory `MemEventStore`: the kernel resets on every page
reload (ADR-0047 Consequences — "IndexedDB persistence is not yet wired").
This blocks the honest web persistence claim tracked by #1007 under the
browser-runtime reset epic #2045: second-launch render, offline-first reads, and
durable offline publish all require a persistent store on the web target.

The `EventStore` trait
(`crates/nmp-store/src/events.rs:144` — `pub trait EventStore: Send + Sync`)
is the load-bearing constraint. It is **synchronous**, `Send + Sync`, and
Iterator-based. Every method takes `&self` (interior mutability required);
several scans return `Box<dyn EventIter + 'a>` borrowed against the store;
`insert` must be atomic across the primary table plus 6+ secondary indexes.
This trait is the contract that both `MemEventStore` and the production
`LmdbEventStore` honor byte-for-byte. **It must not be relaxed** — not made
async, not made `?Send`, not `cfg`-gated in `nmp-core` or `nmp-store`. (ZERO
TECH DEBT, D4 single-writer.)

`query_visit` takes a `limit` and a visitor; the **per-tick budget is owned by
the caller** (`crates/nmp-core/src/kernel/cache_serve/continuation.rs:90-126`:
`visit_limit = tick_remaining.min(...)`, counted per visitor call and consumed
even on dedup). The backend's obligation is to scan newest-first and invoke the
visitor for every scanned row up to `limit`; a SQLite ordered-index query with
`ORDER BY ... LIMIT` satisfies this with O(log n) seek + O(limit) step rather
than an O(N) table scan.

Two hard browser facts shape the solution:

1. **No async file I/O on the trait.** A synchronous `&self` trait can only be
   backed by synchronous storage. In the browser the *only* synchronous
   persistence primitive is the OPFS `FileSystemSyncAccessHandle`, which is
   available **exclusively inside a dedicated Web Worker** — exactly where our
   actor already lives (ADR-0047 §1). IndexedDB and the async OPFS API are
   structurally incompatible with a synchronous `&self` trait.

2. **LMDB cannot target wasm.** The production native backend is LMDB (via
   `heed` + the NMP `nostr-lmdb` fork), which needs `libc`/mmap and does not
   build for `wasm32-unknown-unknown`. The web target therefore needs a
   *different engine* behind the *same trait* — the same architectural shape
   (a feature-gated EventStore backend crate), not the same engine.

Boot hydration and the offline publish queue are *not* new mechanisms. ADR-0045
(Revision 2) already establishes one always-on store→projection replay seam for
every interest; the native `PublishStore`/`DomainPublishStore` already provide a
durable offline publish queue with per-relay retry deadlines. Both run
synchronously over any `EventStore`/`PublishStore` and need no wasm-specific
logic — they simply require a *durable* store underneath them on the web.

## Decision

### 1. Backend: official SQLite-WASM (sqlite.org) over an OPFS SyncAccessHandle pool VFS

The web `EventStore` backend is the **official SQLite-WASM build from
sqlite.org**, compiled with the **OPFS SyncAccessHandle pool VFS**, bridged
into Rust via a hand-written `js-sys`/`wasm-bindgen` shim. It is delivered as a
**vendored, pre-compiled `.wasm` + JS artifact** (public domain), not as a Rust
crate dependency. Consequences: it adds **zero external Rust crates**, it is
first-party and production-proven, and its OPFS SAH pool VFS is synchronous
after an async-once pool open — exactly matching the "async at Start, sync
thereafter" boundary the trait demands. The vendored blob is **not** treated as
"cargo-deny-invisible-therefore-free": because it is outside the Cargo graph it
is also outside the only automated supply-chain gate, so it carries a manual
provenance regime (see Risks, *Vendored artifact provenance*).

A maintained Rust wrapper crate over the same artifact (e.g. a `sqlite-wasm-rs`)
**may** be substituted at implementation time **only if** it (a) clears
`cargo-deny` under the `deny.toml:25-53` allow-list (MIT / Apache-2.0 / BSD
family), and (b) meets the maintenance bar (active, not single-commit
abandonware). It is an allowed ergonomic substitution, **not** the committed
dependency — the committed decision (hand-written bridge + vendored artifact)
stands regardless of what crates.io currently holds.

### 2. Backend lives in a new wasm32-only crate `nmp-sqlite-wasm`

The backend is a new workspace crate `crates/nmp-sqlite-wasm`, mirroring the
structure of `nmp-nostr-lmdb`. It is `#[cfg(target_arch = "wasm32")]`-gated and
wired into `nmp-store` behind a new `opfs-sqlite-backend` feature (parallel to
the existing `lmdb-backend` feature, `crates/nmp-store/Cargo.toml:18`). **The
optional dependency on `nmp-sqlite-wasm` is declared under
`[target.'cfg(target_arch="wasm32")'.dependencies]`** and referenced via `dep:`
in the feature — mirroring the wasm-only dependency block already used in
`crates/nmp-wasm/Cargo.toml:44` — so that native `cargo build/check
--all-features` never tries to compile the wasm-only crate. (`lmdb-backend`'s
plain-`[dependencies]` pattern is *not* copied: `heed` builds on every target;
`nmp-sqlite-wasm` does not.)

The schema mirrors the LMDB index layout: a primary `events` table plus
secondary index tables (`idx_author_kind`, `idx_kind_time`, `idx_kind_dtag`,
p-tag, e-tag, expiry/F-TTL, domain-namespace KV). Every `insert` runs inside
**one SQLite transaction** so the primary + all secondary writes commit
atomically — SQLite's own journal/WAL provides the atomicity the trait's
`insert` contract requires (OPFS SAH alone offers no multi-write atomicity).
`query_visit` is served by an ordered-index `ORDER BY (author, kind,
created_at) DESC LIMIT ?` query, giving O(log n) seek + O(limit) step.

### 3. Send+Sync boundary: `RefCell` interior mutability + a single scoped `unsafe impl`

The core trait bound is **never** touched. The wasm backend obtains
`Send + Sync` via a tightly scoped `unsafe impl`, and obtains interior
mutability via `RefCell` — **not** `Mutex`.

```rust
// crates/nmp-sqlite-wasm/src/lib.rs (wasm32 only)

// The Send+Sync invariant is actor-ownership, not "wasm has no threads":
// the store handle is owned by exactly one Web Worker actor. Enforce that the
// invariant cannot be silently broken by a future wasm-threads build.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
compile_error!(
    "OpfsSqliteStore's unsafe impl Send+Sync assumes a single-threaded Worker \
     actor owns the SQLite handle; wasm threads (atomics) would make \
     Arc<dyn EventStore> shareable across worker threads and the impl unsound."
);

pub struct OpfsSqliteStore {
    db: RefCell<SqliteConn>,   // !Send + !Sync handle, single-actor-bound
}

// SAFETY: This store is constructed inside, and only ever observed by, the
// single Web Worker event loop that opened its OPFS SyncAccessHandle pool
// (ADR-0047 §1: the Worker IS the actor; D4 single writer). No other thread
// ever obtains a reference to `db`; the compile_error above guarantees no
// wasm-threads build can introduce one. This impl is the ONLY unsafe in the
// crate and is forbidden anywhere outside it.
#[cfg(target_arch = "wasm32")]
unsafe impl Send for OpfsSqliteStore {}
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for OpfsSqliteStore {}
```

`Mutex` is **rejected**: it would buy real cross-thread synchronization (and
its overhead) for a single-threaded Worker where no second thread can ever
exist — false safety with a runtime cost. `RefCell` is the honest interior-
mutability primitive for a single-threaded owner; the `unsafe impl` documents
the single-Worker-actor invariant that makes the `Send + Sync` bound vacuously
true, and the `compile_error!` guard makes that invariant load-bearing rather
than incidental. The `unsafe impl` is `#[cfg(target_arch = "wasm32")]`, lives
**only** in `nmp-sqlite-wasm`, and never appears in `nmp-core` or `nmp-store`.

### 4. Store injection seam in `nmp-wasm` (Stage #5), backend-agnostic

The kernel's innermost constructor is split into a store-agnostic
`Kernel::from_parts(...)` plus a thin path-based wrapper that preserves the
native path (`build_event_store` in
`crates/nmp-core/src/kernel/store_init.rs:41`). `KernelReducer` gains a
`with_store(Arc<dyn EventStore>)` constructor
(`crates/nmp-core/src/kernel_reducer.rs:72`). The hidden raw wasm ABI adapter gains an
`injected_store: Rc<RefCell<Option<Arc<dyn EventStore>>>>` slot
(`crates/nmp-wasm/src/runtime.rs:65,108`) and a `set_injected_store()` setter.
At the **top** of the raw wasm ABI adapter's `start()` path — before relay drivers and the tick
loop capture `Rc` clones of the reducer (`runtime.rs:264-271`) and before
`reduce(Start)` (`runtime.rs:229`) — if a store was injected, the reducer cell
is rebuilt:

```rust
if let Some(store) = self.injected_store.borrow_mut().take() {
    *self.reducer.borrow_mut() = KernelReducer::with_store(store);
}
```

Because the swap mutates the existing `Rc<RefCell<KernelReducer>>` in place, any
handle taken via `reducer_handle()` (`runtime.rs:136`) before Start keeps
pointing at the same cell — now holding the store-injected reducer. This
sidesteps the stale-captured-clone trap (the watermark_fn / PublishStore /
PublishEngine clones captured in `Kernel`'s constructor). Stage #5 is **pure
plumbing**: with no injected store the default `MemEventStore` path is byte-for-
byte unchanged, and the seam lands and is tested with `MemEventStore` *before*
the OPFS backend (#6) exists.

App composition that captures `event_store_handle()` must run through
the raw wasm ABI adapter's pre-start hook. The hook runs after the injected store has
rebuilt the reducer and before relay drivers, publish routing, observers, or
typed projections capture handles. A composition root that registers feed
engines at constructor time would keep reading the original `MemEventStore`
after OPFS injection, so constructor-time store capture is forbidden.

### 5. Async OPFS open happens in the composition root, once, before Start

The OPFS SAH pool is opened **async exactly once** in the composition root's
async init hook (the wasm binding layer, to be re-established under the new
browser-runtime architecture — see #2052), using `database_name` from
`StartConfig` as the file key. The opened `OpfsSqliteStore` is installed via
`set_injected_store()`; then `start()` rebuilds the reducer (§4). The handle is
**never re-acquired mid-session** unless the user explicitly clears it (reload,
sign-out). This keeps the async boundary at Start and the kernel path fully
synchronous thereafter.

### 6. Degraded mode mirrors the existing `store_open_failure` contract (D6)

The web open-failure path mirrors `build_event_store`'s native contract
(`store_init.rs:41` doc): when OPFS SAH is unavailable (Safari < 17.4, private
browsing, quota denial, handle loss, **or a second tab cannot acquire the
exclusive pool lock** — see Risks, *Multi-tab*), the composition root **falls
back to `MemEventStore`** and surfaces a stable reason on
`Kernel::store_open_failure`, which flows out through the normal Tier-3 snapshot
channel (ADR-0044). It **never panics across the FFI seam** (D6) and never
synthesizes product state to hide the degradation (ADR-0047 §4). The JS host
renders an honest degraded-mode banner from the snapshot field. Mid-session
write failures (e.g. quota exhaustion on `insert`) must be mapped to the same
degraded snapshot surface, never an FFI panic — see Risks, *Mid-session write
failure*.

### 7. Boot hydration (#7) and offline publish (#8) reuse existing mechanisms unchanged

- **Hydration (#7):** the ADR-0045 Rev-2 always-on store→projection replay seam
  runs unchanged once a durable store is plugged in. Every opened interest is
  served from the local store first via the post-store projection-dispatch path
  (`insert_timeline_id_sorted` + `events` read-cache + `notify_event_observers`,
  and `IngestParser` for kind:1059 gift-wraps), **never** `store.insert`. Offline
  render is the degenerate case where the network half delivers nothing. No
  wasm-specific hydration path.
- **Offline publish (#8):** the native `PublishStore`/`DomainPublishStore` queue
  (`crates/nmp-core/src/publish/store.rs`) runs over the durable store. At boot,
  immediately after Start and before the first snapshot, the engine's
  `resume_from_store(now_ms)` (`publish/engine/dispatch.rs`) restores all
  non-terminal `PublishRecord` rows and **preserves per-relay retry deadlines**
  across restart (no thundering herd). No new mechanism.

### 8. Test infrastructure: a Worker-execution conformance vehicle

Stage #6 must validate the backend **byte-for-byte** against the existing
`MemEventStore`/`LmdbEventStore` conformance paths through the `nmp-testing`
harness. **`wasm_bindgen_test_configure!(run_in_browser)` cannot be used for
this**: it executes on the page main thread
(`crates/nmp-wasm/tests/wasm_boot.rs:36`) where `createSyncAccessHandle` does
not exist (OPFS SAH is dedicated-Worker-only — Context fact 1). The conformance
gate therefore runs the backend **inside a real dedicated Worker**, driven by
the Playwright harness in the browser-runtime composition layer
(see #2052/#2038), which spawns the nmp-wasm Worker and reports
results back to the test runner; a bespoke Worker test runner is the only
alternative. This vehicle must be scoped and stood up *before* #6 begins — it is
the sole mitigation for #6's HIGH risk.

### 9. Degraded-open reason taxonomy + multi-tab decision (PR-8, shipped)

§6's contract is realised in the browser-runtime composition root (#1007 PR-8).
The async pre-`Start` hook (`NmpWasmRuntime::prepare_store`,
`crates/nmp-browser-runtime/src/wasm/mod.rs`) classifies any
`OpfsSqliteEventStore::open` failure into a **single, stable reason string** —
the taxonomy lives in one place, `crates/nmp-browser-runtime/src/wasm/store_failure.rs`:

| Reason | Cause |
|---|---|
| `opfs_store_open_failure: safari_or_sah_pool_unavailable` | Safari < 17.4 / no `createSyncAccessHandle` / sahpool VFS missing |
| `opfs_store_open_failure: private_browsing` | OPFS blocked by a `SecurityError` in a private window |
| `opfs_store_open_failure: quota_denied` | Origin storage quota exhausted at pool pre-allocation |
| `opfs_store_open_failure: handle_loss` | A `SyncAccessHandle` was lost / invalidated (`InvalidStateError`) |
| `opfs_store_open_failure: second_tab_pool_lock` | Another tab holds the exclusive sahpool for this `database_name` |
| `opfs_store_open_failure: unknown` | Open failed outside the known taxonomy (never silently dropped) |

The reason is parked on the runtime core and threaded at `Start` through
`BrowserAppBuilder::with_store_open_failure` →
`KernelReducer::set_store_open_failure` → `Kernel::store_open_failure`, i.e. the
**same** Tier-3 snapshot field the native LMDB degraded-open path
(`build_event_store`) writes. The wrapper collapses the engine's
`SqliteWasmError` into `StoreError::Io(domexception_text)`, so classification is
case-insensitive substring matching on the JS `DOMException` text (D6: engine
text only, no private content).

**Multi-tab decision (resolves the *Multi-tab correctness* risk):** a second tab
on the same `database_name` is an **explicit ephemeral tier**, not an error. The
sahpool is exclusive per origin+name; the second opener's `open` fails, is
classified `second_tab_pool_lock`, and the tab predictably degrades to an
in-memory `MemEventStore` with that reason surfaced through the snapshot (honest
banner via §6). Full Web-Locks single-durable-tab *arbitration* (electing one
lock-holder that runs the durable store while others knowingly run ephemeral) is
a **documented follow-up** — the predictable degrade above is the shipped
behaviour and is correct regardless of which arbitration we later add.

**Mid-session quota (resolves the *Mid-session write-failure* risk):** a quota
exhaustion on `insert` surfaces as `StoreError::Io` and is handled at the kernel
ingest chokepoint (`kernel/ingest/persistence.rs`, the `Err(e) => self.log(…);
None` arm) — the event is logged and dropped, **never** an FFI/Worker panic,
exactly mirroring the native LMDB `MDB_MAP_FULL` analog. The browser pump
(`map_pump_events`, `pump_and_push_snapshot` with `try_borrow_mut`) carries no
panic path either.

## Consequences

- **The honest web persistence claim (#1007 under #2045) becomes reachable**
  once #5–#9 land: second-launch render, offline-first reads, and durable
  offline publish all work on the web with the same code paths as native (with
  the scoping caveats in Risks, *Relay-author-score durability* and *Multi-tab*).
- **One new crate, zero new Rust deps.** `crates/nmp-sqlite-wasm` is added; the
  SQLite-WASM artifact is vendored (public domain) under a manual provenance
  regime. A `Cargo.lock` entry appears only if a maintained wrapper crate is
  later substituted per §1 — gated on `deny.toml` compliance.
- **`nmp-store` gains an `opfs-sqlite-backend` feature**, off by default, wasm32-
  only, with the optional dep behind a `cfg(target_arch="wasm32")` target table
  so native `--all-features` builds stay green. Native builds never compile it;
  wasm builds never compile LMDB.
- **Exactly one `unsafe impl Send + Sync`** in the whole persistence stack,
  scoped to `nmp-sqlite-wasm`, justified by single-Worker-actor ownership, and
  guarded by a `target_feature="atomics"` `compile_error!`. The core
  `EventStore: Send + Sync` bound is untouched.
- **Stage #5 ships before the hard work.** The injection seam lands and is
  tested against `MemEventStore` with zero behavioral change, isolating the
  novel OPFS-SQLite risk (#6) behind a proven seam. **Stage #5 is the accepted
  scope of this ADR; #6–#9 are proposed and gated on the open items below.**
- **No ADR amendments needed** for hydration or publish: ADR-0045 §2/§6 and the
  `PublishStore` trait already define the wasm contract.

## Risks and implementation gates

Each item folds in a finding from design review. Severity and the stage it
gates are noted. Items marked *blocking #6* must be resolved before Stage #6
implementation starts.

- **Worker-context test vehicle (HIGH, blocking #6).** The §8 conformance gate
  is the only mitigation for #6's novel risk, and `run_in_browser` cannot reach
  OPFS SAH (main-thread). *Resolution:* §8 commits to a Playwright-driven Worker
  vehicle (or a bespoke Worker runner). Do not start #6 until this is stood up.

- **Native-compile landmine for the feature (MED, blocking #6).** A wasm-only
  optional dep declared in plain `[dependencies]` would break native
  `cargo check --all-features` (`supply-chain.yml:70` exercises `--all-features`
  for cargo-deny graph-only, but other native jobs invoke `rustc`). *Resolution:*
  §2 declares the dep under `[target.'cfg(target_arch="wasm32")'.dependencies]`
  with `dep:` in the feature; add a CI assertion that no native job enables
  `opfs-sqlite-backend`.

- **Vendored artifact provenance (MED, blocking #6).** "cargo-deny-invisible" is
  a governance *hole*, not a benefit: an unscanned prebuilt blob bypasses the
  only automated supply-chain gate. *Resolution:* pin the exact sqlite.org
  release version + source SHA, commit a SHA-256 of the vendored `.wasm`+JS, add
  a CI step that re-verifies the hash, document a reproducible rebuild-from-source
  procedure, and review the blob manually as a first-class dependency. The
  claimed public-domain license is asserted, not machine-verified — record the
  exact license string at vendoring time.

- **Send+Sync enforcement (MED, addressed in §3).** The justification is
  single-Worker-actor ownership, *not* "wasm32 has no threads" (true only for the
  current non-atomics build; wasm threads would make the impl genuinely unsound).
  *Resolution:* §3 ships the `#[cfg(target_feature="atomics")] compile_error!`
  guard so enabling wasm-threads later cannot silently break the invariant.

- **Multi-tab correctness (MED, blocking #6 — shapes the open path).** The OPFS
  SAH pool VFS holds an exclusive lock; a second tab opening the same
  `database_name` cannot acquire the handle and would silently fall back to an
  ephemeral `MemEventStore`, losing its writes (including the #8 publish queue)
  on close. *Resolution (decide before #6):* either elect a single durable
  tab via the Web Locks API (only the lock-holder runs the durable store), or
  explicitly enumerate "tab-2 is ephemeral" as an accepted degraded tier in #9's
  `docs/wasm-surface.md` contract with an honest banner via §6. The exact
  `opfs-sahpool` behavior on a concurrent second opener (throw vs block vs
  degrade) is unverified from the repo and needs a browser test.
  *Resolved in PR-8 (§9):* the explicit-ephemeral-tier option is chosen — the
  second tab degrades predictably to in-memory with reason `second_tab_pool_lock`
  on the Tier-3 snapshot. Web-Locks single-durable-tab arbitration remains a
  documented follow-up. (A browser test confirming the exact sahpool throw/block
  behaviour is still owed once the Worker conformance vehicle lands.)

- **Performance is behavioral parity, not perf parity (LOW, addressed in §2/§8).**
  `EventIter: Send` (`events.rs:22-23`) forces every scan to materialize owned
  results (no borrowing a live SQLite cursor), and each visited row is
  deserialized from a SQLite blob into a `StoredEvent` before the visitor runs —
  unlike `MemEventStore` (live values) and LMDB (near-zero-copy mmap'd
  flatbuffers). *Resolution:* the conformance harness target is **result/behavior
  parity** (byte-for-byte query results, dedup, gc, F-TTL semantics), explicitly
  **not** allocation or throughput parity. The §2 complexity claims are
  index-seek complexity only; no zero-alloc claim is made for this backend.

- **Relay-author-score durability out of scope (LOW, addressed in #9 wording).**
  Stages #5/#7 inject only `Arc<dyn EventStore>`; the native composition root
  also wires a separate `LmdbRelayAuthorScoreStore` via
  `kernel.set_relay_score_store` (`kernel/mod.rs:2039-2040`). The wasm path
  leaves relay-author scoring non-durable. This is **not a regression** (wasm is
  already `None` today), but #1007/#2045 closure wording must state that
  relay-author-score durability is out of scope for the web target, so "same code
  paths as native" does not imply score-store parity.

- **Asset-pipeline wiring (LOW, scoped into #6).** The browser-runtime build runs
  `wasm-pack build --target web` into a public assets directory served by the
  browser-runtime composition root (see #2052/#2038). The vendored sqlite3 `.wasm`+JS
  must also be placed in that directory, imported by the hand-written shim
  (`#[wasm_bindgen(module = ...)]`), and instantiated async-once. *Resolution:*
  add a #6 sub-task for vendoring the assets, wiring the shim import URL, and
  verifying async-once pool init under `--target web`. Confirm the SAH-pool
  variant needs no COOP/COEP cross-origin isolation (unlike `SharedArrayBuffer`).

- **Mid-session write-failure surfacing (LOW–MED, addressed in §6).** §6's
  fallback covers open time; `store.insert` returning `Err` on quota exhaustion
  mid-session must also map to the degraded snapshot surface and never panic
  across the FFI seam (D6), mirroring the native LMDB `MDB_MAP_FULL` analog. The
  native map-full path was not traced and must be verified during #6.
  *Resolved in PR-8 (§9):* traced — the kernel ingest chokepoint
  (`kernel/ingest/persistence.rs`) handles an `insert` `Err` with `self.log(…);
  None` (log + drop, no panic); the browser pump has no panic path. Quota mid-session
  is therefore non-fatal end to end.

## Alternatives considered

**`rusqlite` on wasm32.** Rejected. `rusqlite` requires `libc` and cannot target
`wasm32-unknown-unknown`.

**`sql.js`.** Rejected. In-memory only; no durable OPFS backing — fails the
second-launch / offline-render requirement outright.

**`wa-sqlite`.** Rejected as the committed choice. Third-party, single-maintainer,
less optimized than the first-party sqlite.org build, and not obviously cargo-deny
clean. The official artifact is the safer, doctrine-aligned default.

**IndexedDB (async) behind an async-at-boot cache.** Rejected. IndexedDB is
fundamentally async; backing a synchronous `&self` trait with it would require
either relaxing the trait to async (forbidden — relaxes the core bound, breaks
native) or a hidden async-to-sync bridge (impossible on a single-threaded Worker
without blocking the event loop). OPFS SyncAccessHandle is the only synchronous
browser primitive.

**`Mutex` for interior mutability.** Rejected. A single-threaded Worker has no
second thread to synchronize against; `Mutex` adds overhead and false thread-
safety. `RefCell` + a documented scoped `unsafe impl` (with the atomics
`compile_error!` guard) is the honest expression of the single-owner invariant.

**Relax `EventStore` to async / `?Send` / `cfg`-gated bound on wasm.** Rejected
categorically. This is the ZERO TECH DEBT line. The trait is the byte-for-byte
contract shared with the native LMDB backend; relaxing it on wasm forks the
contract and poisons `nmp-core`. The constraint is absorbed entirely inside the
backend crate (sync SQLite over OPFS SAH + one scoped `unsafe impl`), never
pushed up into the trait.

**Port the native threaded actor / its store-open path to wasm.** Rejected
(consistent with ADR-0047): the native actor and its blocking LMDB open depend on
`feature = "native"` primitives absent on wasm32. The `KernelReducer` + injection
seam is the correct substrate.
