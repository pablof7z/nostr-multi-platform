---
title: Chirp DM Conversation Auto-Scroll
slug: chirp-dm-conversation-auto-scroll
summary: DmConversationView auto-scrolls to the newest message on both initial load and when new messages arrive, using ScrollViewReader with a sentinel bottom anchor
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-21
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
---

# Chirp DM Conversation Auto-Scroll

## Auto-Scroll Behavior

DmConversationView auto-scrolls to the newest message on both initial load and when new messages arrive, using ScrollViewReader with a sentinel bottom anchor. MarmotGroupChatView auto-scrolls to the latest message on initial load, matching the DmConversationView behavior. [^19e07-5]

## See Also

