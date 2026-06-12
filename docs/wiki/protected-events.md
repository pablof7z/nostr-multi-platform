---
title: Protected Events and Tag Enforcement
slug: protected-events
topic: nostr-protocol
summary: "Protected events provide only a boolean predicate (`is_protected`) and a `Tag::protected()` constructor"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-12
updated: 2026-06-12
verified: 2026-06-12
compiled-from: conversation
sources:
  - session:954c56b2-d292-4021-8b55-977d3fd8df4d
---

# Protected Events and Tag Enforcement

## Protected Events

Protected events provide only a boolean predicate (`is_protected`) and a `Tag::protected()` constructor. Enforcement belongs at the relay/network layer, not the tag layer. <!-- [^954c5-24] -->
