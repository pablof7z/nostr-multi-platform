# Architecture Review Queue

Issues found by Opus architectural review agents (2026-05-20). Work through in priority order.
Status: `[ ]` pending · `[~]` in-progress · `[x]` done

---

## CRITICAL — UB / crash risk

- [ ] **`catch_unwind` on all C-callback fanout** — `crates/nmp-core/src/ffi/event_observer.rs:182`, `raw_event_tap.rs:247`, `lifecycle.rs:84`. Every `(registration.callback)(ctx, ptr)` invocation is a foreign call that can unwind across FFI = UB. Wrap each in `catch_unwind`; on panic emit a toast, do not propagate. The actor-thread `catch_unwind` in `ffi/mod.rs:240` does NOT cover these.
- [x] D6: `expect()` at `ViewModule` boundary — `nmp-nip01/src/view.rs` (fixed `0483fe28`)
- [x] D6: `expect()` at grouper public API — `nmp-threading/src/grouper.rs` (fixed `44223c5b`)
- [x] D6: `unwrap()` in marmot FFI + gift-wrap policy in wrong layer — `marmot/ffi.rs` (fixed `7150202c`)

---

## HIGH — reliability / correctness

- [ ] **Observer fanout blocks actor thread** — `event_observer.rs` and `raw_event_tap.rs` invoke callbacks synchronously on the actor thread. A slow or blocking Swift callback stalls relay ingest. Post events to a bounded channel and return immediately (same pattern as `update_listener` thread in `ffi/mod.rs:267-277`).
- [ ] **`unsafe impl Send/Sync` false justification** — `apps/chirp/nmp-app-chirp/src/ffi.rs:47-52` and `marmot/ffi.rs:118-120` claim Swift serializes FFI calls on one thread — it does not (`KernelHandle` is a plain `final class`, no queue). The retained `*mut NmpApp` is a UAF if Swift calls `nmp_app_free` before `_unregister`. Fix: make `nmp_app_free` invalidate all registered projections, or use a generational handle.
- [ ] **`Kernel: !Send`** — the single-writer invariant is convention, not type-enforced. Add `PhantomData<*const ()>` to `Kernel` so sharing it cross-thread is a compile error. ~10 lines in `kernel/mod.rs`.
- [ ] **Unbounded channels + stale backpressure docs** — `ffi/mod.rs:204-205` creates unbounded `mpsc::channel()`. Docs still describe a removed `BOUNDED_ACTOR_CMD_CAPACITY=4096` with drop policy. `dispatch_drops` counter is structurally always zero. Make `relay_rx` bounded with a coalesce/drop policy; delete dead counter and docs.
- [ ] **Per-message `catch_unwind` in actor** — a panic processing one bad relay frame kills the entire kernel (all views, caches, in-flight publishes). Wrap the per-message dispatch in `catch_unwind`; log + emit toast; continue the loop. The thread-level `catch_unwind` in `ffi/mod.rs:240` is not enough.
- [ ] **ADR-0009 violation: `nmp-core` → `nmp-nwc`** — `actor/commands/wallet.rs:24-27` imports `nmp-nwc` types directly. ADR-0009 classifies `nmp-nwc` as Layer 2. Fix: extract a `WalletCapability` trait in the kernel; invert the dependency so `nmp-nwc` implements it. Or reclassify in the ADR.

---

## MEDIUM — architectural hygiene

- [ ] **`[workspace.dependencies]`** — `nostr = "0.44"`, `serde`, `rustls`, etc. are copy-pasted across ~12 manifests. One drift = duplicate-version build. Hoist all shared third-party deps into `[workspace.dependencies]`; use `dep.workspace = true` per crate.
- [ ] **`ChirpCapabilities.swift` JSON substring matching** — `:50,61` use `envelope.resultJSON.contains("\"status\":\"ok\"")`. Brittle; will false-positive if any payload contains that literal. Use `JSONDecoder` like the rest of the file. Also: `retrieveSecret` collapses 3-state `KeyringResult` (Ok/NotFound/Error) to `String?` — caller can't distinguish missing from failed.
- [ ] **`nmp-substrate-types` dead directory** — `crates/nmp-substrate-types/src/` exists with PD-029 content but no `Cargo.toml`, not a workspace member, referenced nowhere. Finish the refactor or delete the directory.
- [ ] **`make_update` double-serializes** — `kernel/update.rs` calls `serde_json::to_string` twice per tick (once to measure `payload_bytes`, once for the actual payload). Cache or restructure.
- [ ] **`nmp-android-ffi` outside workspace** — has its own `[workspace]` table; invisible to root-workspace CI and `cargo build --workspace`. Add to CI explicitly.
- [ ] **No `cargo build`/`cargo test` CI** — only doctrine-lint, file-size, supply-chain gates. `lmdb-backend` feature may not compile; no automated proof. Add a matrix CI workflow.
- [ ] **`nmp-nip51` zero reverse-deps** — built and tested but consumed by no crate or app. Wire into something or remove from workspace.
- [ ] **`nmp-reactions` misnamed** — doubles as the cross-NIP composition layer (`nmp-relations` responsibility). Misleads future readers.
- [ ] **`NmpCore.h` drift** — hand-maintained C header with no CI gate against exported symbols. Add `nm`-diff CI check.

---

## DESIGN DEBT — requires explicit decision

- [ ] **Module dispatch seam (NMP-145)** — `substrate::ModuleRegistry` stores only `{ namespace, family, type_name }` strings. No dispatch closures, no `Box<dyn>`. `FfiApp::dispatch` in codegen hard-rejects all module actions with `UriRejected`. Every kind handler is hardcoded in `kernel/ingest/mod.rs`. Build the `kind→Vec<ModuleId>` dispatch table or mark `substrate` as pre-wiring in docs.
- [ ] **Codegen type naming contract** — codegen emits `<crate>::Action`/`::Update`/`::ViewSpec` but real modules export `RepliesSpec`, `ThreadSpec`, etc. A generated app crate for real modules would not compile. Either enforce the naming convention in module crates or have `nmp.toml` declare type paths.
- [ ] **LMDB key-space isolation** — `DomainModule::MigrationTx` exposes raw `put(key, value)` over a shared LMDB env. Keys are not prefixed by module namespace. A buggy module can corrupt another's records.
- [ ] **`ProjectionRegistry` abstraction** — multi-projection pattern (ChirpHandle + MarmotHandle) is copy-pasted. Won't scale past 2-3 projections. Build a `ProjectionRegistry` trait; fold into M14 UniFFI work.
- [ ] **Clock trait injection** — `SystemTime::now()`/`Instant::now()` called inside the reducer (`kernel/ingest/mod.rs`). Blocks deterministic replay despite `kernel/replay.rs` existing. Inject a `Clock` trait.
- [ ] **Two competing extension idioms** — `ViewModule` (aspirational, not kernel-driven) vs `KernelEventObserver` (actual shipping path, undocumented as the v1 extension model). Document which to use; deprecate or build the other.
- [ ] **`nmp-core` `AppState` is dead** — `app.rs:56` defines `AppState { rev, open_view_count }` that nothing in the actor reads or writes. Codegen-only artifact. Clarify or remove.

---

## DONE

- [x] D6 panics at public API boundaries — nmp-nip01, nmp-threading, marmot/ffi.rs (2026-05-20)
- [x] D7 gift-wrap subscription policy moved from marmot/ffi.rs → nmp-marmot::interest (2026-05-20)
