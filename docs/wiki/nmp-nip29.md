---
title: "NMP NIP-29 Crate: Group Chat Wiring & Registration"
slug: nmp-nip29
summary: "NIP-29 wiring functions live in `nmp-nip29::register`, not in any app crate.  `nmp_nip29::register::wire_group_chat(app, group_id)` registers the GroupChatProje"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-05-23
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:64c4fde3-6f5e-456a-b4bb-9f17517e301c
  - session:1670fcb8-f275-498c-975b-8bd912331ded
---

# NMP NIP-29 Crate: Group Chat Wiring & Registration

## Wiring Registration

NIP-29 wiring functions live in `nmp-nip29::register`, not in any app crate.

`nmp_nip29::register::wire_group_chat(app, group_id)` registers the GroupChatProjection as a KernelEventObserver and a snapshot projection under key `nmp.nip29.group_chat`. `nmp_nip29::register::wire_group_discovery(app, host_relay_url)` wires the DiscoveredGroupsProjection. `nmp_nip29::register::register_actions(app)` binds all 5 NIP-29 ActionModules to the app via `register_action`.

Chirp FFI symbols (`nmp_app_chirp_register_group_chat`, `nmp_app_chirp_register_group_discovery`, `register_nip29_actions`) are thin C-ABI shells that delegate to `nmp_nip29::register` functions. [^64c4f-3]



NIP-29 groups accept ANY event kind — the only requirement is an `h` tag pointing at the group; no kind should be special-cased in nmp-nip29 beyond NIP-29's own kinds (9/10/11/12, 9000-9022, 39000-39003). [^1670f-11]
## Round-Trip Tests

NIP-29 group-chat round-trip tests live in `crates/nmp-nip29/tests/group_chat_round_trip.rs`, not in any app test directory.

The receive-side round-trip test proves a kind:9 event with an `h`-tag matching the group local_id surfaces in `projections["nmp.nip29.group_chat"]["messages"]` via the `nmp_app_set_update_callback` path, and that a decoy kind:9 event for a different `h`-tag group does NOT appear in the projection output.

The publish-side test proves `dispatch_action` for `nmp.nip29.post_chat_message` returns a 32-hex correlation_id and rejects malformed payloads missing the `group` field. [^64c4f-4]
## See Also

