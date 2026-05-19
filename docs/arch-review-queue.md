# Architecture Review Queue

Issues found by Opus architectural review agents (2026-05-20). Work through in priority order.
Status: `[ ]` pending · `[~]` in-progress · `[x]` done

---

## CRITICAL — UB / crash risk

- [x] **`catch_unwind` on all C-callback fanout** — `crates/nmp-core/src/ffi/event_observer.rs:182`, `raw_event_tap.rs:247`, `lifecycle.rs:84`. Every `(registration.callback)(ctx, ptr)` invocation is a foreign call that can unwind across FFI = UB. Wrap each in `catch_unwind`; on panic emit a toast, do not propagate. The actor-thread `catch_unwind` in `ffi/mod.rs:240` does NOT cover these. (fixed `7a2c556f`)
- [x] D6: `expect()` at `ViewModule` boundary — `nmp-nip01/src/view.rs` (fixed `0483fe28`)
- [x] D6: `expect()` at grouper public API — `nmp-threading/src/grouper.rs` (fixed `44223c5b`)
- [x] D6: `unwrap()` in marmot FFI + gift-wrap policy in wrong layer — `marmot/ffi.rs` (fixed `7150202c`)
- [ ] **NIP-46 signing blocks the actor thread for 45s** — `crates/nmp-core/src/actor/commands/identity.rs:208-217`, `sign_active` calls `handle.sign(unsigned).wait(REMOTE_SIGN_TIMEOUT)` with `REMOTE_SIGN_TIMEOUT=45s` — a hard `rx.recv_timeout` block on the actor thread. Any compose/follow/react with a NIP-46 signer freezes the entire kernel: relay ingest, subscription management, all UI updates. The `SignerOp::Pending`/`poll` infrastructure exists to avoid this (`op.rs:67-88`) but is not used. Fix: park the in-flight `SignerOp` in actor state and resolve on a later tick.
- [ ] **Atomic account-switch is dead code; production path lacks atomicity** — `crates/nmp-signers/src/identity/active_account_reactor/mod.rs` contains `bundle_for()` which produces the correct ordered atomic bundle `CloseAccountSubs → RebindPublishSigner → OpenAccountSubs → EmitFullState`. This subsystem is never invoked by `nmp-core`. The production path (`identity.rs:443-467` `IdentityRuntime::switch_active`) flips `active` first, then rebinds, without the close-before-rebind atomicity guarantee. The multi-account isolation tests give false confidence — they test the dead code path. Fix: wire the actor through `ActiveAccountReactor`/`bundle_for`, or port its atomicity guarantee into `IdentityRuntime`.
- [ ] **Silenced memory-safety crash at nsec FFI boundary** — `ios/Chirp/Chirp/Bridge/KernelModel.swift:109-110`: keychain restore is disabled with comment "was triggering crash in parse_secret (stored nsec causes invalid &str at 0x800000000000000c in actor thread)". A use-after-free / bad-pointer at the `*const c_char` → `CStr` → `&str` FFI seam. Not fixed — disabled. Cold relaunch loses every local session. Fix: root-cause the lifetime bug in `nmp_app_signin_nsec`; do not ship with restore disabled.

---

## HIGH — reliability / correctness

