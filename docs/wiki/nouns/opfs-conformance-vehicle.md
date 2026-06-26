---
type: noun-entry
slug: opfs-conformance-vehicle
name: "OPFS conformance vehicle"
origin: extracted
source_refs:
  - transcript:217-217
  - transcript:643-677
---

# OPFS conformance vehicle

dedicated-Worker test harness running in headless Chromium (Playwright) that exercises OpfsSqliteStore byte-for-byte against MemEventStore/LmdbEventStore expectations; the sole mitigation for the backend's novel Worker-only risk (ADR-0054 §8)
