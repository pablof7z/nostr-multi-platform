---
title: D8 — No Polling, Ever
slug: d8-no-polling-ever
summary: Polling (sleep+check loops) is forbidden at every layer of the stack — this is one of the project's hardest rules, codified as doctrine D8 and enforced in CI via doctrine-lint.
tags:
  - doctrine
  - architecture
  - reactivity
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
---

# D8 — No Polling, Ever

> Polling (sleep+check loops) is forbidden at every layer of the stack — this is one of the project's hardest rules, codified as doctrine D8 and enforced in CI via doctrine-lint.

## Canonical Rule

Polling is forbidden at every layer of the stack. This means no `sleep` + check loops, no `Timer.scheduledTimer` querying state, no `try_recv` + `sleep` spin loops, no `Task { while !cancelled { sleep; checkState() } }` tasks. [^d0690-1]

## Approved Alternatives

Instead of polling, use: in Rust — block with `recv()` / `recv_timeout()`, drain with `try_recv()` (but never inside a sleep loop); on iOS — consume kernel-pushed `ViewBatch` snapshots, use `AVFoundation` / `NWPathMonitor` / `NotificationCenter` callbacks; for background persistence — piggy-back on an existing event tick with a wall-clock gate, never a parallel sleep loop. [^d0690-2]

## Edge-Triggered Functions with 'Poll' in the Name Are Permitted

Functions named with 'poll' are D8-compliant when they are driven by the actor's existing idle tick or wall-clock-gated observer, not by a dedicated `sleep` loop. Example: `Kernel::poll_claim_expansion` is edge-triggered and passes. [^d0690-3]

## Enforcement

D8 is enforced by the doctrine-lint tool (at `crates/nmp-testing/bin/doctrine-lint/`) as part of CI. The rule targets hot-path allocations and polling patterns via grep-based static analysis against code-pattern fixtures. Live violations are actively tracked in the backlog (e.g. V-91, V-54). [^d0690-4]

## Documentation Locations

D8 is documented in: `AGENTS.md:139` (canonical contributor statement), `docs/plan.md:77`, `docs/architecture-review-gate.md:17` (rejection criterion), and `docs/builder-guide/06-reactivity-contract.md` (full rationale with Doctrine D8 designation). [^d0690-5]

## See Also
- [[adr-0025-bespoke-ffi-anti-pattern|ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection]] — related guide
- [[podcast-player-polling-incident|Podcast-Player Polling Incident — Second-App ADR-0025 Anti-Pattern]] — related guide
- [[doctrine-lint|Doctrine Lint Tool — D0–D16 Rules and Missing D17]] — related guide
- [[d1-snapshot-before-relay-io|D1 Doctrine — First Snapshot Must Precede Relay I/O]] — related guide
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[architectural-compliance-verification-gate|Architectural Compliance Verification Gate — Verify Before Implementing]] — related guide

