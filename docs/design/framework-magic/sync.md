# Framework Magic §C9–§C10 — Sync, Provenance, Watermarks

> Parent: `docs/design/framework-magic.md`.
> Read first: `docs/product-spec/subsystems.md` §7.1 (provenance + watermarks), §7.8 (sync engine); `docs/design/lmdb/watermarks.md` (storage); `docs/design/ndk-applesauce-lessons.md` §6 (NIP-77 lessons), §9.8 (coverage ≠ cache presence).

## C9. Provenance preserved across redeliveries

**Statement.** When the same event id arrives from N different relays, the event store keeps exactly one event record with an N-entry provenance set (relay URL + first-seen + last-seen + source, deterministic primary relay). The original `id` and `signature` are never re-derived; the event is byte-stable across redeliveries.

**Framework does:** the dedup-with-provenance-merge rule at `docs/product-spec/subsystems.md` §7.1 row "Duplicate id". Storage of provenance sidecars at `docs/design/lmdb/watermarks.md` (the 32-distinct-relay-per-event bound is set there). The "primary relay" selection is a deterministic function of the first observer, used for cache locality and diagnostics.

**App writes:** nothing. The view payload's `id` field is the event id from the first observation; any per-event diagnostic UI ("seen on N relays") reads `Provenance` through `DebugDiagnostics` per `subsystems.md` §7.16.

**Failure mode prevented:** `product-spec/overview-and-dx.md` §3.3 **bug #10** ("Re-published event missing its original `id` due to re-signing"). Plus the related "duplicate event in timeline" bug where naive dedup-on-id is missing and the same note appears twice from two relays. Plus the diagnostic-visibility regression where the app loses the ability to say "this event came from relay X" because the cache layer collapsed provenance.

**Test:** `c9_provenance_merges_across_relay_redeliveries`. The test uses two mock relays:

1. Relay-1 delivers event `e1` (kind:1 by Alice) at clock-now; insert observed.
2. Assert event store contains exactly one event with id = `e1.id`, provenance set = `[{ relay: "wss://r1", first_seen: T0, last_seen: T0, source: Live }]`, primary relay = `wss://r1`.
3. Relay-2 delivers the same `e1` at T1 = T0 + 5s; insert observed.
4. Assert event store **still contains exactly one event** with id = `e1.id`, provenance set has two entries (r1 unchanged at T0/T0, r2 added at T1/T1), primary relay = `wss://r1` (unchanged — primary is sticky to first observer).
5. Assert the `signature` and `id` fields are byte-identical to the original Relay-1 delivery (no re-derivation; the second insert did not re-sign).
6. Relay-1 delivers `e1` again at T2 = T0 + 60s; assert the existing provenance entry for r1 updates `last_seen` to T2, no duplicate r1 entry is created.
7. Run 33 more relay deliveries of `e1` from distinct relay URLs; assert the provenance set caps at 32 entries per the `docs/design/lmdb/watermarks.md` bound, with the **primary** entry preserved as the anchor.

**Milestone owner:** **[PENDING M3]**. Sub-paths 1–6 are testable today against the in-memory kernel (the current `relay_count` field at `crates/nmp-core/src/kernel/ingest.rs:238` is the primitive shape; M3 graduates it to a typed `Provenance` sidecar). Sub-path 7 requires M3's storage cap logic. Test checked in as `#[ignore = "pending M3 provenance schema"]`.

## C10. Watermarks gate backfill; cache miss becomes authoritative; NIP-77 is the default

**Statement.** Every `(filter, relay)` pair the framework reads from has a durable **sync watermark** recording how far back coverage has been reconciled. Before issuing any historical REQ, the planner consults the watermark: a fully-synced pair serves cache-misses as **authoritative** ("this event does not exist on that relay"); an unsynced or partially-synced pair triggers a backfill that prefers **NIP-77 negentropy** when the relay supports it, falling back to bounded REQ scan otherwise.

**Framework does:** the watermark schema at `docs/product-spec/subsystems.md` §7.1 (the watermarks table); the consult-before-REQ behavior at `subsystems.md` §7.2 line 62 ("Coverage-aware backfill"); the three sync triggers (foreground, view open, reconnect) at `subsystems.md` §7.8 lines 261–263; per-relay NIP-77 capability negotiation at `subsystems.md` §7.8 line 277. The watermark is durable across restart (`subsystems.md` §7.1 line 44). The authoritative-miss rule lives at `subsystems.md` §7.1 line 46: *"A cache-miss query against a fully-synced (filter, relay) pair is authoritative."*

**App writes:** nothing. The app opens a view; the framework decides whether to serve from cache (with confidence backed by coverage), backfill via NIP-77, or fall back to bounded REQ. The view payload streams in as the gap closes; no spinner gates the cached render (per C13 and D1).

