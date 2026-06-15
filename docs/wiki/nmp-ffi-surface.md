---
title: NMP FFI Surface
slug: nmp-ffi-surface
topic: nmp-ffi-surface
summary: The legacy author/thread C-ABI open surfaces (nmp_app_open_author, nmp_app_close_author, nmp_app_open_thread, nmp_app_close_thread) are removed; consumers must
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:bf035812-6f7a-46ec-a11d-30fc7369342f
  - session:ab8061fc-b277-4ba4-bf55-1532bcb1aa90
  - session:78b50727-bccd-4088-8493-a07624a4fa83
---

# NMP FFI Surface

## Removed Legacy C-ABI Surfaces

The legacy author/thread C-ABI open surfaces (nmp_app_open_author, nmp_app_close_author, nmp_app_open_thread, nmp_app_close_thread) are removed; consumers must use nmp_app_open_interest/close_interest with NIP-01 filter JSON. The open_interest seam replaces the deleted open_author and open_thread seams; callers use nmp_app_open_interest/close_interest with a verbatim NIP-01 filter JSON, composing author feeds or threads via ids + #e interests. The v0.4.0 release is versioned 0.4.0 (not 0.3.1) because of the C-ABI break from removing four nmp_app_* symbols; its headline instructs Android consumers to skip v0.3.0 entirely and pin v0.4.0 directly.

<!-- citations: [^da6b1-29] [^da6b1-57] [^da6b1-89] -->
## v0.5.0 Breaking Changes

The v0.5.0 release carries ADR-0045 universal cache-serve E1+E2+E3 and two breaking changes: deletion of nmp_app_open_timeline (replaced by nmp_app_open_contact_feed / nmp_app_close_contact_feed per ADR-0042 §2, with Chirp wrappers nmp_app_chirp_open/close_home_feed), and deletion of the nmp-codegen gen modules scaffolder plus apps/fixture.

<!-- citations: [^da6b1-30] [^da6b1-58] [^da6b1-90] -->
## Promoted Internal Surface

The claimed_profiles decode cluster is promoted to the public typed surface (pub visibility, cfg(test) gate dropped) as part of the legacy surface deletion migration. <!-- [^da6b1-31] -->

## Android Version Pinning

Android consumers must skip v0.3.0 and pin v0.4.0 directly because v0.3.0 shipped with Android completely dark (the KernelUpdateFrameDecoder was not rebuilt for the typed-frame wire). The final master SHA consumers should pin is 92fdfca327a782b82ee999a414190d39265b8243 (tag nmp-v0.7.1).

<!-- citations: [^da6b1-32] [^2e544-397] -->
## Bunker Connection State Localization

The bunker connection state label and tone should be moved into Rust (BunkerConnectionStateDto) so both shells render verbatim through a single tone resolver instead of synthesizing English labels and hardcoding colors; tracked as #1099 at priority p3/post-v1. <!-- [^da6b1-33] -->

## Handle & Actor Startup

nmp_app_new returns a passive handle with config; nmp_app_start moves config into the spawned actor, deleting the preflight kernel and the #601 rev hack. No setter exists after start, making late config inexpressible and closing #618 by construction (no LateWiring runtime diagnostic will be built).

<!-- citations: [^2e544-31] [^2e544-378] [^2e544-417] [^2e544-469] -->
## iOS KernelHandle Lifetime

iOS `KernelHandle.listen()` uses `passUnretained` with an implicit ARC lifetime dependency; `passRetained` would be safer given Rust's quiescence contract that no callback fires after `nmp_app_set_update_callback(raw, nil, nil)` returns. <!-- [^02745-11] -->

## Issue #1283 Fix Location

Issue #1283's fix belongs in `nmp-ffi` (not the kernel), because `nmp-core` has no `nmp-content` dependency and the KCEV FlatBuffer documents that the kernel-owned projection stays protocol-agnostic; `nmp-ffi` sits above both and may legally depend on `nmp-content`. The correct fix for the EmbedHost D0 violation is to resolve embeds in nmp-ffi via nmp-content and ship a typed `EmbedKindProjection` on a sidecar key for all shells to decode, not to enrich the kernel's claimed_events buffer (which would invert the crate dependency graph). F-CR-12 (#1225) already built the iOS typed EmbedKindProjection structs and golden tests, making the nmp-ffi resolve + typed sidecar approach half-built on the consumer side.

<!-- citations: [^02745-12] [^02745-38] [^02745-59] -->
## Decided vs Owner-Input Issues

