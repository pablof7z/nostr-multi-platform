---
title: Testable Time and Clock Seams
slug: testable-time
topic: test-infrastructure
summary: Testable-time APIs should expose the explicit-time variant as the primitive (e.g., `is_expired_at(now)`) with the wall-clock version as a convenience wrapper.
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

# Testable Time and Clock Seams

## API Design

Testable-time APIs should expose the explicit-time variant as the primitive (e.g., `is_expired_at(now)`) with the wall-clock version as a convenience wrapper. <!-- [^954c5-8] -->
