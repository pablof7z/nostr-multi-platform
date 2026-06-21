---
title: TUI Thread View
slug: tui-thread-view
topic: tui
summary: Thread parent linkage is only implicit in the block structure (Module variant groups root-first, newest-last); there is no explicit reply_to tag in the card
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-26
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:b48d81e1-411c-45db-a440-340bcaee2631
  - session:fa300009-e498-4c80-a2d3-64d1531a09d4
---

# TUI Thread View

## Thread Parent Linkage

Thread parent linkage is only implicit in the block structure (Module variant groups root-first, newest-last); there is no explicit reply_to tag in the card. <!-- [^4f377-32] -->

## Thread Display Layout

Nostr threading uses a depth-indented flat view rather than a tree pane, because NIP-10 produces a DAG rather than a clean tree. Reply and new note composition replace the right pane (Pattern C-detail), keeping the feed list visible on the left, with quoted parent text at the top of the composer. TimelineRow.content stores the full, untruncated note content rather than a 95-character preview, so that both the list view and the detail/reply view receive complete text. The list view (post_list) truncates content to terminal width at render time, making the stored content_preview truncation redundant for that view. The detail/reply view (post_detail) uses wrap_body to word-wrap full text, so it must receive untruncated content to display correctly. The embedded event widget renders inside a full rectangular box (Borders::ALL) rather than a left-side bar. Its preferred_height adds 2 rows for top and bottom borders; inner width is calculated as -2 for borders and -2 for body indentation.

<!-- citations: [^4f377-33] [^93c59-21] [^b48d8-1] [^fa300-2] -->
