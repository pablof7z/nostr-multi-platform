---
title: Kind Trait Design
slug: kind-trait-design
topic: data-modeling
summary: Kinds should use a trait, not a centralized enum, so each domain module can add its own kinds independently without creating a bottleneck.
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

# Kind Trait Design

## Kind Definition

Kinds should use a trait, not a centralized enum, so each domain module can add its own kinds independently without creating a bottleneck. <!-- [^954c5-21] -->
