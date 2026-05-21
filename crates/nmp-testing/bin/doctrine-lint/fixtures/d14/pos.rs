//! Positive D14 fixture — must trigger at least one D14 finding.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced from a
//! Cargo.toml `path = ...` entry). It exists solely as text for the
//! doctrine-lint smoke test to scan.
//!
//! Each of the three structs below carries a bare `Arc<Mutex<Vec<…>>>`
//! field — the exact pattern PR-I forbade. D14 must fire one finding per
//! offending field (three findings total).

use std::sync::{Arc, Mutex};

pub struct Kernel {
    // Bare `Arc<Mutex<Vec<…>>>` field on `Kernel` — D14 fires.
    indexer_relays: Arc<Mutex<Vec<String>>>,
}

pub struct NmpApp {
    // Same pattern on `NmpApp` — D14 fires.
    pending_outbound: Arc<Mutex<Vec<u32>>>,
}

pub struct ActorRuntime {
    // Any `Actor*` struct is in scope too — D14 fires here.
    queued_commands: Arc<Mutex<Vec<String>>>,
}
