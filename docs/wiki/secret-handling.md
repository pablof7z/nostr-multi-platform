---
title: Secret Material Handling and Debug Safety
slug: secret-handling
topic: security
summary: NMP must audit that nmp-core never Debug- or Display-formats secret material into logs, routing traces, or snapshots
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-12
updated: 2026-06-12
verified: 2026-06-12
compiled-from: conversation
sources:
  - session:954c56b2-d292-4021-8b55-977d3fd8df4d
---

# Secret Material Handling and Debug Safety

## Secret Handling

SecretKey must not implement Display; custom redacted Debug should live at the type where the material resides, forcing explicit to_hex()/to_nsec() calls. NMP must audit that nmp-core never Debug- or Display-formats secret material into logs, routing traces, or snapshots. This is especially critical because rust-nostr's SecretKey derives Debug directly, which risks leaking secrets via default formatting.

<!-- citations: [^954c5-7] [^954c5-26] -->
