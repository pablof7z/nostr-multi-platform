# Framework Magic §C8 — Subscription Planner Hygiene

> Parent: `docs/design/framework-magic.md`.
> Read first: `docs/product-spec/subsystems.md` §7.2 (subscription planner behaviors); `docs/design/subscription-compilation/compiler.md` (compilation pipeline); `docs/design/reactivity/scheduling-and-data-model.md` (buffer / batch policy); `docs/design/firehose-bench.md` (the modeled-perf companion benchmark for the ≤60Hz budget).

## C8. Subscriptions auto-dedup, auto-coalesce, auto-close, and auto-buffer

**Statement.** The framework guarantees four properties on every wire subscription it issues:

1. **Dedup.** Two logical interests with the same canonical filter share one wire REQ per relay; each logical consumer still receives only events matching its own filter.
2. **Coalesce / merge.** Logical interests with structurally compatible filters (per the merge lattice in `subsystems.md` §7.2) merge into one broader REQ per relay; each consumer is filtered locally from the broader stream.
3. **Auto-close.** A wire REQ with no remaining logical consumers is CLOSE'd. One-shot interests (those without a live tail, only an `until` upper bound) are CLOSE'd on EOSE.
4. **Buffered batching.** Inbound events for one view are batched into a single `ViewBatch` per actor tick at ≤60Hz; backpressure drops batches in favor of a single `FullState` catch-up. The platform's reactive primitive sees one re-render per tick, not per event.

**Framework does:** the subscription-compilation pipeline (`docs/design/subscription-compilation/compiler.md`) for dedup and coalesce; the wire-emitter's diff (compiler §3 final stage) for auto-close on plan changes; the view registry's refcount drop for auto-close on consumer loss; `docs/design/reactivity/scheduling-and-data-model.md` for the per-tick batching; the FullState backpressure fallback at `subsystems.md` §7.2 line 69. The hard cap of 60Hz is the budget in `subsystems.md` §7.16 table row "ViewBatch frequency under hashtag firehose".

**App writes:** nothing. The app opens views; it does not name a REQ. The reactivity scheduling is invisible — the platform's `useTimeline()` rune/observable emits at the framework's batched cadence regardless of relay throughput.

**Failure mode prevented:** the entire class of subscription-management bugs in `product-spec/overview-and-dx.md` §3.3 numbers 2 ("Subscription leaked after its UI is destroyed") and 8 ("Two concurrent UI subscriptions for the same filter producing two relay REQs"). Plus the hand-rolled grouping-window + dedup-LRU pattern that `ndk-applesauce-lessons.md` §7 calls out as the work clients typically do manually.

**Test:** `c8_subscriptions_coalesce_autoclose_and_buffer`. The test has four sub-paths in one `#[test] fn`:

1. **Dedup:** open two `TimelineView`s with identical filters; assert the planner produces one wire REQ per relay (not two); destroy one; assert the wire REQ stays alive; destroy the second; assert the REQ is CLOSE'd after the warmth grace expires (`subsystems.md` §7.6 line 226: 30s default).
2. **Coalesce:** open `TimelineView { authors: [A, B], kinds: [1] }` and `ProfileView { pubkey: C }`; assert the planner merges into one REQ per relay containing the union shape, with each view receiving only its filtered subset locally (no REQ for kind:0 alone if the relay already has the merged stream covering it). The merge lattice's exact rules live in `subsystems.md` §7.2 line 65 and `docs/design/subscription-compilation/intro.md` §1 open-question #2 (lattice formalization); the test asserts the *observable* (wire frame count = correct fewer-than-naive, payload coverage = correct) rather than the lattice mechanics.
3. **Auto-close on EOSE for one-shot:** open `ProfileClaim { pubkey: D }` (which `docs/design/subscription-compilation/intro.md` §2.2 line 112 specifies as `lifecycle: OneShot, limit: 1`); the mock relay sends the kind:0 then EOSE; assert the planner CLOSEs the wire REQ within one tick of EOSE; assert no further REQs touch that relay for that filter.
4. **Buffered batching under firehose:** the mock relay sends 600 events for one filter in 1 second (10× the budget); assert the platform reconciler observes ≤60 `ViewBatch` emissions in the window; assert no events are dropped from the underlying store (only the *render emission rate* is capped, not the ingestion); assert the actor queue depth stays below `subsystems.md` §7.16 budget (steady-state < 16).

