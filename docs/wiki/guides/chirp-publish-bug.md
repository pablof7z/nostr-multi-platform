---
title: "Chirp Publish Bug: Silent Write Failures (chirp#69)"
slug: chirp-publish-bug
topic: write-pipeline
summary: "chirp#69 is a systemic write-publish bug: replaceable event kinds â kind:0 (profile edit), kind:3 (follow/unfollow), and kind:6 (repost) â silently fail to"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Chirp Publish Bug: Silent Write Failures (chirp#69)

## Summary

chirp#69 is a systemic write-publish bug: replaceable event kinds — kind:0 (profile edit), kind:3 (follow/unfollow), and kind:6 (repost) — silently fail to publish to the relay while the Outbox falsely reports "All published." Regular kind:1 notes and replies publish correctly.

<!-- citations: [^dcc80-57115] [^dcc80-4e59e] -->
