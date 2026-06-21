---
title: TUI DM View
slug: tui-dm-view
topic: tui
summary: DM conversations provide is_outgoing pre-classified per message and sender_pubkey authenticated via NIP-44 seal, not from tags.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-06-18
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# TUI DM View

## DM Conversations

DM conversations provide is_outgoing pre-classified per message and sender_pubkey authenticated via NIP-44 seal, not from tags. <!-- [^4f377-3] -->

The Chats tab uses a master-detail layout with conversation list on the left and transcript on the right, with 'i' to start inline composing. <!-- [^93c59-4] -->


DmInboxLookup on ProtocolCommandContextParts is left as-is; it is a Noop D15 capability, not a D0 per-kind leak. <!-- [^11850-129] -->
## DM Compose

The DM compose strip is inline at the bottom of the transcript pane (Pattern C-inline), showing the conversation above and a 3-row compose area below, with Ctrl+Enter to send and Esc to cancel. <!-- [^93c59-5] -->