Ten of eleven needs-decision issues are determined by documented product direction (D0 thin-shell doctrine, zero-debt rule, v1=iOS+Android+desktop plan, single-mechanism cache-serve); only #1281 (backfill semantics for since=None) genuinely requires owner input. <!-- [^02745-39] -->

## Breaking-Change Policy

On breaking changes and migrations, do the right thing and upgrade NMP consumer apps by hand each time; never hedge on breaking changes or ask about timing.

<!-- citations: [^02745-60] [^02745-90] -->
## PendingOpRow & LastOpError Schema

NMMS FlatBuffers schema version is 2; PendingOpRow includes age_secs and last_op_error is a structured LastOpError table with op/reason/at_secs/correlation_id; shells map the reason machine code to user-visible banner text. <!-- [^78c8e-55] -->

## Dead-Island Parking & Manifest

Issue #1250 is resolved by parking nmp-blossom and nmp-nip60 behind off-by-default feature/exclusion flags and removing them from the release manifest. PR #1324 (#1250 park dead-islands) subsequently removes nmp-blossom, nmp-nip60, and nmp-wallet-poc from the release manifest and workspace after they were excluded behind feature flags. Re-activation of nmp-blossom and nmp-nip60 is tracked by #998 and #1001.

<!-- citations: [^02745-88] [^02745-105] -->
## TimelineItem Cycle Fix (#920)

Issue #920's naive fix (move TimelineItem to nmp-nip01) would create a cycle because nmp-nip01 → nmp-core already exists; the right fix is a snapshot-envelope cut, and nmp-nip01 already owns the typed timeline row family. <!-- [^02745-89] -->

## CI Feature-Gate Regression Check

Android --features marmot must compile in CI (cargo check --features marmot in the android-ffi standalone workspace) to catch build regressions in the feature-gated dependency graph. <!-- [^78c8e-89] -->

## Push-Event File-Size CI Orphan Handling

The push-event file-size CI workflow should fall back to the merge-base (or skip with a notice) when the before SHA is orphaned after a rebase, instead of reporting a permanent red that reviewers learn to ignore. <!-- [^78c8e-90] -->

## WASM Spec Location

The WASM spec (ADR-0047, wasm-surface.md) belongs at the framework level, not under the chirp example app's design docs. <!-- [^bf035-167] -->

## Incremental Apply Declaration

Omission is gated on a per-instance nmp_app_declare_incremental_apply() capability. The function returns an i32 error code (0=ok, 1=AlreadyStarted after nmp_app_start—a repeat declare before start is idempotent and returns 0, 2=RegistryUnavailable, -1=null app) rather than using a debug_assert, ensuring fail-closed behavior on the Android FFI path; all callers including NmpCore.h are updated. On Android, a nonzero return aborts initialization (the dead-handle sentinel causes all subsequent JNI calls to no-op). When nmp_app_declare_incremental_apply returns nonzero on Android, the app pointer allocated at line 53 is freed via nmp_app_free(app) on the error path before the early return to avoid a memory leak, since Session (which owns the free) is never constructed.

<!-- citations: [^78c8e-456] [^78c8e-472] [^78c8e-505] -->
## Gallery Kotlin Regeneration

The gallery's curated minimal nmp/transport Kotlin subset (SnapshotFrame.kt, Value.kt) must not be force-regenerated; only android/app/src/main/java/nmp/ gets the full regen. <!-- [^78c8e-473] -->

## v0.8.0 Breaking Changes

The v0.8.0 release cuts version 0.8.0 for a breaking C-ABI change: the `nmp_app_claim_profile` FFI signature expanded from 4 to 5 arguments by adding `liveness: c_int` (0=CacheOk for feed avatars, non-zero=Live for profile screens). All in-repo callers (nmp-ffi, nmp-android-ffi, nmp-wasm, apps, ffi-stress) were updated to pass the 5th argument. Kernel PR #1436 merged to master, followed by iOS PR #1437, making the 5-arg FFI consistent across Rust and iOS. The release also ships the full profile-resolution overhaul and adds nmp-blossom to the release manifest, fixing the long-standing release-manifest CI red.

A runnable throwaway stress-harness binary drives the real NMP public/FFI surface against a real fixture relay and real Schnorr signing; it is not part of the test suite and is not landed to master permanently. <!-- [^78b50-249] -->

<!-- citations: [^ab806-159] [^ab806-172] [^ab806-214] -->
