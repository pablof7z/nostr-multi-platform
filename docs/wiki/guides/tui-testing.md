---
title: TUI Testing
slug: tui-testing
topic: tui
summary: All new features in chirp-tui must be tested end-to-end using rexpect with real relays and relay code
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-26
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:f9938ae5-cc1b-4aaa-a6cb-6212e31dacf6
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:64f3e239-c4c1-4c32-82de-458516b28418
---

# TUI Testing

## Testing Strategy

All new features in chirp-tui must be tested end-to-end using rexpect with real relays and relay code. Tests must explicitly set PTY dimensions (e.g. stty rows 40 cols 120 on the slave PTY), otherwise ratatui sees a 0-column terminal and renders an empty frame. The status bar is the reliable assertion point for rexpect tests because it updates synchronously on every action with predictable strings, whereas network-received content (note IDs, pubkeys) is non-deterministic. The onboarding flow is a state machine with phases: welcome (choose create/import/bunker/browse) → relay picker → done, each emitting a unique status string for e2e tests. The NMP runtime delivers at least one snapshot update even without an explicit relay argument, likely from cached or local data, so navigation-flow tests do not require network. No local relay exists in the repository yet; real-relay tests use wss://relay.damus.io, and deterministic content tests require a local strfry or nostr-rs-relay fixture. The SharedSnapshot parser must unwrap the {t:snapshot,v:...} wire envelope before reading projections, metrics, and relay data; FeatureSnapshot already does this correctly. chirp-repl reads the snapshot synchronously immediately after firing REQs without waiting, causing 0 results even though events exist on the relay; session.wall (8s) is displayed in diagnostics but never used as a sleep.

<!-- citations: [^4f377-29] [^4f377-30] [^4f377-31] [^f9938-1] [^93c59-20] [^86221-11] [^64f3e-8] -->
