---
title: Feed Pagination
slug: feed-pagination
topic: event-acquisition
summary: Feed pagination caps at MAX_FEED_WINDOW_LIMIT (500)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# Feed Pagination

## Feed Pagination

Feed pagination caps at MAX_FEED_WINDOW_LIMIT (500). The load_older operation grows one page at a time and returns false at the ceiling, and snapshot_json reads the live limit. <!-- [^02745-56] -->