- [ ] **Observer fanout blocks actor thread** — `event_observer.rs` and `raw_event_tap.rs` invoke callbacks synchronously on the actor thread. A slow or blocking Swift callback stalls relay ingest. Post events to a bounded channel and return immediately (same pattern as `update_listener` thread in `ffi/mod.rs:267-277`).
- [ ] **`unsafe impl Send/Sync` false justification** — `apps/chirp/nmp-app-chirp/src/ffi.rs:47-52` and `marmot/ffi.rs:118-120` claim Swift serializes FFI calls on one thread — it does not (`KernelHandle` is a plain `final class`, no queue). The retained `*mut NmpApp` is a UAF if Swift calls `nmp_app_free` before `_unregister`. Fix: make `nmp_app_free` invalidate all registered projections, or use a generational handle.
- [ ] **`Kernel: !Send`** — the single-writer invariant is convention, not type-enforced. Add `PhantomData<*const ()>` to `Kernel` so sharing it cross-thread is a compile error. ~10 lines in `kernel/mod.rs`.
- [ ] **Unbounded channels + stale backpressure docs** — `ffi/mod.rs:204-205` creates unbounded `mpsc::channel()`. Docs still describe a removed `BOUNDED_ACTOR_CMD_CAPACITY=4096` with drop policy. `dispatch_drops` counter is structurally always zero. Make `relay_rx` bounded with a coalesce/drop policy; delete dead counter and docs.
- [ ] **Per-message `catch_unwind` in actor** — a panic processing one bad relay frame kills the entire kernel (all views, caches, in-flight publishes). Wrap the per-message dispatch in `catch_unwind`; log + emit toast; continue the loop. The thread-level `catch_unwind` in `ffi/mod.rs:240` is not enough.
- [ ] **ADR-0009 violation: `nmp-core` → `nmp-nwc`** — `actor/commands/wallet.rs:24-27` imports `nmp-nwc` types directly. ADR-0009 classifies `nmp-nwc` as Layer 2. Fix: extract a `WalletCapability` trait in the kernel; invert the dependency so `nmp-nwc` implements it. Or reclassify in the ADR.
- [ ] **No zeroization of key material** — `crates/nmp-signers/src/signers/local.rs:20-31`: `LocalKeySigner.keys: Keys` and `.password: Option<String>` have no zeroizing `Drop`. `local.rs:123-125` `secret_hex()` allocates a plaintext hex `String` the caller owns. `crates/nmp-core/src/ffi/mod.rs:178` `active_local_nsec: Arc<Mutex<Option<String>>>` holds the nsec process-global as plaintext. `payload.rs:41-45` `LocalKeyMaterial::Raw(String)` and `Nip46Payload.local_secret_hex` are in `Clone`/`Serialize` structs. Secrets persist in freed memory — recoverable via memory dump, core dump, crash report, or swap. Fix: wrap all secret material in `zeroize::Zeroizing`; implement `ZeroizeOnDrop` on signer types.
- [ ] **`SignerPayload` derives `Debug` exposing raw secrets** — `crates/nmp-signers/src/signers/payload.rs:16,29,39-46`: `SignerPayload`, `LocalPayload`, `LocalKeyMaterial` all `#[derive(Debug)]` while `LocalKeyMaterial::Raw(String)` and `Nip46Payload.local_secret_hex` hold raw secret hex. `LocalKeySigner` has a hand-written redacting `Debug` (`local.rs:33-41`) — but `to_payload()` produces a `SignerPayload` whose derived `Debug`/`Serialize` prints the key verbatim. Any `{:?}` log, panic message, or diagnostic dump leaks the private key. Fix: hand-write `Debug` for all payload types to redact secrets.
- [ ] **nsec crosses FFI as plaintext C string** — `ios/Chirp/Chirp/Bridge/KernelModel.swift:191-199`: `signInNsec` holds the nsec in `cachedSecretKey: String?` (line 68), passes it across the C ABI into `nmp_app_signin_nsec`. The `active_local_nsec` doc in `ffi/mod.rs:172-178` claims "secret never crosses FFI" — this is false for every imported account. Fix: import path should write to keychain from Swift, then kernel reads via capability — nsec never enters a kernel FFI argument.
- [ ] **Silent tag drop in `LocalKeySigner::sign_now`** — `local.rs:140-144`: `unsigned.tags.iter().filter_map(|t| Tag::parse(t).ok()).collect()` silently drops malformed tags. The actor's `sign_with` (`identity.rs:162-172`) hard-fails on bad tags. The signed event can silently differ from intent — for security-relevant tags (auth, expiration, delegation) this is a correctness-becomes-security defect. Fix: make `sign_now` hard-fail on tag parse failure, matching `sign_with`.
- [ ] **Observer fanout blocks actor thread** — `crates/nmp-core/src/ffi/event_observer.rs` and `raw_event_tap.rs` invoke callbacks synchronously on the actor thread. Even after the `catch_unwind` fix, a slow Swift callback stalls relay ingest. Post events to a bounded channel; return immediately (same pattern as `update_listener` thread in `ffi/mod.rs:267-277`).

