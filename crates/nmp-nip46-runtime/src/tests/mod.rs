//! Unit tests for `nmp-nip46-runtime`.
//!
//! Each test targets a specific mechanism; none require a running actor
//! thread or a real WebSocket.
//!
//! ## Test inventory
//!
//! - `relay_persistence` — `RelayRole::Signer` is treated as persistent by
//!   `relay_socket_is_persistent` (the make-or-break relay-lifetime contract).
//! - `enqueue_outbound_ordering` — `ActorCommand::EnqueueOutbound` frames
//!   are returned from dispatch in source order and wake the sender.
//! - `interceptor_translates_subscribe_effect` — `Effect::Subscribe` triggers
//!   persistent-sub registration and an outbound REQ frame.
//! - `interceptor_translates_progress_effect` — `Effect::Progress` posts
//!   `bunker_handshake_progress` on the `CommandSender`.
//! - `interceptor_translates_error_effect` — `Effect::Error` posts "failed"
//!   progress AND `bunker_connection_state_changed`.
//! - `connected_hook_replays_req_on_reconnect` — reconnect triggers an
//!   `EnqueueOutbound` REQ frame via `CommandSender`.
//! - `connected_hook_arms_deadline` — `arm_deadline` is called after replay,
//!   not at session init; deadline > `now` after connect.
//! - `transport_send_rpc_fire_and_forget` — `ActorLaneTransport::send_rpc`
//!   posts `EnqueueOutbound` without blocking.
//! - `transport_send_rpc_ordering` — frames are enqueued in call order.

#[path = "connected_hook_tests.rs"]
mod connected_hook_tests;
#[path = "enqueue_outbound.rs"]
mod enqueue_outbound;
#[path = "interceptor_effects.rs"]
mod interceptor_effects;
#[path = "relay_persistence.rs"]
mod relay_persistence;
#[path = "transport_tests.rs"]
mod transport_tests;
