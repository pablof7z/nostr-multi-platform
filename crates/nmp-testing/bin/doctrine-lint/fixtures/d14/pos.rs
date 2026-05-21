//! Positive D14 fixture — must trigger at least one D14 finding.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced from a
//! Cargo.toml `path = ...` entry). It exists solely as text for the
//! doctrine-lint smoke test to scan.
//!
//! Each of the structs below carries a bare `Arc<Mutex<Vec<…>>>` field —
//! the exact pattern PR-I forbade. D14 must fire one finding per offending
//! field (four findings total).

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

pub struct Nip65OutboxResolver {
    // PR-I2 follow-up: the resolver IS the substrate's relay-slot owner
    // (the original PR-I bare-slot trio originated here). A future
    // regression that re-introduces a bare `Arc<Mutex<Vec<…>>>` field on
    // `Nip65OutboxResolver` must fail D14 the same way it does on `Kernel`.
    local_write_relays: Arc<Mutex<Vec<String>>>,
}
