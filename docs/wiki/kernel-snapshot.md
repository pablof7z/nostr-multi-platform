---
title: Kernel Snapshot
slug: kernel-snapshot
topic: kernel-snapshot
summary: The kernel omits Unchanged projection rows entirely from the wire, keeps an explicit payload-less Cleared row, and keeps full Changed rows.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-14
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
---

# Kernel Snapshot

## Wire Format for Snapshot Deltas

The kernel omits Unchanged projection rows entirely from the wire, keeps an explicit payload-less Cleared row, and keeps full Changed rows. <!-- [^78c8e-485] -->
