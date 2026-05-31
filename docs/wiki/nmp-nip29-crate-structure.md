---
title: nmp-nip29 — Crate Structure and Actual Directory Layout
slug: nmp-nip29-crate-structure
summary: The nmp-nip29 crate contains action/, cache/, group_id.rs, interest.rs, kinds.rs, lib.rs, projection/, and register.rs
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-23
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:c3f757f1-6292-4e52-b520-5bb52e7de2bf
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:7b4ae585-801c-441f-811d-5308e1002f08
  - session:64c4fde3-6f5e-456a-b4bb-9f17517e301c
  - session:1670fcb8-f275-498c-975b-8bd912331ded
---

# nmp-nip29 — Crate Structure and Actual Directory Layout

## Crate Structure

The nmp-nip29 crate contains action/, cache/, group_id.rs, interest.rs, kinds.rs, lib.rs, projection/, register.rs, and a tests/ directory. There are no domain/ or view/ directories. NIP-29 discovery/join UI is wired using a nmp.nip29.discover action and projection. The Groups tab displays both NIP-29 groups (unencrypted, relay-managed) and MLS (Marmot) groups (encrypted, private), with the NIP-29 section always present regardless of Marmot state.

NIP-29 wiring functions (wire_group_chat, wire_group_discovery, register_actions) live in the nmp-nip29 crate, not in Chirp's FFI layer. Chirp's nmp_app_chirp_register_group_chat and nmp_app_chirp_register_group_discovery are thin C-ABI delegates that parse arguments and call nmp_nip29::register functions, and register_nip29_actions in Chirp's ffi.rs is a one-liner delegate to nmp_nip29::register::register_actions. The lib.rs re-exports of nmp_app_chirp_register_group_chat and nmp_app_chirp_register_group_discovery were reverted as they were architectural violations.

nmp-nip29 depends on nmp-core with test-support as a dev-dependency, enabling it to host both wiring functions and integration tests. NmpApp methods register_action, register_event_observer, and register_snapshot_projection are pub and accessible cross-crate, allowing nmp-nip29 to own its own wiring. GroupChatProjection uses KernelEventObserver (reachable via IngestPreVerifiedEvents), not RawEventObserver, so a hermetic round-trip test does not require a real relay.

The NIP-29 group chat round-trip test lives at crates/nmp-nip29/tests/group_chat_round_trip.rs, not in the Chirp app crate. The test uses nmp_app_set_update_callback (the iOS production read path) to assert kind:9 events appear in projections["nmp.nip29.group_chat"]["messages"] within 3 seconds. It injects a decoy event for a different h-tag room and asserts it does NOT appear, proving h-tag filtering works. The publish-side test proves nmp.nip29.post_chat_message dispatch returns a 32-hex correlation_id and rejects malformed payloads missing the group field.

nmp-nip29 must contain zero code specific to kind:1111 (NIP-22). Any event kind can appear in a NIP-29 group context with just an h tag; NIP-29 only owns its own kinds (9/10/11/12, 9000–9022, 39000–39003).

<!-- citations: [^c3f75-13] [^1c093-20] [^7b4ae-6] [^64c4f-1] [^1670f-12] -->
## See Also

