---
title: Error Enum Granularity
slug: error-enum-granularity
topic: code-architecture
summary: Each module should use a single error enum (KeyError, EncryptionError, EventError) rather than per-operation error types, deferring granularity until it earns i
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

# Error Enum Granularity

## Module Error Enums

Each module should use a single error enum (KeyError, EncryptionError, EventError) rather than per-operation error types, deferring granularity until it earns its place. <!-- [^954c5-17] -->
