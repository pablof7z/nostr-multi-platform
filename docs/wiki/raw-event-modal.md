---
title: RawEventModal — View Raw Event JSON
slug: raw-event-modal
summary: The `/` palette action includes a "View raw event" option that opens a scrollable modal overlay showing the raw NMP card JSON for the selected event.
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
  - session:ba231ac9-fb08-4dce-bf60-7166eed32e5b
---

# RawEventModal — View Raw Event JSON

## Overview

The `/` palette action includes a "View raw event" option that opens a scrollable modal overlay showing the raw NMP card JSON for the selected event. If the detail pane cursor is on a reply (detail_cursor > 0), "View raw event" shows the raw card for that reply row rather than the selected root. The modal displays the exact NIP-01 JSON as it arrived on the wire, including `sig` and `tags`, not a reconstruction or projection. Internal framework fields such as `relation_counts`, `content_tree`, and `author_display` are excluded from the raw event display.

<!-- citations: [^a889f-7] [^a889f-8] [^ba231-1] -->
## Modal Layout

The RawEventModal renders an 80% screen overlay with a border, a title hint, and scrollable pretty-printed JSON content. [^a889f-9]

## Keyboard Controls

The RawEventModal supports Esc/q to close and j/k or arrow keys to scroll. [^a889f-10]

## Raw Event Cache

A RawEventCacheObserver caches verbatim NIP-01 wire JSON for every accepted inbound event, keyed by event id. The observer registers at startup before `nmp_app_start` to ensure no timeline events are missed. The "View raw event" action looks up the cached raw event by event id via a `raw_event_json(event_id)` method rather than using the NMP projection card. [^ba231-2]
## See Also

