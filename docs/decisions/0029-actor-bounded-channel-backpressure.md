# ADR-0029 — Bounded actor command channel + shed-load policy

- **Status:** Superseded / Not implemented — the bounded-channel + shed-load
  design described below was never built. The actor command channel remains an
  unbounded `std::sync::mpsc::channel()` (constructed in `nmp_app_new`,
  `crates/nmp-ffi/src/lib.rs`), and the concurrency model evolved toward the
  dual-channel / `MailScheduler` approach instead (see ADR-0050 §D3a). The
  dormant always-zero `dispatch_drops_total` counter that this ADR was going to
  make live has been removed (kernel handle + getter, the `Metrics` field, the
  `nmp_update.fbs` schema field, and all Rust/Swift/Kotlin/TS bindings and
  hand-written shell readers).
- **Date:** 2026-05-22 (proposed); 2026-06-21 (marked never-implemented)

## Historical note

This ADR proposed replacing the unbounded actor command channel with a bounded
`SyncSender(BOUNDED_ACTOR_CMD_CAPACITY = 4096)` plus a **drop-newest** shed
policy, routed through a single `Clone`able `ActorCommandSink` typed sink so
that every producer (FFI verbs, `nmp-signer-broker`, `nmp-marmot` key-package
fanout) traversed one backpressure gate. The shed count was to be surfaced by
reusing the existing-but-dormant `dispatch_drops_total` snapshot metric.

None of that mechanism was built:

- `ActorCommandSink` and `BOUNDED_ACTOR_CMD_CAPACITY` never existed in the code
  (only a comment referencing them survived in `retention_tests.rs`).
- The actor command channel stayed an unbounded `mpsc::channel()`.
- `dispatch_drops_total` had **no increment site anywhere** — it was an
  always-zero `Arc<AtomicU64>` plumbed end-to-end (kernel handle, `Metrics`
  struct, FlatBuffers field, and every platform binding) but never written.
  It has now been removed as dead plumbing rather than left as a placeholder
  for an abandoned design.

The concurrency model that actually shipped is the single waking-inbox
dual-channel / `MailScheduler` design in ADR-0050 §D3a, where relay traffic and
host commands share one inbox so a command send wakes a relay-blocked actor.

## Open issue (if revisited)

The original risk this ADR was written for — an unbounded command channel that a
pathological producer loop can grow without bound — is **not** closed by an
enforcement mechanism today; the channel is still unbounded. Command-channel
occupancy is *observable* via the live `actor_queue_depth` gauge, but there is
no shed/backpressure *enforcement*. If a future iteration wants to add
backpressure, it should start from a measured depth-p99 histogram (we never had
one) rather than the precedent constant `4096`, and should be filed as a fresh
ADR/issue rather than reviving this superseded design.

## References

- ADR-0050 §D3a — the dual-channel / `MailScheduler` model that actually shipped.
- G-S4 — the `actor_queue_depth` instrumentation (the live observation surface).
- Direction reviews #19 (Mutex proliferation), #20–#22 (broker bypass),
  #34 (`actor_queue_depth` legitimate).
