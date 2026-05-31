---
title: V-68 Stage 2 — Thread-Half D0 Kind Externalization (HIGH · D0)
slug: v-68-s2-thread-half-d0-kinds
summary: "V-68 Stage 2 thread half (HIGH D0): externalized {1,6} reply-kinds from nmp-core to nmp-ffi, ABI-safe, PR #840."
tags:
  - backlog
  - V-68
  - D0
  - kind
  - thread
  - nmp-ffi
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-21
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
---

# V-68 Stage 2 — Thread-Half D0 Kind Externalization (HIGH · D0)

> V-68 Stage 2 thread half (HIGH D0): externalized {1,6} reply-kinds from nmp-core to nmp-ffi, ABI-safe, PR #840.

## Overview

V-68 Stage 2 is a HIGH-priority D0 backlog item: externalize the hardcoded kind:1/6 reply-kinds literal out of nmp-core's thread.rs. The Opus architect returned IMPLEMENTABLE-NOW with decisive doctrine evidence showing it's genuine D0 resolution, not relocation: nmp-ffi already carries BTreeSet::from([1,6]) in nmp_app_open_timeline with a blessed D0-clean comment (the exact precedent). The D0 lint scopes its ban to nmp-core only; nmp-ffi (Layer 6 host-adapter) is the legitimate home for social policy. [^4edd4-153]


V-68-S2 initially appeared to require a C-ABI change that would force an iOS call-site update, risking build break while the iOS profile-fetch peer was mid-refactor. An Opus explorer discovered that nmp-ffi already carries the BTreeSet::from([1,6]) default in nmp_app_open_timeline — exact precedent — and that the D0 lint scopes its ban to nmp-core only (not nmp-ffi). This made the implementation ABI-safe: the C-ABI signature stays unchanged, the FFI body fills the default internally, and iOS is untouched. The load-bearing subtlety: kinds must be stored in ThreadViewState.reply_kinds (not just param-threaded) because the deferred-relay hydration fires on a later tick, exactly like follow_feed_kinds. [^4edd4-197]
## ABI-Safe Design

The C-ABI signature stays unchanged — the FFI body fills the default {1,6}, so iOS is untouched — no peer collision, no build break. The load-bearing subtlety: kinds must be stored in ThreadViewState.reply_kinds (not just param-threaded), because the deferred-relay hydration fires on a later tick — exactly like follow_feed_kinds. This was the key Opus finding that turned an apparent 'ABI blast radius, defer it' into a clean implementable plan. [^4edd4-154]

## Implementation

Delivered as PR #840: {1,6} gone from thread.rs (grep-confirmed empty), reply_kinds stored in ThreadViewState (the load-bearing deferred-path fix), C-ABI unchanged with the default in nmp-ffi, and 3 tests including the kind-agnostic proof (T2 with {30023}) and the deferred-path test (T3). [^4edd4-155]


PR-D (tests-first floor) covers `nmp-nip59` round-trip, `nmp-signer-iface` SignerOp resolution, and `nmp-threading` reentry before downstream PRs. [^1c093-26]
## Review and Merge

The reviewer APPROVED pending cargo test: literal provably gone from nmp-core, deferred path reads stored reply_kinds (T3 exercises it directly), T2 proves kind-agnosticism, C-ABI unchanged, D0-legitimate nmp-ffi default, clean scope. Merged as the 4th correctly-prioritized HIGH item (master at 713a6f2f). [^4edd4-156]

## See Also
- [[v-68-s2-opus-explorer-abi-safe-design|V-68-S2 — Opus Explorer ABI-Safe Design Discovery]] — related guide

