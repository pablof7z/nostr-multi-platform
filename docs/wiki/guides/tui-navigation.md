---
title: TUI Navigation
slug: tui-navigation
topic: tui
summary: "Miller columns (3-pane: relay list â feed â event detail) must be used for hierarchical navigation, modeled on ranger"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-25
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
---

# TUI Navigation

## Navigation

Miller columns (3-pane: relay list → feed → event detail) must be used for hierarchical navigation, modeled on ranger. Fuzzy finding must use nucleo (not skim) for lower allocation and async-friendly search-as-you-type over npubs, channels, and hashtags.

<!-- citations: [^4f377-21] [^4f377-22] [^93c59-13] -->

## Title Bar

The title bar shows the active account name, tab labels with unread badges, relay health dot and count, and the current time. Tab labels show persistent badges: •N for unread count (cyan), ● for active connection (green/red), ⚠ for attention needed (yellow). <!-- [^93c59-14] -->

## Home Tab

The chirp-tui Home tab uses a 38/62 horizontal split between post list and detail pane, with a relay health panel in the bottom 25% of the left column. <!-- [^93c59-15] -->

## Wallet Tab

The Wallet tab has two visual states: disconnected (shows 'No wallet connected' with 'Press n to connect' hint) and connected (shows balance, recent transactions, and key hints for pay/send/receive/disconnect). <!-- [^93c59-16] -->

## Settings Tab

The Settings tab uses a master-detail layout with sections (Account, Relays, Outbox, Keys, Appearance, About) on the left and section content with inline editors on the right. <!-- [^93c59-17] -->
