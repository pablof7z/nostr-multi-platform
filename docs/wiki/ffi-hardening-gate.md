---
title: FFI Hardening Gate (M10.5)
slug: ffi-hardening-gate
summary: M10.5 (FFI hardening / iOS empirical proof) is a hard gate that must be cleared before M11 begins.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-21
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:7f0f0c78-d1aa-49db-b659-c9cf49827117
  - session:e50d12a1-0a49-4a45-9bb1-251fa0f434b6
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:12b3f443-3c2d-4e47-976a-7f4ceab75343
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
---

# FFI Hardening Gate (M10.5)

## FFI Hardening Gate

M10.5 (FFI hardening / iOS empirical proof) is a hard gate that must be cleared before M11 begins. The FFI-hardening workstream is scoped to docs/ffi-surface.md, docs/perf/m10.5/**, docs/design/ffi-hardening.md, ios/NmpStress/**, and crates/nmp-testing/bin/ffi-stress/**, excluding crates/nmp-core/**, docs/builder-guide/**, and Cargo.* The M10.5 literal exit gate's hardware/M10 items are deferred into the Pulse track rather than faked. Deliverables must contain no faked numbers; if a result cannot be produced on the simulator, the deliverable explicitly says so and routes it to the Pulse-track deferral. If any stress scenario fails its threshold, the finding is reported rather than papered over. The re-scoped M10.5 exit gate includes a re-scope addendum in docs/plan/m10.5-ffi-hardening.md recording the user decision, the re-scoped gate items, and the deferral of iPhone-hardware numbers and M1–M10 perf re-runs into the Pulse e2e validation track. The deferred Pulse-track items include the iPhone-hardware baseline, firehose-bench live real-device battery, the M1–M10 perf re-run, M10 Blossom UI scenarios, and a first-class XCUITest target. The expected exit-gate artifacts currently absent from docs/perf/m10.5/ include sim-baseline.md, doctrine-review.md, and Instruments evidence.

<!-- citations: [^7f0f0-9] [^e50d1-2] [^57528-5] -->
## FFI Surface Specification

docs/ffi-surface.md must enumerate every exported C symbol in crates/nmp-core/src/ffi.rs, every boundary-crossing type, every capability trait, and the ownership/lifetime invariant of each, cross-checked against the RMP bible and ADR-0010. [^e50d1-3]

## FFI Stress Harness

The ffi-stress simulator run captures per-scenario p50/p95/p99 marshal time, allocation counts, dropped-message counts, and the gate pass/fail. The ffi-stress harness includes a `retained_heap_after_drain_bytes ≤ 1 MiB` regression gate. [^e50d1-4]

## Empirical Proof and Doctrine Signoff

The S1 mount-unmount cycle failure (463,207 cycles < 540,000 threshold) is caused by a macOS host-timer sleep artifact, not a kernel leak. The leak-freedom proof relies on structural evidence (S1 net-heap-slope of 0 B/s over 463,207 mount/unmount cycles with 0 unmatched refcounts) because xcrun xctrace did not produce a parseable Instruments trace in this environment. The doctrine signoff passes D0–D7, logs an exception for D8 (working-set bounded fails via the S2 RSS overrun), and defers D7 capability-socket re-review. Re-verification of ffi/capability.rs against D1/D5 is needed because it landed on master after the pin time, introducing a Rust-allocates-caller-frees ownership contract on free_string. All production FFI code is D6-compliant — `unwrap()`/`expect()` calls exist only inside `#[cfg(test)]` blocks; production paths use `unwrap_or_default()`, `unwrap_or_else()`, `lock().ok()`, or `let Ok(...) = ... else { return; }` patterns. The `capability_socket` module uses the exemplary `lock().ok().and_then(|guard| *guard)` pattern for D6-safe mutex access, handling poisoned mutexes gracefully. All `unsafe` blocks are confined to FFI call sites — no `unsafe` exists outside the canonical FFI boundary. The 65 panics identified in nmp-core do not require blanket auditing; D6 already governs production paths, and the real gap is that host-supplied closures must be `catch_unwind`-wrapped. D15 lint requires that closures from `Box<dyn Fn>` be `catch_unwind`-wrapped at the call site, including `ActionRegistry::deliver_result`, `event_observer.rs`, and `raw_event_observer.rs`. The actor command drain intentionally must NOT be `catch_unwind`-wrapped; internally-generated commands should panic-loud. TODOs to close include: `subs/mod.rs:93` (`coverage_hook` never installed), `identity.rs:391` (NIP-46 not behind `AuthSignerFn`), `inbox.rs:167` (bunker error disambiguation), and `marmot/interest.rs:83,112` (missing `limit` field).

<!-- citations: [^e50d1-5] [^12b3f-4] [^1c093-6] -->
## See Also

