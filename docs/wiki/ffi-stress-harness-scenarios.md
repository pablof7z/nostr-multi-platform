---
title: Rust FFI Stress Harness — Scenario Coverage (S6–S11)
slug: ffi-stress-harness-scenarios
summary: "The Rust FFI stress harness must implement scenarios S6 through S11: capability lifecycle storms, error-shape exhaustion, subscription planner DOS, relay flap,"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:7f0f0c78-d1aa-49db-b659-c9cf49827117
  - session:e50d12a1-0a49-4a45-9bb1-251fa0f434b6
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
---

# Rust FFI Stress Harness — Scenario Coverage (S6–S11)

## FFI Stress Harness Scenarios

The Rust FFI stress harness must implement scenarios S6 through S11: capability lifecycle storms, error-shape exhaustion, subscription planner DOS, relay flap, long suspend, and RSS instrumentation. The S2 dispatch-flood scenario exhibits genuine unbounded retention of ~38 MiB net heap (0.13% reclaimed after drain), scaling with total dispatch count rather than working-set size, which constitutes a D8 violation and mandates a bounded channel fix (Option A); threshold revision (Option B) is foreclosed. The S2 drain measurement was approved and run to determine whether RSS growth was a transient spike or retained memory, producing the data that made the Option A vs Option B decision. The harness carries a regression gate requiring retained_heap_after_drain_bytes ≤ 1 MiB for S2. RSS is a misleading gate signal for memory retention; the counting-allocator net-heap metric is deterministic and must be used instead. The ffi-stress simulator run must capture per-scenario p50/p95/p99 marshal time, allocation counts, dropped-message counts, and gate pass/fail; if any scenario fails its threshold, it must be reported as a real finding, not papered over. D6 is enforced at the FFI surface: there are no production panics in `ffi/mod.rs`, and all 15 `unwrap()` calls in `capability.rs:146` are `#[cfg(test)]`. A regression test must feed malformed input through the FFI surface to ensure the lattice `Result` replacement prevents panics from reachable actor threads.

<!-- citations: [^7f0f0-5] [^e50d1-2] [^57528-8] -->
## See Also

