---
title: Stale Publish In-Flight Toast Bug
slug: publish-in-flight-toast
topic: ui-components
summary: A stuck 'publish already in flight' toast must clear when the referenced event's publish acknowledges, not persist on screen and block UI elements
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

# Stale Publish In-Flight Toast Bug

## Known Issue

A stuck 'publish already in flight' toast must clear when the referenced event's publish acknowledges, not persist on screen and block UI elements. A stale publish-in-flight toast can remain stuck on screen when an in-flight-publish tracking entry fails to clear on ack. This is tracked as chirp#54.

<!-- citations: [^dcc80-aeef7] [^dcc80-0deb0] -->
