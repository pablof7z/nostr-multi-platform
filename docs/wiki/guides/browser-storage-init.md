---
title: Browser Durable Storage Initialization
slug: browser-storage-init
topic: data-persistence
summary: Browser durable storage (Worker/OPFS) must initialize before the product starts
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
---

# Browser Durable Storage Initialization

## Storage Initialization

Browser durable storage (Worker/OPFS) must initialize before the product starts. A silent fallback to in-memory storage cannot count as a successful initialization. <!-- [^3c942-39fb3] -->
