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
