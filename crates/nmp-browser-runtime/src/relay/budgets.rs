//! Capacity and budget constants for the browser relay transport (#2070).
//!
//! Mirrors the native constants where applicable:
//! - `BROWSER_RELAY_DRAIN_BUDGET` mirrors `nmp-core`'s `COMMAND_DRAIN_BUDGET` (64).
//! - `MAX_INBOUND_QUEUED` bounds the inbound event queue so a flood cannot grow
//!   memory unboundedly between `pump()` calls.
//! - `MAX_CONCURRENT_SOCKETS` bounds the number of live WebSocket connections
//!   the browser pool opens simultaneously; excess spawns emit
//!   [`BrowserRuntimeEvent::RelayBudgetExceeded`] (D6-honest, never silent).

// Constants are consumed in wasm32-gated code (handlers, spawn, mod). On native
// they appear unused, but they exist to keep the budget configuration in one
// canonical place. // doctrine-allow: dead_code on native is expected for wasm32-only paths.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

/// Maximum number of inbound relay events applied per `pump()` turn.
///
/// Mirrors the native `COMMAND_DRAIN_BUDGET` for fairness. When the budget is
/// hit the `relay_yielded` flag is set so the host re-pumps.
pub(crate) const BROWSER_RELAY_DRAIN_BUDGET: usize = 64;

/// Maximum number of inbound relay events queued between `pump()` calls.
///
/// When the queue is full, oldest events are dropped and the drop counter is
/// incremented (never a silent loss — the counter is visible in diagnostics).
/// 1024 gives ~16 pump-budgets of headroom before any drop occurs.
pub(crate) const MAX_INBOUND_QUEUED: usize = 1024;

/// Maximum number of concurrent relay WebSocket connections.
///
/// Above this limit `fan_out_outbound` refuses new spawns and emits
/// `BrowserRuntimeEvent::RelayBudgetExceeded { url }`. The limit is generous
/// (typical apps use < 10 relays) and matches a reasonable browser socket
/// constraint.
pub(crate) const MAX_CONCURRENT_SOCKETS: usize = 64;
