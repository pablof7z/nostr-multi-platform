## Direction Review #6 — 2026-05-23

### What I verified

- `crates/nmp-core/src/ffi/mod.rs:1487` exports **8** `pub extern "C" fn`; workspace-wide `nmp_app_*` total is **57** (51 production + 6 D11 lint fixtures). `plan.md:27` says "48 bespoke `nmp_app_*` FFI symbols in `crates/nmp-core/src/ffi/mod.rs` (1,487 LOC)" — the LOC matches but the 48 are spread across `ffi/`'s submodules; mod.rs alone has 8. Worth a one-line correction to plan.md so the next reviewer doesn't redo the grep.
- `crates/nmp-core/src/substrate/action.rs:56-148`: `ActionModule::start` returns `Result<(), ActionRejection>` (no payload). `ActionResult.result_json` is documented as fire-and-forget null. There is **no sync-return path**.
- `crates/nmp-wasm/src/runtime.rs:207-254` is a 294-LOC `Vec<LocalNote>` fake that hand-mocks the `chirpTimeline` snapshot shape and does not link `nmp-core`. Stub status confirmed.
- Marmot's "second app" is not standalone. `apps/marmot/.../lib.rs:75-97` and `apps/chirp/.../lib.rs:68-87` show the `nmp_marmot_*` C cluster shipping inside `libnmp_app_chirp.a` via rlib re-export. Live Swift calls in `ios/Chirp/Chirp/Bridge/MarmotBridge.swift:301-491`.
- NIP-17 send: `nmp.nip17.send` validator (`crates/nmp-nip17/src/action.rs:51`) → `ActorCommand::SendGiftWrappedDm` (`actor/mod.rs:460`) → two `nmp_nip59::gift_wrap_with_signer` calls (`actor/commands/dm.rs:277`). Inbox unwrap at `nmp-nip17/src/inbox.rs:329` needs raw `nostr::Keys` → bunker can't decrypt own DMs (V-08).
- Swift comments still namedrop deleted symbols (`nmp_app_publish_note`, `nmp_app_react`, etc.) in `NmpCore.h`. Live publish/react/follow all go through `nmp_app_dispatch_action` (`KernelBridge.swift:333-362`).

### Q1 — Bespoke FFI Ceiling

The debt is to future framework users, not to Chirp. The 57 symbols split:

- **~10 unavoidable lifecycle** (`nmp_app_new/free/start/stop/configure/reset/set_*/lifecycle_*`).
- **~12 projection wiring** (`nmp_app_chirp_register*`, `nmp_app_chirp_snapshot{,_free}`, `nmp_marmot_*` cluster, `register_event_observer`, `register_snapshot_projection`, `register_raw_event_observer`). **This is the real cost.** `MarmotBridge.swift` calls 6 of them; `ModularTimelineBridge.swift` calls 2. A new domain (long-form, marketplace) adds 4–8 more. **Linear in app domains.**
- **~17 view + identity verbs** (`nmp_app_open_*`, `nmp_app_close_*`, `nmp_app_claim/release_profile`, `nmp_app_signin_*`, `nmp_app_switch_active`, `nmp_app_*_relay`, `cancel_bunker_handshake`, `nostrconnect_uri`) — *each* of these could be a `dispatch_action("nmp.view.open", {...})` call.

Replacing the 17 verb-class symbols saves 17 globally but does **nothing** for the projection class. The projection class is unbounded under current design. The right answer is a typed `register_projection<P: Projection>` Rust API + ONE generic FFI `nmp_app_projection_snapshot(name)` — collapsing N-per-app to **two**.

**Recommendation.** Extend D11 (or add D15) to ban *new* `nmp_app_open_*` / `nmp_app_signin_*` / `nmp_app_*_relay` symbols. Add D16 to ban new per-app `*_register` / `*_snapshot` clusters; introduce a generic projection registry. Without D16, the 2027 Chirp will have 100+ symbols.

### Q2 — The Marmot Test

Marmot does not prove the thesis — it exposes the gap. Three concrete inventions that should not have been in app-land:

1. **Second store** (`mdk-sqlite-storage`) alongside the kernel's LMDB — `apps/marmot/.../lib.rs:17-37`. The only place in the workspace running two stores. `nmp-core` has no "capability-private storage" concept; Marmot punched a hole.
2. **11 bespoke C symbols** (`nmp_marmot_*` cluster + `nmp_app_chirp_identity_*`) for what is conceptually two verbs + one projection.
3. **`chirp_identity` prefix on a Marmot file** (`apps/marmot/.../identity.rs`) — the smoking gun that the core's identity bridge was too thin for a "second" app to reuse.

**Recommendation.** Treat Marmot as an API design test: any framework abstraction that cannot host it is incomplete. Write `ADR-0029 capability-private storage` and `ADR-XXXX typed projection registry`. Use them to retire the `nmp_marmot_*` cluster into one generic projection seam.

### Q3 — Synchronous Return Gap

There is no sync-return pattern today. The async escape is `is_async_completing = true` + `record_action_stage` (`wallet/action.rs:116`, `publish/action.rs:206`) — every terminal answer takes a snapshot-tick round-trip. This is why `nmp_app_signin_nsec` (`ffi/identity.rs:21`) remains bespoke: the host needs an immediate "valid hex / decoded keypair" answer, and `dispatch_action` can only mint a correlation_id.

