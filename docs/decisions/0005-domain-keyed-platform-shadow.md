# ADR 0005: Domain-keyed platform shadow with refcounted component wrappers

**Date:** 2026-05-17
**Status:** accepted

## Context

The platform shadow holds the data components actually read. The reconciler writes incoming `ViewBatch` payloads into a platform-side reactive structure (`@Observable` on iOS, `mutableStateOf` on Android, signals on web, iced state on desktop). Components read from that structure via the platform's native idiom — no FFI on the read path, only on subscription lifecycle.

The original design keyed the shadow by `ViewId` (a framework-generated handle). In practice components don't think "I'm rendering view 47" — they think "I'm rendering pubkey X's avatar." Keying by framework concept forces every component to track ViewIds and makes cross-component sharing manual.

## Decision

**The platform shadow is a domain-keyed cache; Rust is the sole source of truth.** The shadow is organized as typed domain-keyed entries, not a flat `[ViewId: ViewPayload]` map. `ViewId` remains an internal framework token used by the FFI but does not appear in the component-facing read path.

For each view kind, the platform cache key is one of:

- **Single domain key** (pubkey, event_id, peer pubkey) — for view kinds that reduce to a single target.
- **Spec hash** — for view kinds with richer parameters (timelines, threads, searches).

The live mechanism is `KernelEventObserver`: the platform subscribes to kernel-emitted projection batches and writes them into the keyed cache; components read from the cache via the platform's native idiom. Subscription lifecycle (open/close, refcount, warm-close grace) is driven from that observer seam, not by per-component FFI calls.

## Consequences

- **Rust side does not change.** Projections are already domain-keyed (`Projections.author_display[pubkey]`); the planner already dedups identical specs; view warmth is already the TTL. The reverse index already routes by domain attributes. Nothing in `nmp-core` shifts.
- **Cross-component sharing is automatic.** Many components reading the same domain key share one cache entry by construction.
- **`ViewId` becomes an internal token.** Tests and debug diagnostics still use it. Component code never sees it.
- **The fat-vs-lean payload tension dissolves.** A Timeline ships fat `TimelineItem`s into its spec-keyed cache (every cell renders complete on first paint). A component reading a domain-keyed profile cache gets independent updates. Components on the same screen can pick either source.
- **Three-tier data model becomes explicit.** Rust durable storage → Rust working set + projections → platform domain-keyed shadow. Each layer derives from the layer below; only Rust is source of truth.

## Alternatives considered

- **`ViewId`-keyed shadow (original design).** Rejected — components naturally express interest by domain; framework handles are a leaky abstraction at the component level.
- **Per-component FFI subscriptions** (one subscription per UI component). Rejected — multiplies FFI surface and pushes subscription lifecycle into native component code, which has known race conditions across fast remount/unmount in all three platforms.
- **Single global flat cache on the platform side.** Simpler but loses per-kind type safety; harder to introspect; harder to evict by view-kind policy if needed.

## Validation

- **Cross-platform consistency tests** (already in the plan): the same scripted scenario produces identical platform cache state across platforms.
- **Memory tests**: TTL eviction frees platform memory under component churn (rapid scroll, screen transitions).
- **Performance tests**: an avatar component reading the profile cache does not generate FFI calls after the first subscription; only payload updates flow back.
