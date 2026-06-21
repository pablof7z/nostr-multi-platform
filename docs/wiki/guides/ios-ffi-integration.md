---
title: iOS FFI Integration
slug: ios-ffi-integration
topic: ios-build
summary: "iOS discards the correlation_id returned by nmp_app_dispatch_action; KernelBridge.swift at lines 244â256 discards correlation_id, and PublishAction::Publish a"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-26
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:45fcf96e-5b37-414f-a080-820b74a4e179
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:9fc44c34-8e49-4959-91b3-714d4722ac3d
  - session:7b06d382-8fc2-4d52-bef5-f4d90e38cb2a
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:37e351ee-aa2b-43eb-9793-482de338f883
  - session:56d215c4-1aee-47cc-95c2-fd17269b92b6
---

# iOS FFI Integration

## Correlation ID Handling

iOS discards the correlation_id returned by nmp_app_dispatch_action; KernelBridge.swift at lines 244–256 discards correlation_id, and PublishAction::Publish at publish/engine.rs:265–277 does not set correlation_id_override. The JSON return value is freed without parsing, causing a 100-200ms delay before spinners appear.

UIApplication.canOpenURL (OS capability) must stay in Swift per the capability bridge pattern; only protocol knowledge (the signer-app scheme table) moves to Rust.

<!-- citations: [^1c093-15] [^45fcf-3] [^47203-6] [^95d02-10] -->
## FFI File Organization

Publish-handle FFI symbols are extracted into a dedicated `ffi/publish.rs` sibling file, co-located by owner rather than by role. <!-- [^47203-7] -->

## Observer Registration

Observer registration for `register_dm_inbox` and `register_group_chat` is idempotent on re-invoke, using atomic-swap slots on `NmpApp` that replace the prior observer rather than stacking duplicates.

The observer-leak fix uses slots on `NmpApp` (Option A) rather than a pointer-keyed static map, because the static map would outlive `nmp_app_free` and risk stale-entry aliasing if the allocator reuses the `NmpApp` address.

The swap-style observer slot on `NmpApp` does not generalize to N concurrent observers; a future multi-inbox or multi-group host would need a handle-returning variant of the FFI entry points. <!-- [^47203-8] -->

## FFI Edge Cases and Semantics

The stale comment at KernelBridge.swift:368 claiming 'auto-dispatches ActorCommand::WalletPayInvoice' describes V-41 deleted behavior that was never restored.

Observer IDs of `0` from `nmp_app_register_event_observer` are silently poisoned — unregistering `0` is a no-op — and callers must check for zero to detect registration failure.

Registering a `nmp.*`-prefixed namespace via `nmp_app_register_action_executor` silently no-ops without returning an error, giving the caller no way to detect the rejection.

The 5 `nmp_app_chirp_register_*` FFI entry points use two different idempotency patterns: the main `nmp_app_chirp_register` returns a handle-with-unregister, while the 4 specialized ones use per-feature swap-slots on `NmpApp`.

iOS has zero `claimProfile` call sites in any view despite the FFI being fully wired (KernelBridge.swift:140, KernelModel.swift:318). Android FFI (`nmp-android-ffi/src/lib.rs`) has no `claimProfile`/`releaseProfile` JNI symbols, so Android cannot register a UI profile claim. Web has no `claimProfile`/`releaseProfile` seam and defines its own divergent `shortKey` fallback (`<first8>..<last4>`) that does not match the canonical Rust display helpers.

CI enforces that the Rust UpdateCallback FFI signature is (*mut c_void, *const u8, usize) and that the nmp_app_set_update_callback signature matches, guarding the hot update callback ABI against regressions even when the symbol name is unchanged. CI also validates that each update callback header (NmpCore.h for Chirp and Notes, NmpGallery.h) contains both the exact NmpUpdateCallback typedef and the nmp_app_set_update_callback declaration, catching C function signature drift across FFI boundaries. <!-- [^37e35-4] -->

<!-- citations: [^47203-9] [^7b06d-2] [^86221-2] -->
## Open PRs

PD-032 requires user resolution of a 3-file merge conflict in PR #11 (marmot/ffi.rs, marmot/ffi/tests.rs, MarmotBridge.swift).

UniFFI is M14 PLANNED; raw C FFI is the live production surface. Post-v1 backlog includes achieving Android parity with iOS Chirp, blocked on UniFFI (M14) to avoid maintaining two separate FFI surfaces.

<!-- citations: [^95d02-9] [^9fc44-4] [^56d21-6] -->
## V1 Exit Checklist

Three previously untracked items surfaced from the 2026-05-23 opus direction review: an honest cross-platform claim (either wire wasm or rewrite 'cross-platform' in `aim.md`), a bespoke-FFI deprecation calendar (48 `nmp_app_*` symbols vs `dispatch_action`), and a snapshot serialization CI regression gate (`make_update_us`/`serialize_us` instrumented but no threshold). <!-- [^9fc44-5] -->
