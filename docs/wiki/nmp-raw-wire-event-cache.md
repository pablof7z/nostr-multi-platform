---
title: NMP Raw Wire Event Cache & Verbatim JSON Display
slug: nmp-raw-wire-event-cache
summary: A RawEventCacheObserver registers before app startup to cache verbatim NIP-01 wire JSON for every event accepted via the relay ingest pipeline, keyed by event i
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-27
updated: 2026-05-27
verified: 2026-05-27
compiled-from: conversation
sources:
  - session:ba231ac9-fb08-4dce-bf60-7166eed32e5b
---

# NMP Raw Wire Event Cache & Verbatim JSON Display

## Wire-Event Cache

A RawEventCacheObserver registers before app startup to cache verbatim NIP-01 wire JSON for every event accepted via the relay ingest pipeline, keyed by event id. The observer fires at the same point in the relay ingest pipeline (kernel/ingest/mod.rs::handle_event) that populates the timeline, ensuring complete coverage of displayed events. [^ba231-1]


The 'View raw event' modal must display the exact verbatim NIP-01 JSON as it was received from the relay wire, including the `sig` and `tags` fields. The 'View raw event' palette action retrieves the raw event JSON by calling a `raw_event_json` lookup on the runtime using the event id, rather than serializing the internal NMP projection card. [^ba231-2]
## See Also

