---
title: Protocol Ingest Safety
slug: protocol-ingest-safety
topic: protocol-ingest-safety
summary: NWC `url_decode` casts arbitrary bytes to `char` via `bytes[i] as char`, producing ill-formed Unicode for percent-encoded multi-byte UTF-8 sequences and potenti
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

# Protocol Ingest Safety

## Known Ingest Vulnerabilities

NWC `url_decode` casts arbitrary bytes to `char` via `bytes[i] as char`, producing ill-formed Unicode for percent-encoded multi-byte UTF-8 sequences and potentially connecting to a different host than configured. NIP-77 NegMsg accepts relay-controlled hex strings with no inbound size cap, allowing up to 32MiB heap allocations per message. Description hashes reject inputs longer than 8192 characters by returning None. Bolt11 invoice strings must be capped at 8192 characters before allocation to prevent DoS via unbounded Vec<Fe32>.

<!-- citations: [^02745-17] [^02745-47] [^02745-65] -->
