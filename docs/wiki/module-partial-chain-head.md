---
title: Module Partial Chain Head — Detecting Unresolved Ancestors
slug: module-partial-chain-head
summary: "When a Module block has `root: Some(pointer)`, the first event in `events` is NOT the true thread root — it is the head of a partial chain (a reply whose ancest"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-27
updated: 2026-05-27
verified: 2026-05-27
compiled-from: conversation
sources:
  - session:a889fe39-a56b-4ba4-8fc2-4c202a3ecfbe
  - session:9e632bcb-fecc-4cda-a228-9a09e8db07ed
---

# Module Partial Chain Head — Detecting Unresolved Ancestors

## Partial Chain Heads in Module Blocks

When a Module block has `root: Some(pointer)`, the first event in `events` is NOT the true thread root — it is the head of a partial chain (a reply whose ancestor is unresolved). Approach A (keep depth=0, add `is_partial_chain_head` bool, visual indicator) is the correct architectural fix — not bumping depth to 1, and not using `has_gap` as a proxy. [^a889f-1]



TimelineBlock::Standalone carries an optional root ThreadPointer so reply chains of length 1 do not lose their root reference. [^9e632-3]
## ids_from_block and root Detection

The `ids_from_block` function must read the `root` field from Module blocks to detect partial chain heads, not just `has_gap` and `events`. [^a889f-2]


The grouper removal-collapse path (grouper.rs:269) also dropped the root on length-1 chains; this second instance was fixed alongside the chain-build path. [^9e632-4]
## TimelineRow: is_partial_chain_head Field

TimelineRow includes an `is_partial_chain_head: bool` field set only on the first event of a Module block where `root: Some(_)` is present. [^a889f-3]

## Post List Visual Indicator

The left pane (post_list) renders a `↳ reply in thread` indicator above the author for rows where `is_partial_chain_head` is true, rather than filtering them out entirely. [^a889f-4]

## Serde Deserialization of root

The serde deserialization for Module blocks must handle `root` being absent entirely (not just null) because it uses `skip_serializing_if = "Option::is_none"`. [^a889f-5]
## See Also