---

## MEDIUM — architectural hygiene

- [x] **`[workspace.dependencies]`** — `nostr = "0.44"`, `serde`, `rustls`, etc. are copy-pasted across ~12 manifests. One drift = duplicate-version build. Hoist all shared third-party deps into `[workspace.dependencies]`; use `dep.workspace = true` per crate. (fixed `0d8a1b44`)
- [ ] **`ChirpCapabilities.swift` JSON substring matching** — `:50,61` use `envelope.resultJSON.contains("\"status\":\"ok\"")`. Brittle; will false-positive if any payload contains that literal. Use `JSONDecoder` like the rest of the file. Also: `retrieveSecret` collapses 3-state `KeyringResult` (Ok/NotFound/Error) to `String?` — caller can't distinguish missing from failed.
- [ ] **`nmp-substrate-types` dead directory** — `crates/nmp-substrate-types/src/` exists with PD-029 content but no `Cargo.toml`, not a workspace member, referenced nowhere. Finish the refactor or delete the directory.
- [ ] **`make_update` double-serializes** — `kernel/update.rs` calls `serde_json::to_string` twice per tick (once to measure `payload_bytes`, once for the actual payload). Cache or restructure.
- [ ] **`nmp-android-ffi` outside workspace** — has its own `[workspace]` table; invisible to root-workspace CI and `cargo build --workspace`. Add to CI explicitly.
- [ ] **No `cargo build`/`cargo test` CI** — only doctrine-lint, file-size, supply-chain gates. `lmdb-backend` feature may not compile; no automated proof. Add a matrix CI workflow.
- [ ] **`nmp-nip51` zero reverse-deps** — built and tested but consumed by no crate or app. Wire into something or remove from workspace.
- [ ] **`nmp-reactions` misnamed** — doubles as the cross-NIP composition layer (`nmp-relations` responsibility). Misleads future readers.
- [ ] **`NmpCore.h` drift** — hand-maintained C header with no CI gate against exported symbols. Add `nm`-diff CI check.
- [ ] **`NoopRelay::send` silently drops frames** — `crates/nmp-signer-broker/src/broker.rs:537-545`: during handshake, a `NoopRelay` is staged before the real relay is installed. `NoopRelay::send` returns `Ok(())` while dropping the frame — a dropped sign request reported as success. Fix: return `RelayError::Disconnected` so premature sends surface as errors.
- [ ] **nostrconnect session secret derived from ephemeral key bytes** — `broker.rs:128-133`: connection secret is first 8 bytes of the ephemeral private key, hex-encoded. 64 bits of a 256-bit private key are published in the `nostrconnect://` URI and QR code. Fix: derive session secret from an independent CSPRNG draw.
- [ ] **NIP-46 RPC pending queue never cleaned up on disconnect** — `nmp-signer-broker/src/transport.rs`: `reconnect_hint` is a no-op; dropped bunker relay is terminal. `Nip46Signer.pending` entries for abandoned requests are never resolved — accumulate for signer lifetime. Fix: on disconnect, drain `pending` and resolve all entries with `SignerError::Backend("bunker disconnected")`; implement auto-reconnect.
- [ ] **Subscription planner merge produces cross-product** — `crates/nmp-core/src/planner/lattice/mod.rs:113-125`: merging two filters with different authors + different tags produces a superset filter (cross product on wire). Wire over-delivery: extra bandwidth, extra Schnorr-verify CPU, store pollution. The lattice doc should explicitly state merge yields a superset. P2 — not urgent but worth documenting.
- [ ] **Mem backend has zero indexes — all O(N) scans** — `crates/nmp-core/src/store/mem/query.rs:36-53`: all six scan functions are full table scans + O(N log N) sort regardless of limit. `store/mem/insert.rs:189-207`: replaceable-event supersession is O(N) per insert. The trait advertises named indexes; the Mem backend has none. Document this loudly in the module header so users don't hit the cliff.
- [ ] **Claim GC has no RAII guard — leaks possible** — `crates/nmp-core/src/store/mem/gc.rs`: a module that errors between `claim()` and `release()` leaks a claim permanently (`ClaimerId` is monotonic, never reused). No `ClaimGuard` RAII type exists. Fix: add `ClaimGuard` that calls `release` on `Drop`.
- [ ] **D2 (negentropy-first) enforced by call-site only** — `crates/nmp-nip77/src/planner_gate.rs:70`: nothing in the type system prevents a caller from emitting a `CompiledPlan` without passing through `apply_coverage_filter`. D2 is a convention with a helper, not a structural guarantee. Consider making `CompiledPlan` un-emittable until the coverage gate transforms it.

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

