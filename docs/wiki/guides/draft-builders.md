---
title: Draft Builder Composability and Side-Effect Limits
slug: draft-builders
topic: publish-workflow
summary: "Event construction is composable: template event builders (such as react_to_event or reply_to_event) produce unsigned draft events, and the publish action may t"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:019f0dc3-5b56-79d1-a14b-5746c93e5879
---

# Draft Builder Composability and Side-Effect Limits

## Draft Builders

Event construction is composable: template event builders (such as react_to_event or reply_to_event) produce unsigned draft events, and the publish action may trigger further envelope mutations such as adding an h tag for NIP-29. Draft builders are allowed and composable but must not sign, publish, or choose relays as side effects.

<!-- citations: [^3c942-dbf3e] [^019f0-db308] -->
