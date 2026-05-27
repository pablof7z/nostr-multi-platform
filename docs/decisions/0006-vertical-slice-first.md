# ADR 0006: Vertical-slice-first delivery for Phase 1

**Date:** 2026-05-17
**Status:** accepted (discipline preserved; positioning modified by ADR-0009)
**Modified by:** ADR-0009 (the slice is now built on top of the kernel substrate; the kind:0 path is a Profile `ViewModule` in `nmp-nip01`, not a built-in feature of `nmp-core`)

## Context

The firehose-bench harness runs in three modes (replay, capture, live). Replay and capture currently model the runtime; live correctly reports `blocked` because the real adapters do not exist. Reactivity-bench run 002 validated the reactive model against ADRs 0001–0005 in a self-consistent fashion.

This is the right state to be in: the budget contract is concrete, the algorithmic core is validated, and we have honest visibility into what is and isn't proven. The next milestone is converting the model into running code without exploding scope.

The classic failure mode at this stage is **horizontal expansion** — building "the EventStore" comprehensively, then "the planner" comprehensively, then "the views" comprehensively, then finally stitching them together at the end, only to discover that the FFI surface or the relay adapter or the storage backend doesn't actually compose the way the model assumed.

The walking-skeleton / tracer-bullet pattern argues for the opposite: build one **narrow vertical** through every layer first, validate it works end-to-end against a real relay and real storage, *then* expand.

## Decision

Phase 1 of the build plan opens with a **vertical slice**: kind:0 profile metadata, end-to-end, through every architectural layer. The broader Phase 1 scope (full event store, full planner with all invariants, all view kinds) builds on top of this slice, not in parallel with it.

### The vertical slice

```
┌──────────────────────────────────────────────────────────────┐
│  Desktop iced shell (no FFI; direct rlib link)               │
│  - `Avatar { pubkey }` component                             │
│  - calls `useProfile(pubkey)` wrapper                        │
└──────────────────────▲───────────────────────────────────────┘
                       │ refcount + reactive subscription
┌──────────────────────┴───────────────────────────────────────┐
│  Generated wrapper (ADR-0005, manually written for slice)   │
│  - refcount per pubkey                                       │
│  - dispatch OpenView(Profile(pubkey)) on 0→1                 │
│  - dispatch CloseView(id) after 30s grace on 1→0             │
│  - write ProfileDelta::Replaced into profiles[pubkey]        │
└──────────────────────▲───────────────────────────────────────┘
                       │ AppAction / AppUpdate (no FFI for slice;
                       │ direct fn calls into nmp-core)
┌──────────────────────┴───────────────────────────────────────┐
│  nmp-core actor (minimal)                                    │
│  - handle OpenView/CloseView                                 │
│  - view registry: ProfileView only                           │
│  - on_event_inserted dispatched via composite reverse index  │
│  - DeltaBuffer with within-view coalescing                   │
└──────────────────────▲───────────────────────────────────────┘
                       │ insert(event)
┌──────────────────────┴───────────────────────────────────────┐
│  EventStore (minimal)                                        │
│  - in-memory only (no LMDB yet)                              │
│  - kind:0 replaceable supersession                           │
│  - composite reverse index keyed by (kind, author)           │
│  - claim-based GC                                            │
└──────────────────────▲───────────────────────────────────────┘
                       │ events from relay
┌──────────────────────┴───────────────────────────────────────┐
│  Relay adapter (minimal)                                     │
│  - one WebSocket via nostr-sdk to one relay                  │
│  - REQ for kind:0 by pubkey; CLOSE on view close             │
│  - no outbox routing yet (hardcoded relay)                   │
│  - no negentropy yet (REQ only)                              │
└──────────────────────────────────────────────────────────────┘
```

### What's explicitly out of scope for the slice

