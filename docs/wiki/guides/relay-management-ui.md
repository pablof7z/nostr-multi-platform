---
title: Relay Management UI
slug: relay-management-ui
topic: ui-components
summary: Relay management is a dedicated view accessed from Settings via a NavigationLink, not inlined in SettingsHubView
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-26
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:87fd49fb-4869-4c40-9a6a-96545bd2313d
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:fbebb78b-07ed-4e26-8e2e-56fb66929a63
---

# Relay Management UI

## Relay Management UI

Relay management is a dedicated view accessed from Settings via a NavigationLink, not inlined in SettingsHubView. The relay settings view has a toolbar + button to add a new relay via a sheet. Tapping an existing relay opens an edit sheet where the URL is read-only and roles can be changed. Relays can be deleted via swipe-to-remove. The Swift relay sheet shows four independent toggles (Read, Write, Indexer, Wallet) instead of a single forced-choice picker. The `NostrRelayList` SwiftUI component renders relay URLs with `wss://` stripped, animated connection status dots (green=connected, orange/pulsing=connecting, red=error, gray=unknown), and role badges using semantic color tokens. The relay panel shows a per-relay event count as a right-aligned dim counter that ticks up live every 250ms, with no counter shown for disconnected or connecting relays. The Settings > Relays panel must display relay connection status using real diagnostics data (connection_label from state.relays), not the role_label field.

<!-- citations: [^87fd4-1] [^93c59-2] [^45258-25] [^fbebb-9] -->
