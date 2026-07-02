---
title: Relay Route Provenance and Access Control
slug: relay-provenance
topic: write-pipeline
summary: "Every explicit relay route must carry a typed provenance class: automatic, host-pin, verified-private-inbox, manual, imported, or diagnostic"
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
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
---

# Relay Route Provenance and Access Control

## Relay Provenance

Every explicit relay route must carry a typed provenance class: automatic, host-pin, verified-private-inbox, manual, imported, or diagnostic. Private relay routes fail closed without verified-inbox provenance. Publish route provenance (ADR-0071) is threaded through the entire write stack, and `PublishRaw` is banned from starter and developer-experience paths. WRITE-004 deletes the anonymous route default (the `Default` for `PublishRouteClass`) so a publish can never silently route without declared provenance.

<!-- citations: [^3c942-dbf3e] [^898a4-db356] -->