**Milestone owner:** **[PENDING M2]** for sub-paths 1–3 (the compiler + lifecycle); **partial overlap with reactivity-bench** for sub-path 4 (the buffer cadence is exercised by `docs/perf/reactivity-bench/` already; the contract test asserts the same property through the public view path). Test checked in as `#[ignore = "pending M2 compiler"]` initially.

## Why this is one bullet, not four

The four properties (dedup / coalesce / close / batch) are observable as one contract from the app's perspective: *the app opens N views, the framework opens ≤N REQs, the framework closes them at the right moment, the framework caps emit cadence.* Splitting into four bullets would suggest the app might experience them separately; it does not. The four sub-paths of the test are the four conditions the single contract bullet asserts.

The reason this is C8 and not bundled with C6/C7 is that C6/C7 govern *which relay* a REQ targets; C8 governs *how many REQs and at what cadence* regardless of the relay. D5 (snapshots bounded by what's open) covers the view-scoped auto-close; `aim.md` §6 doctrine 6 covers the auto-group/dedup/buffer properties. The ≤60Hz emit cap derives from RMP bible invariant 9 ("No high-frequency FFI loops") + ADR-0002 (`docs/design/reactivity/scheduling-and-data-model.md` §7.2), not from D5's definition. Different milestone responsibility from C6/C7.

## Cross-references to the existing test surface

- `docs/design/subscription-compilation/tests.md` §9.2 assertion 2 already asserts the per-relay author partition + sub-shape merge (the coalesce property at the planner layer). The framework-magic version of sub-path 2 reuses that mailbox cache setup but reads the wire output through the platform shadow's audit log instead of through the planner harness.
- `docs/design/firehose-bench.md` is the modeled-perf companion: it asserts ≤60Hz holds under sustained load. The framework-magic sub-path 4 asserts the *correctness* of the cap (no event loss); the bench asserts the *budget* under realistic load.
- `docs/design/reactivity/validation-harness.md` covers reactive-primitive validation (Swift `@Observable`, Kotlin `Flow`, etc.). C8's sub-path 4 cross-validates that the platform-side emissions match the actor-side `ViewBatch` count.

## What this chapter does not cover

- **Reconnect-resumption.** When a relay disconnects and reconnects, the planner re-issues the same wire REQ set (`subsystems.md` §7.2 line 71). That is a planner *resumption* behavior, not a contract bullet — the app sees no surface change. It is covered implicitly by the dedup/close properties (the resumed REQs are the same REQs the planner already tracks).
- **NIP-77 sync vs live REQ split.** C10 in `sync.md` covers the sync side; C8 covers the live tail only.
- **Per-view payload size budgets.** `subsystems.md` §7.16 table rows. The contract guarantees the buffering happens; the budget is an instrumentation concern with its own test surface in `nmp-metrics`.

**Applesauce cross-validation** (`docs/research/applesauce/event-store-query-builders.md`): the logical-vs-wire split maps as follows. Applesauce's logical layer is `EventModels.model(Constructor, ...args)` at `event-models.ts:50-86`, which returns one shared `Observable` per `(constructor, key)` hash regardless of how many callers subscribe — equivalent to NMP's `LogicalInterest`. The wire layer is the `EventStore.insert$` / `remove$` subjects at `event-store.ts:93-99`, which all pipelines read from. NMP's `LogicalInterest` (`docs/design/subscription-compilation/intro.md` §2.1) covers the same surface: multiple view consumers sharing one compiled wire REQ, each filtered locally. No observable property is lost; the primary architectural difference is Applesauce uses RxJS `share()` while NMP uses an actor-owned compiler that emits wire frames.
