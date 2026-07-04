---
title: Relay Route Provenance and Access Control
slug: relay-provenance
topic: write-pipeline
summary: "Every explicit relay route must carry a typed provenance class: automatic, host-pin, verified-private-inbox, manual, imported, or diagnostic"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-07-04
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
  - session:5ad70acc-1442-4343-92a7-f79b2fc59071
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
  - session:fb992e80-b32b-4673-b2c2-40e8044504ee
---

# Relay Route Provenance and Access Control

## Relay Provenance

Every explicit relay route must carry a typed provenance class: automatic, host-pin, verified-private-inbox, manual, imported, or diagnostic. Private relay routes fail closed without verified-inbox provenance. Publish route provenance (ADR-0071) is threaded through the entire write stack, and `PublishRaw` is banned from starter and developer-experience paths. WRITE-004 deletes the anonymous route default (the `Default` for `PublishRouteClass`) so a publish can never silently route without declared provenance. No code path treats kind:17375 relay tags as authoritative relay selection. The relay-authority fix in #2874 stopped `build_wallet_event` from emitting relay tags and made `publish_nutzap_info` take an explicit `relays` parameter; by design and NIP-60 reality, kind:10019/NIP-65 is authoritative and `legacy_relay_hint` is decode-only. PR #2870 item 1 — killing the legacy_relay_hint leak to authoritative kind:10019 relay seeding — must land before Phase 1 wires `publish_nutzap_info`. `InterestShape.relay_pin` is the existing generic relay-pinning field that pins an arbitrary filter to client-specified relays for one-shot use; it is planner-enforced to never merge (lattice Rule 9) and is never serialized onto the wire's relay model.

<!-- citations: [^3c942-dbf3e] [^898a4-db356] [^5ad70-e664c] [^91a86-7fec6] [^91a86-6e442] [^fb992-c5e09] -->
