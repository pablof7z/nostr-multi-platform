//! Per-verb command modules. Each is a small `run(&mut Session, args)`
//! function. The dispatch table lives in `main.rs`.

pub mod set_seed;
pub mod req;
pub mod show;
pub mod set_app_relays;
pub mod set_indexer;
pub mod set_dead;
pub mod set_budget;
pub mod refresh;
pub mod expand;
pub mod help;

// ── Identity + MLS / Marmot (bypass-kernel, direct-WebSocket) ────────────
//
// `load-key` only touches identity (`Option<nostr::Keys>`) so it stays in
// the default build. `create-account` publishes kind:0 + kind:10002 to the
// network, so the dispatch arm + parser arm are gated behind the `mls`
// feature; the module itself stays compiled (its deps are unconditional)
// to keep the source tree simple. The `mls_*` commands drive
// `MarmotService` (from `nmp-marmot`) and the MDK-backed in-memory store,
// so they're gated behind the `mls` Cargo feature.
pub mod create_account;
pub mod load_key;
#[cfg(feature = "mls")]
pub mod mls_util;
#[cfg(feature = "mls")]
pub mod mls_init;
#[cfg(feature = "mls")]
pub mod mls_status;
#[cfg(feature = "mls")]
pub mod mls_create;
#[cfg(feature = "mls")]
pub mod mls_fetch_kp;
#[cfg(feature = "mls")]
pub mod mls_invite;
#[cfg(feature = "mls")]
pub mod mls_accept;
#[cfg(feature = "mls")]
pub mod mls_send;
#[cfg(feature = "mls")]
pub mod mls_messages;
