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
  - session:d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Relay Route Provenance and Access Control

## Relay Provenance

Every explicit relay route must carry a typed provenance class: automatic, host-pin, verified-private-inbox, manual, imported, or diagnostic. Private relay routes fail closed without verified-inbox provenance. Publish route provenance (ADR-0071) is threaded through the entire write stack, and `PublishRaw` is banned from starter and developer-experience paths. WRITE-004 deletes the anonymous route default (the `Default` for `PublishRouteClass`) so a publish can never silently route without declared provenance. No code path treats kind:17375 relay tags as authoritative relay selection. The relay-authority fix in #2874 stopped `build_wallet_event` from emitting relay tags and made `publish_nutzap_info` take an explicit `relays` parameter; by design and NIP-60 reality, kind:10019/NIP-65 is authoritative and `legacy_relay_hint` is decode-only. PR #2870 item 1 — killing the legacy_relay_hint leak to authoritative kind:10019 relay seeding — must land before Phase 1 wires `publish_nutzap_info`. `InterestShape.relay_pin` is the existing generic relay-pinning field that pins an arbitrary filter to client-specified relays for one-shot use; it is planner-enforced to never merge (lattice Rule 9) and is never serialized onto the wire's relay model. Issue #2970 (NIP-17 wss-only gate blocks `nak serve`) is deferred to post-v1 — the parser gate must NOT be relaxed. Correct closures for local `ws://` testing are an in-workspace integration test injecting a `ws://` DmRelayCache entry directly, or a cert-trusted wss local-relay recipe. When an account has no relay list to resolve against, a publish attempt fails closed with a Rust-owned diagnostic ("pre-signed publish target rejected: requires an explicit non-empty imported/protocol relay target") rather than crashing.

<!-- citations: [^3c942-dbf3e] [^898a4-db356] [^5ad70-e664c] [^91a86-7fec6] [^91a86-6e442] [^fb992-c5e09] [^d8bc6-49c52] [^dcc80-f0386] -->
