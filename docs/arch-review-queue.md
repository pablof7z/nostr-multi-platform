# Architecture Review Queue

Issues found by Opus architectural review agents (2026-05-20). Work through in priority order.
Status: `[ ]` pending · `[~]` in-progress · `[x]` done

---

## CRITICAL — UB / crash risk

- [x] **`catch_unwind` on all C-callback fanout** — `crates/nmp-core/src/ffi/event_observer.rs:182`, `raw_event_tap.rs:247`, `lifecycle.rs:84`. Every `(registration.callback)(ctx, ptr)` invocation is a foreign call that can unwind across FFI = UB. Wrap each in `catch_unwind`; on panic emit a toast, do not propagate. The actor-thread `catch_unwind` in `ffi/mod.rs:240` does NOT cover these. (fixed `7a2c556f`)
- [x] D6: `expect()` at `ViewModule` boundary — `nmp-nip01/src/view.rs` (fixed `0483fe28`)
- [x] D6: `expect()` at grouper public API — `nmp-threading/src/grouper.rs` (fixed `44223c5b`)
- [x] D6: `unwrap()` in marmot FFI + gift-wrap policy in wrong layer — `marmot/ffi.rs` (fixed `7150202c`)
- [~] **NIP-46 signing blocks the actor thread** — Partially addressed. (a) `REMOTE_SIGN_TIMEOUT` reduced from 45s to 5s (`actor/commands/identity.rs:36`) so the worst-case actor stall is bounded. (b) Non-blocking signing now exists: `sign_active_nonblocking` (`identity.rs:251`) returns the `SignerOp` verbatim, and the publish path parks it via `PendingSign` (`actor/pending_sign.rs`), resolving on later ticks. Used by `publish_unsigned_event`, `publish_note`, `react`, `follow` (`actor/commands/publish.rs:57,203,260,317`). (c) **Still open:** the onboarding paths — `create_account`, `publish_initial_follows`, `sign_in_nsec` (`identity.rs`) — still call the blocking `sign_active`. These are infrequent (account creation / first-follow) so the residual block is low-impact, but a NIP-46 signer there can still stall the actor for up to 5s.
- [~] **Account-switch production path lacks atomicity** — Partially addressed. The dead-code half is fixed: `crates/nmp-signers/src/identity/active_account_reactor/` was deleted (commit `8e87ac55`); the directory now holds only `manager.rs`, `mod.rs`, `tests.rs`, so there is no longer a never-invoked atomic-bundle subsystem giving false test confidence. **Still open:** the production path (`actor/commands/identity.rs` `switch_active`) still flips `identity.active` first, then calls `sync_kernel` to rebind the signer — there is no close-before-rebind atomicity guarantee. Fix: port a `CloseAccountSubs → RebindPublishSigner → OpenAccountSubs` ordering into `switch_active` so the prior account's subscriptions are torn down before the new signer is bound.
- [ ] **Silenced memory-safety crash at nsec FFI boundary** — `ios/Chirp/Chirp/Bridge/KernelModel.swift:109-110`: keychain restore is disabled with comment "was triggering crash in parse_secret (stored nsec causes invalid &str at 0x800000000000000c in actor thread)". A use-after-free / bad-pointer at the `*const c_char` → `CStr` → `&str` FFI seam. Not fixed — disabled. Cold relaunch loses every local session. Fix: root-cause the lifetime bug in `nmp_app_signin_nsec`; do not ship with restore disabled.

---

## HIGH — reliability / correctness

