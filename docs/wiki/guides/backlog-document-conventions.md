---
title: Backlog Document Conventions
slug: backlog-document-conventions
topic: developer-workflow
summary: Completed items are removed from the backlog document rather than kept, preventing the file from becoming super long
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-27
updated: 2026-06-18
verified: 2026-05-27
compiled-from: conversation
sources:
  - session:af1b9182-4b24-4f78-9fed-67a2d68b5718
  - session:019edc01-fdde-7b20-a348-5a2a9ce1a0f9
  - session:019edc13-83b1-7143-8631-b0e695ea4afd
  - session:019edc3c-53d4-73a0-8c42-a6b88a318e8c
  - session:019edc4d-4175-7441-b5af-cb2012068335
  - session:019edc84-6e5c-74a2-9ed9-57938dae31a1
  - session:019edc92-b628-7ce1-be8a-c3d1013f2969
---

# Backlog Document Conventions

## Backlog Document Conventions

Completed items are removed from the backlog document rather than kept, preventing the file from becoming super long. The backlog document's active sections contain only open work; fully-done violations, closed pending decisions, and completed features are removed entirely. Backlog items with partial completion have their done stages/history trimmed, leaving only the remaining open work. Open violations, pending decisions, and active features in the backlog are preserved verbatim. Existing issues must be edited in place rather than appending parallel ones; append-only history files are explicitly historical and new ones must not be invented. Plans must not survive as reference documentation after they have been implemented, executed, or invalidated; lasting knowledge belongs in durable documentation (docs/aim.md, docs/product-spec/, docs/architecture/, docs/design/, docs/decisions/, builder guide, wiki) instead of planning files. Short-lived migration plans may live in docs/architecture-audit/<plan>.md only when they gate a specific active milestone or violation and link back to the owning issue. Executed plans must be retired; they are no longer a source of truth, and only the smallest remaining live follow-up issue or durable lesson should be preserved in the owning durable doc.

<!-- citations: [^af1b9-1] [^019ed-19] [^019ed-67] [^019ed-75] [^019ed-85] [^019ed-107] [^019ed-131] -->