**Failure mode prevented:** the cache-miss-disguised-as-empty bug (`product-spec/overview-and-dx.md` §3.3 **bug #6**: "Cache miss returning empty without triggering a fallback fetch") and its inverse, the over-fetch bug — issuing the same historical REQ on every view open because the framework can't tell the cache is complete. Plus the bandwidth waste `ndk-applesauce-lessons.md` §6 highlights: re-fetching a 10k-event historical window via REQ scan when the relay supports NIP-77 reconciliation.

**Test:** `c10_watermark_gates_backfill_and_authoritative_miss`. The test uses a mock relay with declared NIP-77 capability and a `SimulatedClock`:

1. **Unsynced pair → fetch.** Open `TimelineView { authors: [A], kinds: [1], since: T-1d, until: T }` against a fresh store (no watermark for this `(filter, relay)` pair). Assert the planner schedules a backfill — NIP-77 reconciliation against the mock relay (because capability negotiation succeeded). Mock relay returns a 50-event set; assert all 50 land in the store; assert the watermark for `(filter_sig, "wss://mock")` updates to `synced_up_to = T`.
2. **Fully-synced pair → authoritative miss.** Close the view, re-open with the same filter, query for an event known not to exist in the response set. Assert the planner **does not issue a wire frame** (no REQ, no NIP-77); the cache-miss returns empty as authoritative; the watermark is unchanged.
3. **Capability fallback.** Switch to a second mock relay that **does not** support NIP-77 (capability negotiation reports unsupported). Open the same filter against the new relay; assert the planner falls back to bounded REQ scan for that relay only; assert the first relay's plan is untouched (the fallback is per-relay).
4. **Reconnect gap-fill.** Simulate disconnect/reconnect on the mock relay after T+30s of being away; assert on reconnect, the planner re-establishes the live REQ tail (per C8 / `subsystems.md` §7.2 line 71) and schedules a NIP-77 gap fill for the disconnect window; assert the watermark updates after the gap closes.
5. **`bytes_saved_vs_req` instrumentation.** Assert the cumulative counter for the synced relay is non-zero after step 1, per `subsystems.md` §7.1 watermarks-table column and §7.8 line 279.

**Milestone owner:** **[PENDING M4]**. M4 is the NIP-77 milestone (per `docs/plan/scope-adjustments-2026-05-18.md` v1 ladder). The watermark *schema* lands earlier in M3 (`docs/design/lmdb/watermarks.md`); M4 lands the engine that reads/writes them and the capability negotiation. Test checked in as `#[ignore = "pending M4 sync engine"]`. Sub-path 2 (authoritative-miss given a populated watermark) is the structural assertion that does not require NIP-77 — it could be flipped to non-ignored as soon as M3 lands the schema and a stub engine.

## Why these two are paired

C9 is **what** the store remembers about each event's provenance. C10 is **what** the store remembers about each `(filter, relay)` pair's coverage. Together they answer the question `ndk-applesauce-lessons.md` §9.8 raises: *"Having an event in the local store does not prove that a view is complete."* C9 is "we have this event"; C10 is "we have everything matching this filter from this relay up to this timestamp." The framework needs both to render correctly without fetching needlessly.

The two are paired in one chapter rather than split because their tests share the mock-relay-with-capability harness and because their failure modes intersect (a redelivered event from a new relay updates both the provenance set and the watermark, per `subsystems.md` §7.1 line 101 "Watermarks intersect with outbox").

## Cross-references

- `docs/design/lmdb/watermarks.md` for the storage schema and the 32-distinct-relay cap.
- `docs/design/subscription-compilation/compiler.md` Stage X for the planner's watermark consultation in the compile pipeline. (TBD: confirm Stage number in research-fold; the compiler file specifies it.)
- The "shared relay policy between sync and live REQ" lesson from `ndk-applesauce-lessons.md` §6 last paragraph is implicit in the per-relay watermark — both engines key by the same `(filter_sig, relay_url)` pair, so they cannot disagree on the relay universe.

`TBD-from-research(applesauce/event-store-query-builders.md)`: cite Applesauce's coverage/watermark equivalent and the API by which a query-builder reads it. NMP's `WatermarksSummary` (`subsystems.md` §7.8 line 287) is the analogous app-visible surface; the research-fold commit verifies the surface covers the same diagnostic needs.

## What this chapter does not cover

- The action-ledger row schema for a manual `RunSync` action (`subsystems.md` §7.8 line 268 `SyncSpec`) — that's an actions-catalog concern owned by §7.5.
- The proof-app sync overlay rendering — `subsystems.md` §4.5 owns the proof app.
- Per-event verification re-running during sync — `subsystems.md` §7.1 row "Query matching" specifies that *every* stored event passes the canonical matcher; that is implicit, not a contract bullet.