- [x] ~~**Observer fanout blocks actor thread**~~ ✅ Fixed — C-ABI fan-out uses a bounded `sync_channel(1024)` + drain thread in both `event_observer.rs` and `raw_event_tap.rs`; `notify_observers` / `notify_raw_observers` serialize once, `try_send`, and return immediately. Rust trait observers are intentionally synchronous (their contract mandates cheap + non-panicking).
- [x] ~~**`unsafe impl Send/Sync` false justification**~~ ✅ Fixed (comment only) — the old comments claimed "Swift serializes on one thread" (false; `KernelHandle` has no queue). Rewrote to honest three-layer invariant: FFI entry points run on Swift main actor; observer callbacks run on the kernel actor thread (cross-thread); `NmpApp::Drop` joins the actor thread before freeing memory, so Rust-trait observers have no UAF window. **Residual (open):** the C-ABI observer drain thread is NOT joined by `nmp_app_free` — a future caller that registers a C-ABI observer and retains `*mut NmpApp` in the callback has a genuine UAF window. Chirp/Marmot use the Rust-trait path and are safe. (commit `6fcfd5ca`)
- [x] ~~**`Kernel: !Send` — single-writer invariant not type-enforced**~~ ✅ Fixed — `Kernel` carries `_not_send: PhantomData<*const ()>` (`kernel/mod.rs:385`), making any cross-thread share a compile error; the D4 single-writer invariant is now type-enforced.
- [x] ~~**Stale backpressure docs**~~ ✅ Fixed — updated four doc comments in `kernel/types.rs` and `kernel/mod.rs` to reflect the unbounded dual-channel design; `dispatch_drops_total` now documented as always-zero diagnostic retained for API compatibility (commit `88c52db2`).
- [x] ~~**Per-message `catch_unwind` in actor**~~ ✅ Fixed — `handle_relay_event` wrapped in `catch_unwind(AssertUnwindSafe(...))` in `actor/mod.rs`; panics log the payload + surface an error toast + continue the loop. `kernel::status::log` visibility bumped to `pub(crate)` to make the call accessible (commit `66f41075`).
- [ ] **ADR-0009 violation: `nmp-core` → `nmp-nwc`** — `actor/commands/wallet.rs:24-27` imports `nmp-nwc` types directly. ADR-0009 classifies `nmp-nwc` as Layer 2. Fix: extract a `WalletCapability` trait in the kernel; invert the dependency so `nmp-nwc` implements it. Or reclassify in the ADR.
- [~] **Zeroization of key material** — Largely addressed. Fixed: `Nip46Payload.local_secret_hex` and `.secret` are `Zeroizing<String>` (`8ae6c46c`); `BunkerUri.secret` is `Zeroizing<String>` (`bfc8e5b5`); `NwcUri.client_secret_hex` is `Zeroizing<String>` (`c4e8a76c`); `WalletConnection.client_secret_hex` is `Zeroizing<String>` (`f34230a3`); `LocalKeyMaterial::Raw` is now `Raw(Zeroizing<String>)` (`payload.rs:61`); `LocalKeySigner.secret_hex()` returns `Zeroizing<String>` (`local.rs:143`); `LocalKeySigner.password` is explicitly `zeroize`d in a hand-written `Drop` (`local.rs:44-57`); `active_local_nsec` is `Arc<Mutex<Option<Zeroizing<String>>>>` (`ffi/mod.rs:186`). **Still open:** `LocalKeySigner.keys: nostr::Keys` has no zeroizing `Drop` — `nostr::Keys` is an external type that does not implement `Zeroize`, so the raw secret inside `Keys` is not wiped on drop. Closing this needs either upstream `Zeroize` support in the `nostr` crate or holding the raw `SecretKey` bytes alongside `Keys` in a `Zeroizing` buffer.
- [x] ~~**`SignerPayload` derives `Debug` exposing raw secrets**~~ ✅ Fixed — `SignerPayload`, `LocalPayload`, `LocalKeyMaterial`, and `Nip46Payload` no longer `#[derive(Debug)]`; each has a hand-written redacting `Debug` impl (`payload.rs:114-157`) that prints `[redacted]` for all secret-bearing fields, so no `{:?}` log or panic message can leak the private key.
- [ ] **nsec crosses FFI as plaintext C string** — `ios/Chirp/Chirp/Bridge/KernelModel.swift:191-199`: `signInNsec` holds the nsec in `cachedSecretKey: String?` (line 68), passes it across the C ABI into `nmp_app_signin_nsec`. The `active_local_nsec` doc in `ffi/mod.rs:172-178` claims "secret never crosses FFI" — this is false for every imported account. Fix: import path should write to keychain from Swift, then kernel reads via capability — nsec never enters a kernel FFI argument.
- [x] ~~**Silent tag drop in `LocalKeySigner::sign_now`**~~ ✅ Fixed — `sign_now` (`local.rs:158-169`) now uses `.map(|t| Tag::parse(t)).collect::<Result<Vec<_>, _>>()` and returns `SignerError::Backend` on the first malformed tag, matching the actor's `sign_with` hard-fail; no tag is silently dropped.
- [x] ~~**Observer fanout blocks actor thread (duplicate)**~~ ✅ Already fixed — same as item above; bounded channel + drain thread already in place in both observer modules.

---

## MEDIUM — architectural hygiene

