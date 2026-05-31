---
title: Relay Settings UI
slug: relay-settings-ui
summary: Relay settings is a dedicated view accessible from a NavigationLink in SettingsHubView, showing the relay count as a subtitle.
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
  - session:6e4c3a3a-9515-4437-a4bf-b4228a10ae57
---

# Relay Settings UI

## Navigation & Access

Relay settings is a dedicated view accessible from a NavigationLink in SettingsHubView, showing the relay count as a subtitle. [^87fd4-5]


The relay settings view has a toolbar + button to add new relays via a sheet. [^87fd4-6]


The Settings > Relays pane uses a three-tier fallback for its relay data: primary `state.features.relay_edit_rows`, fallback `state.relays`, and final fallback `relay_lines(state)`. [^6e4c3-3]
## Relay List

Each configured relay row displays a colored badge per assigned capability. Swipe-to-delete is supported on relay rows. [^87fd4-7]

## Capability Badge Colors

Role color mapping: Read is blue, Write is green, Indexer is ChirpColor.zap (orange), Wallet is purple, Both is ChirpColor.accent. [^87fd4-8]

## Add / Edit Sheet

The relay add/edit sheet shows four independent toggles for Read, Write, Indexer, and Wallet rather than a single-choice picker. When editing an existing relay, the URL field is read-only and the role toggles are pre-populated with the relay's current capabilities. [^87fd4-9]

If `relay_edit_rows` is empty, relay bootstrap falls back to hardcoded constants: `wss://relay.primal.net` for content relays and `wss://purplepag.es` for indexer relays. [^6e4c3-4]
## See Also

