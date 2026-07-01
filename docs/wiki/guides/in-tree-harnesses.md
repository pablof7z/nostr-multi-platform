---
title: In-Tree Conformance Harnesses and Extracted Apps
slug: in-tree-harnesses
topic: project-status
summary: Gallery stays in-tree as a cross-platform conformance and regression harness â a storybook proving every NMP component decodes and renders on every platform a
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
---

# In-Tree Conformance Harnesses and Extracted Apps

## In-Tree Harnesses

Gallery stays in-tree as a cross-platform conformance and regression harness — a storybook proving every NMP component decodes and renders on every platform at HEAD — not a shippable product. Extraction to an external pinned repo was explicitly rejected; Gallery must remain in-tree permanently and must not be extracted before #2256 (the clean-room DX gate). Chirp is fully extracted to a standalone repo at `github.com/pablof7z/chirp`; in-tree NMP app code is deleted. The governing rule: products live outside the framework repo, conformance harnesses live in-tree.

<!-- citations: [^898a4-ca491] [^3c942-c551d] -->