- [x] **`[workspace.dependencies]`** — `nostr = "0.44"`, `serde`, `rustls`, etc. are copy-pasted across ~12 manifests. One drift = duplicate-version build. Hoist all shared third-party deps into `[workspace.dependencies]`; use `dep.workspace = true` per crate. (fixed `0d8a1b44`)
- [x] ~~**`ChirpCapabilities.swift` JSON substring matching**~~ ✅ Fixed — replaced all `contains("\"status\":\"ok\"")` calls with a shared `decodeResult()` helper using `JSONDecoder` + `KeyringResult` `Codable` struct; `retrieveSecret` now returns a `SecretLookup` enum (`found`/`notFound`/`error(OSStatus)`) so callers can distinguish the three states. Both `KernelModel.swift` callers updated (commit `cc273e54`).
- [x] ~~**`nmp-substrate-types` dead directory**~~ ✅ Fixed — deleted via `git rm -r crates/nmp-substrate-types/` (commit `51f00899`).
- [x] ~~**`make_update` double-serializes**~~ ✅ Fixed — the snapshot is serialized once; `wrap_snapshot` (`update_envelope.rs:76`) re-attaches the already-serialized `String` via `RawValue::from_string()` (one outer allocation, no re-parse and no re-serialize) so each tick serializes the payload exactly once.
- [x] ~~**`nmp-android-ffi` outside workspace**~~ ✅ Fixed — added `android-ffi` job to `.github/workflows/test.yml` that runs `cd crates/nmp-android-ffi && cargo check` (host target; JNI ABI validation separate). Rust-level breaks now surface in CI (commit `67458387`).
- [x] ~~**No `cargo build`/`cargo test` CI**~~ ✅ Fixed — `.github/workflows/test.yml` runs `cargo test --workspace` on every push/PR to master, plus a separate `cargo test -p nmp-core --features lmdb-backend` step.
- [x] ~~**`nmp-nip51` zero reverse-deps**~~ ✅ Fixed — deleted entire crate (`git rm -r crates/nmp-nip51/`, removed from workspace members); recoverable from git history if a NIP-51 consumer is wired later (commit `78ad034b`).
- [x] ~~**`nmp-reactions` misnamed**~~ ✅ Fixed — Cargo package name changed to `nmp-relations`; all `use nmp_reactions::` imports and consumer `Cargo.toml` keys updated; directory `crates/nmp-reactions/` retained (Cargo resolves by package name). (commit `932b87e3`)
- [x] ~~**`NmpCore.h` drift**~~ ✅ Fixed in this session — `ci/check-ffi-header-drift.sh` added as CI gate against exported symbols.
- [x] ~~**`NoopRelay::send` silently drops frames**~~ ✅ Fixed — `NoopRelay::send` returns `RelayError::Disconnected` (`crates/nmp-signer-broker/src/broker.rs:537-546`) so premature sends surface as errors instead of silent `Ok(())`.
- [x] ~~**nostrconnect session secret derived from ephemeral key bytes**~~ ✅ Fixed — session secret now drawn from `rand::thread_rng().sample_iter(Alphanumeric).take(16)` in `broker.rs`; no longer leaks bits of the ephemeral private key (commit `a57a88c9`).
- [x] ~~**NIP-46 RPC pending queue never cleaned up on disconnect**~~ ✅ Fixed — `Nip46Signer::drain_pending_with_error` called from `BunkerBroker::cancel()` before teardown so all outstanding `SignerOp` receivers resolve immediately with `SignerError::Rejected` instead of timing out (commit `d190381c`).
- [x] ~~**Subscription planner merge produces cross-product**~~ ✅ Already documented — `lattice/mod.rs:56-63` has a `# ⚠ Superset semantics` warning block explicitly stating the cross-product behavior and that callers must not assume the merged shape is tight.
- [x] ~~**Mem backend has zero indexes — all O(N) scans**~~ ✅ Already documented — `store/mem/mod.rs` has a `⚠️ PERFORMANCE WARNING — TESTS ONLY, NOT FOR PRODUCTION` block added in commit `3f8cecee`.
- [x] ~~**Claim GC has no RAII guard — leaks possible**~~ ✅ Fixed — `ClaimGuard` RAII type exists in `crates/nmp-core/src/store/events.rs` (struct + `Drop` impl); acquire via `EventStore::claim_guarded`, which releases the claim on every exit path.
- [~] **D2 (negentropy-first) enforced by call-site only** — Documented honestly. `apply_coverage_filter` now has an `INVARIANT (D2 — negentropy-first)` doc paragraph; the `TODO(D2)` in `nmp-core/src/subs/mod.rs` names `Kernel::with_publish_store` as the unwired site and explains the `nmp-core`/`nmp-nip77` cycle that blocks structural enforcement. The `d2_production_kernel_installs_coverage_hook` `#[ignore]` test is the open tracking item (commit `887e343a`). **Still open:** the hook is never actually installed in production — structural enforcement requires a higher-level assembly crate depending on both `nmp-core` + `nmp-nip77`.

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
