---
title: Type-State Pipelines and Compile-Time Enforcement
slug: type-state-pipelines
topic: code-architecture
summary: State machine transitions should use compile-time-enforced type pipelines rather than runtime checks where applicable
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

# Type-State Pipelines and Compile-Time Enforcement

## Type-State Pipelines

State machine transitions should use compile-time-enforced type pipelines rather than runtime checks where applicable. For example, the type system should prevent signing data that has not yet been hashed. Events should use separate structs for distinct lifecycle stages (UnsignedEvent, Event, etc.) rather than Option fields, making the pipeline visible in the type system. Immutability should be the default; builders should use consuming-self chains rather than mutable setter methods.

<!-- citations: [^954c5-9] [^954c5-28] -->
