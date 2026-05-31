---
title: V-68-S2 — Opus Explorer ABI-Safe Design Discovery
slug: v-68-s2-opus-explorer-abi-safe-design
summary: How an Opus explorer discovered that V-68-S2 thread-half was ABI-safe (nmp-ffi precedent) — turning a 'defer' into clean implementation without touching iOS.
tags:
  - v-68
  - opus-explorer
  - d0
  - abi
  - nmp-ffi
  - thread
volatility: cold
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# V-68-S2 — Opus Explorer ABI-Safe Design Discovery

> How an Opus explorer discovered that V-68-S2 thread-half was ABI-safe (nmp-ffi precedent) — turning a 'defer' into clean implementation without touching iOS.

## Overview

V-68-S2 (thread half, HIGH D0) appeared to require a C-ABI change that would break the iOS build while the iOS peer agent was mid-refactor. An Opus explorer discovered that nmp-ffi already carries the BTreeSet::from([1,6]) default in nmp_app_open_timeline (identity.rs) with a blessed D0-clean comment — exact precedent. This made the thread-half implementable without touching iOS, turning a 'defer' into 'implementable now'. [^4edd4-180]

## Key Doctrine Findings

Three doctrine findings made implementation safe: (1) nmp-ffi already carries BTreeSet::from([1,6]) in nmp_app_open_timeline with a blessed D0-clean comment — the exact precedent for putting social-policy defaults in the FFI layer. (2) The D0 lint (d0.rs) scopes its ban to nmp-core only — nmp-ffi (Layer 6 host-adapter) is the legitimate home for social policy defaults. (3) BACKLOG's own Stage 3 finalizer bans [1,6] in nmp-core/nmp-planner only, not nmp-ffi. The C-ABI signature stays unchanged: the FFI body fills the default internally, so iOS is untouched. [^4edd4-181]

## Load-Bearing Subtlety — ThreadViewState.reply_kinds

The kinds must be stored in ThreadViewState.reply_kinds, not just param-threaded. This is because the deferred-relay hydration fires on a later tick — exactly like follow_feed_kinds. If the kinds were only passed as a parameter rather than stored, the deferred path would lose them. This is the load-bearing correctness requirement that the Opus explorer identified. [^4edd4-182]

## Implementation — PR #840

PR #840 implemented V-68-S2 exactly to the Opus plan: {1,6} gone from thread.rs (grep-confirmed empty), reply_kinds stored in ThreadViewState for deferred-path correctness, C-ABI unchanged with the default in nmp-ffi, and 3 tests including T2 (kind-agnostic proof with kind 30023) and T3 (deferred-path test). Review confirmed: literal provably gone from nmp-core, deferred path reads stored reply_kinds, T2 proves kind-agnosticism, D0-legitimate nmp-ffi default. [^4edd4-183]

## Architectural Significance

This is the difference between a smallest-patch dodge and staging the BACKLOG's own design correctly. The Opus explorer turned an apparent 'ABI blast radius, defer it' situation into a clean implementable design with doctrine citations. Without the explorer, V-68-S2 would have been incorrectly deferred alongside the iOS legs. [^4edd4-184]

## See Also
- [[v-68-s2-thread-half-d0-kinds|V-68 Stage 2 — Thread-Half D0 Kind Externalization (HIGH · D0)]] — related guide

