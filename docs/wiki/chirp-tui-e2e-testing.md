---
title: Chirp TUI End-to-End Testing with Rexpect
slug: chirp-tui-e2e-testing
summary: All new features in chirp-tui MUST be tested end-to-end using rexpect with real relays and relay code
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-27
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:f9938ae5-cc1b-4aaa-a6cb-6212e31dacf6
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:9e632bcb-fecc-4cda-a228-9a09e8db07ed
---

# Chirp TUI End-to-End Testing with Rexpect

## End-to-End Testing

All new features in chirp-tui MUST be tested end-to-end using rexpect with real relays and relay code. Deterministic content tests require a local strfry or nostr-rs-relay fixture; no local relay exists in the repo yet. [^f9938-1]


## PTY Configuration

PTY size must be explicitly set (e.g. `stty rows 40 cols 120` on the slave PTY) when testing chirp-tui, otherwise ratatui sees a 0-column terminal and renders an empty frame. [^f9938-2]

## Assertion Strategy

The status bar is the reliable assertion point in chirp-tui tests because it updates synchronously on every action with predictable strings. After the Home tab redesign removed the Feed block title, e2e tests assert 'Relays' instead of the old 'Feed' string. Chirp-tui prefers the typed NOFS path and falls back to the generic RootFeedSnapshot; the NFTS-for-feed test wiring is retired.

<!-- citations: [^f9938-3] [^93c59-1] [^9e632-1] -->
## See Also