## STRATEGIC DIRECTION — requires owner decision

- [ ] **`ViewModule` has zero runtime callers — extension system is dead scaffolding** — `substrate/mod.rs:56-93` stores only name strings; no dispatch closures. Per-kind dispatch is hardcoded in `kernel/ingest/mod.rs:343-382`. 10+ `ViewModule` impls across protocol crates are never called by the kernel. The real shipping extension path is `KernelEventObserver` — undocumented and at odds with the builder guide. **Decision needed:** build the generic dispatch runtime, OR collapse to `KernelEventObserver` + composition crates as the documented v1 architecture. Do not keep both paths aspirationally alive.
- [ ] **NMP reimplemented the relay pool it promised to borrow** — `aim.md §3` says NMP depends on `nostr-sdk` for "relay pool management, subscription routing, async streaming." `nmp-core/Cargo.toml` depends on `nostr` (types) and `tungstenite` (raw WebSocket) — NOT `nostr-sdk`. `relay_worker/` is 1079 LOC reimplementing the SDK's transport layer. **Decision needed:** formally adopt the fork as intentional (write ADR) and stop claiming dependency on the SDK; or migrate back to the SDK. The current state — saying one thing, doing another — is invisible from inside.
- [x] **No `cargo test` in CI** — `.github/workflows/` has `doctrine-lint`, `file-size-gate`, `s2-retention-gate`, `supply-chain` — no build or test workflow. `lmdb-backend` feature may not compile; no automated proof anything works. Add a `cargo test` matrix CI workflow. (fixed `e847650e`)
- [ ] **Priority inversion: MLS shipped before write path** — `nmp-marmot` (OpenMLS encrypted groups, highest-complexity surface) is built and wired. Write path, LMDB persistence, and multi-account are still aspirational. Ship fundamentals before exotic features.

---

## DONE

- [x] D6 panics at public API boundaries — nmp-nip01, nmp-threading, marmot/ffi.rs (2026-05-20)
- [x] D7 gift-wrap subscription policy moved from marmot/ffi.rs → nmp-marmot::interest (2026-05-20)
- [x] Secret key zeroization (`Zeroizing<String>` on all key material) (2026-05-20)
- [x] `Debug` redaction on `SignerPayload`/`LocalKeyMaterial` — no more plaintext key in logs (2026-05-20)
- [x] `NoopRelay::send` returns error instead of silent Ok (2026-05-20)
- [x] Silent tag drop in `LocalKeySigner::sign_now` → hard-fail (2026-05-20)
- [x] Observer fanout decoupled from actor thread via bounded channel (2026-05-20)
- [x] `cargo test` CI workflow added — first build gate in the repo (2026-05-20)
- [x] `[workspace.dependencies]` hoisted to workspace root (2026-05-20)
- [x] `catch_unwind` on all C-callback fanout sites (2026-05-20)
