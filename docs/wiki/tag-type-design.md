---
title: Tag Type Design
slug: tag-type-design
topic: data-modeling
summary: Tag types should remain a thin wrapper over Vec<String> rather than a heavy enum, because tag meaning is kind-dependent and an enum bakes in a false abstraction
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

# Tag Type Design

## Tag Type Representation

Tag types should remain a thin wrapper over Vec<String> rather than a heavy enum, because tag meaning is kind-dependent and an enum bakes in a false abstraction. <!-- [^954c5-27] -->
