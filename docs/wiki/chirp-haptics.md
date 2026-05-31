---
title: Chirp Haptics
slug: chirp-haptics
summary: Chat send actions in GroupChatView, DmConversationView, and MarmotGroupChatView trigger a light haptic on send
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

# Chirp Haptics

## Haptic Feedback

Chat send actions in GroupChatView, DmConversationView, and MarmotGroupChatView trigger a light haptic on send. Note publish in ComposeView triggers a success haptic; follow triggers a medium haptic; unfollow triggers a light haptic. The haptic system across Chirp uses consistent weights: soft for like, light for chat send and unfollow, medium for follow, success for publish. [^19e07-6]

## See Also

