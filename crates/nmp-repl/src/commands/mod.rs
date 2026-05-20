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
pub mod chirp;

// ── MLS / Marmot (bypass-kernel, direct-WebSocket) ───────────────────────
pub mod mls_util;
pub mod create_account;
pub mod load_key;
pub mod mls_init;
pub mod mls_status;
pub mod mls_create;
pub mod mls_fetch_kp;
pub mod mls_invite;
pub mod mls_accept;
pub mod mls_send;
pub mod mls_messages;
