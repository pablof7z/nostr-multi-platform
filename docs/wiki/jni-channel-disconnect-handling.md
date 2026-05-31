---
title: JNI Channel Disconnected Handling
slug: jni-channel-disconnect-handling
summary: "When receiving from the channel inside the JNI loop, RecvTimeoutError::Disconnected must break the loop because the channel is closed"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-26
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
---

# JNI Channel Disconnected Handling

## JNI Channel Disconnect Handling

When receiving from the channel inside the JNI loop, RecvTimeoutError::Disconnected must break the loop because the channel is closed. In contrast, RecvTimeoutError::Timeout is a normal idle tick and the loop should continue running. [^f2605-10]

## See Also

