---
title: Watermark Rewrite and Multi-Author Shapes
slug: watermark-rewrite
topic: watermark-rewrite
summary: The watermark rewrite for multi-author shapes now uses per-author AuthorKind(limit=1) queries against the B-tree index instead of the author-blind KindTime glob
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-11
updated: 2026-06-12
verified: 2026-06-11
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# Watermark Rewrite and Multi-Author Shapes

## Watermark Rewrite

The watermark rewrite's `KindTime` branch was author-blind for multi-author shapes, flooring `since` at the newest event from any author, causing new follows' past notes to never be fetched. This was fixed with per-author `AuthorKind(limit=1)` B-tree index lookups that return `None` for zero-author shapes and the minimum timestamp across all authors for multi-author shapes. The watermark rewrite stays, guarded by the invariant that no watermark floor is applied without replay coverage for the same shape.

<!-- citations: [^da6b1-22] [^da6b1-37] [^da6b1-58] [^da6b1-71] [^da6b1-84] [^da6b1-93] -->
