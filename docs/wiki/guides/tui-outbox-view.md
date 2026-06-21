---
title: TUI Outbox View
slug: tui-outbox-view
topic: tui
summary: "The Outbox pane in Settings has two sections: an active (in-flight) section navigable with `j`/`k` and `Enter`/`Esc`, and a read-only 'ââ Published ââ'"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-26
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:fbebb78b-07ed-4e26-8e2e-56fb66929a63
  - session:7174d4d4-371b-4b8e-87a6-91024c2b4c2a
---

# TUI Outbox View

## Outbox View

The Outbox pane in Settings has two sections: an active (in-flight) section navigable with `j`/`k` and `Enter`/`Esc`, and a read-only '── Published ──' history section showing all settled events (kind:0, kind:10002 on account creation, reactions, notes, relay lists) newest-first — not just active publishes. The OutboxLine struct must include a `relays: Vec<OutboxRelayLine>` field with `relay_url`, `status_label`, `reason`, and `message` sub-fields parsed from the kernel JSON projection. chirp-tui must add an `outbox_selected` field to `AppState` and render a per-relay breakdown (URL, status dot, reason, message) when an item is selected. The publish history pane caps at 20 entries in the TUI parser; the kernel caps publish_queue at 16 entries (`MAX_PUBLISH_WINDOW`). The TUI publish history parser filters out `accepted_locally` rows so in-flight publishes never appear in both the active outbox and the history pane.

<!-- citations: [^fbebb-10] [^7174d-13] -->
