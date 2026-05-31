---
title: "Claim Expansion — terminate_claim Is the Sole Phase::Terminal Transition Point"
slug: claim-expansion-terminate-claim-invariant
summary: "All Phase::Terminal mutations must go through terminate_claim, the only function that cleans claim_sub_index. Bypassing it leaves dangling entries that panic on the next terminate_claim call."
tags:
  - rust
  - claims
  - kernel
  - panic
  - invariant
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-30
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Claim Expansion — terminate_claim Is the Sole Phase::Terminal Transition Point

> All Phase::Terminal mutations must go through terminate_claim, the only function that cleans claim_sub_index. Bypassing it leaves dangling entries that panic on the next terminate_claim call.

## Invariant

All transitions to `Phase::Terminal` must go through `terminate_claim`. The `terminate_claim` function is the sole place that cleans up `claim_sub_index` — removing entries that point to the claim being terminated. Any code path that sets `claim.phase = Phase::Terminal(...)` directly (inline mutation) bypasses this cleanup and violates the invariant. [^4edd4-1]


The fix for the `advance_to_phase2` bypass was delivered in PR #810 alongside the FlatBuffers CodingKey fix. A grep confirmed there are no other inline `Phase::Terminal` mutations anywhere in the codebase outside of `terminate_claim` itself. [^4edd4-32]

All transitions to `Phase::Terminal` must go through `terminate_claim`. The `terminate_claim` function is the sole place that cleans up `claim_sub_index` — removing entries that point to the claim being terminated. Any code path that sets `claim.phase = Phase::Terminal(...)` directly (inline mutation) bypasses this cleanup and violates the invariant. [^4edd4-217]
## Failure Mode

When a `Phase::Terminal` mutation bypasses `terminate_claim`, the `claim_sub_index` retains entries pointing to an ID no longer in `pending_claims`. Later, when `poll_claim_expansion`'s `retain` removes the claim from `pending_claims`, the dangling `claim_sub_index` entry persists. The next call to `terminate_claim` hits a `debug_assert` in `claim_expansion_helpers.rs:220` that verifies `claim_sub_index` entries point to existing claims — and panics. [^4edd4-2]


In release mode (Android), the `debug_assert` in `claim_expansion_helpers.rs:220` is compiled out entirely. The kernel does not panic — instead, it silently stops processing. The rev counter freezes (e.g., stuck at rev 6) with no crash message or banner. This makes the bug harder to detect on Android than on iOS (where the debug_assert fires and the crash banner is displayed). The root cause and fix are identical across platforms: replace the inline `Phase::Terminal` mutation in `advance_to_phase2` with a call to `terminate_claim`. [^4edd4-28]
## Specific Violation — advance_to_phase2

In `advance_to_phase2`, when `to_pick == 0` and the phase is `Phase1`, the code directly sets `claim.phase = Phase::Terminal(Exhausted)` — bypassing `terminate_claim` entirely. The fix replaces this inline mutation with a call to `terminate_claim`, which handles the phase transition and `claim_sub_index` cleanup atomically. [^4edd4-3]

## Verification

After fixing the violation, a grep for all inline `Phase::Terminal` mutations across the codebase should yield only the one inside `terminate_claim` itself. Any other inline `Phase::Terminal` mutation is a fresh violation of this invariant. [^4edd4-4]

## Cross-Cutting Impact

Because this panic originates in `nmp-core`, it affects every Chirp client (iOS, Android, TUI, desktop) that links against the same Rust library. Fixing it once in `nmp-core` resolves the issue for all platforms simultaneously. [^4edd4-5]


When rebuilding the Android APK after applying the `nmp-core` fix, verify the build timestamp to confirm the fix is included. The Android JNI `.so` must be rebuilt targeting the Android architectures (e.g., `aarch64-linux-android`) — the iOS simulator build (`aarch64-apple-ios-sim`) is a separate target and does not update the Android library. In the session where this fix was applied, the Android APK was rebuilt after the fix and the kernel reached rev 11+ without stalling, confirming the fix works on Android as well. [^4edd4-33]

## Claim Send Gate

The `claim_send_gate` must use `any_relay_connected` (`.any()`) instead of `all_relays_connected` (`.all()`), so that claims proceed when at least one relay role is available rather than waiting for all roles. [^6a951-117]

## Parked Claim Relay Hints

When a parked claim has URI relay hints and `can_send` is false, the kernel falls through to register and dial those hint relays directly. [^6a951-118]

## Teardown Timing

Kernel claim-expansion teardown only occurs at `terminal_exhausted` or `terminal_hit`, not on per-EOSE. This prevents a fast relay's empty EOSE from tearing down a claim before a slower relay delivers the matching event. [^6a951-119]
## See Also
- [[op-centric-home-feed|OP-Centric Home Feed (V-80) — Architecture and Status]] — related guide
- [[claimed-events|claimed_events Snapshot Projection]] — related guide
- [[chirp-cross-platform-feature-parity-testing|Chirp Cross-Platform Feature Parity — Mandated Testing Across All Clients]] — related guide
- [[chirp-ios-embed-system-implementation|Chirp iOS Embed System — Implementation and Architecture]] — related guide
- [[android-testing-pitfalls|Android Testing Pitfalls — Gesture Nav, Onboarding, and Account Creation]] — related guide

