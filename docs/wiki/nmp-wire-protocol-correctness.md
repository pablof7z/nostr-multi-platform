---
title: NMP Wire Protocol & Filter Serialization Correctness
slug: nmp-wire-protocol-correctness
summary: "The hand-rolled `filter_json_for` in `subs/wire.rs` must be replaced with `nostr::Filter::as_json()`"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-23
updated: 2026-05-27
verified: 2026-05-23
compiled-from: conversation
sources:
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:e4861768-9a00-4d83-b7a3-a39d07749d1c
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# NMP Wire Protocol & Filter Serialization Correctness

## Wire Protocol Correctness

The hand-rolled `filter_json_for` in `subs/wire.rs` must be replaced with `nostr::Filter::as_json()`. The current implementation poses a real correctness risk because it performs no escaping for tag values. A typed-tables follow-up is tracked to resolve the ~1.8× wire size and ~3× encode time hot-path regression caused by the added JSON serialization pass. V-38 has a sub-item tracking the #[ignore] conformance test in `nmp-nip47/tests/nip47_tag_conformance.rs:14-16` that is blocked on `Kernel::new_for_test()` not being publicly exported. [^1670f-16] [^e4861-9] [^v38-125] [^v38-158]

<!-- citations: [^1670f-16] [^e4861-9] [^cd2b6-19] -->
## See Also

