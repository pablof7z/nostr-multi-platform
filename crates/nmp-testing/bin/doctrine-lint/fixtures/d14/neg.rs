//! Negative D14 fixture — must produce zero D14 findings.
//!
//! Every relay-shaped slot below uses a typed wrapper (the PR-I escape
//! hatch), so the bare `Arc<Mutex<Vec<…>>>` pattern never appears on a
//! field-declaration line.

use std::sync::{Arc, Mutex};

// Typed wrapper around the underlying `Vec<String>` — D14 treats this as
// the "promoted" shape and ignores it on field declarations.
pub struct RelayUrls(Vec<String>);

pub type IndexerRelaysSlot = Arc<Mutex<RelayUrls>>;

pub struct Kernel {
    // Typed slot — D14 stays silent.
    indexer_relays: IndexerRelaysSlot,
}

pub struct NmpApp {
    // Typed slot — D14 stays silent.
    pending_outbound: IndexerRelaysSlot,
}

pub struct PublishEngine {
    // Out-of-scope struct: even a bare `Arc<Mutex<Vec<…>>>` here is
    // tolerated because D14 only disciplines `NmpApp` / `Kernel` /
    // `Actor*` structs.
    pending: Arc<Mutex<Vec<u32>>>,
}

pub struct ActorContext {
    // Per-line escape hatch: a deliberate `Arc<Mutex<Vec<…>>>` is
    // permitted when the field carries a `// doctrine-allow: D14` comment.
    // The fixture exercises this exemption so a future regression that
    // silently breaks the opt-out path is loud.
    legacy_buffer: Arc<Mutex<Vec<u8>>>, // doctrine-allow: D14 — fixture for the opt-out path
}