- **LMDB / durable storage.** In-memory only; cold restart loses everything. Wire LMDB in *after* the slice works.
- **Outbox routing.** Hardcoded single-relay configuration. NIP-65 fan-out comes after.
- **Negentropy.** Plain REQ. The sync engine layers on once the REQ path is proven.
- **FFI.** Desktop slice uses direct rlib linking (per `docs/aim.md` §4.3.1 — canonical for desktop). UniFFI is wired in when porting the slice to iOS/Android.
- **Other view kinds.** Profile only. Timeline / Thread / Reactions / Conversation all wait.
- **Multi-account.** Single account, single signer. Account scope is in the API from the start but not exercised yet.
- **Wallet, WoT, Messaging, Blossom.** All later phases.

### Exit gate for the slice

A working desktop app where:

1. Opening the app subscribes to one known pubkey's profile.
2. The avatar component renders immediately with the shortened-npub placeholder.
3. When the relay delivers a kind:0, the avatar updates in place to the real picture / name / NIP-05.
4. Closing the window CLOSEs the relay subscription after the 30s grace.
5. The same component instance mounted N times in the UI shares one underlying relay REQ (per the wrapper's refcount).
6. A replaced kind:0 (newer `created_at`) supersedes the old one in the cache without UI flicker.
7. `firehose-bench live` mode can run the same flow against a real relay and report measured (not modeled) numbers for cold_start and a tiny version of profile_thrashing.

When this is real, the architecture has graduated from "modeled budget contract" to "runtime path." Everything else in Phase 1 is now layered on top of working code, not invented from spec.

### What this validates

- The composite-keyed reverse index works against real-world event arrival patterns (not just synthetic streams).
- The refcounted domain-keyed wrapper pattern survives a real component lifecycle (mount/unmount during scroll, hot-reload during dev, app suspend/resume).
- The `OpenView`/`CloseView` API shape is right (or surfaces the bugs that need ADR follow-ups).
- The actor's synchronous fan-out hits the latency budget against real relay frame arrival, not just `mem::replace` calls in a benchmark.
- A real WebSocket → real EventStore → real DeltaBuffer → real component update is measurable end-to-end.

### What this does NOT validate (deferred to later phases)

- LMDB performance (slice is in-memory).
- UniFFI marshaling cost (slice is desktop-only).
- Outbox routing fan-out (slice is single-relay).
- Negentropy bytes-saved (slice is REQ-only).
- NSE budget compliance (Phase 5).
- Cross-platform consistency (only desktop in the slice).
- Multi-account isolation (Phase 3).

Each of these graduates to a real measurement when its phase lands.

## Consequences

- **Phase 1 produces a runnable desktop demo at its first checkpoint**, not just passing unit tests. Anyone can `cargo run` and see an avatar appear from a real relay.
- **The firehose-bench live mode becomes unblocked** for the slice's narrow scope (cold_start with a single Profile view, profile_thrashing with mount/unmount churn). Other scenarios stay blocked until their dependencies (LMDB, multi-relay, NSE, etc.) land.
- **Subsequent expansion has a working substrate to build on.** Adding LMDB is a `Box<dyn EventStore>` swap. Adding multi-relay is a planner change. Adding negentropy is a planner change. Adding iOS is a UniFFI wrap of the existing actor. None require redesigning the architecture.
- **Bugs surface where they actually live.** A model can hide them; running code can't.

## Alternatives considered

- **Horizontal expansion (build each subsystem comprehensively before integrating).** Rejected — defers integration risk to the end and produces large rework when integration reveals API mismatches.
- **Multiple parallel slices.** Rejected for v1 — Profile is the simplest representative slice; one is enough. Other slices (Timeline, Thread) come after the Profile slice proves the pattern.
- **Slice through FFI to iOS first.** Rejected — desktop's direct rlib linking lets us validate the architecture without UniFFI noise. iOS wrap comes after the actor's API is stable.

## Validation

- Firehose-bench live mode runs cold_start and a slice version of profile_thrashing against a real relay.
- A 5-minute manual demo: launch the desktop app, see the avatar appear, kill the relay connection and watch the reconnect, mount/unmount the avatar 100 times rapidly, check no leaks.
- Reactivity-bench continues to pass standard gates against the slice's actor — confirms the synthetic measurements still hold with the real code path replacing the model.
