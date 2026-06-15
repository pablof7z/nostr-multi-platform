---
title: WASM Relay Pool
slug: wasm-relay-pool
topic: relay-routing
summary: "The nmp-wasm relay pool opens **one** physical `web_sys::WebSocket` per distinct relay URL, keyed by `CanonicalRelayUrl` alone, matching the native pool's `ensu"
tags:
  - capture
volatility: warm
confidence: high
created: 2026-06-14
updated: 2026-06-15
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:ac3ebc43-5320-419f-994e-b37d436010c9
  - session:ab8061fc-b277-4ba4-bf55-1532bcb1aa90
  - session:78b50727-bccd-4088-8493-a07624a4fa83
---

# WASM Relay Pool

## Connection model — one socket per URL

The nmp-wasm relay pool opens **one** physical `web_sys::WebSocket` per distinct relay URL, keyed by `CanonicalRelayUrl` alone, matching the native pool's `ensure_open` behavior. The driver for that URL stores the full set of declared roles in a `roles: Vec<RelayRole>` field (e.g., `[Content, Indexer]` for a "both,indexer" URL), preserving the complete role information for UI display independently of the single-socket dedup. The dedup/role-collapse logic is factored into a pure, native-testable planner module (`relay_plan::plan_drivers`), separate from the wasm32-gated `relay_pool::spawn_drivers` that turns plans into live drivers. Earlier the WASM transport spawned one `BrowserRelayDriver` per `(URL, role)` pair, so a "both,indexer" relay opened two WebSockets to the same host — a divergence from native that showed up as duplicate, half-idle relay connections in the browser network panel.

<!-- citations: [^ac3eb-3] -->
## Role attribution is diagnostics-only

Each collapsed driver reports inbound frames under a single `primary_role` — the first of `RelayRole::all()` (`[Content, Indexer]`) present in the URL's declared role set, so `"both,indexer"` reports as `Content`, exactly like the native pool's first-role-wins slot. This is behaviour-preserving because **inbound role is diagnostics-only**: the kernel ingests events identically regardless of role (`ingest_timeline_event` marks the role `_role`) and routes outbound purely by URL. The host's full declared role set is preserved in the driver's `roles` field and still reaches the UI via the kernel's `configured_relays` projection (seeded from bootstrap config, the source of truth for relay roles), independent of the driver pool — so role badges are unaffected. (This corrects an earlier capture claiming the driver "attributes frames to each role lane"; under first-role-wins it does not, and native does not either.)

<!-- citations: [^ac3eb-4] -->
## The kernel owns socket lifecycle (spawn-on-miss)

Socket lifecycle decisions — which and how many sockets to open — belong to nmp-core's relay-management actor, not the transport layer; the WASM runtime must obey the kernel's decisions rather than parsing role strings itself to create sockets. Per the crate-boundary doctrine the kernel owns spawn/close/route and the network layer obeys. On native this is implicit: the kernel emits an `OutboundMessage` targeting a URL, and `send_outbound` spawns a worker on demand for any URL the pool has not seen. The WASM `fan_out_outbound` now does the same — on a miss it **spawns** a `BrowserRelayDriver` for the targeted URL (under the message's role) instead of dropping the frame. So the kernel decides *which* relays to dial on web (bootstrap at `Start`, then NIP-65 mailboxes / event-tag hints discovered at runtime); the transport merely obeys.

The transport does **not** re-check admission: the router (`nmp-router`) already applies `RelayAdmissionPolicy` on the untrusted lanes (NIP-65 mailbox, hints, provenance) and filters per-account blocked relays before an `OutboundMessage` exists, so every URL reaching the transport is already admissible — and native's `send_outbound` carries no admission check either. Spawning requires the kernel-handler bag (`handlers_slot`, an `Rc`-based cloneable handle threaded through `runtime.rs`, `tick.rs`, `publish_path.rs`, and the relay_pool closures to resolve the spawn-time borrow cycle for on-demand driver creation); when it is empty (pool not started) or a URL fails to dial, the frame is dropped.

However, DoS mitigation for hostile relays sending unsolicited events **does** belong at the transport layer (per-relay quotas), not in the ingest admission gate — a malicious relay can flood the client independently of kernel routing decisions, and the transport is the right place to throttle or disconnect before events reach ingest.

<!-- citations: [^ac3eb-5] [^78b50-112] -->
### Pre-connect send buffer (why on-demand relays carry traffic)

A spawned driver is still `CONNECTING` when the REQ that triggered it is sent, and `handle_relay_connected` does **not** replay subscriptions on the first connect (`is_reconnect == false` only emits `startup_requests` + `pending_view_requests`; subscription replay is the reconnect path). So the driver must not drop that REQ. `BrowserRelayDriver::send_text` therefore gates on `ready_state() == OPEN` (not on the presence of `current_socket`, because `dial()` sets `current_socket = Some(socket)` immediately during the CONNECTING state, which would otherwise cause `InvalidStateError`), and buffers frames sent while the socket is not `OPEN` into a per-driver `pending` queue (bounded by `MAX_PENDING_FRAMES`) and flushes them in `build_on_open` on connect — mirroring the native `relay_worker`'s `pending` VecDeque + flush-on-connect. Bootstrap relays never needed this (their REQs are emitted *by* the connect event, when the socket is already open); on-demand relays do, because their REQ is emitted *before* connect. Without the buffer an on-demand relay would connect but stay idle — the original duplicate-socket complaint in a new form.

<!-- citations: [^ac3eb-6] -->
### Not yet covered

Transient relay connections — the transport pool dials arbitrary relay URLs on demand via `send_outbound` → `ensure_relay_worker_with_kind` → `pool.ensure_open_with_role`, with transient author sockets managed as `RelayConnectionKind::Temporary` with a 60-second idle teardown grace; no new transport capability is needed.

<!-- citations: [^ac3eb-1] [^ac3eb-2] [^ab806-71] [^ab806-78] [^ab806-175] -->

## Not yet covered

NMP's transport pool dials arbitrary relay URLs on demand and spawns a worker for any new URL, not just pre-configured relays, using RelayConnectionKind::Temporary with a 60-second idle teardown grace; no new transport capability is needed for connecting to third-party author relays.

<!-- citations: [^ab806-193] [^ab806-213] [^ab806-223] [^ab806-235] [^ab806-240] [^ab806-274] [^ab806-281] -->