**Minimal extension.** Add an associated `type ValidationOk: Serialize = ()` to `ActionModule` and let `start` optionally return that payload:

```rust
fn start(...) -> Result<Option<Self::ValidationOk>, ActionRejection>
```

Surface that JSON in the `dispatch_action` response alongside `correlation_id`. Realistic cost: 50–100 LOC across substrate + `ffi/action.rs` + at least one host parser change — the response shape evolves from `{"correlation_id":"..."}` to also carry the validation payload, which every Swift caller (`KernelBridge.swift:333`, `DmBridge.swift:67`, `GroupChatBridge.swift:163`) decodes. Six existing `ActionModule` impls (`publish`, `wallet`, `nip17` ×2, `nip57`, `nip25`/reactions) keep working unchanged via the `= ()` default.

**Recommendation.** Ship as `ADR-0029-sync-validation` BEFORE the second-app spike. Use it to delete `nmp_app_signin_nsec` and the wallet bolt11-validation 1-tick stutter.

### Q4 — NIP-17 Production Readiness

Send path is complete for **local-keys** identities, incomplete for **bunker**.

Works: action validator (`nmp-nip17/src/action.rs:51`), actor routing (`actor/dispatch.rs:561` → `commands/dm.rs:277`), two-fold gift-wrap (recipient + self-copy for multi-device D10), DM-relay-list reconciler (`dm_runtime.rs:32-100`).

Missing for ship:
- **F-02** — live-relay cold-start receive verification (Rust pipeline tests pass; real round-trip pending).
- **Bunker DM receive** — `unwrap_gift_wrap` at `inbox.rs:329` needs raw `Keys`. Bunker users will silently see no DMs. V-08 defers the broker RPC; that decision needs a *visible* UI flag (`active_identity_can_decrypt_dms`) projected from the kernel so the dead-end isn't invisible.
- **iOS NSE background decryption** — `aim.md §7 #5` flags this; no impl. Stays post-v1, but write the ADR now.

**Recommendation.** Close F-02 next sprint. Project the capability flag for bunker. NSE ADR before v1, impl post-v1.

### Q5 — Framework vs. Product

The codebase is **80% Chirp-as-product, 20% framework**, but is *labeled* as the inverse.

Evidence:
- `nmp-cli` exists; no `nmp init` user has ever scaffolded a working app (`plan.md:87`).
- Marmot ships *inside Chirp's staticlib*; ADR-0025 calls this an exception but it's the rule.
- `apps/longform` is read-only, `apps/fixture` is non-Nostr — **zero standalone stateful Nostr binaries** (`plan.md:25`).
- `KernelBridge.swift` has 1,988 LOC of handwritten `Decodable` (F-05) no second app would rewrite the same way.
- `crates/nmp-wasm/src/runtime.rs` is a 294-LOC fake. "Cross-platform" in `aim.md §1` is aspirational.
- Symbol names like `nmp_app_chirp_*` and `nmp_app_chirp_identity_*` hardcode Chirp into the supposedly-generic core.

**Honest verdict.** You are not yet building a framework. You are building Chirp very carefully with framework habits, and the habits pay off (D0–D14 are real). The moment the second-app spike happens, Marmot's hole-punching will be revealed as the norm, not the exception. Better to discover that on a 300-LOC spike than after committing to v1.

Path B (framework) needs: typed projection registry (Q1/Q2), sync-validation (Q3), real WASM linkage (V-01), `nmp init` 5-minute scaffold, **and** renaming the per-domain symbols (`nmp_app_chirp_register_dm_inbox` → `nmp_app_dm_*`, `nmp_app_chirp_register_follow_list` → `nmp_app_follow_*`, etc. — the current cluster is not all "timeline") so Chirp is the first user, not the namespace. Path A (Chirp-as-product) is the default trajectory of unmodified code velocity. Pick one; don't drift.

### The single most important thing to do in the next sprint

**Pull ADR-0025 relocation forward AND keep review #5's spike bar.** Two moves, in this order, same sprint:

1. **Relocate `nmp_marmot_*` out of `libnmp_app_chirp.a`** into its own staticlib (already deferred to post-v1 in `plan.md:125` — pull it forward). This is structural, not a spike.
2. **Run review #5's spike unchanged:** NIP-01 publish + NIP-46 signin from a non-Chirp binary, ≤300 LOC Swift, **zero bespoke FFI symbols** (not "zero *new*" — zero, period). This is the falsifiable thesis test. The Marmot relocation does not replace it; Marmot already failed that bar by needing 11 bespoke symbols, which is *why* it exposed the framework gaps. Lowering the bar to "Marmot-standalone with 0 *new* symbols" tests something weaker (can we package what already exists?) than what review #5 tests (can a new app reach the kernel through only the generic seam?).

If you only have time for one: do (2). The relocation is housekeeping; the spike is the thesis falsifier. Sharpened pass-bar for (2) — if the spike requires more than `nmp_app_dispatch_action` + `register_event_observer` + `register_snapshot_projection` + lifecycle, **stop and write the missing substrate first** (the projection registry from Q1/Q2 and the sync-validation from Q3 are the two most likely gaps). If you ship the spike in 300 LOC against today's API, the thesis is provisionally confirmed and Q1/Q2 become optimization, not blocker.
