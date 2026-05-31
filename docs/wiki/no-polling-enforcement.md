---
title: No Polling Enforcement Across All Layers
slug: no-polling-enforcement
summary: The codebase must not contain any polling patterns in any layer—including Rust channels, iOS timers, and background tasks
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-29
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:cb671af9-5784-4174-9c3d-d10151d9fb01
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
---

# No Polling Enforcement Across All Layers

## Absolute Prohibition

The codebase must not contain any polling patterns in any layer—including Rust channels, iOS timers, and background tasks. Forbidden patterns include sleep+check loops, timer-based state queries, try_recv+sleep spin loops, and background Task sleep loops. Nostr apps should be event-driven with reactive UIs. Edge-triggered functions with 'poll' in their name are compliant if they are driven by an existing actor idle tick or wall-clock-gated observer rather than a dedicated sleep loop.

<!-- citations: [^cb671-1] [^d0690-9] -->
## Rust Layer Remediations

Rust relay_client must use try_recv() instead of recv_timeout(0ms) drain loops. Rust nip46 handle must use SignerOp::wait() instead of poll() + sleep(10ms) loops. Rust fanout workers must use blocking recv() instead of try_recv() + sleep(50ms) spin loops. [^cb671-2]

## iOS Layer Remediations

NetworkSettingsStore must use the applyStatus() event hook instead of a 2-second Task.sleep loop polling refreshDiagnostics(). BookScannerModel must accumulate data in the metadata delegate with a 500ms debounced clear instead of a 0.25s Timer polling detectedBoxes. PodcastPlayerStore must use a wall-clock gate inside an existing 0.25s time observer instead of a 5-second sleep loop for position persistence. [^cb671-3]

## Repository Documentation Requirements

The AGENTS.md file at the repository root must contain a brief, top-level 'No polling — ever' section covering all three layers with concrete forbidden patterns. The D8 row in 03-doctrine-d0-d8.md must explicitly list sleep+poll loops as forbidden alongside allocations and false wakes. The anti-pattern section in 06-reactivity-contract.md must be expanded from UI-only scope to a full-stack polling prohibition covering Rust, iOS, and test-helper examples. A memory note (feedback_no_polling.md) indexed in MEMORY.md must exist so agents are reminded of the no-polling rule on every dispatch. [^cb671-4]
## See Also

