---
title: Account Operations Must Use Bespoke C-ABI Symbols — Not dispatch_action
slug: account-operations-c-abi-symbols
summary: Account creation, sign-in, switch, and remove operations must call the bespoke C-ABI symbols directly, because no ActionModule is registered for the nmp.create_account / nmp.sign_in_nsec / nmp.switch_account / nmp.remove_account namespaces.
tags:
  - ffi
  - account
  - desktop
  - android
  - dispatch
  - c-abi
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:ecf13381-c8ef-40bf-9498-04a1d1f2af8f
---

# Account Operations Must Use Bespoke C-ABI Symbols — Not dispatch_action

> Account creation, sign-in, switch, and remove operations must call the bespoke C-ABI symbols directly, because no ActionModule is registered for the nmp.create_account / nmp.sign_in_nsec / nmp.switch_account / nmp.remove_account namespaces.

## The Four Bespoke C-ABI Symbols

The canonical path for account lifecycle operations is the four bespoke C-ABI symbols that directly enqueue the corresponding ActorCommand variant. These symbols bypass the ActionModule dispatch system entirely. [^ecf13-1]

## Why dispatch_action Fails for Account Operations

Routing account operations through dispatch_action silently fails because no ActionModule is registered for the namespaces nmp.create_account, nmp.sign_in_nsec, nmp.switch_account, or nmp.remove_account. The dispatch always returns {"error":"…"} and the operation never reaches the kernel actor. The action registry only has PublishModule built-in, plus the NIPs registered via nmp_app_template::register_defaults — none of which cover account lifecycle. [^ecf13-2]

## Relay Format for nmp_app_create_new_account

The relay list passed to nmp_app_create_new_account must be a JSON array of 2-element arrays: [["url","role"],…]. The object format [{"url":"…","role":"…"},…] is incorrect and will not be parsed correctly. [^ecf13-3]

## Platform Correctness

Platforms that already call the bespoke symbols directly are correct: chirp-tui and iOS both call nmp_app_create_new_account and nmp_app_signin_nsec directly. chirp-desktop and Android were routing through dispatch_action and are now fixed. [^ecf13-4]


The desktop fix landed in commit b8615b07 (doctrine lint clean, all tests pass). The Android fix landed in commit 984599bb (squashed). [^ecf13-37]
## Desktop Bridge Account Methods

The chirp-desktop bridge routes all four account operations through the bespoke C-ABI symbols: create_account calls nmp_app_create_new_account, sign_in_nsec calls nmp_app_signin_nsec, switch_account calls nmp_app_switch_active, and remove_account calls nmp_app_remove_account. These are free functions, not ActionModule dispatch calls. [^ecf13-5]

## Android Account Creation

Android's nativeCreateLocalAccount calls nmp_app_create_new_account directly with the correct relay array format. This ensures account creation works on Android and is consistent with the desktop, TUI, and iOS paths. [^ecf13-6]


Before the fix, Android's nativeCreateLocalAccount used dispatch_action with the wrong relay format — [{"url":"…","role":"…"}] instead of the required [["url","role"]] tuple-array format. The fix also updated default_chirp_relays_json_array to produce the correct format. The Android Kotlin layer was also checked for a sign-in nsec path that might route via the broken nativeDispatchAction("nmp.sign_in_nsec", ...), but the Kotlin code is not in this repo. [^ecf13-36]
## Relationship to ActionModule Architecture

The ActionModule dispatch system (ADR-0027) covers publish, react, follow, DM, zap, and other protocol operations — but not account lifecycle. Account creation, sign-in, switch, and remove are kernel-level ActorCommand variants that have no ActionModule namespace registration. They must always be dispatched via the bespoke C-ABI symbols, never via dispatch_action. [^ecf13-7]

## See Also
- [[adr-0025-bespoke-ffi-anti-pattern|ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection]] — related guide
- [[android-write-capability|Android Write Capability — Dispatch Door and Write Baseline]] — related guide
- [[chirp-desktop-feature-parity|Chirp Desktop Feature Parity — What Landed and Remaining Gaps]] — related guide
- [[chirp-ffi-boot-and-callback-lifetime|Chirp FFI Boot Sequence & Callback Object Lifetimes]] — related guide
- [[adr-0027-action-module-status|ADR-0027 — Unified ActionModule Executor Trait (Complete)]] — related guide

