---
title: TUI Hotlist
slug: tui-hotlist
topic: tui
summary: "Unread items must be prioritized in a hotlist bar following the order: mentions/zaps > DMs > reactions > noise, modeled on weechat's hotlist."
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

# TUI Hotlist

## Hotlist Priority

Unread items must be prioritized in a hotlist bar following the order: mentions/zaps > DMs > reactions > noise, modeled on weechat's hotlist. <!-- [^4f377-10] -->

## Groups Tab

The Groups tab uses a single mixed list with NIP-29 groups marked [#] in blue (REPLY_COLOR) and Marmot MLS groups marked [E] in yellow (ZAP color), not separated into two sub-lists. <!-- [^93c59-7] -->
