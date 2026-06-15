---
title: Snapshot Performance
slug: snapshot-performance
topic: kernel-snapshot
summary: Chirp's primary performance issue is subscription compilation weight combined with full snapshot serialization and FFI round-trip on every command dispatch, whi
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-15
updated: 2026-06-15
verified: 2026-06-15
compiled-from: conversation
sources:
  - session:c9a794f6-6ad7-4ee9-a620-fc342fd495c3
---

# Snapshot Performance

## Subscription Compilation Hotpath

Chirp's primary performance issue is subscription compilation weight combined with full snapshot serialization and FFI round-trip on every command dispatch, which cascades into continuous SwiftUI state invalidation. SubscriptionCompiler::compile_with_context is the single hottest Rust function, with cost dominated by cloning and traversing nested BTreeSet/BTreeMap inside InterestShape, and no short-circuit when nothing has changed. Compilation is not unconditional on every tick; `drain_tick` early-returns when the trigger inbox is empty, so the high compile weight indicates a `CompileTrigger` is being spuriously enqueued on nearly every tick. `recompile_and_diff` lacks an input-equality short-circuit: it unconditionally rebuilds the compiler and calls `compiler.compile()` plus `apply_selection`, `coverage_hook`, and `apply_watermark_rewrite` on every drain, even when inputs are byte-identical to the last compile. The root cause of the spurious `CompileTrigger` enqueue must be investigated before memoization is implemented. (Previously: unconditional full subscription recompilation on every actor drain tick.)

<!-- citations: [^c9a79-4] [^c9a79-15] [^c9a79-21] [^c9a79-29] -->
## Lattice Merge Cost

lattice::merge costs ~31ms by cloning BTreeSet<String> at each merge step, which compounds quickly for subscriptions with large author sets. InterestShape replaces BTreeSet<String> hex pubkeys with a more cache-friendly representation (e.g., sorted Vec<[u8; 32]> parsed bytes) to reduce clone/drop cost in lattice::merge.

<!-- citations: [^c9a79-5] [^c9a79-16] -->
## NostrAvatar Re-evaluation

NostrAvatar.body is re-evaluated on every Rust snapshot emission, calling ChirpColor.avatar(from:) to construct a LinearGradient each time, because NostrAvatar lacks Equatable conformance preventing SwiftUI from skipping body re-evaluation for unchanged profile data. NostrAvatar must gain Equatable conformance on (pubkey, url, colorHex) so SwiftUI can skip body re-evaluation when a snapshot delivers the same profile data. <!-- [^c9a79-6] -->

## Full Snapshot on Every Command Dispatch

make_update runs a full snapshot and FlatBuffer serialization across the FFI boundary on every single command dispatch, including trivial UI interactions. <!-- [^c9a79-7] -->

## FlatBuffer Decode Copy Overhead

The FlatBuffer decode path is not zero-copy; FlatbufferVector.subscript triggers a copy of ByteBuffer.Storage.Blob per subscript call, and projection arrays are materialized into new Data heap allocations on every decode pass. <!-- [^c9a79-8] -->

## Residual serde_json::Value Drop Cost

serde_json::Value drop path accounts for ~26ms, indicating residual non-typed paths still returning serde_json::Value in the snapshot/relay pipeline. <!-- [^c9a79-9] -->

## Trait-Collection Propagation Recursion

Trait-collection propagation recurses 4+ levels of _wrappedProcessTraitChanges on each layout pass, triggered by each new SwiftUI state update from the Rust kernel. <!-- [^c9a79-10] -->

## NMP-Level Fixes

Fixes for subscription compile memoization and InterestShape memory layout are NMP-level changes located in nmp-planner and nmp-core, benefiting all NMP consumers. <!-- [^c9a79-17] -->

## Subscription Compile Memoization

Subscription compile memoization must be input-keyed plan memoization at the `recompile_and_diff` seam, hashing the full input tuple (interest snapshot, mailbox generation, dead_relays, app_relays, watermark generation, and selection budget), not per-subscription caching, because the compiler is whole-set and merges interests across relays. The watermark store generation must be included in the memoization key to prevent serving a stale `since` value and silently under-fetching. (Previously: memoized per-subscription so that recompilation only occurs when the subscription's input interest actually changes.) <!-- [^c9a79-22] -->

## Implementation Priority

The recommended implementation order for the snapshot performance fixes is: (1) NostrAvatar Equatable conformance now, (2) investigate the CompileTrigger storm, (3) input-keyed plan memoization with watermark generation in the key, (4) serde_json::Value drop cost deferred and only as interning + serde adapter if still needed. <!-- [^c9a79-23] -->
